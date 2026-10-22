use super::RecordEntry;
use crate::errors::{AppError, Result};
use fxhash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize, Clone)]
struct DecisionRow {
    key: String,
    candidate_id: String,
    source_file: String,
    model: String,
    #[serde(rename = "Prompt Tokens")]
    prompt_tokens: u32,
    #[serde(rename = "Candidate Tokens")]
    candidate_tokens: u32,
    #[serde(rename = "Total Tokens")]
    total_tokens: u32,
    text_preview: String,
    /// "mantener" para conservar este candidato, "ignorar" (o vacío) para descartarlo.
    decision: String,
}

/// Identificador estable del candidato entre corridas, usado para enlazar
/// `decisions.csv` con el candidato que describe. Se deriva de lo que se exporta
/// (texto + `TokenReport`, que incluye la key), el mismo criterio que define un
/// duplicado exacto: así no depende de cuál de varias líneas equivalentes quedó como
/// representante del grupo (eso varía con el orden de lectura) ni de campos que no se
/// reportan, como `responseId`.
fn candidate_id(entry: &RecordEntry) -> String {
    // JSON con orden de campos fijo: una representación canónica del candidato.
    let canonical = serde_json::to_string(&(&entry.text, &entry.report))
        .expect("texto y TokenReport siempre se pueden serializar");
    format!("{:016x}", fxhash::hash64(canonical.as_bytes()))
}

/// Trunca y aplana un texto para usarlo como vista previa en `decisions.csv`.
fn text_preview(text: &str) -> String {
    let flattened: String = text
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect();
    let truncated: String = flattened.chars().take(100).collect();
    if flattened.chars().count() > 100 {
        format!("{}...", truncated)
    } else {
        truncated
    }
}

/// Resuelve los grupos de candidatos con la misma key+texto pero distinta metadata.
///
/// Sin `decisions_path`: escribe `decisions.csv` con los diffs y detiene la ejecución
/// para que el usuario indique, por candidato, "mantener" o "ignorar". Con
/// `decisions_path`: aplica esas decisiones y agrega los candidatos conservados a
/// `resolved_records`; si algún grupo queda sin ningún candidato marcado "mantener",
/// detiene la ejecución señalando qué keys faltan por resolver.
///
/// Toma `conflict_groups` por valor (no por referencia) porque la rama de resolución
/// mueve los `RecordEntry` conservados directamente a `resolved_records`, sin
/// clonarlos: la única llamadora (`deduplicate_files`) no los necesita después.
pub(super) fn resolve_conflicts(
    dir: &Path,
    decisions_path: Option<&Path>,
    conflict_groups: Vec<Vec<RecordEntry>>,
    resolved_records: &mut Vec<RecordEntry>,
) -> Result<usize> {
    let Some(decisions_path) = decisions_path else {
        let out_path = dir.join("decisions.csv");
        write_decisions_template(&out_path, &conflict_groups)?;
        return Err(AppError::ConflictsPendingResolution {
            count: conflict_groups.len(),
            decisions_path: out_path,
        });
    };

    let decisions = read_decisions(decisions_path)?;

    let mut unresolved_keys = Vec::new();
    let mut discarded_by_decision = 0usize;
    for group in conflict_groups {
        // Clonamos solo la key (por si el grupo queda sin resolver); los candidatos
        // conservados se mueven directo a `resolved_records`, sin clonar cada uno.
        let group_key = group[0].key.clone();
        let group_size = group.len();
        let mut kept: Vec<RecordEntry> = group
            .into_iter()
            .filter(|entry| {
                decisions.get(&candidate_id(entry)).map(|d| d.as_str()) == Some("mantener")
            })
            .collect();
        if kept.is_empty() {
            unresolved_keys.push(group_key);
        } else {
            discarded_by_decision += group_size - kept.len();
            resolved_records.append(&mut kept);
        }
    }

    if !unresolved_keys.is_empty() {
        return Err(AppError::UnresolvedDecisions {
            decisions_path: decisions_path.to_path_buf(),
            keys: unresolved_keys,
        });
    }

    Ok(discarded_by_decision)
}

fn write_decisions_template(out_path: &Path, conflict_groups: &[Vec<RecordEntry>]) -> Result<()> {
    let mut writer = csv::Writer::from_path(out_path).map_err(|source| {
        AppError::csv(
            format!(
                "No se pudo crear el archivo de decisiones: {}",
                out_path.display()
            ),
            source,
        )
    })?;
    for entry in conflict_groups.iter().flatten() {
        writer
            .serialize(DecisionRow {
                key: entry.key.clone(),
                candidate_id: candidate_id(entry),
                source_file: entry
                    .source_file
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string(),
                model: entry.report.model.clone().unwrap_or_default(),
                prompt_tokens: entry.report.prompt_tokens,
                candidate_tokens: entry.report.candidate_tokens,
                total_tokens: entry.report.total_tokens,
                text_preview: text_preview(entry.text.as_deref().unwrap_or("")),
                decision: String::new(),
            })
            .map_err(|source| {
                AppError::csv(
                    format!("No se pudo escribir en {}", out_path.display()),
                    source,
                )
            })?;
    }
    writer.flush().map_err(|source| {
        AppError::io(
            format!("No se pudo finalizar {}", out_path.display()),
            source,
        )
    })
}

/// Lee `decisions.csv` completado: `candidate_id` → decisión normalizada.
fn read_decisions(decisions_path: &Path) -> Result<FxHashMap<String, String>> {
    let mut reader = csv::Reader::from_path(decisions_path).map_err(|source| {
        AppError::csv(
            format!(
                "No se pudo abrir el archivo de decisiones: {}",
                decisions_path.display()
            ),
            source,
        )
    })?;
    let mut decisions = FxHashMap::default();
    for row in reader.deserialize::<DecisionRow>() {
        let row = row.map_err(|source| {
            AppError::csv(
                format!("No se pudo leer una fila de {}", decisions_path.display()),
                source,
            )
        })?;
        decisions.insert(row.candidate_id, row.decision.trim().to_lowercase());
    }
    Ok(decisions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::test_entry;
    use std::path::PathBuf;

    #[test]
    fn candidate_id_ignores_raw_line_and_source_file() {
        let a = test_entry("k", Some("Hola"), "m", r#"{"responseId": "1"}"#);
        let mut b = test_entry("k", Some("Hola"), "m", r#"{"responseId": "2"}"#);
        b.source_file = PathBuf::from("otro.jsonl");
        assert_eq!(candidate_id(&a), candidate_id(&b));
    }

    #[test]
    fn candidate_id_distinguishes_exported_fields() {
        let base = test_entry("k", Some("Hola"), "model-a", "x");
        let other_model = test_entry("k", Some("Hola"), "model-b", "x");
        let other_text = test_entry("k", Some("Chau"), "model-a", "x");
        assert_ne!(candidate_id(&base), candidate_id(&other_model));
        assert_ne!(candidate_id(&base), candidate_id(&other_text));
    }

    #[test]
    fn text_preview_flattens_whitespace_and_truncates() {
        assert_eq!(text_preview("Hola\nmundo\tcruel"), "Hola mundo cruel");
        let long = "á".repeat(150);
        let preview = text_preview(&long);
        assert_eq!(preview.chars().count(), 103);
        assert!(preview.ends_with("..."));
    }
}
