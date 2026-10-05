# Fase 3: medición de frameworks web en Rust y decisión

Fecha de la medición: 2026-10-05 (tarea T1 del plan `docs/superpowers/plans/2026-10-04-fase-3-term-y-web.md`).
Rama `migration/rust-fase3-web`. Todo lo construido vive en `.spike/ui/`, que no se versiona (`.spike/.gitignore` = `*`).

## Decisión

**Se mantiene `web-sys` + `maud` sin framework.** Ningún framework cumple la regla de reversión: los tres pesan más que la variante sin framework (entre +56 % y +223 % en gzip), y Dioxus y Yew además pisan las clases que pone el JS heredado. B1 cita este informe.

En este informe, «pasa la paridad» significa «pasa el oráculo `xtask dom-diff`», que ignora los comentarios. El JS heredado sí los ve al recorrer `childNodes`, `firstChild` o `nextSibling`. La variante `web-sys` + `maud` no genera comentarios, así que esto no afecta a la decisión.

Tres mediciones no coinciden con lo que la tabla del plan daba por esperado. Ninguna cambia la decisión, pero B1 debe conocerlas:

1. **Esfuerzo.** El plan esperaba «~1:1» en líneas para la variante sin framework. Lo medido es **2,46:1** con `wc -l` (666 líneas Rust frente a 271 JS). Los frameworks no lo mejoran: quedan entre 2,17:1 y 2,32:1.
2. **Coste de la porción.** La variante sin framework tiene el menor coste fijo y el menor total. Sin embargo, la porción sola cuesta más en ella (56,4 KB gzip) que en Yew (51,9 KB). Probablemente el grueso de esos 56 KB es código común a las cuatro variantes (`serde_json`, `core::fmt`, cola de `wasm-bindgen`) y no la plantilla; el coste de Yew lo sugiere. Es una hipótesis: no se midió por símbolo, y B1 debe verificarla con `twiggy`.
3. **Convivencia de Leptos.** El plan decía que Leptos «asume la propiedad de sus nodos». En la prueba conserva la clase y el atributo que añade el JS heredado, porque `class:x=` usa `classList`. Ese resultado depende de una elección del spike: el botón solo tiene una clase dinámica, escrita con `class:wm-animated`. Con `class=move || …` Leptos reescribiría el atributo entero y pisaría la clase ajena. Lo que lo descarta es el tamaño, no la convivencia.

## Qué se midió

Las cuatro variantes implementan la misma porción de `dash/work-marks.js` (commit 3f0a01a, antecesor de la rama):

- leen `fixture/work-marks.json` (filas, marcas, panes y actividad);
- pintan el botón de cada fila como `paint`: el punto `aiIconSvg`, la estrella si el pane es favorito, las clases, `data-wm-state`, `aria-label`, `title` y la `.wm-extra` con sticker o chip «¿Hecho? ✓»;
- abren el menú de `openMenu` al hacer clic: elementos, separador, posición `left/top` y foco en el elemento marcado;
- implementan el teclado del menú, el clic fuera y la elección con `POST /work-marks`.

La página heredada (`index-legacy.html`) carga el `work-marks.js` real. Un arnés de pocas líneas imita el esqueleto de filas que pinta `index.html` y luego llama a `WorkMarks.adopt`. En las variantes Rust ese esqueleto lo pinta el propio WASM. Las pestañas (`#tabbar`), el `IntersectionObserver`, el sondeo de 5 s, `display()` y `load()` quedan fuera de la porción. El idioma es español en todas (no hay `tf`).

Código común a las cuatro variantes (`.spike/ui/common/`):

- `model.rs`: modelo puro (canales, destino de la fila, entradas del menú, `serde`).
- `dom.rs`: utilidades `web-sys` (colocar el menú, teclado, clic fuera, POST, línea `WASM_OK`).

Cada variante añade su `src/lib.rs` con la API idiomática de su framework:

| Variante | Plantillas | Estado | Montaje |
|---|---|---|---|
| `websys` | `maud` (`html!` → `set_inner_html`) | `thread_local` | decora `#wm-host`, que no posee |
| `leptos` | `view!`, `inner_html` en el `<svg>` | `RwSignal`, `Memo`, `For` | `mount_to_body` |
| `dioxus` | `rsx!`, `dangerous_inner_html` | `use_signal`, `use_coroutine` | `LaunchBuilder::web()` sobre `<body>` |
| `yew` | `html!`, `Html::from_html_unchecked` | componente con mensajes | `Renderer::with_root(<body>)` |

Herramientas y versiones exactas:

