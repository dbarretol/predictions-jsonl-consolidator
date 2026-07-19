mod cli;
mod consolidator;
mod errors;
mod exporters;
mod models;
mod parser;

use anyhow::{bail, Context, Result};
use chrono::Local;
use clap::Parser;
use cli::Args;
use consolidator::{deduplicate_files, find_jsonl_files, ConsolidationSummary};
use models::ProcessedRecord;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;
use tracing::error;

fn main() -> Result<()> {
    let args = Args::parse();
    init_tracing(&args);
    let auto_mode = args.input.is_none() && args.dir.is_none();
    let timestamp = Local::now().format("%Y%m%d_%H%M%S").to_string();

    let result = run(args, &timestamp);

    if let Err(ref e) = result {
        eprintln!("Error: {:?}", e);
    }

    if auto_mode {
        pause_before_exit();
    }

    if result.is_err() {
        std::process::exit(1);
    }

    Ok(())
}

/// Nivel de diagnóstico por defecto `info`; `--verbose` lo sube a `debug` (revela
/// las métricas de consolidación) y `--quiet` lo baja a `error` (nada salvo fallos).
/// `RUST_LOG`, si está definida, tiene prioridad sobre ambos flags.
fn init_tracing(args: &Args) {
    let default_level = if args.verbose {
        "debug"
    } else if args.quiet {
        "error"
    } else {
        "info"
    };

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default_level));

    tracing_subscriber::fmt().with_env_filter(env_filter).init();
}

/// En Windows, si el ejecutable se abrió con doble click no hay terminal que
/// mantenga la ventana abierta al terminar: se espera una tecla para poder
/// leer el resultado antes de que se cierre.
fn pause_before_exit() {
    if cfg!(windows) {
        println!("\nPresione Enter para salir...");
        let _ = io::stdout().flush();
        let mut buf = String::new();
        let _ = io::stdin().read_line(&mut buf);
    }
}

fn run(args: Args, timestamp: &str) -> Result<()> {
    let decisions_path = args.decisions.clone();

    // Tanto el modo de archivo único como el de carpeta convergen en la misma lista
    // de archivos a deduplicar: la diferencia es solo cuántos hay y si, al haber más
    // de uno, corresponde escribir un consolidated_*.jsonl con el resultado.
    let (base_dir, source_files, is_dir_scan) = if let Some(dir) = &args.dir {
        (dir.clone(), find_jsonl_files(dir, args.recursive)?, true)
    } else if let Some(input) = &args.input {
        let base = input.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();
        (base, vec![input.clone()], false)
    } else {
        let current_dir = std::env::current_dir()
            .context("No se pudo determinar la carpeta actual")?;
        println!(
            "No se especificó archivo ni carpeta; usando la carpeta actual: {}",
            current_dir.display()
        );
        (current_dir.clone(), find_jsonl_files(&current_dir, args.recursive)?, true)
    };

    if is_dir_scan {
        if source_files.len() == 1 {
            println!("Se encontró un único archivo JSONL: {}", source_files[0].display());
        } else {
            println!("Consolidando {} archivos JSONL...", source_files.len());
        }
    }

    let (resolved_records, total_seen, total_skipped, unreadable_files) =
        deduplicate_files(&source_files, &base_dir, decisions_path.as_deref(), !args.quiet)?;
    let duplicates_removed = total_seen.saturating_sub(resolved_records.len());

    // El archivo consolidated_*.jsonl solo tiene sentido cuando de verdad se combinan
    // varios archivos de origen.
    let consolidated_file = if is_dir_scan && source_files.len() > 1 {
        let mut records: Vec<(&str, &str)> = resolved_records
            .iter()
            .map(|entry| (entry.key.as_str(), entry.line.as_str()))
            .collect();
        records.sort_by(|a, b| natord::compare(a.0, b.0));

        let consolidated_name = format!("consolidated_{}.jsonl", timestamp);
        let consolidated_path = base_dir.join(consolidated_name);
        let mut output_file = File::create(&consolidated_path).with_context(|| {
            format!(
                "No se pudo crear el archivo consolidado: {}",
                consolidated_path.display()
            )
        })?;
        for (_, line) in records {
            writeln!(output_file, "{}", line).with_context(|| {
                format!(
                    "No se pudo escribir en el archivo consolidado: {}",
                    consolidated_path.display()
                )
            })?;
        }
        println!("Archivo consolidado generado: {}", consolidated_path.display());
        Some(consolidated_path)
    } else {
        None
    };

    let summary = ConsolidationSummary {
        files_read: source_files.len().saturating_sub(unreadable_files.len()),
        duplicates_removed,
        parse_errors: total_skipped,
        consolidated_file,
        unreadable_files,
    };

    // Solo se crea jsonl_extract si al menos una ruta de salida usa su valor por
    // defecto; si las tres son personalizadas (--out-txt/--out-md/--out-csv), no
    // hace falta crearlo. Los padres de rutas personalizadas también se crean.
    let extract_dir = base_dir.join("jsonl_extract");

    let out_txt_path = args.out_txt.unwrap_or_else(|| {
        extract_dir.join(format!("consolidado_{}.txt", timestamp))
    });
    let out_md_path = args.out_md.unwrap_or_else(|| {
        extract_dir.join(format!("consolidado_{}.md", timestamp))
    });
    let out_csv_path = args.out_csv.unwrap_or_else(|| {
        extract_dir.join(format!("reporte_tokens_{}.csv", timestamp))
    });

    for path in [&out_txt_path, &out_md_path, &out_csv_path] {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent).with_context(|| {
                format!("No se pudo crear el directorio de salida: {}", parent.display())
            })?;
        }
    }

    // Los registros ya vienen parseados desde `deduplicate_files` (una sola pasada
    // de JSON por línea): solo falta convertirlos a la forma que esperan los reportes.
    let mut processed_records = Vec::new();
    let mut records_without_text = 0usize;

    for entry in resolved_records {
        let text = match entry.text {
            Some(text) => text,
            None => {
                records_without_text += 1;
                error!(key = %entry.key, "Registro sin texto extraído");
                "...".to_string()
            }
        };

        processed_records.push(ProcessedRecord {
            key: entry.key,
            text,
            report: entry.report,
        });
    }

    // Ordenar TODO globalmente por clave usando orden natural
    processed_records.sort_by(|a, b| natord::compare(&a.key, &b.key));
    let lines_processed = processed_records.len();

    exporters::txt::write_txt(&out_txt_path, &processed_records, !args.no_keys)?;
    exporters::md::write_md(&out_md_path, &processed_records)?;
    exporters::csv::write_csv(&out_csv_path, &processed_records)?;

    println!("\nResumen:");
    println!("  Archivos leídos:            {}", summary.files_read);
    println!("  Archivos ilegibles:         {}", summary.unreadable_files.len());
    println!("  Líneas procesadas:          {}", lines_processed);
    println!("  Líneas con error de parseo: {}", summary.parse_errors);
    println!("  Duplicados eliminados:      {}", summary.duplicates_removed);
    println!("  Registros sin texto:        {}", records_without_text);
    println!("  Archivos generados:");
    if let Some(consolidated) = &summary.consolidated_file {
        println!("    - {}", consolidated.display());
    }
    println!("    - {}", out_txt_path.display());
    println!("    - {}", out_md_path.display());
    println!("    - {}", out_csv_path.display());

    if !summary.unreadable_files.is_empty() {
        bail!(
            "No se pudieron leer {} archivo(s), por lo que el resultado puede estar incompleto: {}",
            summary.unreadable_files.len(),
            summary
                .unreadable_files
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    Ok(())
}
