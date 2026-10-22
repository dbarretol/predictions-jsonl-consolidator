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

#[derive(Deserialize, Debug, Serialize, Default, Clone, PartialEq)]
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

#[cfg(test)]
mod tests {
    use super::*;

    fn part(text: Option<&str>) -> Part {
        Part {
            text: text.map(String::from),
        }
    }

    #[test]
    fn extract_text_returns_text_when_present() {
        let pred = PredictionLine {
            key: "k".into(),
            response: Response {
                candidates: Some(vec![Candidate {
                    content: Content {
                        parts: Some(vec![part(Some("Hola"))]),
                    },
                }]),
                model_version: "model-a".into(),
                usage_metadata: serde_json::from_str("{}").unwrap(),
            },
        };
        assert_eq!(pred.extract_text(), Some("Hola"));
    }

    #[test]
    fn extract_text_none_when_no_candidates() {
        let pred = PredictionLine {
            key: "k".into(),
            response: Response {
                candidates: None,
                model_version: "model-a".into(),
                usage_metadata: serde_json::from_str("{}").unwrap(),
            },
        };
        assert_eq!(pred.extract_text(), None);
    }

    #[test]
    fn extract_text_none_when_candidates_empty() {
        let pred = PredictionLine {
            key: "k".into(),
            response: Response {
                candidates: Some(vec![]),
                model_version: "model-a".into(),
                usage_metadata: serde_json::from_str("{}").unwrap(),
            },
        };
        assert_eq!(pred.extract_text(), None);
    }

    #[test]
    fn extract_text_none_when_parts_missing() {
        let pred = PredictionLine {
            key: "k".into(),
            response: Response {
                candidates: Some(vec![Candidate {
                    content: Content { parts: None },
                }]),
                model_version: "model-a".into(),
                usage_metadata: serde_json::from_str("{}").unwrap(),
            },
        };
        assert_eq!(pred.extract_text(), None);
    }

    #[test]
    fn extract_text_none_when_part_text_is_null() {
        let pred = PredictionLine {
            key: "k".into(),
            response: Response {
                candidates: Some(vec![Candidate {
                    content: Content {
                        parts: Some(vec![part(None)]),
                    },
                }]),
                model_version: "model-a".into(),
                usage_metadata: serde_json::from_str("{}").unwrap(),
            },
        };
        assert_eq!(pred.extract_text(), None);
    }

    #[test]
    fn traffic_type_default_is_regular() {
        assert_eq!(TrafficType::default(), TrafficType::Regular);
    }

    #[test]
    fn traffic_type_aliases_deserialize_correctly() {
        let cases = [
            (r#""regular""#, TrafficType::Regular),
            (r#""REGULAR""#, TrafficType::Regular),
            (r#""ON_DEMAND""#, TrafficType::OnDemand),
            (r#""onDemand""#, TrafficType::OnDemand),
            (r#""on_demand""#, TrafficType::OnDemand),
            (r#""batch""#, TrafficType::Batch),
            (r#""BATCH""#, TrafficType::Batch),
            (r#""algo-nunca-visto""#, TrafficType::Unknown),
        ];
        for (json, expected) in cases {
            let parsed: TrafficType = serde_json::from_str(json).unwrap();
            assert_eq!(parsed, expected, "fallo deserializando {json}");
        }
    }

    #[test]
    fn usage_metadata_defaults_when_fields_absent() {
        let metadata: UsageMetadata = serde_json::from_str("{}").unwrap();
        assert_eq!(metadata.candidates_token_count, 0);
        assert_eq!(metadata.prompt_token_count, 0);
        assert_eq!(metadata.thoughts_token_count, 0);
        assert_eq!(metadata.total_token_count, 0);
        assert!(metadata.prompt_tokens_details.is_empty());
        assert_eq!(metadata.traffic_type, TrafficType::Regular);
    }

    #[test]
    fn usage_metadata_reads_partial_fields() {
        let metadata: UsageMetadata =
            serde_json::from_str(r#"{"promptTokenCount": 42, "trafficType": "batch"}"#).unwrap();
        assert_eq!(metadata.prompt_token_count, 42);
        assert_eq!(metadata.traffic_type, TrafficType::Batch);
        // Los demás campos, ausentes en el JSON, deben tomar su default.
        assert_eq!(metadata.total_token_count, 0);
    }
}
