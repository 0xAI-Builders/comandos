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


### Alquimia: el usuario exige movimiento por fotogramas

Tras elegir Alquimia, Jesús pidió sprites cuya animación cambie realmente el dibujo. La propuesta anterior de Quintino era estática y se movía con CSS; no satisface ese nuevo requisito. La siguiente versión conserva la dirección de alquimia y usa fotogramas originales de karsiori, con sus recipientes inmóviles mientras se animan burbujas, líquido y destellos.

| Fuente | Revisión | Uso |
|---|---|---|
| [karsiori, pociones animadas](https://karsiori.itch.io/pixel-art-potion-pack-animated) | Licencia CC0 explícita, spritesheets PNG, rate sugerido de 10 fps. Se inspeccionó el archivo original y sus cuadros. | Sí: seis sprites de 7 a 24 fotogramas. Una poción de foco reemplaza visualmente el reloj de arena estático; el contador sigue mostrando el tiempo real del mockup. |
| [karsiori, llaves animadas](https://karsiori.itch.io/pixel-art-key-pack-animated) | Página y licencia CC0; 10 tipos animados. | No: alternativa disponible para futuros logros, no se descargó ni mezcló en esta revisión. |
| [karsiori, cofres animados](https://karsiori.itch.io/pixel-art-chest-pack-animated) | Página y licencia CC0; secuencias de apertura y cierre. | No: esta revisión muestra una familia de pociones coherente. |
| [karsiori, faroles animados](https://karsiori.itch.io/free-pixel-art-lantern-pack) | Página y licencia CC0; siete tipos animados. | No: posible recurso posterior, no se descargó. |

La galería ofrece una vista de los fotogramas originales. Es una herramienta de revisión del mockup, no un control de producción. Se debe validar visualmente esta nueva familia con el usuario; aceptar el estilo Alquimia no implica haber aprobado ya los sprites concretos de karsiori.


### Catálogo de estilos de Pomodoro

Jesús pidió conservar varios estilos intercambiables, incluidos los que ya había visto, en vez de elegir uno para excluir los demás. El catálogo mantiene Alquimia, Arcade, Fantasía, RPG clásico y Jardín. Añade Cristales con sprites originales de karsiori. La elección aplica en un toque y conserva el tiempo, los XP y los logros.

| Fuente | Revisión | Uso |
|---|---|---|
| [karsiori, gemas animadas](https://karsiori.itch.io/free-pixel-art-gem-pack) | Página CC0, seis hojas PNG originales inspeccionadas; 10 u 11 cuadros según la gema. | Sí: nuevo estilo Cristales, con seis símbolos por fotogramas. |
| [karsiori, flores](https://karsiori.itch.io/free-pixel-art-flower-pack) | Licencia CC0; ilustraciones estáticas, no confirma animación. | No: no resuelve el requisito de sprites para Jardín. |
| [karsiori, pinos animados](https://karsiori.itch.io/spruce-tree-pack-pixel-art-animated) | Licencia CC0; árboles animados de 52 a 128 px de alto. | No: requeriría rediseñar Jardín y sus tamaños. Se conservan las plantas que el usuario ya vio. |
| [Perfil de karsiori](https://karsiori.itch.io/) | Catálogo de packs del artista y enlaces a gemas, flores y árboles. | Sí: descubrimiento de las fuentes; no se copiaron otras ilustraciones. |

Las tarjetas distinguen seis sprites originales, combinación de sprites/iconos, o iconos estáticos con movimiento CSS. No se presenta el movimiento CSS como animación dibujada. Convertir Fantasía, RPG clásico y Jardín a sprites originales completos sigue pendiente de diseño. La preferencia global de producto se propone para desktop y remoto; el prototipo guarda solo el estilo en el navegador y no sincroniza equipos.
