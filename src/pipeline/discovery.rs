use crate::errors::{AppError, Result};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// `true` si `path` es un `.jsonl` candidato a consolidarse (excluye resultados de
/// una consolidación previa, que ya llevan el prefijo `consolidated_`).
fn is_candidate_jsonl(path: &Path) -> bool {
    if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
        return false;
    }
    let filename = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    !filename.starts_with("consolidated_")
}

/// Lista los archivos `.jsonl` a consolidar dentro de una carpeta. Por defecto solo
/// mira el nivel superior; con `recursive` también recorre subcarpetas. El resultado
/// sale en orden natural de ruta.
pub fn find_jsonl_files(dir: &Path, recursive: bool) -> Result<Vec<PathBuf>> {
    let mut jsonl_files = Vec::new();

    if recursive {
        for entry in WalkDir::new(dir) {
            let entry = entry.map_err(|source| {
                AppError::io(
                    format!("No se pudo recorrer la carpeta: {}", dir.display()),
                    source.into(),
                )
            })?;
            let path = entry.path();
            if path.is_file() && is_candidate_jsonl(path) {
                jsonl_files.push(path.to_path_buf());
            }
        }
    } else {
        let entries = fs::read_dir(dir).map_err(|source| {
            AppError::io(
                format!("No se pudo leer la carpeta: {}", dir.display()),
                source,
            )
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| {
                AppError::io(
                    format!(
                        "No se pudo leer una entrada de la carpeta: {}",
                        dir.display()
                    ),
                    source,
                )
            })?;
            let path = entry.path();
            if path.is_file() && is_candidate_jsonl(&path) {
                jsonl_files.push(path);
            }
        }
    }

    if jsonl_files.is_empty() {
        return Err(AppError::NoJsonlFiles {
            dir: dir.to_path_buf(),
        });
    }

    // Ni `read_dir` ni `WalkDir` garantizan un orden: se ordena para que la corrida sea
    // reproducible (orden de lectura y, con él, qué línea representa a un grupo de
    // duplicados en el `consolidated_*.jsonl`).
    jsonl_files.sort_by(|a, b| natord::compare(&a.to_string_lossy(), &b.to_string_lossy()));

    Ok(jsonl_files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn names(files: &[PathBuf]) -> Vec<String> {
        files
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn excludes_non_jsonl_and_previous_consolidations() {
        assert!(is_candidate_jsonl(Path::new("preds.jsonl")));
        assert!(!is_candidate_jsonl(Path::new("preds.json")));
        assert!(!is_candidate_jsonl(Path::new("decisions.csv")));
        assert!(!is_candidate_jsonl(Path::new(
            "consolidated_20260101_000000.jsonl"
        )));
    }

    #[test]
    fn returns_files_in_natural_order() {
        let dir = tempdir().unwrap();
        for name in ["10.jsonl", "2.jsonl", "1.jsonl", "notas.txt"] {
            fs::write(dir.path().join(name), "").unwrap();
        }

        let files = find_jsonl_files(dir.path(), false).unwrap();
        assert_eq!(names(&files), ["1.jsonl", "2.jsonl", "10.jsonl"]);
    }

    #[test]
    fn recursive_includes_subfolders_only_when_requested() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.jsonl"), "").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub").join("b.jsonl"), "").unwrap();

        assert_eq!(
            names(&find_jsonl_files(dir.path(), false).unwrap()),
            ["a.jsonl"]
        );
        assert_eq!(
            names(&find_jsonl_files(dir.path(), true).unwrap()),
            ["a.jsonl", "b.jsonl"]
        );
    }

    #[test]
    fn empty_folder_is_an_error() {
        let dir = tempdir().unwrap();
        assert!(matches!(
            find_jsonl_files(dir.path(), false),
            Err(AppError::NoJsonlFiles { .. })
        ));
    }
}
