use crate::errors::{AppError, Result};
use crate::models::TokenReport;
use crate::parser::PredictionLine;
use fxhash::FxHashMap;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::BufRead;
use std::io::BufReader;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tracing::debug;
use tracing::error;
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct RecordEntry {
    pub key: String,
    pub line: String,
    /// Texto extraído de la respuesta; `None` si el registro no tiene texto (ver
    /// `PredictionLine::extract_text`), para distinguir eso de un texto vacío real.
    pub text: Option<String>,
    pub report: TokenReport,
    pub source_file: PathBuf,
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

/// Resultado de deduplicar uno o más archivos. Separa los descartes "automáticos"
/// (duplicados exactos, sin ambigüedad) de los descartes explícitos vía
/// `decisions.csv` (`ignorar`), que no son duplicados sino decisiones humanas.
pub struct DeduplicationOutcome {
    pub records: Vec<RecordEntry>,
    pub total_seen: usize,
    pub parse_errors: usize,
    pub unreadable_files: Vec<PathBuf>,
    pub discarded_by_decision: usize,
}

#[derive(Default)]
pub struct ConsolidationSummary {
    pub files_read: usize,
    /// Duplicados exactos colapsados automáticamente (misma key+texto+metadata).
    pub duplicates_removed: usize,
    /// Candidatos descartados por decisión explícita en `decisions.csv` (`ignorar`).
    /// No son duplicados: son variantes distintas que el usuario eligió no conservar.
    pub discarded_by_decision: usize,
    pub parse_errors: usize,
    /// Ruta del `consolidated_*.jsonl` generado, si se combinaron varios archivos.
    pub consolidated_file: Option<PathBuf>,
    /// Archivos `.jsonl` que no se pudieron abrir (el consolidado puede quedar incompleto).
    pub unreadable_files: Vec<PathBuf>,
}

/// `true` si `path` es un `.jsonl` candidato a consolidarse (excluye resultados de
/// una consolidación previa, que ya llevan el prefijo `consolidated_`).
fn is_candidate_jsonl(path: &Path) -> bool {
    if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
        return false;
    }
    let filename = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    !filename.starts_with("consolidated_")
}

/// Lista los archivos `.jsonl` a consolidar dentro de una carpeta. Por defecto solo
/// mira el nivel superior; con `recursive` también recorre subcarpetas.
pub fn find_jsonl_files(dir: &Path, recursive: bool) -> Result<Vec<PathBuf>> {
    let mut jsonl_files = Vec::new();

    if recursive {
        for entry in WalkDir::new(dir) {
            let entry = entry.map_err(|source| {
                AppError::io(
                    format!("No se pudo recorrer la carpeta: {}", dir.display()),
                    source.into(),
                )
            })?;
            let path = entry.path();
            if path.is_file() && is_candidate_jsonl(path) {
                jsonl_files.push(path.to_path_buf());
            }
        }
    } else {
        let entries = fs::read_dir(dir).map_err(|source| {
            AppError::io(
                format!("No se pudo leer la carpeta: {}", dir.display()),
                source,
            )
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| {
                AppError::io(
                    format!(
                        "No se pudo leer una entrada de la carpeta: {}",
                        dir.display()
                    ),
                    source,
                )
            })?;
            let path = entry.path();
            if path.is_file() && is_candidate_jsonl(&path) {
                jsonl_files.push(path);
            }
        }
    }

    if jsonl_files.is_empty() {
        return Err(AppError::NoJsonlFiles {
            dir: dir.to_path_buf(),
        });
    }

    Ok(jsonl_files)
}

