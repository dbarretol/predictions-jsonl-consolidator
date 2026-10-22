use super::RecordEntry;
use fxhash::FxHashMap;

/// Resultado de colapsar duplicados: registros sin ambigüedad y grupos en conflicto
/// (misma key y texto, distinta metadata) que requieren una decisión explícita.
pub(super) struct Collapsed {
    pub resolved: Vec<RecordEntry>,
    pub conflicts: Vec<Vec<RecordEntry>>,
}

/// Dos registros son duplicados exactos si coincide lo que se exporta: texto y
/// `TokenReport` (que incluye la key). No se compara la línea cruda: re-ejecutar el
/// mismo prompt cambia campos que no se reportan (`responseId`, `createTime`,
/// `processed_time`, la URI de la imagen, el orden de `promptTokensDetails`) y eso no
/// debe pedir una decisión al usuario, ya que en `decisions.csv` los candidatos se
/// verían idénticos.
fn is_exact_duplicate(a: &RecordEntry, b: &RecordEntry) -> bool {
    a.text == b.text && a.report == b.report
}

/// Agrupa por (key, hash del texto): dos registros con la misma key y el mismo texto
/// son duplicados. Dentro de cada grupo se colapsan los duplicados exactos,
/// conservando el primero en orden de lectura; si quedan variantes distintas por
/// metadata (modelo/tokens), el grupo es un conflicto a resolver.
pub(super) fn collapse_exact_duplicates(entries: Vec<RecordEntry>) -> Collapsed {
    let mut groups: FxHashMap<(String, u64), Vec<RecordEntry>> = FxHashMap::default();
    for entry in entries {
        let text_hash = fxhash::hash64(entry.text.as_deref().unwrap_or("").as_bytes());
        let group = groups.entry((entry.key.clone(), text_hash)).or_default();
        if !group.iter().any(|kept| is_exact_duplicate(kept, &entry)) {
            group.push(entry);
        }
    }

    let mut resolved = Vec::new();
    let mut conflicts = Vec::new();
    for (_key, mut distinct) in groups {
        if distinct.len() == 1 {
            resolved.push(distinct.pop().unwrap());
        } else {
            conflicts.push(distinct);
        }
    }

    Collapsed {
        resolved,
        conflicts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::test_entry;

    fn lines(records: &[RecordEntry]) -> Vec<&str> {
        let mut lines: Vec<&str> = records.iter().map(|r| r.line.as_str()).collect();
        lines.sort();
        lines
    }

    #[test]
    fn identical_records_collapse_keeping_the_first_read() {
        let collapsed = collapse_exact_duplicates(vec![
            test_entry("k", Some("Hola"), "m", "primera"),
            test_entry("k", Some("Hola"), "m", "segunda"),
        ]);
        assert!(collapsed.conflicts.is_empty());
        assert_eq!(lines(&collapsed.resolved), ["primera"]);
    }

    #[test]
    fn same_key_with_different_text_keeps_both() {
        let collapsed = collapse_exact_duplicates(vec![
            test_entry("k", Some("Hola"), "m", "a"),
            test_entry("k", Some("Chau"), "m", "b"),
        ]);
        assert!(collapsed.conflicts.is_empty());
        assert_eq!(lines(&collapsed.resolved), ["a", "b"]);
    }

    #[test]
    fn same_key_and_text_with_different_metadata_is_a_conflict() {
        let collapsed = collapse_exact_duplicates(vec![
            test_entry("k", Some("Hola"), "model-a", "a"),
            test_entry("k", Some("Hola"), "model-b", "b"),
            test_entry("otra", Some("Hola"), "model-a", "c"),
        ]);
        assert_eq!(lines(&collapsed.resolved), ["c"]);
        assert_eq!(collapsed.conflicts.len(), 1);
        assert_eq!(lines(&collapsed.conflicts[0]), ["a", "b"]);
    }

    #[test]
    fn conflict_keeps_one_representative_per_distinct_variant() {
        let collapsed = collapse_exact_duplicates(vec![
            test_entry("k", Some("Hola"), "model-a", "a1"),
            test_entry("k", Some("Hola"), "model-b", "b"),
            test_entry("k", Some("Hola"), "model-a", "a2"),
        ]);
        assert_eq!(collapsed.conflicts.len(), 1);
        assert_eq!(lines(&collapsed.conflicts[0]), ["a1", "b"]);
    }
}
