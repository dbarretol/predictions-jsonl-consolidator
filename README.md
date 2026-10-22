# Predictions JSONL Consolidator

Este proyecto es una herramienta de línea de comandos (CLI) desarrollada en Rust diseñada para procesar, ordenar y consolidar archivos JSONL provenientes de procesos OCR en batch (como los de la API de Gemini).

La herramienta extrae el contenido transcrito, genera documentos legibles y un reporte detallado del uso de tokens.

## Características

- **Ordenamiento Inteligente**: Todas las entradas se ordenan por su clave (`key`) usando orden natural (numéricamente consciente: `item-2` antes que `item-10`), no orden alfabético puro.
- **Consolidación TXT**: Genera un archivo de texto plano con todo el contenido concatenado. Cada texto va precedido por un separador `=== {key} ===` para poder rastrear de qué registro proviene (desactivable con `--no-keys`).
- **Consolidación Markdown**: Genera un archivo `.md` estructurado con encabezados por cada clave, ideal para revisión humana.
- **Reporte de Tokens (CSV)**: Genera un reporte detallado que incluye:
  - Versión del modelo.
  - Tipo de tráfico.
  - Desglose de tokens (Texto, Imágenes, Pensamientos/Thoughts, Candidatos y Totales).
- **Manejo de Errores**: Detecta y notifica entradas donde no se generó texto (ej. por límites de tokens): se cuentan en el resumen final y cada `key` afectada queda registrada en el log de diagnóstico.

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

# Sin argumentos: auto-modo, usa la carpeta actual como si fuera --dir .
cargo run --
```

### Comportamiento por defecto
- **Archivo individual**: Crea una carpeta llamada `jsonl_extract` en el mismo directorio donde se encuentra el archivo de entrada.
- **Carpeta (--dir)**: Consolida todos los `.jsonl` encontrados en un archivo `consolidated_YYYYMMDD_HHMMSS.jsonl` en la carpeta indicada, y genera los reportes en una subcarpeta `jsonl_extract`.
- **Auto-modo (sin `input` ni `--dir`)**: usa la carpeta actual como si se hubiera pasado `--dir .`. Pensado para ejecutar el `.exe` con doble click: en Windows, al terminar, espera "Presione Enter para salir..." antes de cerrar la ventana, para que se alcance a leer el resultado.
- **Ubicación de los reportes**: es una decisión deliberada mantener `jsonl_extract/` como subcarpeta separada (en vez de guardar TXT/MD/CSV junto al `.jsonl` consolidado con el mismo prefijo), para no mezclar los `.jsonl` de origen/consolidado con los reportes derivados.
- **Nombres de archivo**: Todos los archivos generados (`.jsonl` consolidado, `.txt`, `.md`, `.csv`) incluyen un timestamp (`YYYYMMDD_HHMMSS`) para evitar sobreescrituras.
- **Rutas de salida personalizadas** (`--out-txt`/`--out-md`/`--out-csv`): se crean los directorios padre que falten. `jsonl_extract` solo se crea si al menos una de las tres rutas de salida sigue usando su valor por defecto.
- **Archivos `.jsonl` ilegibles** (en modo `--dir` o auto-modo): no detienen la consolidación de los demás archivos, pero el resultado puede quedar incompleto. Un archivo cuenta como ilegible si no se pudo abrir, o si ocurre un error de E/S persistente mientras se lee (ej. fallo físico del disco); una línea puntual con encoding inválido (UTF-8) no cuenta como archivo ilegible, sino como línea con error de parseo, y la lectura del resto del archivo continúa normalmente. Los archivos ilegibles se cuentan y se listan en el resumen final, y el programa termina con código de salida distinto de cero para que no pase desapercibido.
- **Líneas que no se pueden interpretar**: cada una se registra con su número de línea y el motivo (ej. `No se pudo parsear la línea 7 de preds.jsonl: missing field `modelVersion``), se cuenta en "Líneas con error de parseo" y la consolidación sigue con el resto. Si **ninguna** línea resulta válida, casi siempre es porque cambió el formato del JSONL: el programa se detiene **sin generar reportes** (que saldrían vacíos), muestra el primer error y termina con código de salida distinto de cero.

### Ejemplo de uso:

```powershell
# Uso automático en una carpeta:
cargo run -- --dir ./data

