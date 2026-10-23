use super::RecordEntry;
use crate::input::{self, LineOutcome};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use std::fs::File;
use std::io::{BufRead, BufReader, ErrorKind};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tracing::{debug, error};

/// Primera línea que no se pudo interpretar, para explicar la causa si al final
/// ninguna línea resultó válida (típicamente, un cambio de formato del JSONL).
pub(super) struct ParseFailure {
    pub file: PathBuf,
    pub line: usize,
    pub message: String,
}

/// Resultado de leer los archivos. También es el acumulador de la lectura paralela
/// (`fold`/`reduce` de rayon): cada hilo llena el suyo y luego se combinan con `merge`.
#[derive(Default)]
pub(super) struct ReadOutcome {
    /// Registros válidos en orden de lectura (archivo por archivo, línea por línea).
    pub entries: Vec<RecordEntry>,
    pub parse_errors: usize,
    pub failed_predictions: usize,
    pub unreadable_files: Vec<PathBuf>,
    pub first_parse_error: Option<ParseFailure>,
}

impl ReadOutcome {
    fn record_parse_error(&mut self, file: &Path, line: usize, message: String) {
        error!(
            "No se pudo parsear la línea {} de {}: {}",
            line,
            file.display(),
            message
        );
        self.parse_errors += 1;
        if self.first_parse_error.is_none() {
            self.first_parse_error = Some(ParseFailure {
                file: file.to_path_buf(),
                line,
                message,
            });
        }
    }

    // `reduce` sobre un iterador indexado combina tramos contiguos en el orden de
    // `files`, así que `self` siempre es el tramo anterior: concatenar `entries` y
    // conservar su error mantiene el orden de lectura y "el primero" en ese orden.
    fn merge(mut self, other: Self) -> Self {
        self.entries.extend(other.entries);
        self.parse_errors += other.parse_errors;
        self.failed_predictions += other.failed_predictions;
        self.unreadable_files.extend(other.unreadable_files);
        self.first_parse_error = self.first_parse_error.or(other.first_parse_error);
        self
    }
}

/// Lee los archivos en paralelo (fold/reduce, sin Mutex compartido) y convierte cada
/// línea con contenido mediante `input::parse_line`. Nunca falla: los problemas de
/// lectura o de formato quedan contados en el resultado para que quien llama decida.
pub(super) fn read_files(files: &[PathBuf], show_progress: bool) -> ReadOutcome {
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

    let outcome = files
        .par_iter()
        .fold(ReadOutcome::default, |mut acc, file_path| {
            read_file(file_path, &progress, &mut acc);
            acc
        })
        .reduce(ReadOutcome::default, ReadOutcome::merge);

    progress.finish_and_clear();

    let elapsed = started_at.elapsed();
    let valid_records = outcome.entries.len();
    let throughput = valid_records as f64 / elapsed.as_secs_f64().max(0.001);
    debug!(
        total_files = files.len(),
        valid_records,
        elapsed_ms = elapsed.as_millis(),
        throughput_lineas_por_seg = format!("{:.1}", throughput),
        "Consolidación completada"
    );

    if outcome.parse_errors > 0 {
        error!(
            "Total de líneas ignoradas por error de parseo: {}",
            outcome.parse_errors
        );
    }
    if !outcome.unreadable_files.is_empty() {
        error!(
            "Total de archivos que no se pudieron abrir: {}",
            outcome.unreadable_files.len()
        );
    }

    outcome
}

fn read_file(file_path: &Path, progress: &ProgressBar, acc: &mut ReadOutcome) {
    let file = match File::open(file_path) {
        Ok(file) => file,
        Err(e) => {
            error!("No se pudo abrir archivo {:?}: {}", file_path, e);
            acc.unreadable_files.push(file_path.to_path_buf());
            return;
        }
    };

    let file_started_at = Instant::now();
    let mut file_lines = 0usize;
    for (index, line) in BufReader::new(file).lines().enumerate() {
        // Numeración humana (desde 1) contando también las líneas en blanco, para que
        // coincida con lo que muestra un editor.
        let line_number = index + 1;
        match line {
            Ok(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    // No cuenta hacia la barra de progreso ni hacia ningún total: no es
                    // una línea con contenido.
                    continue;
                }
                progress.inc(1);
                file_lines += 1;
                process_line(file_path, line_number, trimmed, acc);
            }
            Err(e) if e.kind() == ErrorKind::InvalidData => {
                // Bytes que no son UTF-8 válido (ej. archivo en Latin-1): `read_line` ya
                // consumió esa línea antes de fallar la conversión, así que la próxima
                // iteración avanza sola. Se cuenta como error de parseo en vez de
                // perderse en silencio.
                progress.inc(1);
                acc.record_parse_error(file_path, line_number, e.to_string());
            }
            Err(e) => {
                // Error de E/S real (no de encoding): a diferencia del caso anterior, el
                // stream puede no haber avanzado, así que reintentar podría repetir el
                // mismo error indefinidamente (ej. sector dañado). Se corta la lectura
                // de este archivo y se cuenta como ilegible/incompleto en vez de
                // arriesgar un loop infinito.
                error!(
                    "Error de E/S leyendo {:?}, se corta la lectura del archivo: {}",
                    file_path, e
                );
                acc.unreadable_files.push(file_path.to_path_buf());
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

/// Parsea la línea una sola vez: el registro completo (texto + `Usage`) queda listo desde
/// acá, para no tener que volver a parsear el JSON al generar los reportes finales. El
/// formato concreto solo lo conoce `input`.
fn process_line(file_path: &Path, line_number: usize, line: &str, acc: &mut ReadOutcome) {
    match input::parse_line(line) {
        Ok(LineOutcome::Record(record)) => {
            acc.entries.push(RecordEntry {
                record,
                line: line.to_string(),
                source_file: file_path.to_path_buf(),
            });
        }
        Ok(LineOutcome::FailedPrediction { key, status }) => {
            error!(
                "Predicción fallida (key {}) en la línea {} de {}: {}",
                key,
                line_number,
                file_path.display(),
                status
            );
            acc.failed_predictions += 1;
        }
        Err(e) => acc.record_parse_error(file_path, line_number, e.to_string()),
    }
}