/// Agrupa las líneas de uno o más archivos JSONL por (key, hash del texto de la
/// respuesta), colapsa duplicados exactos y resuelve conflictos de metadata (ver
/// `resolve_conflicts`). La misma función deduplica tanto un único archivo como
/// varios, para que ambos modos de entrada compartan la lógica de deduplicación.
/// Devuelve los registros resueltos junto con el total de líneas vistas, las
/// líneas descartadas por error de parseo, y los archivos que no se pudieron abrir
/// (el consolidado puede quedar incompleto si esta lista no está vacía).
pub fn deduplicate_files(
    files: &[PathBuf],
    decisions_dir: &Path,
    decisions_path: Option<&Path>,
    show_progress: bool,
) -> Result<DeduplicationOutcome> {
    let started_at = Instant::now();

    let progress = if show_progress {
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::with_template("{spinner:.green} {pos} líneas procesadas ({elapsed})")
                .unwrap(),
        );
        pb.enable_steady_tick(Duration::from_millis(100));
        pb
    } else {
        ProgressBar::hidden()
    };

    // Usamos fold/reduce para mejor paralelismo en lugar de Mutex compartido.
    // Agrupamos por (key, hash del texto): dos líneas con la misma key y el mismo
    // texto son duplicados; si además difieren en metadata (modelo/tokens), el grupo
    // queda con más de una línea distinta y se trata como un conflicto a resolver.
    type Group = FxHashMap<(String, u64), Vec<RecordEntry>>;
    let (groups, total_skipped, total_seen, unreadable_files) = files.par_iter().fold(
        || (Group::default(), 0usize, 0usize, Vec::<PathBuf>::new()),
        |(mut groups, mut skipped, mut total, mut unreadable), file_path| {
            match File::open(file_path) {
                Ok(file) => {
                    let file_started_at = Instant::now();
                    let mut file_lines = 0usize;
                    let reader = BufReader::new(file);
                    for line in reader.lines() {
                        match line {
                            Ok(line) => {
                                let trimmed = line.trim();
                                if trimmed.is_empty() {
                                    // No cuenta hacia la barra de progreso ni hacia
                                    // ningún total: no es una línea con contenido.
                                    continue;
                                }
                                progress.inc(1);
                                file_lines += 1;

                                // Parseamos la línea una sola vez: el registro completo
                                // (`TokenReport`) queda listo desde acá, para no tener que
                                // volver a parsear el JSON al generar los reportes finales.
                                if let Ok(pred) = serde_json::from_str::<PredictionLine>(trimmed) {
                                    let text = pred.extract_text().map(|s| s.to_string());
                                    let text_hash = fxhash::hash64(text.as_deref().unwrap_or("").as_bytes());
                                    total += 1;

                                    let key = pred.key.clone();
                                    let report = TokenReport::from(pred);

                                    groups.entry((key.clone(), text_hash)).or_default().push(RecordEntry {
                                        key,
                                        line: trimmed.to_string(),
                                        text,
                                        report,
                                        source_file: file_path.clone(),
                                    });
                                } else {
                                    error!("No se pudo parsear registro en archivo {:?}: {}", file_path, trimmed.chars().take(50).collect::<String>());
                                    skipped += 1;
                                }
                            }
                            Err(e) if e.kind() == ErrorKind::InvalidData => {
                                // Bytes que no son UTF-8 válido (ej. archivo en Latin-1):
                                // `read_line` ya consumió esa línea antes de fallar la
                                // conversión, así que la próxima iteración avanza sola.
                                // Se cuenta como error de parseo en vez de perderse en silencio.
                                progress.inc(1);
                                error!("No se pudo leer una línea de {:?}: {}", file_path, e);
                                skipped += 1;
                            }
                            Err(e) => {
                                // Error de E/S real (no de encoding): a diferencia del caso
                                // anterior, el stream puede no haber avanzado, así que
                                // reintentar podría repetir el mismo error indefinidamente
                                // (ej. sector dañado). Se corta la lectura de este archivo y
                                // se cuenta como ilegible/incompleto en vez de arriesgar un
                                // loop infinito.
                                error!("Error de E/S leyendo {:?}, se corta la lectura del archivo: {}", file_path, e);
                                unreadable.push(file_path.clone());
                                break;
                            }
                        }
                    }
                    debug!(
                        file = %file_path.display(),
                        lines = file_lines,
                        elapsed_ms = file_started_at.elapsed().as_millis(),
                        "Archivo procesado"
                    );
                }
                Err(e) => {
                    error!("No se pudo abrir archivo {:?}: {}", file_path, e);
                    unreadable.push(file_path.clone());
                }
            }
            (groups, skipped, total, unreadable)
        }
    ).reduce(
        || (Group::default(), 0usize, 0usize, Vec::<PathBuf>::new()),
        |(mut acc_groups, mut acc_skipped, mut acc_total, mut acc_unreadable), (groups, skipped, total, mut unreadable)| {
            for (key, mut entries) in groups {
                acc_groups.entry(key).or_default().append(&mut entries);
            }
            acc_skipped += skipped;
            acc_total += total;
            acc_unreadable.append(&mut unreadable);
            (acc_groups, acc_skipped, acc_total, acc_unreadable)
        }
    );

    progress.finish_and_clear();

    let elapsed = started_at.elapsed();
    let throughput = total_seen as f64 / elapsed.as_secs_f64().max(0.001);
    debug!(
        total_files = files.len(),
        total_lines = total_seen,
        elapsed_ms = elapsed.as_millis(),
        throughput_lineas_por_seg = format!("{:.1}", throughput),
        "Consolidación completada"
    );

    if total_skipped > 0 {
        error!(
            "Total de líneas ignoradas por error de parseo: {}",
            total_skipped
        );
    }
    if !unreadable_files.is_empty() {
        error!(
            "Total de archivos que no se pudieron abrir: {}",
            unreadable_files.len()
        );
    }

    // Dentro de cada grupo, colapsamos duplicados exactos (sin conflicto) y dejamos
    // solo variantes distintas por metadata. "Exacto" se mide sobre lo que se exporta
    // (texto + `TokenReport`), no sobre la línea cruda: re-ejecutar el mismo prompt
    // cambia campos que no se reportan (`responseId`, `createTime`, `processed_time`,
    // la URI de la imagen, el orden de `promptTokensDetails`) y eso no debe pedir una
    // decisión al usuario, ya que en `decisions.csv` los candidatos se verían idénticos.
    let mut resolved_records: Vec<RecordEntry> = Vec::new();
    let mut conflict_groups: Vec<Vec<RecordEntry>> = Vec::new();
    for (_key, entries) in groups {
        let mut distinct: Vec<RecordEntry> = Vec::new();
        for entry in entries {
            if !distinct
                .iter()
                .any(|e: &RecordEntry| e.text == entry.text && e.report == entry.report)
            {
                distinct.push(entry);
            }
        }
        if distinct.len() == 1 {
            resolved_records.push(distinct.into_iter().next().unwrap());
        } else {
            conflict_groups.push(distinct);
        }
    }

    let discarded_by_decision = if !conflict_groups.is_empty() {
        resolve_conflicts(
            decisions_dir,
            decisions_path,
            conflict_groups,
            &mut resolved_records,
        )?
    } else {
        0
    };

    Ok(DeduplicationOutcome {
        records: resolved_records,
        total_seen,
        parse_errors: total_skipped,
        unreadable_files,
        discarded_by_decision,
    })
}

