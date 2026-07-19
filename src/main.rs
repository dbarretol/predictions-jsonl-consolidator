use anyhow::{bail, Context, Result};
use chrono::Local;
use clap::Parser;
use fxhash::FxHashSet;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use tracing::{error, info};

impl From<PredictionLine> for TokenReport {
    fn from(pred: PredictionLine) -> Self {
        let mut image_tokens = 0;
        let mut text_tokens = 0;
        for detail in &pred.response.usage_metadata.prompt_tokens_details {
            match detail.modality.as_str() {
                "IMAGE" => image_tokens += detail.token_count,
                "TEXT" => text_tokens += detail.token_count,
                _ => {}
            }
        }

        TokenReport {
            key: pred.key,
            model: pred.response.model_version,
            traffic_type: pred.response.usage_metadata.traffic_type,
            prompt_image_tokens: image_tokens,
            prompt_text_tokens: text_tokens,
            prompt_tokens: pred.response.usage_metadata.prompt_token_count,
            candidate_tokens: pred.response.usage_metadata.candidates_token_count,
            thoughts_tokens: pred.response.usage_metadata.thoughts_token_count,
            total_tokens: pred.response.usage_metadata.total_token_count,
        }
    }
}
#[derive(Serialize, Clone)]
struct TokenReport {
    key: String,
    model: String,
    #[serde(rename = "Traffic Type")]
    traffic_type: TrafficType,
    #[serde(rename = "Prompt Image Tokens")]
    prompt_image_tokens: u32,
    #[serde(rename = "Prompt Text Tokens")]
    prompt_text_tokens: u32,
    #[serde(rename = "Prompt Tokens")]
    prompt_tokens: u32,
    #[serde(rename = "Candidate Tokens")]
    candidate_tokens: u32,
    #[serde(rename = "Thoughts Tokens")]
    thoughts_tokens: u32,
    #[serde(rename = "Total Tokens")]
    total_tokens: u32,
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Archivo JSONL de entrada (opcional si se usa --dir)
    input: Option<PathBuf>,

    /// Carpeta para buscar y consolidar archivos JSONL
    #[arg(long, short)]
    dir: Option<PathBuf>,

    /// Archivo de salida TXT (opcional, se genera con timestamp por defecto)
    #[arg(long)]
    out_txt: Option<PathBuf>,

    /// Archivo de salida Markdown (opcional, se genera con timestamp por defecto)
    #[arg(long)]
    out_md: Option<PathBuf>,

    /// Archivo de salida CSV para metadata (opcional, se genera con timestamp por defecto)
    #[arg(long)]
    out_csv: Option<PathBuf>,
}

#[derive(Deserialize, Debug, Clone)]
struct Part {
    text: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
struct Content {
    parts: Option<Vec<Part>>,
}

#[derive(Deserialize, Debug, Clone)]
struct Candidate {
    content: Content,
}

#[derive(Deserialize, Debug, Clone)]
struct TokenDetail {
    modality: String,
    #[serde(rename = "tokenCount")]
    token_count: u32,
}

#[derive(Deserialize, Debug, Serialize, Default, Clone)]
enum TrafficType {
    #[default]
    #[serde(rename = "regular", alias = "REGULAR")]
    Regular,
    #[serde(rename = "ON_DEMAND", alias = "onDemand", alias = "on_demand")]
    OnDemand,
    #[serde(rename = "batch", alias = "BATCH")]
    Batch,
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize, Debug, Clone)]
struct UsageMetadata {
    #[serde(rename = "candidatesTokenCount", default)]
    candidates_token_count: u32,
    #[serde(rename = "promptTokenCount", default)]
    prompt_token_count: u32,
    #[serde(rename = "promptTokensDetails", default)]
    prompt_tokens_details: Vec<TokenDetail>,
    #[serde(rename = "thoughtsTokenCount", default)]
    thoughts_token_count: u32,
    #[serde(rename = "totalTokenCount", default)]
    total_token_count: u32,
    #[serde(rename = "trafficType", default)]
    traffic_type: TrafficType,
}

#[derive(Deserialize, Debug, Clone)]
struct Response {
    candidates: Option<Vec<Candidate>>,
    #[serde(rename = "modelVersion")]
    model_version: String,
    #[serde(rename = "usageMetadata")]
    usage_metadata: UsageMetadata,
}

#[derive(Debug, Clone)]
struct RecordEntry {
    key: String,
    text_hash: u64,
    line: String,
}

#[derive(Deserialize, Debug, Clone)]
struct PredictionLine {
    key: String,
    response: Response,
}

impl PredictionLine {
    fn extract_text(&self) -> Option<&str> {
        self.response
            .candidates
            .as_ref()?
            .first()?
            .content
            .parts
            .as_ref()?
            .first()?
            .text
            .as_deref()
    }
}

