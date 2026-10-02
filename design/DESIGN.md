# Controles de ComandOS

La interfaz usa tarjetas para comparar paneles, un selector para preparar cambios y un compositor táctil para escribir en remoto. Los controles comparten la configuración observada y las rutas del servidor.

Los colores proceden del tema de `dash/index.html`. `workspace.css` usa sus variables: fondo, paneles, bordes, texto, texto secundario y acento. El tema oscuro usa texto `#EAF0FB` sobre panel `#121722`; el botón principal combina acento `#8B7CFF` y texto `#0A0D13`.

Se conservan las familias sans y mono del producto. Las acciones principales tienen al menos 44 px de altura. El selector utiliza dos columnas en escritorio y una en teléfonos. Los diálogos permiten desplazamiento interno y adaptan su altura al viewport.

Solo **Aplicar cambios** ejecuta el borrador de configuración. Los controles de cuenta, CLI, modelo y esfuerzo no envían operaciones al seleccionarse. Las opciones incompatibles explican el motivo. La recuperación aparece en el mismo selector cuando hay una operación pendiente de restauración.

El chat propio de CommandOS se conserva por la corrección más reciente de Jesús. Su posible rediseño queda para una revisión posterior. El resumen de sesiones renderiza tarjetas, no terminales. La selección remota utiliza texto del panel en una vista estable. La entrada táctil conserva composición, autocorrección y borradores antes del envío explícito.

Las métricas distinguen configuración de uso observado. No se muestran estimaciones de ahorro como resultados medidos. Las limitaciones de cada CLI forman parte del editor de perfiles para evitar controles que aparenten cambios en caliente.

Las [referencias](references.md) y la [prueba visual](proof.html) registran el alcance de la revisión.

## Barra lateral en revisión (superada)

Esta sección quedó superada el 2026-09-29 por "Barra izquierda: comandos por CLI y terminales rápidas": el chat se retira y la barra no configura la IA, ofrece los comandos nativos de cada CLI.

### Texto original

La comparación basada en proyectos, sesiones y panes queda archivada. Jesús pide definir la utilidad de la barra. Su corrección posterior conserva el chat del producto. MCPs y skills conservan su acceso por split pane. La opción en evaluación es configurar la IA del pane seleccionado, con controles en el lugar y evidencia de lo que quedó activo.

La función contextual, el contenido esencial y la política de aplicación todavía requieren veredicto. El análisis del código, los límites de los comandos nativos y las pruebas propuestas están en /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/design/sidebar-controls-plan.md. Las referencias visuales anteriores documentan investigación; no implican aprobación de la jerarquía.

## Canales de notificación vigentes

Telegram queda excluido por completo del diseño de CommandOS, incluidos ajustes, envíos y comandos del bot. La decisión y el alcance de la retirada están en /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/README-v1-grill.md, apartado "Confirmed: remove Telegram from CommandOS entirely". La retirada de la integración existente corresponde a la implementación posterior al grilling.

El diseño requiere push al celular cuando el usuario no esté viendo CommandOS, incluido en segundo plano o con la pantalla bloqueada. El requisito y la evidencia del código actual están en el mismo registro, apartado "Confirmed: phone push when CommandOS is not being viewed". Los requisitos de instalación, permisos y sonido están documentados en /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/design/mobile-push-research.md. La entrega real en el teléfono sigue pendiente de implementar y probar.

## Lector de noticias aprobado

La base elegida es B, Edición continua, con la opción de mostrar la terminal. Las demás variantes se conservan como referencias. Viven en /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/prototype-v1-grill.html, bajo `round=news-reader`. El historial de decisiones y la verificación se conservan en /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/README-v1-grill.md.

El lector conserva el marco oscuro y la barra lateral actual con datos simulados. El cuerpo del artículo usa 16 px ajustables entre 14 y 22, interlineado 1.75 y una columna de hasta 760 px. Tablas y código pueden desplazarse dentro del artículo. Los filtros horizontales conservan sus controles en móvil. A muestra lista y detalle en escritorio; en móvil alterna ambos con un botón de regreso. C mantiene la terminal junto al lector en escritorio y permite volver a ella desde móvil. B recorre una edición, D despliega temas y E avanza entre artículos.

