use crate::parser::{PredictionLine, TrafficType};
use serde::Serialize;

#[derive(Serialize, Debug, Clone)]
pub struct TokenReport {
    pub key: String,
    pub model: String,
    #[serde(rename = "Traffic Type")]
    pub traffic_type: TrafficType,
    #[serde(rename = "Prompt Image Tokens")]
    pub prompt_image_tokens: u32,
    #[serde(rename = "Prompt Text Tokens")]
    pub prompt_text_tokens: u32,
    #[serde(rename = "Prompt Tokens")]
    pub prompt_tokens: u32,
    #[serde(rename = "Candidate Tokens")]
    pub candidate_tokens: u32,
    #[serde(rename = "Thoughts Tokens")]
    pub thoughts_tokens: u32,
    #[serde(rename = "Total Tokens")]
    pub total_tokens: u32,
}

impl From<PredictionLine> for TokenReport {
    fn from(pred: PredictionLine) -> Self {
        let mut image_tokens = 0;
        let mut text_tokens = 0;
        for detail in &pred.response.usage_metadata.prompt_tokens_details {
            match detail.modality.as_str() {
                "IMAGE" => image_tokens += detail.token_count,
                "TEXT" => text_tokens += detail.token_count,
                _ => {}
            }
        }

        TokenReport {
            key: pred.key,
            model: pred.response.model_version,
            traffic_type: pred.response.usage_metadata.traffic_type,
            prompt_image_tokens: image_tokens,
            prompt_text_tokens: text_tokens,
            prompt_tokens: pred.response.usage_metadata.prompt_token_count,
            candidate_tokens: pred.response.usage_metadata.candidates_token_count,
            thoughts_tokens: pred.response.usage_metadata.thoughts_token_count,
            total_tokens: pred.response.usage_metadata.total_token_count,
        }
    }
}

#[derive(Clone)]
pub struct ProcessedRecord {
    pub key: String,
    pub text: String,
    pub report: TokenReport,
}
