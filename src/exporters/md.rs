use crate::errors::{AppError, Result};
use crate::models::ProcessedRecord;
use std::fs::File;
use std::io::Write;
use std::path::Path;

pub fn write_md(path: &Path, records: &[ProcessedRecord]) -> Result<()> {
    let mut file = File::create(path).map_err(|source| {
        AppError::io(format!("No se pudo crear el archivo Markdown: {}", path.display()), source)
    })?;

    for record in records {
        writeln!(file, "## Clave: {}\n\n{}\n", record.key, record.text).map_err(|source| {
            AppError::io(format!("No se pudo escribir en {}", path.display()), source)
        })?;
    }

    Ok(())
}