| Herramienta o crate | Versión |
|---|---|
| `rustc` / `cargo` | 1.96.0 |
| `wasm-bindgen` (CLI y crate) | 0.2.129 |
| `wasm-opt` | 116 (`version_116`) |
| `web-sys` / `js-sys` | 0.3.106 |
| `wasm-bindgen-futures` | 0.4.79 |
| `serde` / `serde_json` | 1.0.228 / 1.0.150 |
| `maud` | 0.27.0 |
| `leptos` | 0.8.21, `csr` |
| `dioxus` | 0.7.10, `web` (con los rasgos por omisión; no se midió una configuración reducida) |
| `futures-util` | 0.3.34 (solo Dioxus, para `rx.next()` del coroutine) |
| `yew` | 0.23.0, `csr` |

El perfil de compilación es el mismo en las cuatro: `opt-level = "z"`, `lto = true`, `codegen-units = 1`, `panic = "abort"`, `strip = "symbols"`. Después vienen `wasm-bindgen --target web` y `wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int`. El gzip es `gzip -9`. Cada variante tiene además una versión «vacía» (rasgo `empty`) que solo monta un `<div>` con el framework. Los `.wasm` finales solo llevan la sección `producers` (60 B); Leptos añade `__wasm_split_unstable` (25 B).

## Tamaño

Bytes del `.wasm` tras `wasm-opt -Oz` y de la cola JS de `wasm-bindgen`:

| Variante | Vacía: raw | Vacía: gzip | Porción: raw | Porción: gzip | Coste de la porción (gzip) | Cola JS de la porción (raw / gzip) |
|---|---:|---:|---:|---:|---:|---:|
| `web-sys` + `maud` | 17 933 | **7 902** | 143 834 | **64 273** | 56 371 | 28 967 / 5 959 |
| Leptos 0.8.21 | 46 271 | 19 340 | 237 835 | 101 555 | 82 215 | 37 976 / 7 214 |
| Dioxus 0.7.10 | 332 724 | 142 312 | 494 660 | 207 866 | 65 554 | 80 313 / 12 213 |
| Yew 0.23.0 | 113 387 | 48 629 | 229 310 | 100 509 | **51 880** | 36 982 / 7 024 |

- El coste fijo es el de la variante vacía. El coste de la porción es la porción menos la vacía.
- Frente a `web-sys` + `maud`, el gzip de la porción completa queda así: Leptos +58,0 %, Dioxus +223,4 %, Yew +56,4 %.
- Contando la cola JS, lo que baja el navegador en gzip es: `web-sys` 70 232 B, Leptos 108 769 B, Dioxus 220 079 B, Yew 107 533 B.

## Paridad de DOM

Método:

1. Servir `.spike/ui` con el servidor de fixtures de T2 (`xtask fixtures --root .spike/ui --port 7356`) y exponerlo con `cc-browser-expose start 7356`.
2. Abrir cada página en el Chrome del Mac (`chrome-bg`; UA `HeadlessChrome/153.0.0.0`).
3. Esperar el montaje, hacer clic en el botón de la fila `beta|%3`, que es un pane congelado y favorito, y capturar `#wm-host.outerHTML` más `.wm-menu.outerHTML`.
4. Comparar cada captura con la heredada con `xtask dom-diff`, que usa `comandos-domdiff::normalize`: atributos ordenados, espacios colapsados y comentarios ignorados.

| Variante | `xtask dom-diff` | `outerHTML` literal | Comentarios en `<body>` | Otros |
|---|---|---|---:|---|
| `web-sys` + `maud` | igual | **idéntico byte a byte** (5 404 B) | 0 | — |
| Leptos | igual | distinto (5 607 B) | 29 (`<!---->`) | reordena atributos (`class` primero) |
| Dioxus | **distinto** (nodo 5) | distinto (5 893 B) | 15 (`<!--placeholder-->`) | `data-dioxus-id` en 11 elementos con oyente; sin ese atributo el normalizado coincide |
| Yew | igual | distinto (5 404 B) | 0 | solo cambia el orden de los atributos |

El menú queda en las cuatro en `<body>`, con `style="left: 12px; top: 102px;"`, como el heredado. El foco cae en el elemento marcado (`data-i="2"`, Congelado).

`normalize` ignora los comentarios, así que Leptos pasa el oráculo aunque su DOM real lleve 29 nodos que no existen en el heredado. Esos comentarios no rompen `.wm-extra:empty{display:none}`, porque `:empty` no cuenta comentarios.

## Convivencia con el JS heredado

