# opentype.js (bundled)

Bundle mínimo de [opentype.js](https://opentype.js.org/) 1.3.4 — parser de
fuentes OpenType/TrueType en JavaScript puro (funciona en browser sin Node).

## Uso en ComandOS

`dash/term.html` lo usa para leer la tabla GSUB de `JetBrainsMono Nerd Font
Mono` y aplicar shaping OpenType (calt / liga) sobre las secuencias de
caracteres, dibujando las ligaduras como overlay encima del canvas de xterm.js
(que por sí solo renderea celda-por-celda sin shaping).

## Licencia

**MIT** — © opentype.js authors. Ver
<https://github.com/opentypejs/opentype.js/blob/master/LICENSE>.

## Actualizar

```
curl -sSL -o opentype.min.js \
  https://cdn.jsdelivr.net/npm/opentype.js@<version>/dist/opentype.min.js
```
