# Controles de sesiones y acceso remoto

ComandOS permite preparar un cambio de CLI, motor, modelo, esfuerzo y cuentas desde un único selector. Los perfiles de sesión guardan esa configuración junto con preferencias de skills y MCPs para nuevos procesos. La implementación vive en `dash/workspace.js`, `bin/cc-dash`, `lib/session_operations.py` y `lib/session_profiles.py`.

El botón **Todas las sesiones** muestra una tarjeta por panel con su estado observado. Las tarjetas abren la terminal o el selector correspondiente. Esta vista no crea terminales adicionales, por lo que su coste depende de actualizar texto y controles.

El chat se oculta con **Ocultar chat** y se recupera con **Abrir chat**. El navegador conserva su borrador y la preferencia de visibilidad. **Resultados de acciones** distingue solicitudes, acciones enviadas, confirmaciones y fallos. Una acción enviada a la interfaz no demuestra por sí sola que terminó.

## Terminal remota

**Seleccionar**, en la barra inferior, abre una copia estable del texto del panel activo. La barra cambia a **Volver**, **Copiar**, **Todo**, un selector **Inicio/Fin** y cuatro flechas. Izquierda/derecha ajustan un carácter completo; arriba/abajo ajustan una línea del extremo elegido. También se puede seleccionar directamente sobre el texto con los controles nativos del navegador. Estas flechas nunca envían teclas al TUI. Al volver se conserva el borrador sin enviar.

**Historial** y la pulsación larga siguen abriendo esa misma vista. El selector de panel conserva la identidad del split; consultar texto no cambia el foco ni el mouse de tmux. La copia usa el rango del control de texto, incluso si un botón recibió el foco. La vista seleccionable tiene un tamaño mínimo de letra de 16 px en táctil para evitar el zoom de edición de iOS.

**Paneles**, en la barra inferior, permite activar un split o pulsar **Cerrar** y confirmar el panel concreto. Cambiar de foco mientras está abierta la confirmación no cambia su destino. La API verifica la generación del servidor y el proceso del panel; tmux vuelve a comprobar la identidad y que exista otro panel antes de cerrar. El último split permanece abierto. No hay reintentos automáticos de cierre.

Antes de cerrar se guarda la distribución, metadatos de reanudación disponibles y hasta 500.000 caracteres de texto en `~/.local/state/comandos/closed-panes/`, con permisos privados y un máximo de 50 registros. Un fallo al guardar impide el cierre. Esta copia no conserva un proceso vivo ni garantiza reanudar un CLI que no ofrezca esa función.

La vista de historial permanece estable mientras llega salida nueva. **Actualizar** toma otra copia y **Final** desplaza la lectura al final. La consulta devuelve hasta 2.000 líneas por defecto, con un máximo de 5.000 y una respuesta limitada a un millón de caracteres. El historial disponible depende del buffer de tmux.

En dispositivos táctiles, el compositor nativo conserva el texto mientras el teclado compone, corrige o dicta. **Insertar** envía el texto sin Enter; **Enviar** lo entrega con Enter. El borrador permanece al desconectarse o recargar. **Teclado directo** permite usar la entrada del terminal. No se reenvían automáticamente comandos cuyo envío sea incierto.

Los gestos de redimensionamiento de splits y las teclas físicas siguen disponibles. El desplazamiento remoto acumula eventos y mantiene como máximo una solicitud de scroll en curso, con un acumulador limitado. Las pruebas de navegador en el Mac cubren 320, 390, 844 y 1400 px con sockets simulados; las pruebas de cierre usan además servidores tmux aislados. La lista de paneles se consulta al abrirla, sin añadir sondeos en segundo plano.

La composición se probó con eventos IME simulados y entrada real de Chrome sin interfaz gráfica. Falta validación física con Samsung Keyboard, SwiftKey y Safari en iOS. Estas pruebas no garantizan el mismo comportamiento en todos los teclados.

## Cambios y recuperación

Seleccionar opciones modifica un borrador. **Aplicar cambios** envía una operación con un identificador de reintento. Las peticiones concurrentes al mismo panel se excluyen mediante su identidad, que incluye la generación del servidor tmux. Una cuenta incompatible no se reemplaza silenciosamente.

El coordinador valida el destino, espera el turno si corresponde y guarda un snapshot antes de cerrar el origen. Los cambios se aplican mediante un relanzamiento con configuración explícita. Si el destino falla, intenta recuperar la conversación original por su identificador exacto. Los archivos de handoff se conservan, pero no se declaran inyectados ni generan un turno automáticamente.

Si ComandOS se interrumpe durante una operación incierta, conserva la información de recuperación. El selector ofrece **Recuperar sesión original** cuando corresponde. La recuperación se detiene si otro proceso o conversación ocupa el panel. Un destino cuya identidad no alcanzó a guardarse debe cerrarse manualmente antes de recuperar el origen.

