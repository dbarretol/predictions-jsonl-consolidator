use crate::models::ProcessedRecord;
use anyhow::{Context, Result};
use std::path::Path;

pub fn write_csv(path: &Path, records: &[ProcessedRecord]) -> Result<()> {
    // Ruta absoluta (`::csv`) para no ambiguar con este propio módulo `exporters::csv`.
    let mut writer = ::csv::Writer::from_path(path)
        .with_context(|| format!("No se pudo crear el archivo CSV: {}", path.display()))?;

    for record in records {
        writer
            .serialize(&record.report)
            .with_context(|| format!("No se pudo escribir en {}", path.display()))?;
    }

    writer
        .flush()
        .with_context(|| format!("No se pudo finalizar el archivo CSV: {}", path.display()))?;

    Ok(())
}
