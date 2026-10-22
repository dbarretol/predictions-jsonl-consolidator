use crate::errors::{AppError, Result};
use crate::models::ProcessedRecord;
use std::fs::File;
use std::io::Write;
use std::path::Path;

pub fn write_txt(path: &Path, records: &[ProcessedRecord], include_keys: bool) -> Result<()> {
    let mut file = File::create(path).map_err(|source| {
        AppError::io(
            format!("No se pudo crear el archivo TXT: {}", path.display()),
            source,
        )
    })?;

    for record in records {
        let result = if include_keys {
            writeln!(file, "=== {} ===\n{}\n", record.key, record.text)
        } else {
            writeln!(file, "{}\n", record.text)
        };
        result.map_err(|source| {
            AppError::io(format!("No se pudo escribir en {}", path.display()), source)
        })?;
    }

    Ok(())
}
