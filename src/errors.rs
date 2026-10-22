use std::io;
use std::path::PathBuf;
use thiserror::Error;

/// Alias de conveniencia para las funciones de la lógica de negocio (deduplicación,
/// resolución de conflictos, exportadores), que devuelven `AppError` en vez de
/// `anyhow::Result`. El binario (`main.rs`) las propaga con `?` hacia su propio
/// `anyhow::Result`; la conversión es automática porque `AppError` implementa
/// `std::error::Error`.
pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("No se encontraron archivos .jsonl en {}", dir.display())]
    NoJsonlFiles { dir: PathBuf },

    #[error(
        "Se detectaron {count} key(s) con candidatos en conflicto (mismo key+texto, distinta metadata).\n\
         Complete la columna 'decision' en {path} con 'mantener' (puede marcar más de un candidato por key) \
         o 'ignorar'/vacío para descartar, y vuelva a ejecutar agregando --decisions \"{path}\"",
        path = decisions_path.display()
    )]
    ConflictsPendingResolution {
        count: usize,
        decisions_path: PathBuf,
    },

    #[error(
        "El archivo de decisiones {} no tiene ningún candidato marcado 'mantener' para estas key(s): {}. \
         Complételo y vuelva a intentar.",
        decisions_path.display(), keys.join(", ")
    )]
    UnresolvedDecisions {
        decisions_path: PathBuf,
        keys: Vec<String>,
    },

    /// Falla de E/S genérica (crear/leer/escribir un archivo o directorio); `message`
    /// ya describe qué operación falló, `source` conserva el error original de Rust.
    #[error("{message}")]
    Io {
        message: String,
        #[source]
        source: io::Error,
    },

    /// Falla al leer/escribir un archivo CSV (formato inválido, fila corrupta, etc.).
    #[error("{message}")]
    Csv {
        message: String,
        #[source]
        source: csv::Error,
    },
}

impl AppError {
    pub fn io(message: impl Into<String>, source: io::Error) -> Self {
        AppError::Io {
            message: message.into(),
            source,
        }
    }

    pub fn csv(message: impl Into<String>, source: csv::Error) -> Self {
        AppError::Csv {
            message: message.into(),
            source,
        }
    }
}
