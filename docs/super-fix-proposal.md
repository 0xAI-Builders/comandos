# Propuesta de corrección de chat, terminal remota y cambios de agente

ComandOS necesita una operación única por panel para cambiar agente, cuenta, modelo y esfuerzo, con recuperación verificable. El chat debe usar esa misma operación y confirmar su resultado. La terminal remota necesita controles de selección y entrada que se prueben con teclados reales.

Documento de diagnóstico original del 10 de septiembre de 2026. La implementación posterior y sus límites están descritos en [Controles de sesiones y acceso remoto](remote-session-controls.md). Las evidencias de esta propuesta describen el estado anterior al cambio.

## Evidencia y límites del diagnóstico

“Reproducido” indica ejecución aislada del código. “Inspeccionado” indica una ruta identificada en el código. “Pendiente” requiere reproducción adicional, especialmente en el dispositivo remoto.

| Problema | Evidencia | Certeza |
|---|---|---|
| El chat no tiene control propio para ocultarlo y recuperarlo. | `dash/index.html`, sección `#op-chat`: historial, nueva conversación y parar; sin botón de ocultar. El modo `only-panel` lo oculta como parte del diseño global. | Inspeccionado |
| El chat puede confirmar un fallo como éxito. | `lib/operator_dispatch.py`, `Dispatcher._run_api`: solo examina `error`. Una respuesta simulada `{"ok": false}` produce `ok: true` y `Hecho.`. | Reproducido |
| Las acciones de interfaz se confirman antes de ejecutarse. | `Dispatcher._run_ui` devuelve éxito al construir la acción; `opApplyActions` muestra errores locales, sin acuse de ejecución al servidor. | Inspeccionado |
| Hay contratos incompatibles de tema e idioma. | `operator_build_dispatcher` emite `type: ui, target: theme, theme: ...`; `opApplyActions` espera `value`. El evento de idioma usa `target: lang`, pero el consumidor espera `type: lang`. | Inspeccionado |
| Codex pierde modelo y esfuerzo con `max` o `ultra`. | `bin/cc-dash`, `codex_pane_model`: sobre `gpt-6-astra high` devuelve ambos valores; sobre `gpt-6-astra max` y `gpt-6-astra ultra` devuelve dos cadenas vacías. | Reproducido |
| Los datos de una pestaña pueden contaminar otros splits. | `read_states` aplica `_costumes_now.get(sess)` a cada panel. También conserva alternativas por carpeta y esfuerzo por sesión. | Inspeccionado |
| Los cambios de CLI, modelo y cuenta no comparten borrador. | `motorPopRender` separa `MPOP.harnessArm`, `MPOP.staged` y el envío inmediato a `/account/switch`. Los botones `.mp-cli-go` y `.mp-go .do` tienen condiciones independientes. | Inspeccionado |
| Cambiar de agente no tiene recuperación automática del anterior. | `harness_switch_apply` termina el agente original y, si falla el destino, deja una shell. El lanzamiento del destino se construye después de cerrar el origen. | Inspeccionado |
| La confirmación de handoff exagera lo ocurrido. | `harness_switch_apply` escribe `.comandos-handoff.md` y declara “contexto inyectado”, aunque el código evita pegarlo como turno. Compartir carpeta también comparte ese archivo. | Inspeccionado |
| El chat puede perder el registro de acciones al cortarse la conexión. | `operator_handle_chat_stream` guarda la conversación después del bucle de escritura SSE. Un error al escribir puede impedir alcanzar ese guardado. | Inspeccionado |
| Hay entrada descartada durante desconexiones. | `dash/term.html`, `sendInput`: retorna sin enviar cuando el socket no está abierto. | Inspeccionado |
| La selección táctil y la duplicación del teclado necesitan reproducción en dispositivo. | El terminal usa Canvas, gestos propios y un interceptor de `beforeinput` para borrar y Enter. No hay evidencia suficiente para atribuirles toda la duplicación reportada. | Pendiente |

Se ejecutó este grupo existente contra el árbol local:

```sh
pytest -q tests/test_operator_catalog.py tests/test_operator_dispatch.py tests/test_operator_agent_loop.py tests/test_operator_chat.py tests/test_agent_launch.py tests/test_remote_ui.py
```

