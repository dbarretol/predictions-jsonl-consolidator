use serde::{Deserialize, Serialize};

#[derive(Deserialize, Debug, Clone)]
pub struct Part {
    pub text: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Content {
    pub parts: Option<Vec<Part>>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Candidate {
    pub content: Content,
}

#[derive(Deserialize, Debug, Clone)]
pub struct TokenDetail {
    pub modality: String,
    #[serde(rename = "tokenCount")]
    pub token_count: u32,
}

#[derive(Deserialize, Debug, Serialize, Default, Clone)]
pub enum TrafficType {
    #[default]
    #[serde(rename = "regular", alias = "REGULAR")]
    Regular,
    #[serde(rename = "ON_DEMAND", alias = "onDemand", alias = "on_demand")]
    OnDemand,
    #[serde(rename = "batch", alias = "BATCH")]
    Batch,
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize, Debug, Clone)]
pub struct UsageMetadata {
    #[serde(rename = "candidatesTokenCount", default)]
    pub candidates_token_count: u32,
    #[serde(rename = "promptTokenCount", default)]
    pub prompt_token_count: u32,
    #[serde(rename = "promptTokensDetails", default)]
    pub prompt_tokens_details: Vec<TokenDetail>,
    #[serde(rename = "thoughtsTokenCount", default)]
    pub thoughts_token_count: u32,
    #[serde(rename = "totalTokenCount", default)]
    pub total_token_count: u32,
    #[serde(rename = "trafficType", default)]
    pub traffic_type: TrafficType,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Response {
    pub candidates: Option<Vec<Candidate>>,
    #[serde(rename = "modelVersion")]
    pub model_version: String,
    #[serde(rename = "usageMetadata")]
    pub usage_metadata: UsageMetadata,
}

#[derive(Deserialize, Debug, Clone)]
pub struct PredictionLine {
    pub key: String,
    pub response: Response,
}

impl PredictionLine {
    pub fn extract_text(&self) -> Option<&str> {
        self.response
            .candidates
            .as_ref()?
            .first()?
            .content
            .parts
            .as_ref()?
            .first()?
            .text
            .as_deref()
    }
}
