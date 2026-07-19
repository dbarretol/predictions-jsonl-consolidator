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
# Para un archivo individual:
cargo run -- <RUTA_AL_ARCHIVO_JSONL> [OPCIONES]

# Para consolidar una carpeta completa de archivos JSONL:
cargo run -- --dir <RUTA_A_LA_CARPETA> [OPCIONES]
```

### Comportamiento por defecto
- **Archivo individual**: Crea una carpeta llamada `jsonl_extract` en el mismo directorio donde se encuentra el archivo de entrada.
- **Carpeta (--dir)**: Consolida todos los `.jsonl` encontrados en un archivo `consolidated_YYYYMMDD_HHMMSS.jsonl` en la carpeta indicada, y genera los reportes en una subcarpeta `jsonl_extract`.
- **Nombres de archivo**: Todos los archivos generados (`.jsonl` consolidado, `.txt`, `.md`, `.csv`) incluyen un timestamp (`YYYYMMDD_HHMMSS`) para evitar sobreescrituras.

### Ejemplo de uso:

```powershell
# Uso automático en una carpeta:
cargo run -- --dir ./data

# Uso automático con un archivo:
cargo run -- example/predictions.jsonl
```

## Parámetros y Opciones

| Parámetro | Descripción | Valor por defecto |
| :--- | :--- | :--- |
| `input` | Ruta al archivo `.jsonl` de entrada. | N/A |
| `--dir, -d` | Carpeta para buscar y consolidar archivos `.jsonl`. | N/A |
| `--out-txt` | Ruta personalizada para el archivo TXT. | `[dir_input]/jsonl_extract/consolidado_[TS].txt` |
| `--out-md` | Ruta personalizada para el archivo Markdown. | `[dir_input]/jsonl_extract/consolidado_[TS].md` |
| `--out-csv` | Ruta personalizada para el archivo CSV. | `[dir_input]/jsonl_extract/reporte_tokens_[TS].csv` |
| `--decisions` | Ruta a un `decisions.csv` ya completado, para resolver conflictos de deduplicación de una corrida previa. | N/A |
| `-h, --help` | Muestra la ayuda del programa. | N/A |
| `-V, --version` | Muestra la versión del programa. | N/A |

## Deduplicación (modo `--dir`)

Al consolidar varios archivos, dos líneas se consideran **duplicadas** si tienen la misma
`key` **y** el mismo texto extraído de la respuesta (`response.candidates[0].content.parts[0].text`).
La metadata (modelo, tokens, traffic type) no forma parte de la clave de deduplicación.

- Si dos líneas duplicadas son además idénticas byte a byte (misma metadata), se colapsan
  automáticamente sin intervención: no hay ambigüedad sobre qué conservar.
- Si dos líneas comparten `key` y texto pero **difieren en metadata** (ej. mismo texto
  generado por dos modelos distintos, o con distinto conteo de tokens), es un **conflicto**:
  la herramienta no elige por vos. En su lugar:
  1. Se detiene sin generar el consolidado ni los reportes.
  2. Escribe `decisions.csv` en la carpeta procesada, con una fila por candidato en conflicto
     (`key`, `candidate_id`, `source_file`, `model`, tokens, una vista previa del texto y una
     columna `decision` vacía).
  3. Completá la columna `decision` con `mantener` (podés marcar más de un candidato por
     `key` si querés conservar ambas variantes) o `ignorar`/vacío para descartarlo.
  4. Volvé a ejecutar agregando `--decisions decisions.csv`. Si alguna `key` queda sin ningún
     candidato marcado `mantener`, la ejecución se detiene de nuevo indicando cuáles faltan.

Este flujo evita que la elección de "cuál sobrevive" dependa del orden no determinista de la
consolidación paralela: siempre es una decisión explícita registrada en `decisions.csv`.

> Nota: el modo de archivo único (`input` sin `--dir`) todavía no deduplica — ver plan de
> mejoras, tarea 2.2.

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
