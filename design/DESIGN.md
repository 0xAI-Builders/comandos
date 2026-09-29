# Controles de ComandOS

La interfaz usa tarjetas para comparar paneles, un selector para preparar cambios y un compositor táctil para escribir en remoto. Los controles comparten la configuración observada y las rutas del servidor.

Los colores proceden del tema de `dash/index.html`. `workspace.css` usa sus variables: fondo, paneles, bordes, texto, texto secundario y acento. El tema oscuro usa texto `#EAF0FB` sobre panel `#121722`; el botón principal combina acento `#8B7CFF` y texto `#0A0D13`.

Se conservan las familias sans y mono del producto. Las acciones principales tienen al menos 44 px de altura. El selector utiliza dos columnas en escritorio y una en teléfonos. Los diálogos permiten desplazamiento interno y adaptan su altura al viewport.

Solo **Aplicar cambios** ejecuta el borrador de configuración. Los controles de cuenta, CLI, modelo y esfuerzo no envían operaciones al seleccionarse. Las opciones incompatibles explican el motivo. La recuperación aparece en el mismo selector cuando hay una operación pendiente de restauración.

El chat oculto conserva el estado y libera espacio. El resumen de sesiones renderiza tarjetas, no terminales. La selección remota utiliza texto del panel en una vista estable. La entrada táctil conserva composición, autocorrección y borradores antes del envío explícito.

Las métricas distinguen configuración de uso observado. No se muestran estimaciones de ahorro como resultados medidos. Las limitaciones de cada CLI forman parte del editor de perfiles para evitar controles que aparenten cambios en caliente.

Las [referencias](references.md) y la [prueba visual](proof.html) registran el alcance de la revisión.

## Canales de notificación

Telegram queda excluido por completo del diseño de CommandOS, incluidos ajustes, envíos y comandos del bot. La decisión y el alcance de la retirada están en /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/README-v1-grill.md, apartado "Confirmed: remove Telegram from CommandOS entirely". La retirada de la integración existente corresponde a la implementación posterior al grilling.

## Lector de noticias aprobado

La base elegida es B, Edición continua, con la opción de mostrar la terminal. Las demás variantes se conservan como referencias. Viven en /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/prototype-v1-grill.html, bajo `round=news-reader`. El historial de decisiones y la verificación se conservan en /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/README-v1-grill.md.

El lector conserva el marco oscuro y la barra lateral actual con datos simulados. El cuerpo del artículo usa 16 px ajustables entre 14 y 22, interlineado 1.75 y una columna de hasta 760 px. Tablas y código pueden desplazarse dentro del artículo. Los filtros horizontales conservan sus controles en móvil. A muestra lista y detalle en escritorio; en móvil alterna ambos con un botón de regreso. C mantiene la terminal junto al lector en escritorio y permite volver a ella desde móvil. B recorre una edición, D despliega temas y E avanza entre artículos.

Colores medidos del prototipo: texto de artículo `#D7DCE8`, secundario `#AAB1C0`, acento `#AD9CFF`, fondo `#141821`. Sus contrastes respectivos sobre ese fondo son 12.93:1, 8.25:1 y 7.59:1. Son tokens de la implementación, no valores extraídos de las capturas de Mobbin. El lector usa las fuentes disponibles en el sistema y no descarga tipografías.

Los controles A-E pertenecen al laboratorio. Permanecen separados del área de la app y se pueden arrastrar en pantallas de altura normal. Con altura de hasta 650 px pasan debajo de la app en el flujo del documento para que no tapen los controles de lectura. El contenido, la investigación ampliada, los avisos y las fuentes son ejemplos. La posición de lectura, los guardados y los temas seguidos duran la visita; no existe sincronización real entre dispositivos en este prototipo.

En B, Ver terminal abre la terminal siempre a la izquierda y la edición a la derecha, por indicación de Jesús. En anchuras superiores a 560 px ambas comparten la pantalla; el separador se puede arrastrar o ajustar con flechas. Hasta 560 px cada vista conserva el ancho de la pantalla y se pasa entre ellas con los botones Terminal / Novedades o desplazamiento horizontal, manteniendo ese orden. Ampliar permite usar la terminal sola. Ocultar terminal devuelve el espacio a la edición y conserva el borrador. Mostrar y ocultar mantienen un ancla de lectura para compensar el cambio de ancho. La edición es la vista inicial; `terminal=1` permite compartir un ejemplo con la terminal abierta. Jesús aprobó esta distribución y su adaptación móvil el 2026-09-29. También pidió tres resúmenes de novedades al día. Los horarios, la política de avisos y la implementación en producción siguen pendientes.
