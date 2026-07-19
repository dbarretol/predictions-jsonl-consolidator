use anyhow::{bail, Context, Result};
use chrono::Local;
use clap::Parser;
use fxhash::FxHashMap;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use tracing::error;

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
#[command(
    author,
    version,
    about,
    long_about = None,
    after_help = "Ejemplos:\n  \
        predictions-jsonl-consolidator predicciones.jsonl\n  \
        predictions-jsonl-consolidator --dir carpeta_con_jsonl\n  \
        predictions-jsonl-consolidator --dir carpeta --out-csv reporte.csv\n  \
        predictions-jsonl-consolidator    (sin argumentos: usa la carpeta actual)"
)]
struct Args {
    /// Archivo JSONL de entrada (opcional si se usa --dir)
    #[arg(conflicts_with = "dir")]
    input: Option<PathBuf>,

    /// Carpeta para buscar y consolidar archivos JSONL
    #[arg(long, short, conflicts_with = "input")]
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

    /// Archivo decisions.csv ya completado, para resolver conflictos de deduplicación
    /// (mismo key+texto con distinta metadata) detectados en una corrida previa
    #[arg(long)]
    decisions: Option<PathBuf>,
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
    line: String,
    text: String,
    source_file: PathBuf,
    model: String,
    prompt_tokens: u32,
    candidate_tokens: u32,
    total_tokens: u32,
}

