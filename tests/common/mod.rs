//! Constructores de líneas JSONL (formato batch de Vertex AI / Gemini) para los tests
//! de integración. Usan `serde_json::json!` en vez de `format!`, así que los textos con
//! comillas o saltos de línea se escapan bien y un cambio de formato se ajusta acá, en
//! un solo lugar.

use serde_json::{Value, json};

/// Predicción exitosa con un texto, `model-a` y 1 token en cada conteo.
pub fn line(key: &str, text: &str) -> String {
    line_full(key, text, "model-a", 1)
}

/// Predicción exitosa con modelo y conteos de tokens elegidos (mismo valor en los tres).
pub fn line_full(key: &str, text: &str, model: &str, tokens: u32) -> String {
    prediction(
        key,
        json!({
            "candidates": [{"content": {"parts": [{"text": text}]}}],
            "modelVersion": model,
            "usageMetadata": {
                "promptTokenCount": tokens,
                "candidatesTokenCount": tokens,
                "totalTokenCount": tokens
            }
        }),
    )
}

/// Predicción exitosa sin texto (sin candidatos).
pub fn line_without_text(key: &str) -> String {
    prediction(
        key,
        json!({
            "candidates": [],
            "modelVersion": "model-a",
            "usageMetadata": {
                "promptTokenCount": 1,
                "candidatesTokenCount": 0,
                "totalTokenCount": 1
            }
        }),
    )
}

/// Línea como la que produce re-ejecutar el mismo prompt: mismo texto y tokens, pero
/// distinto `responseId` y, según `image_first`, distinto orden en `promptTokensDetails`.
pub fn line_rerun(key: &str, text: &str, response_id: &str, image_first: bool) -> String {
    let image = json!({"modality": "IMAGE", "tokenCount": 10});
    let text_detail = json!({"modality": "TEXT", "tokenCount": 5});
    let details = if image_first {
        [image, text_detail]
    } else {
        [text_detail, image]
    };
    prediction(
        key,
        json!({
            "responseId": response_id,
            "candidates": [{"content": {"parts": [{"text": text}]}}],
            "modelVersion": "model-a",
            "usageMetadata": {
                "promptTokenCount": 15,
                "promptTokensDetails": details,
                "candidatesTokenCount": 1,
                "totalTokenCount": 16
            }
        }),
    )
}

/// Predicción que la API marcó como fallida: `status` con el error y sin `response`.
pub fn line_failed(key: &str, status: &str) -> String {
    json!({"key": key, "status": status}).to_string()
}

fn prediction(key: &str, response: Value) -> String {
    json!({"key": key, "status": "", "response": response}).to_string()
}
