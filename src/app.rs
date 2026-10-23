//! Orquestación de una corrida: resolver entradas → deduplicar → escribir el
//! consolidado y los reportes → imprimir el resumen.

use crate::cli::Args;
use crate::errors::AppError;
use crate::exporters;
use crate::models::Record;
use crate::pipeline::{
    Deduplication, DeduplicationStats, PendingConflicts, RecordEntry, deduplicate_files,
    find_jsonl_files,
};
use anyhow::{Context, Result, bail};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use tracing::error;

pub fn run(args: Args, timestamp: &str) -> Result<()> {
    let inputs = resolve_inputs(&args)?;

    let outcome = match deduplicate_files(&inputs.files, args.decisions.as_deref(), !args.quiet)? {
        Deduplication::Resolved(outcome) => outcome,
        Deduplication::PendingDecisions(pending) => {
            return request_decisions(&pending, &decisions_template_path(&args, &inputs.base_dir));
        }
    };

    let mut generated_files = Vec::new();
    // El archivo consolidated_*.jsonl solo tiene sentido cuando de verdad se combinan
    // varios archivos de origen.
    if inputs.is_dir_scan && inputs.files.len() > 1 {
        generated_files.push(write_consolidated(
            &outcome.records,
            &inputs.base_dir,
            timestamp,
        )?);
    }

    let outputs = OutputPaths::resolve(&args, &inputs.base_dir, timestamp);
    outputs.create_parent_dirs()?;

    let records = into_sorted_records(outcome.records);
    let records_without_text = count_records_without_text(&records);
    exporters::txt::write_txt(&outputs.txt, &records, !args.no_keys)?;
    exporters::md::write_md(&outputs.md, &records)?;
    exporters::csv::write_csv(&outputs.csv, &records)?;
    generated_files.extend([outputs.txt, outputs.md, outputs.csv]);

    let report = RunReport {
        files_found: inputs.files.len(),
        lines_processed: records.len(),
        records_without_text,
        stats: outcome.stats,
        generated_files,
    };
    report.print();
    report.ensure_complete()
}

/// Archivos a procesar y la carpeta base para las salidas por defecto.
struct Inputs {
    base_dir: PathBuf,
    files: Vec<PathBuf>,
    /// `true` en modo `--dir` o auto-modo (búsqueda en carpeta), `false` con un archivo.
    is_dir_scan: bool,
}

/// Tanto el modo de archivo único como el de carpeta convergen en la misma lista de
/// archivos a deduplicar: la diferencia es solo cuántos hay y si, al haber más de uno,
/// corresponde escribir un consolidated_*.jsonl con el resultado.
fn resolve_inputs(args: &Args) -> Result<Inputs> {
    let inputs = if let Some(dir) = &args.dir {
        Inputs {
            base_dir: dir.clone(),
            files: find_jsonl_files(dir, args.recursive)?,
            is_dir_scan: true,
        }
    } else if let Some(input) = &args.input {
        Inputs {
            base_dir: input
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf(),
            files: vec![input.clone()],
            is_dir_scan: false,
        }
    } else {
        let current_dir =
            std::env::current_dir().context("No se pudo determinar la carpeta actual")?;
        println!(
            "No se especificó archivo ni carpeta; usando la carpeta actual: {}",
            current_dir.display()
        );
        Inputs {
            files: find_jsonl_files(&current_dir, args.recursive)?,
            base_dir: current_dir,
            is_dir_scan: true,
        }
    };

    if inputs.is_dir_scan {
        if inputs.files.len() == 1 {
            println!(
                "Se encontró un único archivo JSONL: {}",
                inputs.files[0].display()
            );
        } else {
            println!("Consolidando {} archivos JSONL...", inputs.files.len());
        }
    }

    Ok(inputs)
}

/// Dónde escribir la plantilla `decisions.csv` cuando hay conflictos sin resolver.
/// Es un archivo de trabajo que acompaña a los reportes: si se eligió otra ubicación
/// para el CSV con `--out-csv`, va junto a él; si no, en la carpeta procesada, que es
/// donde el usuario ya está mirando (y donde quedan los reportes, en `jsonl_extract/`).
fn decisions_template_path(args: &Args, base_dir: &Path) -> PathBuf {
    let dir = args
        .out_csv
        .as_deref()
        .and_then(Path::parent)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(base_dir);
    dir.join("decisions.csv")
}

/// Escribe la plantilla `decisions.csv` y corta la corrida pidiendo completarla:
/// siempre devuelve `Err` (`ConflictsPendingResolution`, o el error de E/S si no se
/// pudo escribir).
fn request_decisions(pending: &PendingConflicts, template_path: &Path) -> Result<()> {
    if let Some(parent) = template_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("No se pudo crear el directorio: {}", parent.display()))?;
    }
    pending.write_template(template_path)?;
    Err(AppError::ConflictsPendingResolution {
        count: pending.len(),
        decisions_path: template_path.to_path_buf(),
    }
    .into())
}

