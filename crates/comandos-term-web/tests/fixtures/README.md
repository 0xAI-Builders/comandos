# Fixtures de `comandos-term-web`

## `xterm-fit.json`

Columnas y filas que dan `FitAddon.fit()` de `@xterm/addon-fit` 0.10 y
`CanvasRenderer._updateDimensions` de `@xterm/addon-canvas` 0.7.0 (los de
`dash/term.html`) para un contenedor de `w × h` px CSS, barra de 10 px y
`scrollback` distinto de 0. Cada caso parte de 80×24 y aplica tres `fit()`
seguidos: la celda CSS de xterm.js es `round(cols·dev_w/dpr) / cols` con la
rejilla actual, así que el resultado puede cambiar (o alternar) entre
llamadas. La prueba `tests/fit.rs` exige que `metrics::fit` dé los mismos
pasos.

Fuentes de ejemplo: `14px` (8,4 × 17 px CSS) y `11px` (6,6 × 13), interlineado
1,2; dpr 1, 1,25, 2, 2,625 y 3; anchos 300–2000 cada 7 px más los de
teléfonos y escritorio con nombre.

### Regenerar

`gen-xterm-fit.js` es una herramienta de pruebas: no la carga ninguna página
ni la ejecuta la suite. Carga el UMD de `addon-fit.js` en un contexto de Node
con un `window.getComputedStyle` falso, extrae `_updateDimensions` del bundle
de `addon-canvas.js` y lo ejecuta sobre una terminal falsa:

```sh
node crates/comandos-term-web/tests/fixtures/gen-xterm-fit.js \
  assets/xterm/addon-fit.js assets/xterm/addon-canvas.js \
  > crates/comandos-term-web/tests/fixtures/xterm-fit.json
```

Hay que regenerarlo solo si cambia alguno de los dos bundles. Si cambian de
forma, el script falla con un error en vez de escribir un fixture vacío.
Después, `cargo test -p comandos-term-web --test fit`.
