pub mod csv;
pub mod md;
pub mod txt;

use crate::models::Record;

/// Marcador que se muestra en el TXT y el MD en lugar del texto de un registro que no
/// lo tiene (ver `Record::text`). Es una decisión de presentación: el CSV no lo usa.
const MISSING_TEXT_PLACEHOLDER: &str = "...";

fn display_text(record: &Record) -> &str {
    record.text.as_deref().unwrap_or(MISSING_TEXT_PLACEHOLDER)
}
