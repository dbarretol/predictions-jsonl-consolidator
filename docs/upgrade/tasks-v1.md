# Plan de mejoras — Predictions JSONL Consolidator

Análisis realizado sobre `src/main.rs` (376 líneas, todo en un archivo). Al ser una CLI,
"UX/UI" se traduce a: experiencia de terminal, mensajes, ayuda, errores y salidas generadas.
Incorpora y ordena los pendientes ya identificados en `TODO.log`.

---

## Fase 1 — UX crítica de la CLI (quick wins, alto impacto)

- [x] **1.1 Auto-modo sin argumentos (doble click del .exe)**
  Si no se pasa ni `input` ni `--dir`, usar `std::env::current_dir()` como carpeta a
  consolidar en lugar de fallar con `InvalidInput`. En Windows, considerar una pausa
  final (`Presione Enter para salir...`) cuando se detecte ejecución sin terminal
  interactiva, para que la ventana no se cierre antes de leer el resultado.

- [x] **1.2 Errores legibles al usuario final**
  `main() -> io::Result<()>` imprime los errores en formato `Debug`
  (`Error: Custom { kind: InvalidInput, error: "..." }`). Migrar a `anyhow::Result`
  (o un `enum` con `thiserror` + `Display`) para que el usuario vea un mensaje limpio
  y con contexto (qué archivo, qué operación).

- [x] **1.3 Resumen final de ejecución**
  Al terminar, imprimir: archivos leídos, líneas procesadas, líneas con error de parseo,
  duplicados eliminados, registros sin texto, y la ruta de CADA archivo generado.
  Hoy solo se imprime la carpeta con `{:?}` (en Windows muestra `\\` escapados);
  usar `.display()` en todas las rutas.

- [x] **1.4 Unificar el canal y el idioma de los mensajes**
  Hoy se mezclan `println!` (sin formato) con `tracing` (`info!`/`error!`, con timestamps),
  y español en mensajes propios con inglés en la ayuda generada por clap.
  Decidir: `tracing` solo para diagnóstico, `println!`/`eprintln!` para la salida dirigida
  al usuario; y un solo idioma consistente.

- [x] **1.5 Conflicto `input` + `--dir`**
  Si se pasan ambos, `--dir` gana silenciosamente. Declarar
  `#[arg(conflicts_with = "input")]` en clap para que sea un error explícito.

- [x] **1.6 Ayuda de la CLI completa**
  `#[command(about)]` toma la descripción de `Cargo.toml`, que no tiene campo
  `description` — hoy `-h` sale sin descripción. Agregar `description` (y `authors`)
  a `Cargo.toml`, y ejemplos de uso en `long_about` o `after_help`.

## Fase 2 — Consistencia y robustez del procesamiento