Resultado: 129 pruebas aprobadas y una fallida, `test_harness_switch_trusts_live_pane_command_over_session_state`. Esa prueba comprueba código fuente; su fallo no equivale a una reproducción completa del cambio en una terminal. Las reproducciones del detector y despachador se hicieron extrayendo la función mediante AST y usando respuestas simuladas, sin enviar comandos a sesiones del usuario.

## Comportamiento propuesto

El chat tendrá un botón de ocultar y otro de abrir siempre accesible. Ocultarlo liberará el espacio y conservará borrador, historial, tamaño y respuesta en curso. La preferencia se guardará por dispositivo para no imponer la vista móvil al escritorio. El compositor permitirá varias líneas y mostrará la pestaña y el panel que recibirán la acción.

Cada función del catálogo deberá indicar dónde está disponible: escritorio, navegador o ambos. Las acciones tendrán identificador y estados solicitado, ejecutando, confirmado o fallido. El chat solo dirá “Hecho” tras recibir evidencia del efecto. Las operaciones pendientes seguirán visibles después de recargar. El botón de parar distinguirá detener la respuesta del chat y cancelar una operación de la sesión.

El selector mantendrá un único borrador con CLI, proveedor del modelo, modelo, esfuerzo y cuentas correspondientes. Solo aparecerá un botón “Aplicar cambios”, acompañado de un resumen del destino. Cambiar el CLI actualizará los modelos y cuentas compatibles sin ejecutar nada. Una cuenta inválida se señalará; el sistema no elegirá otra silenciosamente. Elegir “esperar fin de turno” o “interrumpir” será parte del mismo borrador.

El estado observado se obtendrá por identidad de panel, proceso y conversación. Las lecturas usarán primero metadatos del agente vivo y registros de esa conversación. El texto de la terminal quedará como alternativa identificada. La configuración deseada y la observada se almacenarán por separado. Si no hay evidencia, la interfaz mostrará “sin confirmar”, conservando el último valor confirmado con su fecha.

La terminal remota compartirá pestañas, orden y geometría de splits, tema y controles con el escritorio. En pantallas pequeñas permitirá ampliar un panel sin alterar el árbol guardado. La pulsación larga iniciará selección con controles táctiles; copiar y pegar estarán a un toque. Seleccionar texto no deberá enviar clics a la aplicación ni cambiar el mouse del escritorio.

Para seleccionar historial se propone una vista de texto del panel, tomada de su buffer o del historial disponible en tmux. Esa vista conservará selección y desplazamiento mientras llega salida nueva. Su coste es mantener la correspondencia entre filas y texto, incluidos saltos, Unicode y líneas envueltas. Es preferible a depender exclusivamente de la selección del Canvas en todos los navegadores.

La entrada tendrá un único responsable por modo: terminal directa para teclados físicos y una ruta de composición móvil validada para IME, autocorrección, dictado y pegado. Un compositor opcional permitirá editar textos largos antes de enviarlos. No se deduplicará por contenido repetido, porque repetir letras o comandos puede ser intencional. La deduplicación deberá basarse en el ciclo de eventos que originó cada envío.

El desplazamiento acumulará gestos por frame y limitará las solicitudes pendientes. La vista mantendrá su posición cuando el usuario lea historial y ofrecerá volver al final. La apertura del teclado modificará el espacio visible sin reconstruir la terminal. Si se corta la conexión, el compositor conservará el texto pendiente y el estado de desconexión será visible. No se reenviarán automáticamente comandos cuya recepción sea incierta.

## Recuperación de operaciones

Todos los puntos de entrada, incluido el chat y los endpoints actuales, deberán pasar por el mismo coordinador. Una operación tendrá un identificador durable, una clave de idempotencia y exclusión por panel. La identidad incluirá la generación del servidor tmux para evitar confundir un número de panel reutilizado.

El flujo propuesto es `validar → guardar punto de recuperación → esperar turno si corresponde → comprobar identidad otra vez → aplicar → verificar → confirmar`. Las etapas y sus resultados se guardarán antes de continuar. Después de reiniciar el servidor, el coordinador observará qué proceso existe antes de decidir si continúa o recupera. Las etapas destructivas no se repetirán solo porque una respuesta HTTP se perdió.

Antes de cerrar el origen se validarán el ejecutable, la cuenta, la ruta y las capacidades del destino. También se guardarán conversación exacta, comando de reanudación, cuentas, carpeta y snapshot completo de splits. Si es necesario esperar un turno, se actualizará el punto de recuperación justo antes del cambio. Se reutilizarán `lib/pane_snapshot.py` y `lib/tmux_snapshot.py`, con pruebas de compatibilidad.