#[derive(Clone)]
struct ProcessedRecord {
    key: String,
    text: String,
    report: TokenReport,
}

#[derive(Default)]
struct ConsolidationSummary {
    files_read: usize,
    duplicates_removed: usize,
    parse_errors: usize,
    /// Ruta del `consolidated_*.jsonl` generado, si se combinaron varios archivos.
    consolidated_file: Option<PathBuf>,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();
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
    let (input_path, summary) = if let Some(dir) = &args.dir {
        process_directory(dir, timestamp)?
    } else if let Some(input) = args.input {
        (
            input,
            ConsolidationSummary {
                files_read: 1,
                ..Default::default()
            },
        )
    } else {
        let current_dir = std::env::current_dir()
            .context("No se pudo determinar la carpeta actual")?;
        println!(
            "No se especificó archivo ni carpeta; usando la carpeta actual: {}",
            current_dir.display()
        );
        process_directory(&current_dir, timestamp)?
    };

    let input_dir = input_path.parent().unwrap_or_else(|| Path::new("."));
    let extract_dir = input_dir.join("jsonl_extract");

    if !extract_dir.exists() {
        fs::create_dir_all(&extract_dir).with_context(|| {
            format!(
                "No se pudo crear el directorio de salida: {}",
                extract_dir.display()
            )
        })?;
    }

    // Abrir archivos de salida para streaming (aunque ahora escribiremos al final)
    let out_txt_path = args.out_txt.unwrap_or_else(|| {
        extract_dir.join(format!("consolidado_{}.txt", timestamp))
    });
    let out_md_path = args.out_md.unwrap_or_else(|| {
        extract_dir.join(format!("consolidado_{}.md", timestamp))
    });
    let out_csv_path = args.out_csv.unwrap_or_else(|| {
        extract_dir.join(format!("reporte_tokens_{}.csv", timestamp))
    });

    // Lista para acumular todos los datos y ordenarlos antes de escribir cualquier archivo
    let mut processed_records = Vec::new();
    let mut final_parse_errors = 0usize;
    let mut records_without_text = 0usize;

    let file = File::open(&input_path)
        .with_context(|| format!("No se pudo abrir el archivo de entrada: {}", input_path.display()))?;
    let reader = BufReader::new(file);

    for (idx, line) in reader.lines().enumerate() {
        let line = line.with_context(|| {
            format!(
                "No se pudo leer la línea {} de {}",
                idx + 1,
                input_path.display()
            )
        })?;
        if line.trim().is_empty() {
            continue;
        }

        match serde_json::from_str::<PredictionLine>(&line) {
            Ok(pred) => {
                let text = match pred.extract_text() {
                    Some(text) => text.to_string(),
                    None => {
                        records_without_text += 1;
                        "...".to_string()
                    }
                };

                let report = TokenReport::from(pred.clone());

                processed_records.push(ProcessedRecord {
                    key: pred.key,
                    text,
                    report,
                });
            }
            Err(e) => {
                final_parse_errors += 1;
                error!(
                    line = idx + 1,
                    error = %e,
                    content = %line.chars().take(120).collect::<String>(),
                    "Error parseando línea"
                );
            }
        }
    }

    // Ordenar TODO globalmente por clave usando orden natural
    processed_records.sort_by(|a, b| natord::compare(&a.key, &b.key));
    let lines_processed = processed_records.len();

    // Escribir archivos de salida ahora que todo está ordenado
    let mut txt_file = File::create(&out_txt_path)
        .with_context(|| format!("No se pudo crear el archivo TXT: {}", out_txt_path.display()))?;
    let mut md_file = File::create(&out_md_path)
        .with_context(|| format!("No se pudo crear el archivo Markdown: {}", out_md_path.display()))?;
    let mut csv_writer = csv::Writer::from_path(&out_csv_path)
        .with_context(|| format!("No se pudo crear el archivo CSV: {}", out_csv_path.display()))?;

    for record in processed_records {
        // Escribir TXT
        writeln!(txt_file, "{}\n", record.text)
            .with_context(|| format!("No se pudo escribir en {}", out_txt_path.display()))?;

        // Escribir Markdown
        writeln!(md_file, "## Clave: {}\n\n{}\n", record.key, record.text)
            .with_context(|| format!("No se pudo escribir en {}", out_md_path.display()))?;

        // Escribir CSV
        csv_writer
            .serialize(record.report)
            .with_context(|| format!("No se pudo escribir en {}", out_csv_path.display()))?;
    }

    csv_writer
        .flush()
        .with_context(|| format!("No se pudo finalizar el archivo CSV: {}", out_csv_path.display()))?;