- [x] **2.1 Decidir semántica de deduplicación** *(TODO.log #2 — decisión bloqueante)*
  Hoy: `(key, text_hash)`. Riesgo: registros con igual key+texto pero metadata distinta
  (tokens, modelo) se pierden, y CUÁL sobrevive depende del orden de reducción paralela
  (no determinista). Opciones: hash de línea completa (preserva metadata) vs. actual
  vs. solo key. Documentar la decisión en el README.

- [x] **2.2 Deduplicar también en modo archivo único**
  El modo `--dir` deduplica; el modo `input` individual no. Unificar el pipeline para
  que ambos modos pasen por la misma lógica.

- [x] **2.3 Eliminar el doble parseo en modo `--dir`**
  Cada línea se parsea al consolidar y el archivo consolidado se re-parsea completo en
  `main()`. Refactorizar para que la consolidación devuelva los registros ya parseados
  y los reportes se generen de esa misma pasada (~2x menos trabajo).

- [x] **2.4 Registros sin texto: notificar, no silenciar**
  `extract_text()` fallido escribe `"..."` en el TXT sin aviso (el README promete
  detección y notificación). Loguear cada key afectada y contarlas en el resumen final.

- [x] **2.5 Archivo ilegible en modo `--dir` no debe pasar desapercibido**
  Hoy solo se loguea y se continúa: el consolidado puede quedar incompleto. Contarlo,
  reflejarlo en el resumen y devolver exit code ≠ 0 (o abortar, según se decida).

- [x] **2.6 Rutas `--out-*` personalizadas**
  Si el usuario pasa rutas custom, `jsonl_extract/` se crea igual aunque no se use, y si
  el directorio padre de la ruta custom no existe, el programa falla. Crear solo lo
  necesario y crear los padres de las rutas indicadas.

- [x] **2.7 Resolver ubicación de outputs** *(TODO.log #5)*
  Decidir entre mantener `jsonl_extract/` o guardar junto al `consolidated_*.jsonl`
  con el mismo prefijo. Documentar en README.

- [x] **2.8 Trazabilidad de `key` en el TXT** *(TODO.log #6)*
  El TXT concatena textos sin identificador. Agregar separador con la key
  (p. ej. `=== {key} ===`), opcionalmente desactivable con un flag `--no-keys`.

## Fase 3 — Arquitectura y mantenibilidad

- [x] **3.1 Modularizar `main.rs`** *(TODO.log #4)*
  Separar en `cli.rs`, `models.rs`, `parser.rs`, `consolidator.rs`, `exporters/{txt,md,csv}.rs`,
  `errors.rs`. Hacerlo DESPUÉS de 2.1–2.3 para no refactorizar dos veces.

- [x] **3.2 Sistema de errores con `thiserror` + `anyhow`** *(TODO.log #8)*
  Complementa 1.2: tipos de error propios en la librería, `anyhow` en el binario.

- [x] **3.3 Búsqueda recursiva opcional con `walkdir`** *(TODO.log #3)*
  Flag `--recursive`/`-r` para no cambiar el comportamiento actual por defecto.

- [x] **3.4 Reducir clones y allocations**
  `pred.clone()` por cada línea solo para construir `TokenReport` — cambiar
  `From<PredictionLine>` a `From<&PredictionLine>` o construir el reporte antes de
  mover `pred.key`.

## Fase 4 — Rendimiento y observabilidad

- [x] **4.1 Barra de progreso con `indicatif`**
  Para archivos grandes hoy no hay feedback de avance. Integrarla con el estilo de
  mensajes decidido en 1.4.

- [x] **4.2 Flags `--verbose` / `--quiet`**
  Conectados a `tracing_subscriber` (`EnvFilter`): `-v` para debug, `-q` para solo errores.

- [x] **4.3 Métricas de consolidación** *(TODO.log #9)*
  Tiempo por archivo, throughput, conteos — visibles con `--verbose`.

- [ ] **4.4 Streaming para datasets enormes** *(TODO.log #7 — solo si aparece el caso de uso)*
  External sort / merge incremental. Baja prioridad mientras los datasets quepan en RAM.
  **Sigue sin implementarse a propósito**: el propio plan la marca "solo si aparece el
  caso de uso"; no hay evidencia de datasets que no quepan en RAM, así que construir un
  external sort/merge incremental ahora sería especular sobre un requisito hipotético.

## Fase 5 — Calidad, pruebas y documentación

- [x] **5.1 Tests de integración**
  No existe ningún test. Usar los fixtures ya presentes (`test_data/`, `example/`) desde
  `tests/`: consolidación multi-archivo, dedup, orden natural, líneas corruptas,
  registros sin texto, CSV bien formado. `assert_cmd` + `tempfile` para probar la CLI real.

- [x] **5.2 Tests unitarios de parsing**
  `extract_text`, variantes de `TrafficType` (aliases), `UsageMetadata` con campos ausentes.

- [x] **5.3 Ordenar el repositorio**
  Agregar `.gitignore` para outputs generados (`jsonl_extract/`, `consolidated_*.jsonl`,
  `test-example/jsonl_extract/`); decidir si `example/`, `test_data/` se versionan como
  fixtures (recomendado) y eliminar `test-example/` si es redundante; mover `TODO.log`
  a `docs/` o eliminarlo una vez migrado a este plan.

- [x] **5.4 Actualizar README**
  Dice "ordenamiento alfabético" pero desde `8c50563` es orden natural; documentar
  auto-modo (1.1), semántica de dedup (2.1) y ubicación de outputs (2.7) cuando se decidan.

- [ ] **5.5 Benchmark con `criterion`** *(TODO.log #10 — opcional)*
  Solo si se trabaja la Fase 4.

---

## Fase 6 — Hallazgos de la re-auditoría (2026-07-18)

Verificado en la re-auditoría: `cargo clippy --all-targets` sin advertencias, 17/17 tests
pasan, modularización completa (8 módulos), README y `.gitignore` al día. Lo que sigue
son los puntos residuales encontrados, ninguno crítico.

- [x] **6.1 Líneas con UTF-8 inválido se descartan en silencio**
  En `consolidator.rs` (`if let Ok(line) = line` dentro del loop de lectura), un error
  de lectura/encoding se ignora sin loguear ni contar. Un archivo en Latin-1 perdería
  líneas sin ninguna señal en el resumen. Contarlas como error de parseo o loguearlas.
  Resuelto: ahora se loguea con `error!` y se cuenta como línea con error de parseo.

- [x] **6.2 Test end-to-end del flujo `--decisions`**
  Solo está testeada la detección del conflicto (se escribe `decisions.csv` y falla con
  exit 1). Falta el camino de vuelta: completar el CSV con "mantener"/"ignorar", re-ejecutar
  con `--decisions` y verificar qué candidatos quedan; y el caso de key sin resolver
  (`UnresolvedDecisions`). Es la lógica nueva más compleja y está sin red de tests.
  Resuelto: `decisions_workflow_keeps_marked_candidate_on_rerun` (round-trip completo) y
  `decisions_without_any_kept_candidate_blocks_with_clear_error` (`UnresolvedDecisions`).

- [x] **6.3 Cobertura de flags restantes**
  Sin tests: `--recursive` (subcarpetas), `--no-keys` (TXT sin separadores) y el exit
  code ≠ 0 cuando hay archivos ilegibles en modo `--dir`.
  Resuelto: `recursive_flag_finds_files_in_subfolders`, `no_keys_flag_omits_separators_in_txt`
  y `unreadable_file_in_dir_mode_reports_and_fails` (este último `#[cfg(windows)]`, simula
  un archivo ilegible bloqueándolo con `share_mode(0)`).

- [x] **6.4 `--quiet` no cumple lo que promete su ayuda**
  El help dice "solo muestra errores", pero el resumen final y mensajes como
  "Consolidando N archivos..." se imprimen igual con `-q` (los tests dependen de ello).
  Corregir la descripción del flag (opción simple) o suprimir también el resumen con `-q`.
  Decisión: se corrigió la descripción (opción simple, recomendada) — `--quiet` silencia
  el diagnóstico de fondo (tracing) y la barra de progreso; el resumen final y los errores
  siguen mostrándose porque van dirigidos al usuario, no son diagnóstico.

- [ ] **6.5 Pausa de auto-modo también en terminales interactivas**
  `pause_before_exit()` pide Enter siempre que no haya argumentos, aunque se haya
  lanzado desde una terminal (no solo con doble click). Con stdin redirigido no cuelga
  (EOF inmediato), así que es solo fricción menor. Se puede detectar consola propia con
  `GetConsoleProcessList() == 1` en Windows para pausar únicamente con doble click.
  Decisión: no implementado a propósito — agregaría una dependencia (winapi/windows-sys)
  solo para eliminar una fricción menor (un Enter de más en terminal), sin caso de uso
  urgente que lo justifique, en línea con 4.4/5.5.

- [x] **6.6 Cosmético: semántica del resumen**
  Los candidatos descartados vía "ignorar" en `decisions.csv` se cuentan como
  "Duplicados eliminados" (son descartes por decisión, no duplicados). Y la barra de
  progreso incrementa antes de validar la línea, así que cuenta también líneas vacías.
  Resuelto: nuevo campo `discarded_by_decision` (separado de `duplicates_removed`) con su
  propia línea "Descartados por decisión" en el resumen (solo si > 0); la barra de
  progreso ahora incrementa después de descartar líneas en blanco.

Nota: 4.4 (streaming), 5.5 (criterion) y 6.5 (detección de doble click) siguen abiertas
a propósito — sin caso de uso que las justifique todavía.

---

## Orden sugerido de ejecución

1. **Fase 1 completa** — bajo esfuerzo, es lo que el usuario percibe primero.
2. **2.1 (decisión) → 2.2 → 2.3** — definen el pipeline definitivo.
3. **3.1 + 3.2** — modularizar sobre el pipeline ya estable.
4. **Resto de Fase 2, Fase 5.1–5.4** — robustez + red de seguridad de tests.
5. **Fase 4** — pulido final.

Decisiones bloqueantes que requieren definición del dueño del proyecto: **2.1** (semántica
de dedup), **2.7** (ubicación de outputs), idioma único de mensajes (**1.4**).