Colores medidos del prototipo: texto de artículo `#D7DCE8`, secundario `#AAB1C0`, acento `#AD9CFF`, fondo `#141821`. Sus contrastes respectivos sobre ese fondo son 12.93:1, 8.25:1 y 7.59:1. Son tokens de la implementación, no valores extraídos de las capturas de Mobbin. El lector usa las fuentes disponibles en el sistema y no descarga tipografías.

Los controles A-E pertenecen al laboratorio. Permanecen separados del área de la app y se pueden arrastrar en pantallas de altura normal. Con altura de hasta 650 px pasan debajo de la app en el flujo del documento para que no tapen los controles de lectura. El contenido, la investigación ampliada, los avisos y las fuentes son ejemplos. La posición de lectura, los guardados y los temas seguidos duran la visita; no existe sincronización real entre dispositivos en este prototipo.

En B, Ver terminal abre la terminal siempre a la izquierda y la edición a la derecha, por indicación de Jesús. En anchuras superiores a 560 px ambas comparten la pantalla; el separador se puede arrastrar o ajustar con flechas. Hasta 560 px cada vista conserva el ancho de la pantalla y se pasa entre ellas con los botones Terminal / Novedades o desplazamiento horizontal, manteniendo ese orden. Ampliar permite usar la terminal sola. Ocultar terminal devuelve el espacio a la edición y conserva el borrador. Mostrar y ocultar mantienen un ancla de lectura para compensar el cambio de ancho. La edición es la vista inicial; `terminal=1` permite compartir un ejemplo con la terminal abierta. Jesús aprobó esta distribución y su adaptación móvil el 2026-09-29. También pidió tres resúmenes de novedades al día. Los horarios, la política de avisos y la implementación en producción siguen pendientes.

## Barra izquierda: comandos por CLI y terminales rápidas

Decidido el 2026-09-29 en la fase 2 del grilling; historial en /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/README-v1-grill.md y prototipo en /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/prototype-v2-barra.html.

La barra deja de orquestar cambios de harness, modelo o cuenta. Muestra, siempre visible, un acordeón con los CLI instalados y su versión; el CLI detectado en el pane seleccionado se abre solo y lleva la etiqueta "en este pane". Dentro de cada CLI, primero los comandos de arranque para la terminal rápida y después sus comandos nativos, cada uno con una explicación de una línea y chips para sus argumentos. Un clic escribe el comando letra por letra en el prompt del pane seleccionado sin Enter; el usuario confirma con Enter o lo borra con Esc. El catálogo es curado en el repositorio y verificado contra el binario instalado; lo no verificado se marca.

Debajo del acordeón viven las terminales rápidas en tabs, cada una con su carpeta fechada, con un separador de altura. El chat de ComandOS se retira y su lugar lo ocupan esas terminales. Las cadenas de comandos se arman en un modal centrado de mosaico, abierto desde el botón Cadenas: arrastrar o tocar añade pasos, los pasos se reordenan arrastrando, se guardan en archivos de texto y se corren paso a paso con "Siguiente", sin esperas automáticas. La observación de lo que quedó activo sigue siendo el contrato de docs/tui-command-map.md.

Arte y tono: iconos pixel de Shikashi para grupos y acciones, monogramas por CLI como marcador de laboratorio (en producción, los iconos de proveedor existentes), poción de karsiori en el pane seleccionado, reloj de arena y cofre en las cadenas.

## Cabecera ordenada y Servidores

Aprobado el 2026-09-29 (ronda 9, variante A). Dos filas: en la primera ☰, los contadores esperan/listos/trabajando, Terminal y + Nueva sesión, y a la derecha ⌘K, snippets, Analytics, Remoto, Servidores, Novedades, el Pomodoro en miniatura, Ajustes y la hora; en la segunda el número seguido de las tabs. La fila permanente de servidores SSH desaparece de la cabecera. Bajo 1200 px de ancho del panel, los botones del sistema muestran solo icono con tooltip.

