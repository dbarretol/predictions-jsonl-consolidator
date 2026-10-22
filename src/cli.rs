use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about,
    long_about = None,
    after_help = "Ejemplos:\n  \
        predictions-jsonl-consolidator predicciones.jsonl\n  \
        predictions-jsonl-consolidator --dir carpeta_con_jsonl\n  \
        predictions-jsonl-consolidator --dir carpeta --out-csv reporte.csv\n  \
        predictions-jsonl-consolidator    (sin argumentos: usa la carpeta actual)"
)]
pub struct Args {
    /// Archivo JSONL de entrada (opcional si se usa --dir)
    #[arg(conflicts_with = "dir")]
    pub input: Option<PathBuf>,

    /// Carpeta para buscar y consolidar archivos JSONL
    #[arg(long, short, conflicts_with = "input")]
    pub dir: Option<PathBuf>,

    /// Archivo de salida TXT (opcional, se genera con timestamp por defecto)
    #[arg(long)]
    pub out_txt: Option<PathBuf>,

    /// Archivo de salida Markdown (opcional, se genera con timestamp por defecto)
    #[arg(long)]
    pub out_md: Option<PathBuf>,

    /// Archivo de salida CSV para metadata (opcional, se genera con timestamp por defecto)
    #[arg(long)]
    pub out_csv: Option<PathBuf>,

    /// Archivo decisions.csv ya completado, para resolver conflictos de deduplicación
    /// (mismo key+texto con distinta metadata) detectados en una corrida previa
    #[arg(long)]
    pub decisions: Option<PathBuf>,

    /// Omite el separador "=== {key} ===" antes de cada texto en el TXT de salida
    #[arg(long)]
    pub no_keys: bool,

    /// Busca archivos .jsonl también en subcarpetas (solo aplica con --dir o auto-modo)
    #[arg(long, short = 'r')]
    pub recursive: bool,

    /// Muestra diagnóstico detallado (tiempo por archivo, throughput) además de los errores
    #[arg(long, short = 'v', conflicts_with = "quiet")]
    pub verbose: bool,

    /// Silencia el diagnóstico de fondo (tracing) y la barra de progreso; el resumen
    /// final y los errores se siguen mostrando
    #[arg(long, short = 'q', conflicts_with = "verbose")]
    pub quiet: bool,
}
