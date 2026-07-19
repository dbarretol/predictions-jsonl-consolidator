use crate::errors::{AppError, Result};
use crate::models::ProcessedRecord;
use std::path::Path;

pub fn write_csv(path: &Path, records: &[ProcessedRecord]) -> Result<()> {
    // Ruta absoluta (`::csv`) para no ambiguar con este propio módulo `exporters::csv`.
    let mut writer = ::csv::Writer::from_path(path).map_err(|source| {
        AppError::csv(format!("No se pudo crear el archivo CSV: {}", path.display()), source)
    })?;

    for record in records {
        writer.serialize(&record.report).map_err(|source| {
            AppError::csv(format!("No se pudo escribir en {}", path.display()), source)
        })?;
    }

    writer.flush().map_err(|source| {
        AppError::io(format!("No se pudo finalizar el archivo CSV: {}", path.display()), source)
    })?;

    Ok(())
}
