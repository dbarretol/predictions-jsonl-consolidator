//! Frontera con el formato de entrada: es el único lugar del programa que sabe cómo
//! luce una línea del JSONL. El resto trabaja con `Record` (ver `models.rs`), así que
//! un cambio de formato se absorbe acá sin tocar la deduplicación ni los exportadores.
//!
//! Para soportar un formato nuevo: agregar un adaptador junto a `vertex_batch` que
//! produzca `LineOutcome` y despachar hacia él desde `parse_line`.

mod vertex_batch;

use crate::models::Record;
use std::fmt;

/// Resultado de interpretar una línea con contenido.
pub enum LineOutcome {
    /// Predicción exitosa, lista para deduplicar y exportar.
    Record(Record),
    /// La API informó que la predicción falló. No es un error de formato: la línea es
    /// válida, pero no trae una respuesta que exportar.
    FailedPrediction { key: String, status: String },
}

/// La línea no se pudo interpretar con el formato esperado.
#[derive(Debug)]
pub struct ParseError {
    message: String,
}

impl ParseError {
    fn new(message: impl Into<String>) -> Self {
        ParseError {
            message: message.into(),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// Describe un error de serde sin su sufijo "at line 1 column N": cada línea del
/// JSONL se parsea por separado, así que esa "línea 1" confundiría con el número de
/// línea real del archivo, que quien llama ya informa aparte.
impl From<serde_json::Error> for ParseError {
    fn from(e: serde_json::Error) -> Self {
        let full = e.to_string();
        match full.rsplit_once(" at line ") {
            Some((message, _)) => ParseError::new(format!("{} (columna {})", message, e.column())),
            None => ParseError::new(full),
        }
    }
}

/// Interpreta una línea (ya sin espacios alrededor y no vacía) del JSONL.
pub fn parse_line(line: &str) -> Result<LineOutcome, ParseError> {
    vertex_batch::parse_line(line)
}
