use crate::models::ProcessedRecord;
use anyhow::{Context, Result};
use std::fs::File;
use std::io::Write;
use std::path::Path;

pub fn write_md(path: &Path, records: &[ProcessedRecord]) -> Result<()> {
    let mut file = File::create(path)
        .with_context(|| format!("No se pudo crear el archivo Markdown: {}", path.display()))?;

    for record in records {
        writeln!(file, "## Clave: {}\n\n{}\n", record.key, record.text)
            .with_context(|| format!("No se pudo escribir en {}", path.display()))?;
    }

    Ok(())
}
