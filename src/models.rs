use serde::Serialize;

/// Una predicción ya interpretada, independiente del formato del JSONL de origen
/// (la conversión vive en `input`).
pub struct Record {
    pub key: String,
    /// `None` si la respuesta no trae texto (ej. corte por `MAX_TOKENS`), para
    /// distinguirlo de un texto vacío real.
    pub text: Option<String>,
    pub report: TokenReport,
}

/// Metadata de uso de una predicción. Es a la vez la fila del CSV de tokens y parte
/// del criterio de duplicado exacto (ver `consolidator::deduplicate_files`).
///
/// Los campos de texto son `Option`: si el formato de origen deja de traerlos, la
/// celda queda vacía en vez de descartarse la línea.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct TokenReport {
    pub key: String,
    pub model: Option<String>,
    /// Valor tal cual lo informa la API (ej. `ON_DEMAND`, `PROVISIONED_THROUGHPUT`).
    #[serde(rename = "Traffic Type")]
    pub traffic_type: Option<String>,
    #[serde(rename = "Prompt Image Tokens")]
    pub prompt_image_tokens: u32,
    #[serde(rename = "Prompt Text Tokens")]
    pub prompt_text_tokens: u32,
    /// Tokens del prompt en otras modalidades (audio, video, documento...).
    #[serde(rename = "Prompt Other Tokens")]
    pub prompt_other_tokens: u32,
    #[serde(rename = "Prompt Tokens")]
    pub prompt_tokens: u32,
    #[serde(rename = "Candidate Tokens")]
    pub candidate_tokens: u32,
    #[serde(rename = "Thoughts Tokens")]
    pub thoughts_tokens: u32,
    #[serde(rename = "Total Tokens")]
    pub total_tokens: u32,
    /// Motivo de fin de la respuesta (ej. `STOP`, `MAX_TOKENS`): explica por qué un
    /// registro puede no tener texto.
    #[serde(rename = "Finish Reason")]
    pub finish_reason: Option<String>,
}

#[derive(Clone)]
pub struct ProcessedRecord {
    pub key: String,
    pub text: String,
    pub report: TokenReport,
}
