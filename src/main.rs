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
struct Response {
    candidates: Option<Vec<Candidate>>,
}

#[derive(Deserialize, Debug)]
struct PredictionLine {
    key: String,
    response: Response,
}

fn main() -> io::Result<()> {
    let args = Args::parse();

    let file = File::open(&args.input)?;
    let reader = BufReader::new(file);

    let mut predictions: Vec<(String, String)> = Vec::new();

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
                predictions.push((pred.key, text));
            }
            Err(e) => {
                eprintln!("Error al parsear línea: {}", e);
            }
        }
    }

    // Ordenar alfabéticamente por la clave
    predictions.sort_by(|a, b| a.0.cmp(&b.0));

    // Generar archivo TXT
    let mut txt_file = File::create(&args.out_txt)?;
    for (_, text) in &predictions {
        writeln!(txt_file, "{}\n", text)?;
    }
    println!("Archivo TXT generado: {:?}", args.out_txt);

    // Generar archivo MD
    let mut md_file = File::create(&args.out_md)?;
    for (key, text) in &predictions {
        writeln!(md_file, "## Clave: {}\n\n{}\n", key, text)?;
    }
    println!("Archivo Markdown generado: {:?}", args.out_md);

    Ok(())
}