# Uso automático con un archivo:
cargo run -- example/predictions.jsonl

# uso recursivo
cargo run -- -d "ruta a la carpeta" -r
```

## Parámetros y Opciones

| Parámetro | Descripción | Valor por defecto |
| :--- | :--- | :--- |
| `input` | Ruta al archivo `.jsonl` de entrada. | N/A |
| `--dir, -d` | Carpeta para buscar y consolidar archivos `.jsonl`. | N/A |
| `--recursive, -r` | Busca archivos `.jsonl` también en subcarpetas (solo aplica con `--dir` o auto-modo). | Desactivado (solo el nivel superior) |
| `--out-txt` | Ruta personalizada para el archivo TXT. | `[dir_input]/jsonl_extract/consolidado_[TS].txt` |
| `--out-md` | Ruta personalizada para el archivo Markdown. | `[dir_input]/jsonl_extract/consolidado_[TS].md` |
| `--out-csv` | Ruta personalizada para el archivo CSV. | `[dir_input]/jsonl_extract/reporte_tokens_[TS].csv` |
| `--decisions` | Ruta a un `decisions.csv` ya completado, para resolver conflictos de deduplicación de una corrida previa. | N/A |
| `--no-keys` | Omite el separador `=== {key} ===` antes de cada texto en el TXT de salida. | Desactivado (los separadores se incluyen) |
| `--verbose, -v` | Muestra diagnóstico detallado (tiempo por archivo, throughput) además de los errores. | Desactivado |
| `--quiet, -q` | Silencia el diagnóstico de fondo (tracing) y la barra de progreso; el resumen final y los errores se siguen mostrando. | Desactivado |
| `-h, --help` | Muestra la ayuda del programa. | N/A |
| `-V, --version` | Muestra la versión del programa. | N/A |

`--verbose` y `--quiet` son mutuamente excluyentes. La variable de entorno `RUST_LOG`,
si está definida, tiene prioridad sobre ambos (p. ej. `RUST_LOG=predictions_jsonl_consolidator=trace`).

## Progreso y diagnóstico

Al consolidar, se muestra una barra de progreso con la cantidad de líneas procesadas
(se oculta con `--quiet`). Con `--verbose` además se registra, por cada archivo, el
tiempo que tomó procesarlo y la cantidad de líneas, más un resumen de throughput
(líneas/segundo) al finalizar — útil para diagnosticar archivos grandes o carpetas
con muchos archivos.

## Deduplicación

Tanto el modo de archivo único como el modo `--dir` pasan por la misma lógica de
deduplicación. Dos líneas se consideran **duplicadas** si tienen la misma `key` **y**
el mismo texto extraído de la respuesta (`response.candidates[0].content.parts[0].text`).
La metadata (modelo, tokens, traffic type) no forma parte de la clave de deduplicación.

- Si dos líneas duplicadas tienen además la misma metadata reportada (modelo, traffic type
  y todos los conteos de tokens del CSV), se colapsan automáticamente sin intervención: no
  hay ambigüedad sobre qué conservar. Los campos que no se reportan (`responseId`,
  `createTime`, `processed_time`, URIs de la solicitud, orden de `promptTokensDetails`) no
  cuentan, así que re-ejecutar el mismo prompt no genera un conflicto.
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

En el resumen final, los duplicados exactos colapsados automáticamente se cuentan en
"Duplicados eliminados"; los candidatos descartados por decisión explícita (`ignorar`
en `decisions.csv`) se cuentan aparte, en "Descartados por decisión" (solo aparece si
hubo al menos uno), ya que no son duplicados sino variantes que el usuario eligió no conservar.

## Tests

```powershell
cargo test
```

Incluye tests unitarios de parsing (`src/parser.rs`: `extract_text`, alias de
`TrafficType`, valores por defecto de `UsageMetadata`) y tests de integración
(`tests/cli.rs`) que ejecutan el binario real sobre archivos temporales: consolidación
multi-archivo, deduplicación, orden natural, líneas corruptas, registros sin texto,
conflictos de metadata, validez del CSV generado, el flujo completo de `--decisions`
(candidato conservado en el re-run y el caso sin ningún "mantener"), `--recursive`,
`--no-keys`, y el exit code ≠ 0 con archivo ilegible en modo `--dir` (este último solo
en Windows).

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
