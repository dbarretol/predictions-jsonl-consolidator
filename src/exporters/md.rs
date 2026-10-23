use super::display_text;
use crate::errors::{AppError, Result};
use crate::models::Record;
use std::fs::File;
use std::io::Write;
use std::path::Path;

pub fn write_md(path: &Path, records: &[Record]) -> Result<()> {
    let mut file = File::create(path).map_err(|source| {
        AppError::io(
            format!("No se pudo crear el archivo Markdown: {}", path.display()),
            source,
        )
    })?;

    for record in records {
        writeln!(
            file,
            "## Clave: {}\n\n{}\n",
            record.key,
            display_text(record)
        )
        .map_err(|source| {
            AppError::io(format!("No se pudo escribir en {}", path.display()), source)
        })?;
    }

    Ok(())
}
