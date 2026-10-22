//! Adaptador para la salida de predicciones batch de Vertex AI / Gemini: una línea por
//! predicción con `key`, `status`, `request`, `response` y `processed_time`.
//!
//! Criterio ante cambios de formato: solo `key` es imprescindible. Todo lo demás es
//! opcional y, si falta, se reporta como vacío/cero en vez de descartar la línea, para
//! no perder el texto por un campo de metadata renombrado.

use super::{LineOutcome, ParseError};
use crate::models::{Record, TokenReport};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct PredictionLine {
    key: String,
    /// `""` (o ausente) si la predicción tuvo éxito; el mensaje de error si falló.
    #[serde(default)]
    status: Option<Value>,
    response: Option<Response>,
}

#[derive(Deserialize)]
struct Response {
    candidates: Option<Vec<Candidate>>,
    #[serde(rename = "modelVersion")]
    model_version: Option<String>,
    #[serde(rename = "usageMetadata", default)]
    usage_metadata: UsageMetadata,
}

#[derive(Deserialize)]
struct Candidate {
    content: Option<Content>,
    #[serde(rename = "finishReason")]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct Content {
    parts: Option<Vec<Part>>,
}

#[derive(Deserialize)]
struct Part {
    text: Option<String>,
    /// `true` en las partes de razonamiento (con `includeThoughts`): no son la respuesta.
    thought: Option<bool>,
}

