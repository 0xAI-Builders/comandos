# Fixtures de `comandos-term`

## `xterm-custom-glyphs.json`

Órdenes de canvas que emite `tryDrawCustomChar` de `@xterm/addon-canvas`
0.7.0 (el renderizador de `dash/term.html`) para cada glifo que xterm.js
dibuja a mano, en dos celdas: 9×20 px a dpr 1 con letra de 14 px y 13×29 px a
dpr 1,25 con letra de 15 px. La prueba `draw_ops_match_xterm_js_call_for_call`
(`tests/render.rs`) exige que `glyphs::draw_ops` dé las mismas órdenes con
los mismos números.

Clave: `<ancho>x<alto>@<dpr>/<fontSize>:<código hex>`. Valor: lista de
órdenes con los nombres de `glyphs::DrawOp` (`["MoveTo", x, y]`,
`["Stroke", lineWidth]`, `["FillPattern", máscara]`…).

### Regenerar

`gen-xterm-custom-glyphs.js` es una herramienta de pruebas: no la carga
ninguna página ni la ejecuta la suite. Extrae el módulo de `CustomGlyphs` del
bundle vendorizado, lo ejecuta en Node sobre un contexto 2D falso que anota
las llamadas y escribe el JSON:

```sh
node crates/comandos-term/tests/fixtures/gen-xterm-custom-glyphs.js \
  assets/xterm/addon-canvas.js \
  > crates/comandos-term/tests/fixtures/xterm-custom-glyphs.json
```

Hay que regenerarlo solo si cambia `assets/xterm/addon-canvas.js`. Si el
bundle cambia de forma, el script falla con un error en vez de escribir un
fixture vacío. Después, `cargo test -p comandos-term --test render`.