Los cambios destructivos se rechazan cuando el CLI no ofrece una reanudación exacta comprobada. Esto incluye orígenes OpenCode/Antigravity y algunas combinaciones ACP. Cambiar a otro CLI conserva los historiales originales; un handoff no convierte un historial al formato del otro proveedor. La recuperación de sesión no revierte archivos ni acciones externas ejecutadas por el agente.

El estado separa configuración observada y última configuración confirmada. Los paneles ya no heredan el modelo o los tokens de un vecino por compartir pestaña o carpeta. La lectura de Codex acepta `max` y `ultra`. Los metadatos se almacenan en caché por proceso, conversación y firma del archivo; una modificación invalida la entrada.

El inventario parte de los paneles que existen en tmux. Los hooks agregan estado a su panel; un archivo antiguo no crea una sesión activa. La comprobación local encontró los 29 paneles existentes: 27 con agentes, una shell y una conexión SSH, sin omisiones ni duplicados.

## Perfiles y métricas de herramientas

**Perfiles** permite guardar nombre, CLI, motor, modelo, esfuerzo, cuentas y preferencias de extensiones. **Nueva sesión con este perfil** abre el formulario de inicio. Un perfil no altera los agentes que ya están abiertos. La interfaz muestra capacidades y rechaza selecciones obsoletas o incompatibles antes de lanzar.

**Producto completo** selecciona skills descubiertas de Superpowers, DigitalOcean, diseño, investigación y x402. La interfaz indica las familias sin selección verificable. La ausencia de una skill instalada y activa no se sustituye por una entrada ficticia. El perfil conserva las demás preferencias de MCPs hasta que se editen.

| CLI | Skills por perfil | MCPs por perfil | Aplicación |
|---|---|---|---|
| Codex | Selección individual mediante overrides de configuración. | Selección individual mediante overrides de configuración. | Al iniciar un proceso nuevo. |
| Claude | Sin selección individual verificada. | Condicional: servidores JSON que puedan aislarse sin alterar otras fuentes. | Al iniciar; se bloquean fuentes incompatibles. |
| Otros | No anunciado como compatible sin comprobación. | No anunciado como compatible sin comprobación. | La interfaz muestra el límite. |

**Uso de herramientas** consulta llamadas observadas de skills y MCPs por panel o para todas las sesiones. Muestra duración solo cuando está registrada. Las llamadas antiguas a skills sin nombre se contabilizan aparte. No se atribuyen tokens, dinero ni ahorros a una skill cuando esos datos no existen.

La captura de skills guarda únicamente el selector de la skill en el evento correspondiente. No añade argumentos generales ni resultados de herramientas. La consulta agrega los eventos de SQLite por intervalo y limita el número de grupos devueltos.

## Almacenamiento y verificación

SQLite guarda operaciones, perfiles, eventos y recibos locales. No requiere otro servicio. Los clientes remotos acceden mediante la API de ComandOS, sin abrir el archivo de base de datos por red. Este patrón coincide con los [usos recomendados de SQLite](https://www.sqlite.org/whentouse.html); las lecturas y escrituras usan [WAL](https://www.sqlite.org/wal.html).

Los snapshots de layouts y el chat conservan archivos atómicos con copia anterior. El coordinador fija su snapshot antes del cambio, independientemente del guardado periódico. Las conversaciones del chat se combinan bajo bloqueo para evitar que guardados concurrentes borren mensajes de otra solicitud. Las acciones se registran antes de transmitirse al navegador.

La consulta de estadísticas lee únicamente las columnas que necesita. Las peticiones simultáneas comparten una reconstrucción del resumen, y el caché se invalida al terminar la importación. En procesos aislados contra la misma base local, la memoria máxima de una construcción bajó de 283,5 a 121,2 MB; el tiempo pasó de 1,536 a 1,489 segundos. Esa medición corresponde a la construcción del resumen, no al consumo total de ComandOS ni a todos sus procesos de agentes.

Las pruebas aisladas incluyen errores de lanzamiento, recuperación tardía, historiales divergentes entre cuentas, llamadas repetidas, cancelación y desconexión del chat. Los recorridos de navegador están en `tests/e2e_session_workspace.js` y `tests/e2e_terminal_native.js`; usan servicios y sockets simulados. Las capturas se encuentran en [la prueba visual](../design/proof.html).

La comparación posterior del sidebar sustituye las expectativas antiguas específicas de GTK por comprobaciones de contenido y comportamiento compartidos. Consulta la [comparación visual](../design/sidebar-comparison.md), el [mapa de comandos nativos y sus límites](tui-command-map.md) y la [procedencia de las descripciones de MCP](mcp-descriptions.md).

Las cards se actualizan mediante el mismo observador en escritorio y remoto. El intervalo visible predeterminado es de dos segundos; las consultas no se acumulan si una respuesta tarda. Una confirmación nueva puede actualizar modelo y esfuerzo sin esperar otra respuesta de IA. Si el CLI no publica evidencia reconocible, la card conserva la procedencia y el límite en lugar de afirmar que un cambio está confirmado.
