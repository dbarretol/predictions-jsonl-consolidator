use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

fn cmd() -> Command {
    Command::cargo_bin("predictions-jsonl-consolidator").unwrap()
}

fn line(key: &str, text: &str) -> String {
    line_full(key, text, "model-a", 1)
}

fn line_full(key: &str, text: &str, model: &str, tokens: u32) -> String {
    format!(
        r#"{{"key": "{key}", "response": {{"candidates": [{{"content": {{"parts": [{{"text": "{text}"}}]}}}}], "modelVersion": "{model}", "usageMetadata": {{"promptTokenCount": {tokens}, "candidatesTokenCount": {tokens}, "totalTokenCount": {tokens}}}}}}}"#
    )
}

fn line_without_text(key: &str) -> String {
    format!(
        r#"{{"key": "{key}", "response": {{"candidates": [], "modelVersion": "model-a", "usageMetadata": {{"promptTokenCount": 1, "candidatesTokenCount": 0, "totalTokenCount": 1}}}}}}"#
    )
}

/// Encuentra el único archivo con extensión `ext` dentro de `dir` (los nombres de
/// salida llevan timestamp, así que no se pueden predecir exactamente).
fn find_file_with_ext(dir: &Path, ext: &str) -> PathBuf {
    fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("no se pudo leer {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().and_then(|s| s.to_str()) == Some(ext))
        .unwrap_or_else(|| panic!("no se encontró archivo .{ext} en {}", dir.display()))
}

#[test]
fn single_file_produces_txt_md_csv() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("preds.jsonl");
    fs::write(&input, format!("{}\n", line("item-1", "Hola mundo"))).unwrap();

    cmd()
        .arg(&input)
        .arg("--quiet")
        .assert()
        .success()
        .stdout(predicate::str::contains("Líneas procesadas:          1"));

    let extract_dir = dir.path().join("jsonl_extract");
    let txt = fs::read_to_string(find_file_with_ext(&extract_dir, "txt")).unwrap();
    assert!(txt.contains("=== item-1 ==="));
    assert!(txt.contains("Hola mundo"));

    let md = fs::read_to_string(find_file_with_ext(&extract_dir, "md")).unwrap();
    assert!(md.contains("## Clave: item-1"));

    assert!(find_file_with_ext(&extract_dir, "csv").exists());
}

#[test]
fn multi_file_dedup_removes_exact_duplicates() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("a.jsonl"),
        format!("{}\n", line("item-1", "Hola mundo")),
    )
    .unwrap();
    fs::write(
        dir.path().join("b.jsonl"),
        format!("{}\n", line("item-1", "Hola mundo")),
    )
    .unwrap();

    cmd()
        .arg("--dir")
        .arg(dir.path())
        .arg("--quiet")
        .assert()
        .success()
        .stdout(predicate::str::contains("Duplicados eliminados:      1"))
        .stdout(predicate::str::contains("Líneas procesadas:          1"));

    let consolidated = find_file_with_ext(dir.path(), "jsonl");
    let content = fs::read_to_string(consolidated).unwrap();
    assert_eq!(content.lines().count(), 1);
}

/// Línea como la que produce re-ejecutar el mismo prompt: mismo texto y tokens, pero
/// distinto `responseId` y distinto orden en `promptTokensDetails`.
fn line_rerun(key: &str, text: &str, response_id: &str, image_first: bool) -> String {
    let details = if image_first {
        r#"[{"modality": "IMAGE", "tokenCount": 10}, {"modality": "TEXT", "tokenCount": 5}]"#
    } else {
        r#"[{"modality": "TEXT", "tokenCount": 5}, {"modality": "IMAGE", "tokenCount": 10}]"#
    };
    format!(
        r#"{{"key": "{key}", "response": {{"responseId": "{response_id}", "candidates": [{{"content": {{"parts": [{{"text": "{text}"}}]}}}}], "modelVersion": "model-a", "usageMetadata": {{"promptTokenCount": 15, "promptTokensDetails": {details}, "candidatesTokenCount": 1, "totalTokenCount": 16}}}}}}"#
    )
}

