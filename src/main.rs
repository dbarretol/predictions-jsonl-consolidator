use chrono::Local;
use clap::Parser;
use fxhash::FxHashSet;
use serde::Deserialize;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

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

    let input_dir = input_path.parent().unwrap_or(Path::new("."));
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

    // Escribir cabecera CSV
    csv_writer.write_record(&[
        "key",
        "model",
        "Traffic Type",
        "Prompt Image Tokens",
        "Prompt Text Tokens",
        "Prompt Tokens",
        "Candidate Tokens",
        "Thoughts Tokens",
        "Total Tokens",
    ])?;

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
                csv_writer.write_record(&[
                    &pred.key,
                    &pred.response.model_version,
                    &pred.response.usage_metadata.traffic_type,
                    &image_tokens.to_string(),
                    &text_tokens.to_string(),
                    &pred.response.usage_metadata.prompt_token_count.to_string(),
                    &pred.response.usage_metadata.candidates_token_count.to_string(),
                    &pred.response.usage_metadata.thoughts_token_count.to_string(),
                    &pred.response.usage_metadata.total_token_count.to_string(),
                ])?;
            }
            Err(e) => {
                eprintln!(
                    "Error parseando línea {}: {}\nContenido: {}",
                    idx + 1,
                    e,
                    &line[..line.len().min(120)]
                );
            }
        }
    }
    
    csv_writer.flush()?;
    println!("Archivos generados en: {:?}", extract_dir);

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

    println!("Consolidando {} archivos JSONL...", jsonl_files.len());
    
    let mut seen_hashes: FxHashSet<u64> = FxHashSet::default();
    let mut unique_lines = Vec::new();
    for file_path in &jsonl_files {
        let file = File::open(file_path)?;
        let reader = BufReader::new(file);
        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            
            let hash = fxhash::hash64(trimmed.as_bytes());
            if seen_hashes.insert(hash) {
                unique_lines.push(trimmed.to_string());
            }
        }
    }

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
