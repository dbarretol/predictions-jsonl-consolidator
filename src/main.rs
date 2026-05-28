use chrono::Local;
use clap::Parser;
use fxhash::FxHashSet;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tracing::{error, info};

// ... (other structs)

#[derive(Serialize)]
struct TokenReport {
    key: String,
    model: String,
    #[serde(rename = "Traffic Type")]
    traffic_type: String,
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
    traffic_type: String,
}

#[derive(Deserialize, Debug)]
struct Response {
    candidates: Option<Vec<Candidate>>,
    #[serde(rename = "modelVersion")]
    model_version: String,
    #[serde(rename = "usageMetadata")]
    usage_metadata: UsageMetadata,
}

#[derive(Deserialize, Debug)]
struct PredictionLine {
    key: String,
    response: Response,
}

#[derive(Deserialize)]
struct KeyOnly {
    key: String,
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
                
                // Procesar tokens
                let mut image_tokens = 0;
                let mut text_tokens = 0;
                for detail in &pred.response.usage_metadata.prompt_tokens_details {
                    match detail.modality.as_str() {
                        "IMAGE" => image_tokens += detail.token_count,
                        "TEXT" => text_tokens += detail.token_count,
                        _ => {}
                    }
                }

                // Streaming a los archivos de salida
                writeln!(txt_file, "{}\n", text)?;
                writeln!(md_file, "## Clave: {}\n\n{}\n", pred.key, text)?;
                csv_writer.serialize(TokenReport {
                    key: pred.key,
                    model: pred.response.model_version,
                    traffic_type: pred.response.usage_metadata.traffic_type,
                    prompt_image_tokens: image_tokens,
                    prompt_text_tokens: text_tokens,
                    prompt_tokens: pred.response.usage_metadata.prompt_token_count,
                    candidate_tokens: pred.response.usage_metadata.candidates_token_count,
                    thoughts_tokens: pred.response.usage_metadata.thoughts_token_count,
                    total_tokens: pred.response.usage_metadata.total_token_count,
                })?;
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
            jsonl_files.push(path);
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
    
    let seen_hashes = Mutex::new(FxHashSet::<u64>::default());
    let unique_lines = Mutex::new(Vec::new());

    jsonl_files.par_iter().for_each(|file_path| {
        if let Ok(file) = File::open(file_path) {
            let reader = BufReader::new(file);
            for line in reader.lines() {
                if let Ok(line) = line {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    
                    let hash = fxhash::hash64(trimmed.as_bytes());
                    let mut hashes = seen_hashes.lock().unwrap();
                    if hashes.insert(hash) {
                        unique_lines.lock().unwrap().push(trimmed.to_string());
                    }
                }
            }
        }
    });

    let unique_lines = unique_lines.into_inner().unwrap();
    let mut records = Vec::new();
    for line in unique_lines {
        if let Ok(key_only) = serde_json::from_str::<KeyOnly>(&line) {
            records.push((key_only.key, line));
        } else {
            records.push(("".to_string(), line));
        }
    }

    // Ordenar por clave
    records.sort_by(|a, b| a.0.cmp(&b.0));

    let consolidated_name = format!("consolidated_{}.jsonl", timestamp);
    let consolidated_path = dir.join(consolidated_name);
    let mut output_file = File::create(&consolidated_path)?;
    for (_, line) in records {
        writeln!(output_file, "{}", line)?;
    }

    println!("Archivo consolidado generado: {:?}", consolidated_path);
    Ok(consolidated_path)
}
