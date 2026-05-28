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
#[derive(Serialize)]
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

#[derive(Deserialize, Debug)]
struct Part {
    text: Option<String>,
}

#[derive(Deserialize, Debug)]
struct Content {
    parts: Option<Vec<Part>>,
}

#[derive(Deserialize, Debug)]
struct Candidate {
    content: Content,
}

#[derive(Deserialize, Debug)]
struct TokenDetail {
    modality: String,
    #[serde(rename = "tokenCount")]
    token_count: u32,
}

#[derive(Deserialize, Debug, Serialize, Default)]
enum TrafficType {
    #[default]
    #[serde(rename = "regular")]
    Regular,
    #[serde(rename = "onDemand")]
    OnDemand,
    #[serde(rename = "batch")]
    Batch,
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize, Debug)]
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

#[derive(Deserialize, Debug)]
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

#[derive(Deserialize, Debug)]
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

fn main() -> io::Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();
    let timestamp = Local::now().format("%Y%m%d_%H%M%S").to_string();

    let input_path = if let Some(dir) = &args.dir {
        process_directory(dir, &timestamp)?
    } else if let Some(input) = args.input {
        input
    } else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Debe proporcionar un archivo de entrada o una carpeta con --dir",
        ));
    };

    let input_dir = input_path.parent().unwrap_or_else(|| Path::new("."));
    let extract_dir = input_dir.join("jsonl_extract");

    if !extract_dir.exists() {
        fs::create_dir_all(&extract_dir)?;
    }

    let out_txt_path = args.out_txt.unwrap_or_else(|| {
        extract_dir.join(format!("consolidado_{}.txt", timestamp))
    });
    let out_md_path = args.out_md.unwrap_or_else(|| {
        extract_dir.join(format!("consolidado_{}.md", timestamp))
    });
    let out_csv_path = args.out_csv.unwrap_or_else(|| {
        extract_dir.join(format!("reporte_tokens_{}.csv", timestamp))
    });

    // Abrir archivos de salida para streaming
    let mut txt_file = File::create(&out_txt_path)?;
    let mut md_file = File::create(&out_md_path)?;
    let mut csv_writer = csv::Writer::from_path(&out_csv_path)?;

    let file = File::open(&input_path)?;
    let reader = BufReader::new(file);

    for (idx, line) in reader.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        match serde_json::from_str::<PredictionLine>(&line) {
            Ok(pred) => {
                let text = pred
                    .extract_text()
                    .unwrap_or("...");
                
                // Streaming a los archivos de salida
                writeln!(txt_file, "{}\n", text)?;
                writeln!(md_file, "## Clave: {}\n\n{}\n", pred.key, text)?;
                csv_writer.serialize(TokenReport::from(pred))?;
            }
            Err(e) => {
                error!(
                    line = idx + 1,
                    error = %e,
                    content = %line.chars().take(120).collect::<String>(),
                    "Error parseando línea"
                );
            }
        }
    }
    
    csv_writer.flush()?;
    info!("Archivos generados en: {:?}", extract_dir);

    Ok(())
}

fn process_directory(dir: &Path, timestamp: &str) -> io::Result<PathBuf> {
    let mut jsonl_files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
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
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("No se encontraron archivos .jsonl en {:?}", dir),
        ));
    }

    if jsonl_files.len() == 1 {
        println!("Se encontró un único archivo JSONL: {:?}", jsonl_files[0]);
        return Ok(jsonl_files[0].clone());
    }

    info!("Consolidando {} archivos JSONL...", jsonl_files.len());
    
    // Usamos fold/reduce para mejor paralelismo en lugar de Mutex compartido
    // Ahora acumulamos RecordEntry en un Vec para mantener la consistencia
    let (_, final_records, total_skipped) = jsonl_files.par_iter().fold(
        || (FxHashSet::<(String, u64)>::default(), Vec::<RecordEntry>::new(), 0usize),
        |(mut seen, mut acc_records, mut skipped), file_path| {
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
            (seen, acc_records, skipped)
        }
    ).reduce(
        || (FxHashSet::<(String, u64)>::default(), Vec::<RecordEntry>::new(), 0usize),
        |(mut acc_seen, mut acc_records, mut acc_skipped), (_seen, records, skipped)| {
            for entry in records {
                if acc_seen.insert((entry.key.clone(), entry.text_hash)) {
                    acc_records.push(entry);
                }
            }
            acc_skipped += skipped;
            (acc_seen, acc_records, acc_skipped)
        }
    );

    if total_skipped > 0 {
        error!("Total de líneas ignoradas por error de parseo: {}", total_skipped);
    }

    let mut records = Vec::new();
    for entry in final_records {
        records.push((entry.key, entry.line));
    }

    // Ordenar por clave usando orden natural (numérico consciente)
    records.sort_by(|a, b| natord::compare(&a.0, &b.0));

    let consolidated_name = format!("consolidated_{}.jsonl", timestamp);
    let consolidated_path = dir.join(consolidated_name);
    let mut output_file = File::create(&consolidated_path)?;
    for (_, line) in records {
        writeln!(output_file, "{}", line)?;
    }

    println!("Archivo consolidado generado: {:?}", consolidated_path);
    Ok(consolidated_path)
}
