# Reloj de arena «ComandOS» (propio)

Tira `hourglass.png` 864×32 = 27 cuadros de 32×32, mismo contrato que usa
`dash/pomodoro.js` / `bin/cc-app` hoy: **0–20** llenado (0 = toda la arena
arriba, 20 = toda abajo; el motor mapea el avance del bloque y hace el goteo
alternando `base`/`base+1` cada 450 ms), **21–26** giro al terminar (110 ms
por cuadro; el 26 vuelve a ser el 0).

Cómo se hizo (30-sep-2026): el vidrio vacío con marco de latón lo generó
`gpt-image-2.5-flare` (MCP `mcp-image`) a 1024², reducido BOX a 32 px y
cuantizado sin dither a `1a1c2c 8f563b d9a066 ffe9b3 cfe7f5 f4f4f4`. La
arena (`ffcd75`, sombra `e8a84a`, superficie `ffe9b3`), el chorro y el giro
(voltereta vertical: aplastar en Y hasta una línea y abrir invertido) los
pinta `scratchpad/sem/gen/hourglass.py` sobre las máscaras interiores del
vidrio detectadas por relleno. Licencia: nuestra. Colores neutros (latón +
vidrio pálido + ámbar) para que combine con todos los temas claros y oscuros.
`comandos-clean.gif` es solo la muestra en bucle para el grill.