/// Escribe `consolidated_{timestamp}.jsonl` en `base_dir` con las líneas crudas de los
/// registros resueltos, en orden natural de key.
fn write_consolidated(
    records: &[RecordEntry],
    base_dir: &Path,
    timestamp: &str,
) -> Result<PathBuf> {
    let mut sorted: Vec<&RecordEntry> = records.iter().collect();
    sorted.sort_by(|a, b| natord::compare(&a.record.key, &b.record.key));

    let path = base_dir.join(format!("consolidated_{}.jsonl", timestamp));
    let mut file = File::create(&path).with_context(|| {
        format!(
            "No se pudo crear el archivo consolidado: {}",
            path.display()
        )
    })?;
    for entry in sorted {
        writeln!(file, "{}", entry.line).with_context(|| {
            format!(
                "No se pudo escribir en el archivo consolidado: {}",
                path.display()
            )
        })?;
    }
    println!("Archivo consolidado generado: {}", path.display());
    Ok(path)
}

/// Rutas de los tres reportes: las indicadas con `--out-*` o, por defecto, dentro de
/// `jsonl_extract/` en la carpeta base, con timestamp.
struct OutputPaths {
    txt: PathBuf,
    md: PathBuf,
    csv: PathBuf,
}

impl OutputPaths {
    fn resolve(args: &Args, base_dir: &Path, timestamp: &str) -> Self {
        let extract_dir = base_dir.join("jsonl_extract");
        let or_default = |custom: &Option<PathBuf>, name: String| {
            custom.clone().unwrap_or_else(|| extract_dir.join(name))
        };
        OutputPaths {
            txt: or_default(&args.out_txt, format!("consolidado_{}.txt", timestamp)),
            md: or_default(&args.out_md, format!("consolidado_{}.md", timestamp)),
            csv: or_default(&args.out_csv, format!("reporte_tokens_{}.csv", timestamp)),
        }
    }

    /// Crea las carpetas padre que falten. `jsonl_extract` solo se crea si al menos una
    /// ruta usa su valor por defecto; los padres de rutas personalizadas también se crean.
    fn create_parent_dirs(&self) -> Result<()> {
        for path in [&self.txt, &self.md, &self.csv] {
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                fs::create_dir_all(parent).with_context(|| {
                    format!(
                        "No se pudo crear el directorio de salida: {}",
                        parent.display()
                    )
                })?;
            }
        }
        Ok(())
    }
}

/// Separa los registros de su procedencia (línea cruda y archivo de origen, que solo
/// usan el consolidado y `decisions.csv`) y los ordena por key en orden natural, como
/// los esperan los exportadores.
fn into_sorted_records(entries: Vec<RecordEntry>) -> Vec<Record> {
    let mut records: Vec<Record> = entries.into_iter().map(|entry| entry.record).collect();
    records.sort_by(|a, b| natord::compare(&a.key, &b.key));
    records
}

/// Registra en el log cada registro sin texto, con su `finish_reason`, y devuelve
/// cuántos son. En el TXT y el MD aparecen con un marcador (ver `exporters`).
fn count_records_without_text(records: &[Record]) -> usize {
    let mut count = 0;
    for record in records.iter().filter(|r| r.text.is_none()) {
        error!(
            key = %record.key,
            finish_reason = record.usage.finish_reason.as_deref().unwrap_or("desconocido"),
            "Registro sin texto extraído"
        );
        count += 1;
    }
    count
}

/// Todo lo que se informa al final de una corrida exitosa.
struct RunReport {
    files_found: usize,
    lines_processed: usize,
    records_without_text: usize,
    stats: DeduplicationStats,
    generated_files: Vec<PathBuf>,
}

impl RunReport {
    fn print(&self) {
        let stats = &self.stats;
        println!("\nResumen:");
        println!(
            "  Archivos leídos:            {}",
            self.files_found
                .saturating_sub(stats.unreadable_files.len())
        );
        println!(
            "  Archivos ilegibles:         {}",
            stats.unreadable_files.len()
        );
        println!("  Líneas procesadas:          {}", self.lines_processed);
        println!("  Líneas con error de parseo: {}", stats.parse_errors);
        println!("  Predicciones fallidas:      {}", stats.failed_predictions);
        println!("  Duplicados eliminados:      {}", stats.duplicates_removed);
        if stats.discarded_by_decision > 0 {
            println!(
                "  Descartados por decisión:   {}",
                stats.discarded_by_decision
            );
        }
        println!(
            "  Registros sin texto:        {}",
            self.records_without_text
        );
        println!("  Archivos generados:");
        for path in &self.generated_files {
            println!("    - {}", path.display());
        }
    }

    /// Falla (exit ≠ 0) si algún archivo no se pudo leer: los reportes se generaron,
    /// pero pueden estar incompletos y eso no debe pasar desapercibido.
    fn ensure_complete(&self) -> Result<()> {
        let unreadable = &self.stats.unreadable_files;
        if !unreadable.is_empty() {
            bail!(
                "No se pudieron leer {} archivo(s), por lo que el resultado puede estar incompleto: {}",
                unreadable.len(),
                unreadable
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        Ok(())
    }
}