/// Trunca y aplana un texto para usarlo como vista previa en `decisions.csv`.
fn text_preview(text: &str) -> String {
    let flattened: String = text
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect();
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
///
/// Toma `conflict_groups` por valor (no por referencia) porque la rama de resolución
/// mueve los `RecordEntry` conservados directamente a `resolved_records`, sin
/// clonarlos: son la única llamadora (`deduplicate_files`) no los necesita después.
fn resolve_conflicts(
    dir: &Path,
    decisions_path: Option<&Path>,
    conflict_groups: Vec<Vec<RecordEntry>>,
    resolved_records: &mut Vec<RecordEntry>,
) -> Result<usize> {
    let Some(decisions_path) = decisions_path else {
        let out_path = dir.join("decisions.csv");
        let mut writer = csv::Writer::from_path(&out_path).map_err(|source| {
            AppError::csv(
                format!(
                    "No se pudo crear el archivo de decisiones: {}",
                    out_path.display()
                ),
                source,
            )
        })?;
        for group in &conflict_groups {
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
                        model: entry.report.model.clone(),
                        prompt_tokens: entry.report.prompt_tokens,
                        candidate_tokens: entry.report.candidate_tokens,
                        total_tokens: entry.report.total_tokens,
                        text_preview: text_preview(entry.text.as_deref().unwrap_or("")),
                        decision: String::new(),
                    })
                    .map_err(|source| {
                        AppError::csv(
                            format!("No se pudo escribir en {}", out_path.display()),
                            source,
                        )
                    })?;
            }
        }
        writer.flush().map_err(|source| {
            AppError::io(
                format!("No se pudo finalizar {}", out_path.display()),
                source,
            )
        })?;

        return Err(AppError::ConflictsPendingResolution {
            count: conflict_groups.len(),
            decisions_path: out_path,
        });
    };

    let mut reader = csv::Reader::from_path(decisions_path).map_err(|source| {
        AppError::csv(
            format!(
                "No se pudo abrir el archivo de decisiones: {}",
                decisions_path.display()
            ),
            source,
        )
    })?;
    let mut decisions: FxHashMap<String, String> = FxHashMap::default();
    for row in reader.deserialize::<DecisionRow>() {
        let row = row.map_err(|source| {
            AppError::csv(
                format!("No se pudo leer una fila de {}", decisions_path.display()),
                source,
            )
        })?;
        decisions.insert(row.candidate_id, row.decision.trim().to_lowercase());
    }

    let mut unresolved_keys = Vec::new();
    let mut discarded_by_decision = 0usize;
    for group in conflict_groups {
        // Clonamos solo la key (por si el grupo queda sin resolver); los candidatos
        // conservados se mueven directo a `resolved_records`, sin clonar cada uno.
        let group_key = group[0].key.clone();
        let group_size = group.len();
        let mut kept: Vec<RecordEntry> = group
            .into_iter()
            .filter(|entry| {
                decisions.get(&entry.candidate_id()).map(|d| d.as_str()) == Some("mantener")
            })
            .collect();
        if kept.is_empty() {
            unresolved_keys.push(group_key);
        } else {
            discarded_by_decision += group_size - kept.len();
            resolved_records.append(&mut kept);
        }
    }

    if !unresolved_keys.is_empty() {
        return Err(AppError::UnresolvedDecisions {
            decisions_path: decisions_path.to_path_buf(),
            keys: unresolved_keys,
        });
    }

    Ok(discarded_by_decision)
}
