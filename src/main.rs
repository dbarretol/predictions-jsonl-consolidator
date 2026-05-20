use clap::Parser;
use serde::Deserialize;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Archivo JSONL de entrada
    input: PathBuf,

    /// Archivo de salida TXT
    #[arg(long, default_value = "consolidado.txt")]
    out_txt: PathBuf,

    /// Archivo de salida Markdown
    #[arg(long, default_value = "consolidado.md")]
    out_md: PathBuf,

    /// Archivo de salida CSV para metadata
    #[arg(long, default_value = "reporte_tokens.csv")]
    out_csv: PathBuf,
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

struct ConsolidatedData {
    key: String,
    text: String,
    model: String,
    traffic_type: String,
    prompt_image_tokens: u32,
    prompt_text_tokens: u32,
    prompt_tokens: u32,
    candidate_tokens: u32,
    thoughts_tokens: u32,
    total_tokens: u32,
}

fn main() -> io::Result<()> {
    let args = Args::parse();

    let file = File::open(&args.input)?;
    let reader = BufReader::new(file);

    let mut data_list: Vec<ConsolidatedData> = Vec::new();

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        match serde_json::from_str::<PredictionLine>(&line) {
            Ok(pred) => {
                let text = pred
                    .response
                    .candidates
                    .as_ref()
                    .and_then(|c| c.get(0))
                    .and_then(|c| c.content.parts.as_ref())
                    .and_then(|p| p.get(0))
                    .and_then(|p| p.text.clone())
                    .unwrap_or_else(|| {
                        format!("> [Aviso: No se encontró texto para la clave `{}`]", pred.key)
                    });

                let mut image_tokens = 0;
                let mut text_tokens = 0;
                for detail in &pred.response.usage_metadata.prompt_tokens_details {
                    match detail.modality.as_str() {
                        "IMAGE" => image_tokens += detail.token_count,
                        "TEXT" => text_tokens += detail.token_count,
                        _ => {}
                    }
                }

                data_list.push(ConsolidatedData {
                    key: pred.key,
                    text,
                    model: pred.response.model_version,
                    traffic_type: pred.response.usage_metadata.traffic_type,
                    prompt_image_tokens: image_tokens,
                    prompt_text_tokens: text_tokens,
                    prompt_tokens: pred.response.usage_metadata.prompt_token_count,
                    candidate_tokens: pred.response.usage_metadata.candidates_token_count,
                    thoughts_tokens: pred.response.usage_metadata.thoughts_token_count,
                    total_tokens: pred.response.usage_metadata.total_token_count,
                });
            }
            Err(e) => {
                eprintln!("Error al parsear línea: {}", e);
            }
        }
    }

    // Ordenar alfabéticamente por la clave
    data_list.sort_by(|a, b| a.key.cmp(&b.key));

    // Generar archivo TXT
    let mut txt_file = File::create(&args.out_txt)?;
    for data in &data_list {
        writeln!(txt_file, "{}\n", data.text)?;
    }
    println!("Archivo TXT generado: {:?}", args.out_txt);

    // Generar archivo MD
    let mut md_file = File::create(&args.out_md)?;
    for data in &data_list {
        writeln!(md_file, "## Clave: {}\n\n{}\n", data.key, data.text)?;
    }
    println!("Archivo Markdown generado: {:?}", args.out_md);

    // Generar archivo CSV
    let mut csv_writer = csv::Writer::from_path(&args.out_csv)?;
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

    for data in &data_list {
        csv_writer.write_record(&[
            &data.key,
            &data.model,
            &data.traffic_type,
            &data.prompt_image_tokens.to_string(),
            &data.prompt_text_tokens.to_string(),
            &data.prompt_tokens.to_string(),
            &data.candidate_tokens.to_string(),
            &data.thoughts_tokens.to_string(),
            &data.total_tokens.to_string(),
        ])?;
    }
    csv_writer.flush()?;
    println!("Archivo CSV generado: {:?}", args.out_csv);

    Ok(())
}