impl RecordEntry {
    /// Identificador estable del candidato entre corridas (hash de la línea cruda),
    /// usado para enlazar `decisions.csv` con el candidato que describe.
    fn candidate_id(&self) -> String {
        format!("{:016x}", fxhash::hash64(self.line.as_bytes()))
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct DecisionRow {
    key: String,
    candidate_id: String,
    source_file: String,
    model: String,
    #[serde(rename = "Prompt Tokens")]
    prompt_tokens: u32,
    #[serde(rename = "Candidate Tokens")]
    candidate_tokens: u32,
    #[serde(rename = "Total Tokens")]
    total_tokens: u32,
    text_preview: String,
    /// "mantener" para conservar este candidato, "ignorar" (o vacío) para descartarlo.
    decision: String,
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
    let decisions_path = args.decisions.clone();
    let (input_path, summary) = if let Some(dir) = &args.dir {
        process_directory(dir, timestamp, decisions_path.as_deref())?
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
        process_directory(&current_dir, timestamp, decisions_path.as_deref())?
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

fn process_directory(
    dir: &Path,
    timestamp: &str,
    decisions_path: Option<&Path>,
) -> Result<(PathBuf, ConsolidationSummary)> {
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

    println!("Consolidando {} archivos JSONL...", jsonl_files.len());

    // Usamos fold/reduce para mejor paralelismo en lugar de Mutex compartido.
    // Agrupamos por (key, hash del texto): dos líneas con la misma key y el mismo
    // texto son duplicados; si además difieren en metadata (modelo/tokens), el grupo
    // queda con más de una línea distinta y se trata como un conflicto a resolver.
    type Group = FxHashMap<(String, u64), Vec<RecordEntry>>;
    let (groups, total_skipped, total_seen) = jsonl_files.par_iter().fold(
        || (Group::default(), 0usize, 0usize),
        |(mut groups, mut skipped, mut total), file_path| {
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
                                let text = pred.extract_text().unwrap_or("").to_string();
                                let text_hash = fxhash::hash64(text.as_bytes());
                                total += 1;

                                groups.entry((pred.key.clone(), text_hash)).or_default().push(RecordEntry {
                                    key: pred.key,
                                    line: trimmed.to_string(),
                                    text,
                                    source_file: file_path.clone(),
                                    model: pred.response.model_version,
                                    prompt_tokens: pred.response.usage_metadata.prompt_token_count,
                                    candidate_tokens: pred.response.usage_metadata.candidates_token_count,
                                    total_tokens: pred.response.usage_metadata.total_token_count,
                                });
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
            (groups, skipped, total)
        }
    ).reduce(
        || (Group::default(), 0usize, 0usize),
        |(mut acc_groups, mut acc_skipped, mut acc_total), (groups, skipped, total)| {
            for (key, mut entries) in groups {
                acc_groups.entry(key).or_default().append(&mut entries);
            }
            acc_skipped += skipped;
            acc_total += total;
            (acc_groups, acc_skipped, acc_total)
        }
    );

    if total_skipped > 0 {
        error!("Total de líneas ignoradas por error de parseo: {}", total_skipped);
    }

    // Dentro de cada grupo, colapsamos líneas crudas idénticas (duplicado exacto,
    // sin conflicto) y dejamos solo variantes distintas por metadata.
    let mut resolved_records: Vec<RecordEntry> = Vec::new();
    let mut conflict_groups: Vec<Vec<RecordEntry>> = Vec::new();
    for (_key, entries) in groups {
        let mut distinct: Vec<RecordEntry> = Vec::new();
        for entry in entries {
            if !distinct.iter().any(|e: &RecordEntry| e.line == entry.line) {
                distinct.push(entry);
            }
        }
        if distinct.len() == 1 {
            resolved_records.push(distinct.into_iter().next().unwrap());
        } else {
            conflict_groups.push(distinct);
        }
    }

    if !conflict_groups.is_empty() {
        resolve_conflicts(dir, decisions_path, &conflict_groups, &mut resolved_records)?;
    }

    let duplicates_removed = total_seen.saturating_sub(resolved_records.len());

    let mut records = Vec::new();
    for entry in resolved_records {
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

/// Trunca y aplana un texto para usarlo como vista previa en `decisions.csv`.
fn text_preview(text: &str) -> String {
    let flattened: String = text.chars().map(|c| if c.is_whitespace() { ' ' } else { c }).collect();
    let truncated: String = flattened.chars().take(100).collect();
    if flattened.chars().count() > 100 {
        format!("{}...", truncated)
    } else {
        truncated
    }
}

/// Resuelve los grupos de candidatos con la misma key+texto pero distinta metadata.
///
/// Sin `decisions_path`: escribe `decisions.csv` con los diffs y detiene la ejecución
/// para que el usuario indique, por candidato, "mantener" o "ignorar". Con
/// `decisions_path`: aplica esas decisiones y agrega los candidatos conservados a
/// `resolved_records`; si algún grupo queda sin ningún candidato marcado "mantener",
/// detiene la ejecución señalando qué keys faltan por resolver.
fn resolve_conflicts(
    dir: &Path,
    decisions_path: Option<&Path>,
    conflict_groups: &[Vec<RecordEntry>],
    resolved_records: &mut Vec<RecordEntry>,
) -> Result<()> {
    let Some(decisions_path) = decisions_path else {
        let out_path = dir.join("decisions.csv");
        let mut writer = csv::Writer::from_path(&out_path).with_context(|| {
            format!("No se pudo crear el archivo de decisiones: {}", out_path.display())
        })?;
        for group in conflict_groups {
            for entry in group {
                writer
                    .serialize(DecisionRow {
                        key: entry.key.clone(),
                        candidate_id: entry.candidate_id(),
                        source_file: entry
                            .source_file
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("")
                            .to_string(),
                        model: entry.model.clone(),
                        prompt_tokens: entry.prompt_tokens,
                        candidate_tokens: entry.candidate_tokens,
                        total_tokens: entry.total_tokens,
                        text_preview: text_preview(&entry.text),
                        decision: String::new(),
                    })
                    .with_context(|| format!("No se pudo escribir en {}", out_path.display()))?;
            }
        }
        writer
            .flush()
            .with_context(|| format!("No se pudo finalizar {}", out_path.display()))?;

        bail!(
            "Se detectaron {} key(s) con candidatos en conflicto (mismo key+texto, distinta metadata).\n\
             Complete la columna 'decision' en {} con 'mantener' (puede marcar más de un candidato por key) \
             o 'ignorar'/vacío para descartar, y vuelva a ejecutar agregando --decisions \"{}\"",
            conflict_groups.len(),
            out_path.display(),
            out_path.display()
        );
    };

    let mut reader = csv::Reader::from_path(decisions_path).with_context(|| {
        format!("No se pudo abrir el archivo de decisiones: {}", decisions_path.display())
    })?;
    let mut decisions: FxHashMap<String, String> = FxHashMap::default();
    for row in reader.deserialize::<DecisionRow>() {
        let row = row.with_context(|| {
            format!("No se pudo leer una fila de {}", decisions_path.display())
        })?;
        decisions.insert(row.candidate_id, row.decision.trim().to_lowercase());
    }

    let mut unresolved_keys = Vec::new();
    for group in conflict_groups {
        let mut kept: Vec<RecordEntry> = group
            .iter()
            .filter(|entry| decisions.get(&entry.candidate_id()).map(|d| d.as_str()) == Some("mantener"))
            .cloned()
            .collect();
        if kept.is_empty() {
            unresolved_keys.push(group[0].key.clone());
        } else {
            resolved_records.append(&mut kept);
        }
    }

    if !unresolved_keys.is_empty() {
        bail!(
            "El archivo de decisiones {} no tiene ningún candidato marcado 'mantener' para estas key(s): {}. \
             Complételo y vuelva a intentar.",
            decisions_path.display(),
            unresolved_keys.join(", ")
        );
    }

    Ok(())
}