Reconciliación con lo entregado en main el 29–30 de septiembre (posterior a la ronda 9): los botones de cabecera ya son de un solo tamaño con cinco estilos 3D y etiqueta oculta, así que la cabecera ordenada los reutiliza tal cual y todo botón nuevo sigue ese sistema; la campana se queda en la fila 1 porque es lo que abre el cajón de avisos; el reloj de arena de Davitheoles sigue en el botón del Pomodoro; Resúmenes abre el lector junto a la terminal con la barra de comandos visible.

Servidores se conserva exactamente como hoy: el botón de la primera fila muestra la misma fila de chips por host con su estado, "gestionar", conectar e instalar llave. Los controles que salen de la barra izquierda y de la cabecera (Apariencia, Soberanía, volumen, límites, Notificaciones, Todas las sesiones, Perfiles, Uso de herramientas) se recomiendan dentro del menú ☰, pendiente de veredicto (D8 del subplan 06).

## Cierre de la fase 2

El grilling de la segunda fase terminó el 2026-09-29 con la orden de implementar lo aprobado. El plan de implementación es /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/docs/superpowers/plans/2026-09-29-commandos-v1/06-barra-comandos-cabecera.md. Analytics/Reparto, entrada remota, ajustes y contexto de retorno no se grillaron y no tienen diseño aprobado.

## Analytics (grill del 2026-10-01 y 2026-10-02)

Mockup maestro aprobado: `dash/prototypes/prototype-analytics.html`, publicado en https://claude.ai/artifact/TUhpGkx65o7ytP3hRMkggz (versión 12). Es la referencia pixel perfect: la implementación se compara captura contra captura con los mismos datos de ejemplo.

Analytics tiene tres pestañas: **Cuentas · Comparar · Pomodoro**. Arriba a la derecha, flechas ← → para ver la semana anterior (solo lectura). Todo es **por cuenta** (claude·main, claude·relotto, codex·main, grok·main); nunca se suman las cuentas de Claude ni los porcentajes de cuota entre cuentas.

- **Cuentas**: "barra iluminada". Una botella de vidrio por límite (Semana, Fable, Sesión 5 h en Claude; Semana en Codex y Grok), todas en una repisa con luz de fondo, placa con el logo oficial por cuenta. El % es lo que **queda**; cada límite muestra su propio reset; ámbar ≤30 %, rojo ≤10 %. Debajo, hoy/semana por cuenta y el calendario "compacto inteligente": columnas = días, une sesiones seguidas del mismo proyecto (×N), encoge horas sin actividad, máximo 2 en paralelo por grupo de cruce y un botón +N con la lista.
- **Comparar**: "¿qué proyecto cuesta más?". Repisa de botellas por capas (lo gastado de cada cuenta, en capas por proyecto, con lo que queda) y debajo tarjetas de hallazgos: lo más caro, cuota que sobra al reset, vs la semana pasada, cuentas mezcladas y horario por franja. El ranking es por tokens.
- **Pomodoro**: solo stats de pomodoros. Cuatro números (hoy, semana, cancelados, promedio por día), tabla día por día (tomates, cuántos, hora de inicio de cada uno con los cancelados tachados, proyectos con su número, foco) y debajo "¿a qué horas?" y "¿en qué proyectos?".
- **Celular**: la repisa se desliza de lado con una cuenta por pantalla y puntos indicadores; el calendario muestra 3 días con ← →; la tabla de Pomodoro pasa a tarjetas por día.

Se eliminan: Guardia anti-desborde, Cambios recientes, Alertas (y sus avisos al cruzar 70/85/95 %), la propuesta de Reparto con Analizar/Aplicar y arrastrar sesiones entre tanques, comparar configuraciones/modelos, calificaciones, experimentos y costo por proveedor. La aplicación no propone ni aplica cambios por su cuenta.

Datos que el diseño necesita y hoy faltan: cuenta en cada turno de Claude (se recupera por la carpeta del transcript), Codex por turno desde los rollouts, duración de los turnos de Claude y una foto de cuánto quedaba de cada cuota al reset (empieza vacía).