/// Los conteos en cero se omiten en el JSON de Vertex (protobuf), por eso todos los
/// campos tienen valor por defecto.
#[derive(Deserialize, Default)]
#[serde(default)]
struct UsageMetadata {
    #[serde(rename = "promptTokenCount")]
    prompt_token_count: u32,
    #[serde(rename = "promptTokensDetails")]
    prompt_tokens_details: Vec<TokenDetail>,
    #[serde(rename = "candidatesTokenCount")]
    candidates_token_count: u32,
    #[serde(rename = "thoughtsTokenCount")]
    thoughts_token_count: u32,
    #[serde(rename = "totalTokenCount")]
    total_token_count: u32,
    #[serde(rename = "trafficType")]
    traffic_type: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct TokenDetail {
    modality: String,
    #[serde(rename = "tokenCount")]
    token_count: u32,
}

pub(super) fn parse_line(line: &str) -> Result<LineOutcome, ParseError> {
    let pred: PredictionLine = serde_json::from_str(line)?;

    if let Some(status) = failure_status(pred.status) {
        return Ok(LineOutcome::FailedPrediction {
            key: pred.key,
            status,
        });
    }

    let response = pred
        .response
        .ok_or_else(|| ParseError::new("falta `response` y `status` no indica un error"))?;

    Ok(LineOutcome::Record(to_record(pred.key, response)))
}

/// Mensaje de error de la predicción, o `None` si `status` indica éxito (ausente,
/// `null`, `""` u objeto vacío).
fn failure_status(status: Option<Value>) -> Option<String> {
    match status? {
        Value::Null => None,
        Value::String(s) if s.trim().is_empty() => None,
        Value::String(s) => Some(s),
        Value::Object(map) if map.is_empty() => None,
        other => Some(other.to_string()),
    }
}

fn to_record(key: String, response: Response) -> Record {
    let first_candidate = response.candidates.and_then(|c| c.into_iter().next());
    let finish_reason = first_candidate
        .as_ref()
        .and_then(|c| c.finish_reason.clone());
    let text = first_candidate.and_then(extract_text);

    let usage = response.usage_metadata;
    let (mut image_tokens, mut text_tokens, mut other_tokens) = (0, 0, 0);
    for detail in &usage.prompt_tokens_details {
        match detail.modality.as_str() {
            "IMAGE" => image_tokens += detail.token_count,
            "TEXT" => text_tokens += detail.token_count,
            _ => other_tokens += detail.token_count,
        }
    }

    Record {
        report: TokenReport {
            key: key.clone(),
            model: response.model_version,
            traffic_type: usage.traffic_type,
            prompt_image_tokens: image_tokens,
            prompt_text_tokens: text_tokens,
            prompt_other_tokens: other_tokens,
            prompt_tokens: usage.prompt_token_count,
            candidate_tokens: usage.candidates_token_count,
            thoughts_tokens: usage.thoughts_token_count,
            total_tokens: usage.total_token_count,
            finish_reason,
        },
        key,
        text,
    }
}

/// Texto de la respuesta: concatena las partes con texto del candidato, salteando las
/// de razonamiento. `None` si no hay ninguna (ej. corte por `MAX_TOKENS`).
fn extract_text(candidate: Candidate) -> Option<String> {
    let texts: Vec<String> = candidate
        .content?
        .parts?
        .into_iter()
        .filter(|part| part.thought != Some(true))
        .filter_map(|part| part.text)
        .collect();
    if texts.is_empty() {
        None
    } else {
        Some(texts.concat())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(json: &str) -> Record {
        match parse_line(json) {
            Ok(LineOutcome::Record(record)) => record,
            Ok(LineOutcome::FailedPrediction { status, .. }) => {
                panic!("se esperaba un registro, se obtuvo predicción fallida: {status}")
            }
            Err(e) => panic!("se esperaba un registro, se obtuvo error: {e}"),
        }
    }

    fn with_parts(parts: &str) -> String {
        format!(
            r#"{{"key": "k", "response": {{"candidates": [{{"content": {{"parts": {parts}}}}}]}}}}"#
        )
    }

    #[test]
    fn extracts_text_when_present() {
        let rec = record(&with_parts(r#"[{"text": "Hola"}]"#));
        assert_eq!(rec.text.as_deref(), Some("Hola"));
    }

    #[test]
    fn text_is_none_when_response_has_no_text() {
        let cases = [
            r#"{"key": "k", "response": {}}"#.to_string(),
            r#"{"key": "k", "response": {"candidates": []}}"#.to_string(),
            r#"{"key": "k", "response": {"candidates": [{"content": {}}]}}"#.to_string(),
            with_parts("[]"),
            with_parts(r#"[{"text": null}]"#),
        ];
        for json in cases {
            assert_eq!(record(&json).text, None, "caso: {json}");
        }
    }

    #[test]
    fn skips_thought_parts_and_concatenates_the_rest() {
        let rec = record(&with_parts(
            r#"[{"text": "pensando...", "thought": true}, {"text": "Hola "}, {"text": "mundo", "thoughtSignature": "x"}]"#,
        ));
        assert_eq!(rec.text.as_deref(), Some("Hola mundo"));
    }

    #[test]
    fn missing_metadata_does_not_discard_the_line() {
        // Solo `key` es imprescindible: sin `modelVersion` ni `usageMetadata` el texto
        // se conserva y la metadata queda vacía/en cero.
        let rec = record(&with_parts(r#"[{"text": "Hola"}]"#));
        assert_eq!(rec.text.as_deref(), Some("Hola"));
        assert_eq!(rec.report.model, None);
        assert_eq!(rec.report.traffic_type, None);
        assert_eq!(rec.report.total_tokens, 0);
    }

    #[test]
    fn missing_key_is_a_parse_error() {
        let err = parse_line(r#"{"response": {}}"#)
            .err()
            .expect("debía fallar");
        assert!(err.to_string().contains("missing field `key`"), "{err}");
        assert!(!err.to_string().contains("at line"), "{err}");
    }

    #[test]
    fn missing_response_without_failure_status_is_a_parse_error() {
        for json in [r#"{"key": "k"}"#, r#"{"key": "k", "status": ""}"#] {
            let err = parse_line(json).err().expect("debía fallar");
            assert!(err.to_string().contains("falta `response`"), "{err}");
        }
    }

    #[test]
    fn non_empty_status_is_a_failed_prediction() {
        let outcome = parse_line(r#"{"key": "k", "status": "Bad Request: imagen inválida"}"#);
        match outcome {
            Ok(LineOutcome::FailedPrediction { key, status }) => {
                assert_eq!(key, "k");
                assert_eq!(status, "Bad Request: imagen inválida");
            }
            _ => panic!("se esperaba una predicción fallida"),
        }
    }

    #[test]
    fn empty_status_is_a_successful_prediction() {
        let rec = record(r#"{"key": "k", "status": "", "response": {}}"#);
        assert_eq!(rec.key, "k");
    }

    #[test]
    fn keeps_finish_reason_and_traffic_type_verbatim() {
        let rec = record(
            r#"{"key": "k", "response": {"candidates": [{"finishReason": "MAX_TOKENS"}], "usageMetadata": {"trafficType": "PROVISIONED_THROUGHPUT"}}}"#,
        );
        assert_eq!(rec.report.finish_reason.as_deref(), Some("MAX_TOKENS"));
        assert_eq!(
            rec.report.traffic_type.as_deref(),
            Some("PROVISIONED_THROUGHPUT")
        );
    }

    #[test]
    fn sums_prompt_tokens_by_modality_including_unknown_ones() {
        let rec = record(
            r#"{"key": "k", "response": {"modelVersion": "m", "usageMetadata": {
                "promptTokenCount": 20, "candidatesTokenCount": 3, "thoughtsTokenCount": 2, "totalTokenCount": 25,
                "promptTokensDetails": [
                    {"modality": "TEXT", "tokenCount": 5},
                    {"modality": "IMAGE", "tokenCount": 10},
                    {"modality": "AUDIO", "tokenCount": 4},
                    {"modality": "TEXT"}
                ]}}}"#,
        );
        let report = rec.report;
        assert_eq!(report.model.as_deref(), Some("m"));
        assert_eq!(report.prompt_text_tokens, 5);
        assert_eq!(report.prompt_image_tokens, 10);
        assert_eq!(report.prompt_other_tokens, 4);
        assert_eq!(report.prompt_tokens, 20);
        assert_eq!(report.candidate_tokens, 3);
        assert_eq!(report.thoughts_tokens, 2);
        assert_eq!(report.total_tokens, 25);
    }

    /// Fixtures reales del repo: si el adaptador deja de entender el formato con el que
    /// fueron generados, este test lo detecta antes que cualquier usuario.
    #[test]
    fn all_repository_fixtures_parse_without_errors() {
        let fixtures = [
            include_str!("../../example/predictions.jsonl"),
            include_str!("../../example/prediction_dummy.jsonl"),
            include_str!("../../test_data/file1.jsonl"),
            include_str!("../../test_data/file2.jsonl"),
        ];
        for (fixture, content) in fixtures.iter().enumerate() {
            for (index, line) in content.lines().enumerate() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Err(e) = parse_line(line) {
                    panic!("fixture #{fixture}, línea {}: {e}", index + 1);
                }
            }
        }
    }
}