#[test]
fn rerun_with_same_exported_fields_is_collapsed_without_conflict() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("preds.jsonl");
    fs::write(
        &input,
        format!(
            "{}\n{}\n",
            line_rerun("item-1", "Hola mundo", "resp-a", true),
            line_rerun("item-1", "Hola mundo", "resp-b", false)
        ),
    )
    .unwrap();

    cmd()
        .arg(&input)
        .arg("--quiet")
        .assert()
        .success()
        .stdout(predicate::str::contains("Duplicados eliminados:      1"))
        .stdout(predicate::str::contains("Líneas procesadas:          1"));

    assert!(!dir.path().join("decisions.csv").exists());
}

#[test]
fn output_is_sorted_in_natural_order_not_lexicographic() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("preds.jsonl");
    // Orden lexicográfico pondría "item-10" antes que "item-2"; el orden natural no.
    let content = [
        line("item-10", "Diez"),
        line("item-2", "Dos"),
        line("item-1", "Uno"),
    ]
    .join("\n");
    fs::write(&input, content).unwrap();

    cmd().arg(&input).arg("--quiet").assert().success();

    let txt =
        fs::read_to_string(find_file_with_ext(&dir.path().join("jsonl_extract"), "txt")).unwrap();
    let pos_1 = txt.find("=== item-1 ===").unwrap();
    let pos_2 = txt.find("=== item-2 ===").unwrap();
    let pos_10 = txt.find("=== item-10 ===").unwrap();
    assert!(pos_1 < pos_2, "item-1 debería aparecer antes que item-2");
    assert!(
        pos_2 < pos_10,
        "item-2 debería aparecer antes que item-10 (orden natural, no lexicográfico)"
    );
}

#[test]
fn corrupt_line_is_counted_and_skipped_without_failing() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("preds.jsonl");
    let content = format!("{}\nesto no es json válido\n", line("item-1", "Hola mundo"));
    fs::write(&input, content).unwrap();

    cmd()
        .arg(&input)
        .arg("--quiet")
        .assert()
        .success()
        .stdout(predicate::str::contains("Líneas con error de parseo: 1"))
        .stdout(predicate::str::contains("Líneas procesadas:          1"))
        .stdout(predicate::str::contains("No se pudo parsear la línea 2 de"));
}

#[test]
fn format_change_fails_with_reason_instead_of_empty_reports() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("preds.jsonl");
    // Simula un cambio de formato: `modelVersion` renombrado en todas las líneas.
    let renamed = |key: &str| line(key, "Hola mundo").replace("modelVersion", "model");
    fs::write(
        &input,
        format!("{}\n\n{}\n", renamed("item-1"), renamed("item-2")),
    )
    .unwrap();

    cmd()
        .arg(&input)
        .arg("--quiet")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "Ninguna línea se pudo interpretar (2 con error)",
        ))
        .stderr(predicate::str::contains(
            "línea 1: missing field `modelVersion`",
        ));

    // No debe generar reportes vacíos que pasen por un resultado válido.
    assert!(!dir.path().join("jsonl_extract").exists());
}

#[test]
fn record_without_text_is_counted_and_uses_placeholder() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("preds.jsonl");
    fs::write(&input, format!("{}\n", line_without_text("item-1"))).unwrap();

    cmd()
        .arg(&input)
        .arg("--quiet")
        .assert()
        .success()
        .stdout(predicate::str::contains("Registros sin texto:        1"));

    let txt =
        fs::read_to_string(find_file_with_ext(&dir.path().join("jsonl_extract"), "txt")).unwrap();
    assert!(txt.contains("..."));
}

