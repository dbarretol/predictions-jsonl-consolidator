//! Pipeline de consolidación, una etapa por módulo:
//!
//! 1. `discovery`: qué archivos `.jsonl` procesar (en orden natural).
//! 2. `reader`: leerlos en paralelo y convertir cada línea vía `input::parse_line`.
//! 3. `dedup`: colapsar duplicados exactos y separar los conflictos (lógica pura).
//! 4. `decisions`: aplicar un `decisions.csv` completado a los conflictos.
//!
//! Este módulo orquesta las etapas (`deduplicate_files`) y define los tipos que
//! circulan entre ellas. El pipeline solo lee: no escribe archivos (ni siquiera la
//! plantilla de `decisions.csv`, que devuelve como `PendingConflicts`).

mod decisions;
mod dedup;
mod discovery;
mod reader;

pub use decisions::PendingConflicts;
pub use discovery::find_jsonl_files;

use crate::errors::{AppError, Result};
use crate::models::TokenReport;
use std::path::{Path, PathBuf};

/// Un registro leído junto con su origen: la línea cruda (para el
/// `consolidated_*.jsonl`) y el archivo del que salió (para `decisions.csv`).
#[derive(Debug, Clone)]
pub struct RecordEntry {
    pub key: String,
    pub line: String,
    /// Texto extraído de la respuesta; `None` si el registro no tiene texto (ver
    /// `models::Record`), para distinguir eso de un texto vacío real.
    pub text: Option<String>,
    pub report: TokenReport,
    pub source_file: PathBuf,
}

/// Resultado de `deduplicate_files`.
pub enum Deduplication {
    /// Todos los registros quedaron resueltos (sin conflictos, o conflictos resueltos
    /// con `--decisions`).
    Resolved(DeduplicationOutcome),
    /// Hay conflictos y no se pasó `--decisions`: hace falta que el usuario decida.
    PendingDecisions(PendingConflicts),
}

/// Registros resueltos y contadores para el resumen. Separa los descartes "automáticos"
/// (duplicados exactos, sin ambigüedad) de los descartes explícitos vía
/// `decisions.csv` (`ignorar`), que no son duplicados sino decisiones humanas.
pub struct DeduplicationOutcome {
    pub records: Vec<RecordEntry>,
    pub total_seen: usize,
    pub parse_errors: usize,
    /// Líneas válidas en las que la API informó que la predicción falló (`status`).
    pub failed_predictions: usize,
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
    /// Predicciones que la API marcó como fallidas: no traen respuesta que exportar.
    pub failed_predictions: usize,
    /// Ruta del `consolidated_*.jsonl` generado, si se combinaron varios archivos.
    pub consolidated_file: Option<PathBuf>,
    /// Archivos `.jsonl` que no se pudieron abrir (el consolidado puede quedar incompleto).
    pub unreadable_files: Vec<PathBuf>,
}

/// Lee uno o más archivos JSONL, colapsa duplicados exactos y, si hay conflictos de
/// metadata, los resuelve con `decisions_path` (ver `decisions::apply_decisions`) o
/// los devuelve como `Deduplication::PendingDecisions`. La misma función sirve para un
/// único archivo o para varios, para que ambos modos de entrada compartan la lógica.
/// Los archivos que no se pudieron leer quedan en `unreadable_files` (el consolidado
/// puede quedar incompleto si esa lista no está vacía).
pub fn deduplicate_files(
    files: &[PathBuf],
    decisions_path: Option<&Path>,
    show_progress: bool,
) -> Result<Deduplication> {
    let read = reader::read_files(files, show_progress);

    // Si hubo líneas pero ninguna se pudo interpretar, lo más probable es que haya
    // cambiado el formato del JSONL: se corta acá, antes de generar reportes vacíos
    // que pasarían por un resultado válido.
    if let (0, Some(first_error)) = (read.total_seen, read.first_parse_error) {
        return Err(AppError::NoValidRecords {
            parse_errors: read.parse_errors,
            file: first_error.file,
            line: first_error.line,
            message: first_error.message,
        });
    }

    let dedup::Collapsed {
        mut resolved,
        conflicts,
    } = dedup::collapse_exact_duplicates(read.entries);

    let discarded_by_decision = match decisions_path {
        _ if conflicts.is_empty() => 0,
        Some(path) => decisions::apply_decisions(path, conflicts, &mut resolved)?,
        None => {
            return Ok(Deduplication::PendingDecisions(PendingConflicts::new(
                conflicts,
            )));
        }
    };

    Ok(Deduplication::Resolved(DeduplicationOutcome {
        records: resolved,
        total_seen: read.total_seen,
        parse_errors: read.parse_errors,
        failed_predictions: read.failed_predictions,
        unreadable_files: read.unreadable_files,
        discarded_by_decision,
    }))
}

/// Construye un `RecordEntry` mínimo para los tests unitarios de las etapas.
#[cfg(test)]
fn test_entry(key: &str, text: Option<&str>, model: &str, line: &str) -> RecordEntry {
    RecordEntry {
        key: key.to_string(),
        line: line.to_string(),
        text: text.map(String::from),
        report: TokenReport {
            key: key.to_string(),
            model: Some(model.to_string()),
            ..TokenReport::default()
        },
        source_file: PathBuf::from("test.jsonl"),
    }
}
