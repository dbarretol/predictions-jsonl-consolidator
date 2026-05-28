# Predictions JSONL Consolidator

Este proyecto es una herramienta de línea de comandos (CLI) desarrollada en Rust diseñada para procesar, ordenar y consolidar archivos JSONL provenientes de procesos OCR en batch (como los de la API de Gemini).

La herramienta extrae el contenido transcrito, genera documentos legibles y un reporte detallado del uso de tokens.

## Características

- **Ordenamiento Inteligente**: Todas las entradas se ordenan alfabéticamente por su clave (`key`).
- **Consolidación TXT**: Genera un archivo de texto plano con todo el contenido concatenado.
- **Consolidación Markdown**: Genera un archivo `.md` estructurado con encabezados por cada clave, ideal para revisión humana.
- **Reporte de Tokens (CSV)**: Genera un reporte detallado que incluye:
  - Versión del modelo.
  - Tipo de tráfico.
  - Desglose de tokens (Texto, Imágenes, Pensamientos/Thoughts, Candidatos y Totales).
- **Manejo de Errores**: Detecta y notifica entradas donde no se generó texto (ej. por límites de tokens).

## Requisitos

- **Rust** (edición 2024 o superior). Se recomienda instalar vía [rustup](https://rustup.rs/).

## Compilación

Para compilar el proyecto y obtener un binario optimizado:

```powershell
cargo build --release
```

El binario resultante se encontrará en `target/release/predictions-jsonl-consolidator.exe`.

## Ejecución

Puedes ejecutar el proyecto directamente usando `cargo`:

```powershell
cargo run -- <RUTA_AL_ARCHIVO_JSONL> [OPCIONES]
```

### Comportamiento por defecto
Si solo proporcionas la ruta del archivo JSONL, el programa creará automáticamente una carpeta llamada `jsonl_extract` en el mismo directorio donde se encuentra el archivo de entrada y guardará allí los archivos `consolidado.txt`, `consolidado.md` y `reporte_tokens.csv`.

### Ejemplo de uso:

```powershell
# Uso automático (crea carpeta jsonl_extract en example/)
cargo run -- example/predictions.jsonl

# Con parámetros personalizados
cargo run -- example/predictions.jsonl --out-txt resultado.txt --out-md resultado.md --out-csv reporte.csv
```

## Parámetros y Opciones

| Parámetro | Descripción | Valor por defecto |
| :--- | :--- | :--- |
| `input` | **(Requerido)** Ruta al archivo `.jsonl` de entrada. | N/A |
| `--out-txt` | Ruta personalizada para el archivo TXT. | `[dir_input]/jsonl_extract/consolidado.txt` |
| `--out-md` | Ruta personalizada para el archivo Markdown. | `[dir_input]/jsonl_extract/consolidado.md` |
| `--out-csv` | Ruta personalizada para el archivo CSV. | `[dir_input]/jsonl_extract/reporte_tokens.csv` |
| `-h, --help` | Muestra la ayuda del programa. | N/A |
| `-V, --version` | Muestra la versión del programa. | N/A |

## Estructura del CSV de Salida

El archivo CSV generado contiene las siguientes columnas:

1. `key`: Identificador único de la entrada.
2. `model`: Versión del modelo utilizado.
3. `Traffic Type`: Tipo de tráfico (ej. ON_DEMAND).
4. `Prompt Image Tokens`: Cantidad de tokens consumidos por imágenes en el prompt.
5. `Prompt Text Tokens`: Cantidad de tokens consumidos por texto en el prompt.
6. `Prompt Tokens`: Total de tokens en el prompt.
7. `Candidate Tokens`: Tokens generados en la respuesta.
8. `Thoughts Tokens`: Tokens de razonamiento interno (si aplica).
9. `Total Tokens`: Suma total de todos los tokens procesados.

---
Proyecto desarrollado como parte de un experimento en Rust.