El snapshot anterior a la operación quedará fijado hasta confirmar el resultado. El guardado periódico no podrá sustituirlo por una shell transitoria. Cada handoff tendrá una ruta propia por operación y conversación para evitar sobrescrituras entre paneles del mismo proyecto. Los historiales originales permanecerán referenciados y accesibles.

Si falla el destino después de cerrar el origen, el coordinador intentará reanudar el origen por su identificador exacto y verificará esa reanudación. Si también falla, conservará ambos historiales y ofrecerá recuperar el origen o reintentar el destino. No usará una conversación “más reciente” como sustituto. Tampoco declarará éxito por encontrar únicamente un PID del nuevo CLI: comprobará su estado utilizable y la configuración observable.

Cambiar entre agentes distintos conserva los historiales originales y permite entregar contexto mediante handoff; no convierte automáticamente un historial al formato del otro agente. La recuperación tampoco revierte cambios de archivos o efectos externos que un agente ya ejecutó. Su garantía se centra en conservar identidades, historiales y puntos de restauración, y en informar fallos verificables.

Se propone SQLite para las operaciones y eventos, por sus transacciones y restricciones de unicidad. Los snapshots de tmux seguirán usando archivos atómicos con copia anterior. Esta combinación añade migración y reconciliación al arrancar, pero evita ampliar la coordinación mediante varios JSON independientes. Los chats deberán guardar mensajes y acciones antes de transmitirlos al navegador, con reanudación del flujo por identificador de evento.

## Orden de implementación y aceptación

| Entrega | Cambios principales | Criterio de aceptación |
|---|---|---|
| Identidad y recuperación | Estado por panel, detección de esfuerzo y coordinador durable con recuperación exacta. | Dos agentes en la misma carpeta mantienen modelo y cuenta independientes. Un fallo de lanzamiento recupera el origen o muestra recuperación pendiente con referencias intactas. |
| Selector único | Borrador común y un solo botón para cualquier combinación de cambios. Endpoints antiguos usan el coordinador. | Cuenta + CLI + modelo generan una sola operación. Doble toque o reintento de red no lanzan dos agentes. |
| Chat confiable y ocultable | Ocultar/reabrir, borrador persistido, contratos de acciones y confirmaciones reales. | Ocultar durante una respuesta conserva el contenido. Una función fallida nunca termina marcada como ejecutada. |
| Terminal remota | Selección táctil, copiar/pegar, composición móvil, scroll y reconexión. | Texto recibido byte por byte igual al esperado; selección sin entrada accidental; lectura estable mientras llega salida. |

Las pruebas de recuperación usarán un servidor tmux aislado y agentes simulados. Se provocarán fallos después del snapshot, al cerrar el origen, al lanzar el destino y antes de confirmar. También se interrumpirán el servidor y la conexión durante cada etapa. Se verificará que los paneles vecinos y los historiales permanecen intactos.

Las pruebas del selector recorrerán combinaciones de cuenta, CLI, modelo y esfuerzo, incluidos valores incompatibles y selecciones obsoletas. Las del chat ejecutarán los contratos completos hasta el consumidor, además de comprobar que los nombres existen. Se cubrirán acciones no disponibles en remoto y el cierre del navegador después de ejecutar una acción.

La validación móvil incluirá Gboard, Samsung Keyboard y SwiftKey en Android, más Safari en iOS cuando haya dispositivo disponible. Se probarán composición, autocorrección, dictado, borrado, Enter, emojis y pegado multilínea con un receptor de bytes aislado. La emulación de viewport no sustituye estas pruebas. Falta identificar el celular, navegador y teclado del caso reportado.

La fluidez se evaluará con una traza del navegador y salida sostenida en terminal. Como objetivo propuesto, el procesamiento local del gesto tendrá un percentil 95 inferior a un frame de 60 Hz en el dispositivo de referencia. La latencia de red se medirá aparte. Este objetivo todavía no está medido ni constituye una garantía para cualquier dispositivo.

La activación será por bloques reversibles, conservando el formato de snapshots actual y una copia del código anterior. Ninguna prueba deberá cerrar o cambiar los agentes de trabajo del usuario. El criterio final es completar los recorridos reales en escritorio y remoto, además de pasar las pruebas automatizadas.
