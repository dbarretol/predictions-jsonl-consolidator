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
  - Desglose de tokens (Texto, Imágenes, otras modalidades, Pensamientos/Thoughts, Candidatos y Totales).
  - Motivo de fin de la respuesta (`Finish Reason`, ej. `STOP` o `MAX_TOKENS`).
- **Manejo de Errores**: Detecta y notifica entradas donde no se generó texto (ej. por límites de tokens): se cuentan en el resumen final y cada `key` afectada queda registrada en el log de diagnóstico junto con su `finish_reason`.
- **Predicciones fallidas**: las líneas en las que la API informa un error en `status` (sin `response`) se cuentan aparte en "Predicciones fallidas" y su mensaje queda en el log; no se confunden con líneas malformadas.
- **Tolerancia a cambios de formato**: solo `key` es imprescindible. Si faltan `modelVersion`, `usageMetadata` u otros campos de metadata, la línea se conserva y esas celdas quedan vacías o en cero. Las partes de razonamiento (`thought: true`) no se exportan como texto, y si la respuesta trae varias partes de texto se concatenan. Todo el conocimiento del formato vive en `src/input/` (ver [Arquitectura](#arquitectura)).

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
cargo run -- --dir "ruta a la carpeta"

# Uso automático con un archivo:
cargo run -- data/example/input/predictions.jsonl

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
  2. Escribe `decisions.csv` en la carpeta procesada, o junto al CSV de `--out-csv` si se
     indicó una ruta personalizada. Tiene una fila por candidato en conflicto (`key`,
     `candidate_id`, `source_file`, `model`, tokens, una vista previa del texto y una columna
     `decision` vacía).
  3. Completá la columna `decision` con `mantener` (podés marcar más de un candidato por
     `key` si querés conservar ambas variantes) o `ignorar`/vacío para descartarlo.
  4. Volvé a ejecutar agregando `--decisions decisions.csv`. Si alguna `key` queda sin ningún
     candidato marcado `mantener`, la ejecución se detiene de nuevo indicando cuáles faltan.

Este flujo evita que la elección de "cuál sobrevive" dependa del orden no determinista de la
consolidación paralela: siempre es una decisión explícita registrada en `decisions.csv`.
El `candidate_id` se calcula a partir de la key, el texto y la metadata de uso, no de
la línea cruda. Por eso las decisiones siguen siendo válidas en la re-ejecución aunque cambie
el orden o el nombre de los archivos. Los archivos de una carpeta se leen siempre en orden
natural de ruta.

En el resumen final, los duplicados exactos colapsados automáticamente se cuentan en
"Duplicados eliminados"; los candidatos descartados por decisión explícita (`ignorar`
en `decisions.csv`) se cuentan aparte, en "Descartados por decisión" (solo aparece si
hubo al menos uno), ya que no son duplicados sino variantes que el usuario eligió no conservar.

## Datos de ejemplo

`data/example/` permite ver qué genera la herramienta sin ejecutarla:

- `input/`: archivos JSONL de ejemplo (`predictions.jsonl` con datos reales de un batch
  de Gemini y `prediction_dummy.jsonl`).
- `output/`: lo que genera la herramienta para cada entrada, con nombres fijos:
  `<nombre>.txt`, `<nombre>.md` y `<nombre>_tokens.csv`.

Las salidas están versionadas y el test `example_outputs_are_up_to_date` exige que
coincidan con lo que genera el binario actual, así que siempre están al día. Si un cambio
modifica la salida a propósito, se regeneran y se revisa el diff:

```powershell
$env:UPDATE_EXAMPLES=1; cargo test --test cli example_outputs; Remove-Item Env:UPDATE_EXAMPLES
```

Para probar con un ejemplo nuevo, se agrega el `.jsonl` a `input/` y se regenera igual.
`data/example/output/` está marcado como binario en `.gitattributes` para que git no
convierta sus finales de línea en Windows; si lo hiciera, la comparación byte a byte del
test fallaría.

## Tests

```powershell
cargo test
```

Incluye tests unitarios del adaptador de formato (`src/input/vertex_batch.rs`: extracción
de texto y partes de razonamiento, campos opcionales, `status` de predicción fallida,
`finishReason`, `trafficType` literal, suma de tokens por modalidad, y que todos los
fixtures del repo se parseen sin errores), tests unitarios del pipeline (`src/pipeline/`:
orden y filtrado de archivos, colapso de duplicados y conflictos, estabilidad de
`candidate_id`, vista previa de `decisions.csv`) y tests de integración
(`tests/cli.rs`) que ejecutan el binario real sobre archivos temporales: consolidación
multi-archivo, deduplicación, orden natural, líneas corruptas, registros sin texto,
conflictos de metadata, validez del CSV generado, el flujo completo de `--decisions`
(candidato conservado en el re-run y el caso sin ningún "mantener"), `--recursive`,
`--no-keys`, y el exit code ≠ 0 con archivo ilegible en modo `--dir` (este último solo
en Windows).

Las líneas JSONL de los tests de integración se construyen en `tests/common/mod.rs` con
`serde_json::json!`. Si el formato de entrada cambia, los tests se ajustan en ese único
archivo, y los textos con comillas o saltos de línea se escapan correctamente.

## Estructura del CSV de Salida

El archivo CSV generado contiene las siguientes columnas:

1. `key`: Identificador único de la entrada.
2. `model`: Versión del modelo utilizado (vacío si la línea no lo trae).
3. `Traffic Type`: Tipo de tráfico tal cual lo informa la API (ej. `ON_DEMAND`, `PROVISIONED_THROUGHPUT`).
4. `Prompt Image Tokens`: Cantidad de tokens consumidos por imágenes en el prompt.
5. `Prompt Text Tokens`: Cantidad de tokens consumidos por texto en el prompt.
6. `Prompt Other Tokens`: Tokens del prompt en otras modalidades (audio, video, documento...).
7. `Prompt Tokens`: Total de tokens en el prompt.
8. `Candidate Tokens`: Tokens generados en la respuesta.
9. `Thoughts Tokens`: Tokens de razonamiento interno (si aplica).
10. `Total Tokens`: Suma total de todos los tokens procesados.
11. `Finish Reason`: Motivo de fin de la respuesta (ej. `STOP`, `MAX_TOKENS`).

## Arquitectura

El formato del JSONL solo se conoce en `src/input/`:

- `input/mod.rs`: frontera única, `parse_line(&str) -> Result<LineOutcome, ParseError>`.
  `LineOutcome` es un `Record` (predicción exitosa) o una predicción fallida.
- `input/vertex_batch.rs`: structs de serde del formato batch de Vertex AI / Gemini y su
  conversión a `Record`.

El resto del programa trabaja con `models::Record` y `models::Usage` (metadata de uso, sin
nombres de columna). Si el formato cambia, el ajuste se hace en el adaptador. Las columnas
del CSV de tokens se definen aparte, en `CsvRow` (`src/exporters/csv.rs`): agregarlas o
renombrarlas no afecta la deduplicación. Para soportar un formato distinto, se agrega otro
adaptador junto a `vertex_batch.rs` y se despacha hacia él desde `parse_line`.

`src/main.rs` solo arranca el programa (argumentos, logging, pausa en auto-modo y código de
salida). `src/app.rs` orquesta una corrida: resuelve las entradas, ejecuta el pipeline,
escribe el consolidado y los reportes, e imprime el resumen.

La consolidación vive en `src/pipeline/`, una etapa por archivo:

- `discovery.rs`: qué archivos `.jsonl` procesar, en orden natural.
- `reader.rs`: lectura paralela; convierte cada línea con `input::parse_line` y cuenta
  errores de parseo, predicciones fallidas y archivos ilegibles.
- `dedup.rs`: colapsa duplicados exactos y separa los conflictos. Es lógica pura, sin E/S.
- `decisions.rs`: genera la plantilla de `decisions.csv` y aplica la versión completada.
- `mod.rs`: orquesta las etapas (`deduplicate_files`). El pipeline no escribe archivos:
  si hay conflictos sin `--decisions`, los devuelve (`Deduplication::PendingDecisions`) y
  `main.rs` decide dónde guardar la plantilla.

Los exportadores (`src/exporters/`) generan el TXT, el MD y el CSV a partir de los
registros resueltos (`&[Record]`). Las decisiones de presentación viven ahí: el marcador
`...` para registros sin texto (TXT y MD) y las columnas del CSV (`CsvRow`).

---
Proyecto desarrollado como parte de un experimento en Rust.
