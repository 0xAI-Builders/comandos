# Barra lateral: proyectos, sesiones y panes

Esta ronda compara cómo encontrar y retomar trabajo desde la barra izquierda. Mantiene los mismos panes, favoritos, borradores y acciones de ejemplo al cambiar de propuesta. Ninguna variante está elegida todavía.

Revisión: https://nodo-01.tail63a117.ts.net:8444/prototypes/prototype-v1-grill.html?round=sidebar&variant=A

Fuente: /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/prototype-v1-grill.html

| Variante | Organización | Ventaja y costo |
| --- | --- | --- |
| A · Árbol de proyectos | Proyecto → sesión → pane, con proyectos desplegables. | Hace explícita la pertenencia. Con muchos proyectos requiere plegar grupos o buscar. Recomendación inicial para evaluar. |
| B · Sesiones compactas | Cada sesión es una entrada; la seleccionada muestra sus panes. | Da más espacio a las sesiones. Oculta los panes de las otras sesiones hasta entrar. |
| C · Riel de proyectos | Riel de proyectos junto a las sesiones y panes del proyecto consultado. | Reduce la lista visible. Comparar proyectos exige cambiar de entrada en el riel. |
| D · Favoritos y búsqueda | Favoritos, panes abiertos recientemente y árbol completo desplegable. | Acorta el regreso al trabajo habitual. La jerarquía completa necesita abrirse. |
| E · Lista con inspector | Acceso directo a cada pane; contexto del seleccionado debajo. | Permite comparar panes sin desplegar grupos. La pertenencia a una sesión queda menos visible. |

En escritorio la barra acompaña a la terminal y su ancho se ajusta entre 250 y 440 px, con arrastre o flechas en el separador. En el celular la navegación ocupa el área de trabajo. Tocar un pane abre la terminal; Proyectos permite regresar. Los accesos globales pasan al pie de la barra en tamaños estrechos. Se conservan las mismas acciones de ejemplo.

## Comportamiento que se puede probar

- Buscar por proyecto, sesión, nombre del pane o agente. La búsqueda conserva foco y posición del cursor al escribir.
- Cambiar de pane conservando su borrador. Cambiar A–E conserva también favoritos y borrador del chat.
- Marcar y quitar favoritos sin cambiar de pane. Los recientes dependen de la selección explícita, sin clasificación por IA.
- Renombrar un pane conservando su identidad. Crear una sesión de ejemplo distinta dentro del mismo proyecto.
- Abrir una Terminal rápida sin formulario. Solo añade datos en memoria; no crea carpetas ni procesos.
- Plegar y reabrir la barra en escritorio. Abrir y ocultar el chat con su destino visible.
- Revisar carga, vacío, error con reintento y desconexión. La desconexión simulada bloquea cambios de pane y creación.

La agrupación usa identificadores de sesión; dos sesiones del mismo proyecto no se fusionan por compartir proyecto o nombre. El círculo de turno terminado es neutral y no significa Resuelto. Su representación definitiva sigue pendiente.

## Alcance de esta revisión

El chat y las terminales usan texto de ejemplo. Los controles de Configurar IA, MCPs y skills, Analytics, Remoto, Servidores, Perfiles, Uso de herramientas y Ajustes solo muestran su destino y alcance; sus flujos completos tienen rondas propias. Nueva sesión usa un formulario reducido para probar la navegación, no una propuesta final del formulario.

La vista de terminal seleccionada sirve para comprobar el cambio de contexto. La distribución anidada de tabs y panes continúa en la ronda de docking aprobada; esta ronda no integra ni sustituye ese motor. Tampoco resuelve todavía Hoy, la edición completa de estados organizativos ni todas las acciones de sesión. No hay persistencia entre recargas, sincronización real, llamadas a IA ni modificaciones de sesiones de producción.

## Verificación del 29 de septiembre de 2026

Se usó Chrome remoto de la Mac mini. Se revisaron visualmente las diez capturas, una de escritorio y una móvil por variante, y se transfirieron explícitamente a este equipo.

La matriz A–E a 1440×1000, 820×1180, 390×844, 320×568 y 667×375 pasó 25 comprobaciones de ajuste: sin desbordamiento horizontal del documento, barra dentro del marco y selector del laboratorio separado de los controles. En pantallas bajas se permite desplazamiento vertical del documento. Esto cubre esos tamaños, no constituye una prueba en cada dispositivo real.

Se comprobaron selección móvil y regreso, conservación de borradores, búsqueda con foco y cursor, favoritos, cambio entre las cinco variantes, renombrado, sesión adicional en el mismo proyecto, terminal rápida, bloqueo sin conexión, accesos móviles, ajuste por teclado y cierre/reapertura de la barra. Carga, vacío, error y desconexión muestran sus estados. El vacío ya no deja una terminal ficticia visible.

Los tres scripts inline pasan la comprobación de sintaxis de Node. El lector B con terminal y la preview expandida de Android siguen abriendo sin errores de consola. La ruta Tailscale devuelve HTTP 200 con la ronda nueva. La comprobación inicial de igualdad de texto detectó el registro de errores que el servidor de fixtures antepone al HTML; no era una versión antigua del prototipo.

Capturas: /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/design/shots/sidebar

Referencias inspeccionadas y decisiones de uso: /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/design/references.md

Galería: /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/design/proof.html