Con la página montada, un script hace lo que hace hoy el JS heredado al decorar:

- añade la clase `wm-legacy-touch` y el atributo `data-x` al botón de `beta|%3`;
- inserta un `span.legacy-chip` en `.ident`, delante del botón, y un `i.legacy-in-extra` dentro de `.wm-extra`;
- carga `fixture/work-marks-2.json` con `wmAdopt`, de modo que la fila pasa a `idle`, sin sticker ni favorito;
- vuelve a la fixture 1.

| Variante | Mismo nodo `<button>` | `data-x` | Clase `wm-legacy-touch` | `span.legacy-chip` | Contenido de `.wm-extra` | Errores |
|---|---|---|---|---|---|---|
| JS heredado (referencia) | sí | conserva | conserva | conserva | lo reescribe por `innerHTML` (borra el `i` ajeno) | 0 |
| `web-sys` + `maud` | sí | conserva | **conserva** | conserva | igual que el heredado | 0 |
| Leptos | sí | conserva | **conserva** (`class:wm-animated` usa `classList`; con `class=move \|\| …` la pisaría) | conserva | conserva el `i` ajeno, a diferencia del heredado | 0 |
| Dioxus | sí | conserva | **pisa** (reescribe `class`) | conserva | conserva el `i` ajeno | 0 |
| Yew | sí | conserva | **pisa** (reescribe `class`) | conserva | conserva el `i` ajeno, pero al volver el sticker lo inserta **detrás** del nodo ajeno (orden distinto del heredado) | 0 |

Ninguna variante lanzó excepciones. La referencia heredada tiene un ruido propio: su sondeo de `/work-marks` y el `visibilitychange` vuelven a cargar la fixture 1 durante la prueba. Eso no afecta a las variantes Rust, que no sondean.

## Esfuerzo (líneas)

Las cuentas usan `wc -l`. La porción JS son las 271 líneas de `work-marks.js` que se portaron: 15–74, 84–124, 126–145, 153–231, 247–253 y 263–326 (255 SLOC sin líneas en blanco ni comentarios).

| Variante | `lib.rs` | `lib.rs` + `model.rs` (252) + `dom.rs` (131) | Rust/JS | Frente a `web-sys` (total) | Frente a `web-sys` (solo `lib.rs`) |
|---|---:|---:|---:|---:|---:|
| `web-sys` + `maud` | 283 | 666 | 2,46 | 1,00 | 1,00 |
| Leptos | 205 | 588 | 2,17 | 0,88 | 0,72 |
| Dioxus | 211 | 594 | 2,19 | 0,89 | 0,75 |
| Yew | 247 | 630 | 2,32 | 0,95 | 0,87 |

Cada `lib.rs` incluye unas 15 líneas de la variante vacía y de los `mod`. El modelo Rust es más largo que el JS equivalente porque lleva tipos, `serde` y `match` explícitos donde el JS usa objetos literales.

## Tiempo hasta montar (indicativo)

Es la mediana de 5 cargas en `iframe` en el mismo Chrome. Mide desde que empieza el módulo hasta que vuelve `mount()`, e incluye `init()` y el `fetch` de la fixture.

| Variante | Vacía | Porción |
|---|---:|---:|
| `web-sys` + `maud` | 23,9 ms | 34,0 ms |
| Leptos | 26,3 ms | 33,5 ms |
| Dioxus | 36,7 ms | 53,2 ms |
| Yew | 26,9 ms | 25,3 ms |

Yew y Dioxus programan el primer render de forma asíncrona, así que su número no incluye todo el pintado. Por eso no es un criterio de la regla.

## Mantenimiento

| Crate | Versión fijada | Estado en la caché local a 2026-10-05 |
|---|---|---|
| `web-sys` / `wasm-bindgen` | 0.3.106 / 0.2.129 | no hay en la caché una serie posterior (0.4 / 0.3) |
| `maud` | 0.27.0 | sin runtime: solo macro |
| Leptos | 0.8.21 | ya existe `leptos 0.9.0-beta2` en la caché |
| Dioxus | 0.7.10 | ya existe `dioxus-web 0.8.0-alpha.1` en la caché |
| Yew | 0.23.0 | no hay en la caché una versión posterior |

La fecha de la última ruptura de cada crate **no se verificó**. Los paquetes no traen `CHANGELOG` y la red de esta tarea se limitó a `cargo fetch` de las versiones fijadas.

## WebKitGTK 2.50 (`cc-app`)

**No medido.** El sondeo con Xvfb local no tuvo permiso. La compuerta D3 (`gate.js`, reversión a `?web=off` a los 8 s) cubre en producción el caso de un WebKitGTK que no cargue el WASM.

