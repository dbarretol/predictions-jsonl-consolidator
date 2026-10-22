use serde::Serialize;

/// Una predicción ya interpretada, independiente del formato del JSONL de origen
/// (la conversión vive en `input`).
pub struct Record {
    pub key: String,
    /// `None` si la respuesta no trae texto (ej. corte por `MAX_TOKENS`), para
    /// distinguirlo de un texto vacío real.
    pub text: Option<String>,
    pub usage: Usage,
}

/// Metadata de uso de una predicción. Junto con el texto define qué es un duplicado
/// exacto (ver `pipeline::dedup`) y el `candidate_id` de `decisions.csv`.
///
/// Es un dato del dominio, no una fila de reporte: las columnas del CSV de tokens
/// (nombres, orden, columnas derivadas) se definen en `exporters::csv`, así que
/// cambiarlas no altera la deduplicación. Los campos de texto son `Option`: si el
/// formato de origen deja de traerlos, quedan vacíos en vez de descartarse la línea.
///
/// `Serialize` existe solo para obtener una representación canónica en `candidate_id`.
#[derive(Serialize, Debug, Clone, Default, PartialEq)]
pub struct Usage {
    pub model: Option<String>,
    /// Valor tal cual lo informa la API (ej. `ON_DEMAND`, `PROVISIONED_THROUGHPUT`).
    pub traffic_type: Option<String>,
    pub prompt_image_tokens: u32,
    pub prompt_text_tokens: u32,
    /// Tokens del prompt en otras modalidades (audio, video, documento...).
    pub prompt_other_tokens: u32,
    pub prompt_tokens: u32,
    pub candidate_tokens: u32,
    pub thoughts_tokens: u32,
    pub total_tokens: u32,
    /// Motivo de fin de la respuesta (ej. `STOP`, `MAX_TOKENS`): explica por qué un
    /// registro puede no tener texto.
    pub finish_reason: Option<String>,
}

#[derive(Clone)]
pub struct ProcessedRecord {
    pub key: String,
    pub text: String,
    pub usage: Usage,
}