#[test]
fn csv_output_is_well_formed() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("preds.jsonl");
    let content = [
        line_full("item-1", "Uno", "model-a", 10),
        line_full("item-2", "Dos", "model-b", 20),
    ]
    .join("\n");
    fs::write(&input, content).unwrap();

    cmd().arg(&input).arg("--quiet").assert().success();

    let csv_path = find_file_with_ext(&dir.path().join("jsonl_extract"), "csv");
    let mut reader = csv::Reader::from_path(csv_path).unwrap();

    let headers: Vec<String> = reader.headers().unwrap().iter().map(String::from).collect();
    assert_eq!(
        headers,
        vec![
            "key",
            "model",
            "Traffic Type",
            "Prompt Image Tokens",
            "Prompt Text Tokens",
            "Prompt Tokens",
            "Candidate Tokens",
            "Thoughts Tokens",
            "Total Tokens",
        ]
    );

    let rows: Vec<_> = reader.records().map(|r| r.unwrap()).collect();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].get(0), Some("item-1"));
    assert_eq!(rows[1].get(0), Some("item-2"));
}

#[test]
fn metadata_conflict_blocks_and_writes_decisions_csv() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("preds.jsonl");
    // Mismo key + mismo texto, distinto modelo/tokens: conflicto, no duplicado exacto.
    let content = [
        line_full("item-1", "Hola mundo", "model-a", 10),
        line_full("item-1", "Hola mundo", "model-b", 20),
    ]
    .join("\n");
    fs::write(&input, content).unwrap();

    cmd()
        .arg(&input)
        .arg("--quiet")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("candidatos en conflicto"));

    let decisions_path = dir.path().join("decisions.csv");
    assert!(decisions_path.exists());
    let mut reader = csv::Reader::from_path(decisions_path).unwrap();
    let rows: Vec<_> = reader.records().map(|r| r.unwrap()).collect();
    assert_eq!(rows.len(), 2, "debe listar los dos candidatos en conflicto");
}

/// Marca "mantener" en `decisions.csv` para el candidato cuyo `model` coincide con
/// `keep_model`, dejando el resto sin marcar (se descartan como "ignorar").
fn mark_decision(decisions_path: &Path, keep_model: &str) {
    let mut reader = csv::Reader::from_path(decisions_path).unwrap();
    let headers = reader.headers().unwrap().clone();
    let decision_idx = headers.iter().position(|h| h == "decision").unwrap();
    let model_idx = headers.iter().position(|h| h == "model").unwrap();
    let rows: Vec<csv::StringRecord> = reader
        .records()
        .map(|r| r.unwrap())
        .map(|row| {
            if row.get(model_idx) == Some(keep_model) {
                let mut fields: Vec<String> = row.iter().map(String::from).collect();
                fields[decision_idx] = "mantener".to_string();
                csv::StringRecord::from(fields)
            } else {
                row
            }
        })
        .collect();

    let mut writer = csv::Writer::from_path(decisions_path).unwrap();
    writer.write_record(&headers).unwrap();
    for row in &rows {
        writer.write_record(row).unwrap();
    }
    writer.flush().unwrap();
}

#[test]
fn decisions_workflow_keeps_marked_candidate_on_rerun() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("preds.jsonl");
    let content = [
        line_full("item-1", "Hola mundo", "model-a", 10),
        line_full("item-1", "Hola mundo", "model-b", 20),
    ]
    .join("\n");
    fs::write(&input, content).unwrap();

    cmd().arg(&input).arg("--quiet").assert().failure().code(1);

    let decisions_path = dir.path().join("decisions.csv");
    mark_decision(&decisions_path, "model-b");

    cmd()
        .arg(&input)
        .arg("--quiet")
        .arg("--decisions")
        .arg(&decisions_path)
        .assert()
        .success()
        .stdout(predicate::str::contains("Descartados por decisión:   1"))
        .stdout(predicate::str::contains("Duplicados eliminados:      0"));

    let txt =
        fs::read_to_string(find_file_with_ext(&dir.path().join("jsonl_extract"), "txt")).unwrap();
    assert!(txt.contains("=== item-1 ==="));
    assert!(txt.contains("Hola mundo"));

    let csv_path = find_file_with_ext(&dir.path().join("jsonl_extract"), "csv");
    let mut reader = csv::Reader::from_path(csv_path).unwrap();
    let rows: Vec<_> = reader.records().map(|r| r.unwrap()).collect();
    assert_eq!(
        rows.len(),
        1,
        "solo el candidato marcado 'mantener' debe sobrevivir"
    );
    assert_eq!(rows[0].get(1), Some("model-b"));
}

