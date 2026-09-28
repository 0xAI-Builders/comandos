# Referencias de controles de sesión

El trabajo mejora controles funcionales de ComandOS y su uso desde un teléfono. La implementación conserva la tipografía, los colores y los componentes existentes.

Mobbin no aparece entre las herramientas conectadas de esta sesión. No se consultaron pantallas de Mobbin ni se atribuyen decisiones a referencias de esa plataforma. No se generaron propuestas visuales alternativas: se implementaron los controles sobre la interfaz existente y se verificaron con pruebas aisladas de navegador.

| Fuente consultada | Qué se revisó | Uso |
|---|---|---|
| [Interfaz existente](../dash/index.html) | Tema, selector, tarjetas, chat y formulario de inicio. | Sí: reutilización de estilos y flujos. |
| [Terminal existente](../dash/term.html) | Gestos, entrada, historial y barra de acciones. | Sí: selección nativa y compositor sobre el terminal existente. |
| [SQLite: usos apropiados](https://www.sqlite.org/whentouse.html) | Almacenamiento local de una aplicación. | Sí: decisión de persistencia; no es una referencia visual. |
| [SQLite: WAL](https://www.sqlite.org/wal.html) | Concurrencia de lectura y escritura. | Sí: persistencia local; no es una referencia visual. |

La dirección aplicada es un selector con un solo envío, tarjetas para comparar sesiones y controles táctiles accesibles. Un mosaico que renderice todas las terminales no se usa para el resumen, para evitar trabajo de renderizado innecesario.

Los archivos del trabajo visual son `DESIGN.md`, `proof.html` y `shots/`. Las capturas muestran datos de prueba; no son evidencia de validación en un teléfono físico.


## Pomodoro: pixel art de artistas, 2026-09-28

Jesús rechazó los vectores arcade dibujados para el mockup y pidió assets pixel art abiertos de mejor calidad. La distribución B, Regla de tiempo, sigue elegida. Esta ronda compara cinco conjuntos dentro del mismo mockup, sin reabrir la distribución ni cambiar producción. Las búsquedas y lecturas de las páginas se hicieron con la skill busqueda-web; después se descargaron los archivos gratuitos publicados por los autores. Las imágenes se inspeccionaron directamente y en el prototipo.

| Fuente | Qué se revisó | Uso y motivo |
|---|---|---|
| [Shikashi, pack gratuito](https://shikashipx.itch.io/shikashis-fantasy-icons-pack) | Sprites transparentes de 32 px, reloj de arena, gemas, fogata, plantas, cofre. Página actual CC BY 4.0; notas del ZIP de 2020 conservan referencia CC BY 3.0 de game-icons.net. | Sí. Conjuntos Fantasía y Jardín; reloj y cofre compartidos con Arcade. Se acreditan Matt Firth y game-icons.net. |
| [La Red Games, Gems / Coins Free](https://laredgames.itch.io/gems-coins-free) | Tiras originales de monedas y gemas de 16 px, licencia CC0 indicada por el autor. | Sí. Propuesta inicial Arcade, con animación real de cinco cuadros para monedas y cuatro para gemas. |
| [Quintino Pixels, Free Assorted Icons](https://quintino-pixels.itch.io/assorted-icons) | PNG transparentes de 32 px: reloj, frascos, llave y moneda. CC0 en título y confirmación del autor. | Sí. Conjunto Alquimia, con movimiento CSS añadido a iconos estáticos. |
| [7Soul, compilación en OpenGameArt](https://opengameart.org/content/496-pixel-art-icons-for-medievalfantasy-rpg) | Reloj, medallas, cristal, cofre y fuego. Compilación CC0 que excluye derivados de juegos comerciales. | Sí. Conjunto RPG clásico. Se conservan atribución y referencias de licencia ante las etiquetas históricas diferentes. |
| [7Soul, publicación del autor](https://www.deviantart.com/7soul1/art/420-Pixel-Art-Icons-for-RPG-129892453) | Descripción del autor: Public Domain y eliminación de derivados; metadatos aún CC BY 3.0. | Sí. Corroboración y atribución conservada a Henrique Lazarini. No se presenta la inconsistencia como resuelta. |
| [7Soul, pack actual de pago](https://7soul.itch.io/7souls-rpg-graphics-pack-1-icons) | Producto distinto, más de 1700 iconos. | No. No se compró ni se copió; no confundir con la compilación antigua abierta. |
| [Kenney, Pixel UI Pack](https://kenney.nl/assets/pixel-ui-pack) | Preview y ZIP CC0 de paneles, botones y flechas. | No. Útil para interfaz, pero carece del conjunto de reloj y recompensas que necesita esta ronda. |

El inventario, hashes de archivos y créditos de los packs están en /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/assets/pixel-packs. Los PNG originales no se modificaron. CSS selecciona celdas y añade movimiento; La Red conserva sus cuadros originales. La elección visual sigue pendiente del usuario.