## Tabla de criterios del plan, con los números

| Criterio | `web-sys` + `maud` | Leptos 0.8.21 | Dioxus 0.7.10 | Yew 0.23.0 |
|---|---|---|---|---|
| Tamaño gzip, vacía / porción / coste de la porción | **7,9 / 64,3 / 56,4 KB** | 19,3 / 101,6 / 82,2 KB | 142,3 / 207,9 / 65,6 KB | 48,6 / 100,5 / 51,9 KB |
| Paridad de DOM (`xtask dom-diff`) | pasa; literal idéntico | pasa normalizado; 29 comentarios, atributos reordenados | **falla** (`data-dioxus-id` ×11); 15 comentarios | pasa normalizado; atributos reordenados |
| Convivencia | conserva todo | conserva todo | **pisa la clase** | **pisa la clase**; reordena nodos de `.wm-extra` |
| Esfuerzo (Rust/JS, `wc -l`) | 2,46 | 2,17 (0,88× `web-sys`) | 2,19 (0,89×) | 2,32 (0,95×) |
| Mantenimiento | estable | 0.9.0-beta2 en curso | 0.8.0-alpha.1 en curso | 0.23 |
| WebKitGTK 2.50 | no medido | no medido | no medido | no medido |

La columna de HTML en servidor con htmx no se midió. Su descarte en el plan no depende de números de T1: la terminal, el arrastre de pestañas, Web Audio y el Pomodoro sin red necesitan lógica en el cliente.

## Regla de reversión, aplicada literalmente

La regla del plan dice: «solo se cambia si en T1 un framework pasa la paridad de DOM y la convivencia **y** queda ≥ 30 % por debajo en gzip **y** ≤ 0,8× en líneas».

Se toma como gzip el de la porción completa tras `wasm-opt` y como líneas el total que se mantiene (`lib.rs` + común). Para que un framework la cumpliera, su gzip tendría que ser ≤ 0,70 × 64 273 = 44 991 B.

| Framework | Paridad | Convivencia | gzip ≤ 44 991 B | líneas ≤ 0,8× | ¿Cumple? |
|---|---|---|---|---|---|
| Leptos | sí (normalizado) | sí | **no** (101 555 B, +58 %) | **no** (0,88×; 0,72× contando solo `lib.rs`) | no |
| Dioxus | **no** | **no** | **no** (207 866 B, +223 %) | **no** (0,89×; 0,75×) | no |
| Yew | sí (normalizado) | **no** | **no** (100 509 B, +56 %) | **no** (0,95×; 0,87×) | no |

La regla falla también con la otra lectura de «gzip», la del coste de la porción sola. Haría falta ≤ 0,70 × 56 371 = 39 460 B, y el mejor, Yew, queda en 51 880 B (−8 %). Dioxus, con 65 554 B, y Leptos, con 82 215 B, están por encima de `web-sys`.

En líneas, Leptos cumpliría el 0,8× si se contara solo `lib.rs` (0,72×). Lo descarta el gzip, no las líneas.

Ningún framework cumple. La decisión queda como está en el plan, `web-sys` + `maud` sin framework. Los números de esta medición son los registrados arriba.

## Notas para B1

- Hipótesis por verificar con `twiggy`: lo que más pesa en el gzip de la porción sería lo común, es decir `serde_json`, el formateo (`format!`, `unwrap` de `Result<_, JsValue>`) y la cola de `wasm-bindgen`. No se midió por símbolo, porque el perfil lleva `strip=symbols`. Si se confirma, ese coste se reparte entre los componentes, pero para el objetivo de un tablero ultraligero conviene medirlo por símbolo en B1, por ejemplo con `twiggy`. También conviene valorar `js_sys::JSON` u otro analizador más pequeño en lugar de `serde_json`.
- `xtask dom-diff` ignora los comentarios. Para cualquier componente futuro, «pasa la paridad» significa «pasa el oráculo»: si el JS heredado recorre `childNodes`, `firstChild` o `nextSibling`, conviene comprobar además el `outerHTML` literal.
- Las plantillas `maud` reproducen el `outerHTML` heredado byte a byte en esta porción: atributos booleanos vacíos (`data-wm-suggest`), `aria-checked="true"` como texto y SVG por `PreEscaped`.
- La variante `web-sys` guarda en un `thread_local` lo que el JS guarda en propiedades del nodo (`_wmTarget`, `_wmSignature`). Busca el botón por igualdad de `JsValue` (`===`).
