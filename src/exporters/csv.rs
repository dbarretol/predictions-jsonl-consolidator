use crate::errors::{AppError, Result};
use crate::models::ProcessedRecord;
use serde::Serialize;
use std::path::Path;

/// Una fila del reporte de tokens. El orden de los campos es el orden de las columnas
/// y los `rename` son sus encabezados: es el único lugar donde se define el esquema
/// del CSV, así que agregar o renombrar columnas no afecta la deduplicación.
#[derive(Serialize)]
struct CsvRow<'a> {
    key: &'a str,
    model: Option<&'a str>,
    #[serde(rename = "Traffic Type")]
    traffic_type: Option<&'a str>,
    #[serde(rename = "Prompt Image Tokens")]
    prompt_image_tokens: u32,
    #[serde(rename = "Prompt Text Tokens")]
    prompt_text_tokens: u32,
    #[serde(rename = "Prompt Other Tokens")]
    prompt_other_tokens: u32,
    #[serde(rename = "Prompt Tokens")]
    prompt_tokens: u32,
    #[serde(rename = "Candidate Tokens")]
    candidate_tokens: u32,
    #[serde(rename = "Thoughts Tokens")]
    thoughts_tokens: u32,
    #[serde(rename = "Total Tokens")]
    total_tokens: u32,
    #[serde(rename = "Finish Reason")]
    finish_reason: Option<&'a str>,
}

impl<'a> From<&'a ProcessedRecord> for CsvRow<'a> {
    fn from(record: &'a ProcessedRecord) -> Self {
        let usage = &record.usage;
        CsvRow {
            key: &record.key,
            model: usage.model.as_deref(),
            traffic_type: usage.traffic_type.as_deref(),
            prompt_image_tokens: usage.prompt_image_tokens,
            prompt_text_tokens: usage.prompt_text_tokens,
            prompt_other_tokens: usage.prompt_other_tokens,
            prompt_tokens: usage.prompt_tokens,
            candidate_tokens: usage.candidate_tokens,
            thoughts_tokens: usage.thoughts_tokens,
            total_tokens: usage.total_tokens,
            finish_reason: usage.finish_reason.as_deref(),
        }
    }
}

pub fn write_csv(path: &Path, records: &[ProcessedRecord]) -> Result<()> {
    // Ruta absoluta (`::csv`) para no ambiguar con este propio módulo `exporters::csv`.
    let mut writer = ::csv::Writer::from_path(path).map_err(|source| {
        AppError::csv(
            format!("No se pudo crear el archivo CSV: {}", path.display()),
            source,
        )
    })?;

    for record in records {
        writer.serialize(CsvRow::from(record)).map_err(|source| {
            AppError::csv(format!("No se pudo escribir en {}", path.display()), source)
        })?;
    }

    writer.flush().map_err(|source| {
        AppError::io(
            format!("No se pudo finalizar el archivo CSV: {}", path.display()),
            source,
        )
    })?;

    Ok(())
}
