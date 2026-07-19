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
    fs::write(dir.path().join("a.jsonl"), format!("{}\n", line("item-1", "Hola mundo"))).unwrap();
    fs::write(dir.path().join("b.jsonl"), format!("{}\n", line("item-1", "Hola mundo"))).unwrap();

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

    let txt = fs::read_to_string(find_file_with_ext(&dir.path().join("jsonl_extract"), "txt")).unwrap();
    let pos_1 = txt.find("=== item-1 ===").unwrap();
    let pos_2 = txt.find("=== item-2 ===").unwrap();
    let pos_10 = txt.find("=== item-10 ===").unwrap();
    assert!(pos_1 < pos_2, "item-1 debería aparecer antes que item-2");
    assert!(pos_2 < pos_10, "item-2 debería aparecer antes que item-10 (orden natural, no lexicográfico)");
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
        .stdout(predicate::str::contains("Líneas procesadas:          1"));
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

    let txt = fs::read_to_string(find_file_with_ext(&dir.path().join("jsonl_extract"), "txt")).unwrap();
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
    assert!(!example_path.parent().unwrap().join("jsonl_extract").exists());
}