#[test]
fn decisions_without_any_kept_candidate_blocks_with_clear_error() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("preds.jsonl");
    let content = [
        line_full("item-1", "Hola mundo", "model-a", 10),
        line_full("item-1", "Hola mundo", "model-b", 20),
    ]
    .join("\n");
    fs::write(&input, content).unwrap();

    cmd().arg(&input).arg("--quiet").assert().failure().code(1);

    let decisions_path = dir.path().join("decisions.csv");
    // No se marca ningún candidato como "mantener": la key queda sin resolver.

    cmd()
        .arg(&input)
        .arg("--quiet")
        .arg("--decisions")
        .arg(&decisions_path)
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "no tiene ningún candidato marcado",
        ));
}

#[test]
fn recursive_flag_finds_files_in_subfolders() {
    let dir = tempdir().unwrap();
    let sub = dir.path().join("subcarpeta");
    fs::create_dir(&sub).unwrap();
    fs::write(
        sub.join("nested.jsonl"),
        format!("{}\n", line("item-1", "Anidado")),
    )
    .unwrap();

    // Sin --recursive, el nivel superior no tiene ningún .jsonl: falla.
    cmd()
        .arg("--dir")
        .arg(dir.path())
        .arg("--quiet")
        .assert()
        .failure()
        .stderr(predicate::str::contains("No se encontraron archivos"));

    // Con --recursive sí se encuentra y procesa el archivo de la subcarpeta.
    cmd()
        .arg("--dir")
        .arg(dir.path())
        .arg("--recursive")
        .arg("--quiet")
        .assert()
        .success()
        .stdout(predicate::str::contains("Líneas procesadas:          1"));
}

#[test]
fn no_keys_flag_omits_separators_in_txt() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("preds.jsonl");
    fs::write(&input, format!("{}\n", line("item-1", "Hola mundo"))).unwrap();

    cmd()
        .arg(&input)
        .arg("--quiet")
        .arg("--no-keys")
        .assert()
        .success();

    let txt =
        fs::read_to_string(find_file_with_ext(&dir.path().join("jsonl_extract"), "txt")).unwrap();
    assert!(!txt.contains("=== item-1 ==="));
    assert!(txt.contains("Hola mundo"));
}

#[cfg(windows)]
#[test]
fn unreadable_file_in_dir_mode_reports_and_fails() {
    use std::os::windows::fs::OpenOptionsExt;

    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("good.jsonl"),
        format!("{}\n", line("item-1", "Hola mundo")),
    )
    .unwrap();
    let locked_path = dir.path().join("locked.jsonl");
    fs::write(&locked_path, format!("{}\n", line("item-2", "Bloqueado"))).unwrap();

    // share_mode(0): mientras este handle esté vivo, ningún otro proceso (ni siquiera
    // el binario bajo prueba) puede abrir el archivo; simula un archivo ilegible.
    let _lock = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&locked_path)
        .unwrap();

    cmd()
        .arg("--dir")
        .arg(dir.path())
        .arg("--quiet")
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::contains("Archivos ilegibles:         1"))
        .stderr(predicate::str::contains("No se pudieron leer"));

    drop(_lock);
}

#[test]
fn example_fixture_processes_successfully() {
    let example_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("example/predictions.jsonl");
    let out_dir = tempdir().unwrap();

    cmd()
        .arg(&example_path)
        .arg("--quiet")
        .arg("--out-txt")
        .arg(out_dir.path().join("out.txt"))
        .arg("--out-md")
        .arg(out_dir.path().join("out.md"))
        .arg("--out-csv")
        .arg(out_dir.path().join("out.csv"))
        .assert()
        .success();

    assert!(out_dir.path().join("out.txt").exists());
    assert!(out_dir.path().join("out.md").exists());
    assert!(out_dir.path().join("out.csv").exists());
    // No debe haberse creado jsonl_extract junto al fixture real: las tres rutas
    // de salida eran personalizadas.
    assert!(
        !example_path
            .parent()
            .unwrap()
            .join("jsonl_extract")
            .exists()
    );
}