    println!("\nResumen:");
    println!("  Archivos leídos:            {}", summary.files_read);
    println!("  Líneas procesadas:          {}", lines_processed);
    println!(
        "  Líneas con error de parseo: {}",
        summary.parse_errors + final_parse_errors
    );
    println!("  Duplicados eliminados:      {}", summary.duplicates_removed);
    println!("  Registros sin texto:        {}", records_without_text);
    println!("  Archivos generados:");
    if let Some(consolidated) = &summary.consolidated_file {
        println!("    - {}", consolidated.display());
    }
    println!("    - {}", out_txt_path.display());
    println!("    - {}", out_md_path.display());
    println!("    - {}", out_csv_path.display());

    Ok(())
}

fn process_directory(dir: &Path, timestamp: &str) -> Result<(PathBuf, ConsolidationSummary)> {
    let mut jsonl_files = Vec::new();
    let entries = fs::read_dir(dir)
        .with_context(|| format!("No se pudo leer la carpeta: {}", dir.display()))?;
    for entry in entries {
        let entry = entry
            .with_context(|| format!("No se pudo leer una entrada de la carpeta: {}", dir.display()))?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
            let filename = path.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("");

            if !filename.starts_with("consolidated_") {
                jsonl_files.push(path);
            }
        }
    }

    if jsonl_files.is_empty() {
        bail!("No se encontraron archivos .jsonl en {}", dir.display());
    }

    if jsonl_files.len() == 1 {
        println!("Se encontró un único archivo JSONL: {}", jsonl_files[0].display());
        return Ok((
            jsonl_files[0].clone(),
            ConsolidationSummary {
                files_read: 1,
                ..Default::default()
            },
        ));
    }

    info!("Consolidando {} archivos JSONL...", jsonl_files.len());

    // Usamos fold/reduce para mejor paralelismo en lugar de Mutex compartido
    // Ahora acumulamos RecordEntry en un Vec para mantener la consistencia
    let (_, final_records, total_skipped, total_seen) = jsonl_files.par_iter().fold(
        || (FxHashSet::<(String, u64)>::default(), Vec::<RecordEntry>::new(), 0usize, 0usize),
        |(mut seen, mut acc_records, mut skipped, mut total), file_path| {
            match File::open(file_path) {
                Ok(file) => {
                    let reader = BufReader::new(file);
                    for line in reader.lines() {
                        if let Ok(line) = line {
                            let trimmed = line.trim();
                            if trimmed.is_empty() {
                                continue;
                            }

                            // Intentamos parsear la línea para extraer key y texto
                            if let Ok(pred) = serde_json::from_str::<PredictionLine>(trimmed) {
                                let text = pred.extract_text().unwrap_or("");
                                let text_hash = fxhash::hash64(text.as_bytes());
                                total += 1;

                                if seen.insert((pred.key.clone(), text_hash)) {
                                    acc_records.push(RecordEntry {
                                        key: pred.key,
                                        text_hash,
                                        line: trimmed.to_string(),
                                    });
                                }
                            } else {
                                error!("No se pudo parsear registro en archivo {:?}: {}", file_path, trimmed.chars().take(50).collect::<String>());
                                skipped += 1;
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("No se pudo abrir archivo {:?}: {}", file_path, e);
                }
            }
            (seen, acc_records, skipped, total)
        }
    ).reduce(
        || (FxHashSet::<(String, u64)>::default(), Vec::<RecordEntry>::new(), 0usize, 0usize),
        |(mut acc_seen, mut acc_records, mut acc_skipped, mut acc_total), (_seen, records, skipped, total)| {
            for entry in records {
                if acc_seen.insert((entry.key.clone(), entry.text_hash)) {
                    acc_records.push(entry);
                }
            }
            acc_skipped += skipped;
            acc_total += total;
            (acc_seen, acc_records, acc_skipped, acc_total)
        }
    );

    if total_skipped > 0 {
        error!("Total de líneas ignoradas por error de parseo: {}", total_skipped);
    }

    let duplicates_removed = total_seen.saturating_sub(final_records.len());

    let mut records = Vec::new();
    for entry in final_records {
        records.push((entry.key, entry.line));
    }

    // Ordenar por clave usando orden natural (numérico consciente)
    records.sort_by(|a, b| natord::compare(&a.0, &b.0));

    let consolidated_name = format!("consolidated_{}.jsonl", timestamp);
    let consolidated_path = dir.join(consolidated_name);
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
    Ok((
        consolidated_path.clone(),
        ConsolidationSummary {
            files_read: jsonl_files.len(),
            duplicates_removed,
            parse_errors: total_skipped,
            consolidated_file: Some(consolidated_path),
        },
    ))
}
