# Semáforo «ComandOS bot» (propio)

Robot-terminal con cara CRT `>_`. Un personaje, cinco estados, dos cuadros
cada uno (tira horizontal de 48×48 por estado):

| estado | qué hace |
|---|---|
| `idle`  | quieto, el cursor de la pantalla parpadea |
| `work`  | teclea con chispas, cara concentrada |
| `need`  | brazos arriba, `!` ámbar en la pantalla y en la antena (parpadea) |
| `done`  | brazos en V, ✓ verde, estrellita |
| `error` | volcado, ojos X rojos, humo |

Cómo se hizo (30-sep-2026): cuadro base generado con `gpt-image-2.5-flare`
(MCP `mcp-image`) pidiendo rejilla 32×32 dura y paleta Sweetie-16; los
estados y los segundos cuadros son ediciones del mismo cuadro con
`inputImagePath` + `maintainCharacterConsistency`. Después
`scratchpad/sem/gen/px.py`: recorte común por estado (para que no salte
entre cuadros), reducción BOX a 48 px y cuantización sin dither a la paleta
`1a1c2c 333c57 566c86 94b0c2 f4f4f4 ffcd75 ef7d57 38b764 a7f070 b13e53
5d275d 3b5dc9 41a6f6 73eff7 257179`. Licencia: nuestra (asset generado por
el proyecto). `sheet-48.png` = 5 columnas (estados) × 2 filas (cuadros).
