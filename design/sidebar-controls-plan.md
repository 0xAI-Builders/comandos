# Barra lateral para operar la IA

La propuesta es dedicar la barra a controlar la IA del pane seleccionado. Jesús descarta organizar esta ronda alrededor de las etiquetas proyecto, sesión o pane. El chat de CommandOS queda excluido del diseño del producto. MCPs y skills mantienen su acceso en cada split pane. Configurar IA es una posibilidad que el usuario quiere evaluar, todavía no una función aprobada para la barra.

Este documento es una propuesta para Jesús y una guía de comprobación para quien implemente los controles. Define alcance y criterios de fiabilidad. No declara reparados los cambios de modelo, cuenta o harness, ni autoriza una migración de conversaciones en vivo.

## Qué está decidido

- Retirar el chat propio de CommandOS del diseño, incluida su futura ronda de grilling. Esto no afecta las conversaciones de los agentes dentro de las terminales.
- Evitar otro selector de MCPs y skills en la barra; el control pertenece al pane.
- Dejar de comparar la jerarquía de proyectos y sesiones como objetivo de esta ronda.
- Conservar las decisiones previas sobre terminales, restauración, docking y adaptación remota.

La retirada del chat ya se refleja en la ronda `sidebar` del prototipo. Su implementación de producción, rutas de operador y procesos asociados siguen pendientes de retirada. No se borró el historial guardado. Las capturas de la comparación anterior son históricas y todavía muestran el chat.

## Qué hace el código inspeccionado

Se revisó el código de /home/someguy/codebase/0xJesus/ComandOS sin ejecutar cambios sobre panes reales.

| Hecho | Evidencia |
| --- | --- |
| Los controles de sesión envían la elección a `/session/configure`; elegir modelo o esfuerzo puede aplicar inmediatamente. Cambiar harness o proveedor, o interrumpir, requiere una confirmación adicional. | /home/someguy/codebase/0xJesus/ComandOS/dash/session-controls.js, `scChoose` y `scApply`; /home/someguy/codebase/0xJesus/ComandOS/dash/session-config.js, `requiresConfirmation`. |
| La ruta principal de configuración valida, espera, guarda un snapshot, aplica y verifica. | /home/someguy/codebase/0xJesus/ComandOS/lib/session_operations.py, `run_operation`. |
| En esa ruta, aplicar un cambio sale del agente y lanza un comando de inicio o reanudación. No existe ahí una excepción para cambiar únicamente el modelo en el proceso vivo. | /home/someguy/codebase/0xJesus/ComandOS/bin/cc-dash, `SessionConfiguration.apply`. |
| Los alias de cambio de cuenta, harness y modelo desembocan en el coordinador de configuración. | /home/someguy/codebase/0xJesus/ComandOS/bin/cc-dash, rutas POST y `model_switch_apply`. |
| La detección de actividad usa señales diferentes. Claude examina texto de la terminal; otros casos consultan estados publicados. Algunas lecturas fallidas devuelven libre. La espera puede durar hasta 45 minutos. | /home/someguy/codebase/0xJesus/ComandOS/bin/cc-dash, `harness_pane_busy`, `claude_pane_busy`, `grok_pane_busy` y `SessionConfiguration.wait_idle`. |
| La verificación combina identidad, conversación, configuración observada y texto de la terminal. El resultado puede quedar pendiente de confirmación. | /home/someguy/codebase/0xJesus/ComandOS/bin/cc-dash, `SessionConfiguration._verify`; /home/someguy/codebase/0xJesus/ComandOS/lib/session_operations.py. |
| ACP ofrece operaciones de modelo y algunas opciones en vivo, dependiendo de lo que anuncie el agente. Esto no demuestra que todos los CLIs nativos ofrezcan el mismo contrato. | /home/someguy/codebase/0xJesus/ComandOS/bin/cc-acp y /home/someguy/codebase/0xJesus/ComandOS/tests/test_acp_reconfiguration.py. |

Estas observaciones señalan dónde simplificar y comprobar el comportamiento. No reproducen por sí solas los fallos que Jesús describió, ni prueban que tmux sea su causa.

## Los comandos no tienen todos el mismo alcance

La documentación oficial de [configuración de modelos de Claude Code](https://code.claude.com/docs/en/model-config), consultada el 29 de septiembre de 2026, describe `/model` como una vía de cambio durante la sesión. También indica que desde la versión 2.1.153 puede guardar el modelo como predeterminado para sesiones futuras; el selector ofrece una elección limitada a la sesión. Las banderas de inicio tienen otro alcance.

Por ello, una función rotulada para un pane no puede limitarse a escribir cualquier comando equivalente. Debe comprobar su efecto, alcance y compatibilidad con la versión instalada. Esta observación de Claude no se extrapola a Codex ni a otros harnesses.

| Intención | Ruta propuesta |
| --- | --- |
| Cambiar modelo o esfuerzo | Operación nativa o de protocolo dentro del proceso si está comprobada y limitada al pane. Si no existe, explicar que requiere reanudar antes de ejecutarla. |
| Cambiar cuenta | Conservar la identidad de la conversación y usar un contexto de autenticación aislado cuando el adaptador lo permita. Mostrar si requiere iniciar sesión o reanudar. No alterar otras terminales mediante un login global. |
| Cambiar harness | Mostrar si se retoma un historial ya existente en el destino o se abre otra conversación. Conservar el origen y hacer explícito el traspaso de contexto. Un handoff no convierte historiales entre proveedores. |

## Alternativas para la barra

| Dirección | Valor | Costo |
| --- | --- | --- |
| Controles del pane seleccionado, recomendada | Permite consultar y cambiar la IA mientras la terminal sigue visible. | Necesita un destino inequívoco y no puede reinterpretar una operación pendiente al cambiar de foco. |
| Cuentas y preferencias generales | Sirve para preparar cuentas y valores de inicio. | No explica ni controla bien lo que utiliza una conversación ya abierta. |
| Solo accesos a herramientas | Ocupa poco espacio y abre las funciones existentes. | No resuelve la confianza en los cambios de IA ni muestra su resultado junto al trabajo. |

## Contenido propuesto, pendiente de aprobación

La barra contextual mostraría el pane de destino, harness, cuenta, modelo y esfuerzo activos. Cada valor sería editable en el lugar, con opciones compatibles. El proveedor aparecería separado solo cuando su diferencia con el harness importe para elegir. Un cambio compatible dentro del proceso no abriría un formulario completo.

Debajo habría una explicación breve de la operación solo mientras haga falta: qué sigue activo, qué se pidió, si espera un turno real y qué acción permite resolver un fallo. Cuando termine, la configuración observada ocuparía de nuevo ese lugar. Los registros del provider quedarían accesibles desde el fallo, con secretos ocultos y sin un flujo permanente de logs.

Cuota, consumo o contexto podrían ocupar una fila secundaria cuando exista una medición atribuible a ese pane o cuenta. No convertir una estimación de tokens, una cuota de cuenta o una ventana declarada en consumo observado. El usuario aún no eligió qué métricas merecen espacio.

En escritorio la terminal permanecería junto a la barra. En el celular se abriría el mismo conjunto de controles con espacio suficiente y regreso claro a la terminal. La colocación exacta espera el veredicto funcional. No se incluyen chat, otro selector de extensiones ni acciones globales por rellenar espacio.

## Condiciones para confiar en el control

1. Usar un contrato común de intención y resultado, con adaptadores por harness y versión. Elegir primero una operación comprobada en vivo; recurrir a reanudación solo cuando corresponda.
2. Fijar el destino al solicitar: servidor, pane, proceso y conversación. Cambiar de tab o de dispositivo no cambia el destinatario de esa solicitud.
3. Separar lo elegido de lo activo. Una tecla enviada o un proceso arrancado no confirma modelo, cuenta ni conversación.
4. Representar actividad como libre, trabajando, esperando permiso o desconocida. Una señal ausente o caducada no se convierte automáticamente en libre ni trabajando. Mostrar la acción apropiada sin una espera indefinida.
5. Conservar los locks, identificadores de reintento y snapshots existentes. No repetir comandos de resultado incierto después de reconectar.
6. Observar también los cambios hechos manualmente en la terminal. Barra y terminal comparten la misma evidencia de configuración.
7. Mantener el origen si falta una vía comprobada de recuperación o de transferencia. Mostrar la alternativa de abrir el destino por separado.
8. Limitar el trabajo en segundo plano al estado necesario: caché y eventos cuando existan, lectura acotada cuando no. No crear agentes, terminales adicionales ni cargar historiales completos para pintar la barra.

## Pruebas de aceptación propuestas

- Cambiar solo modelo, esfuerzo, cuenta y harness por separado en cada ruta declarada compatible. Comprobar identidad, configuración y continuidad reales.
- Repetir con turno activo, permiso pendiente, señales caducadas, CLI desconocido y adaptador sin operación en vivo.
- Cambiar de foco mientras la operación sigue en curso; el destino original permanece fijo.
- Cortar conexión después del envío y volver desde otro dispositivo; consultar el resultado sin repetir la operación.
- Hacer el cambio desde la terminal; comprobar que la barra lo refleja sin apropiarse del modelo de un pane vecino.
- Comprobar que una elección local no modifica preferencias globales, otras cuentas ni panes hermanos.
- Probar fallos de inicio, recuperación y confirmación con conversaciones de prueba, nunca con una sesión de trabajo como ensayo.
- Verificar el mismo comportamiento en escritorio y Android físico antes de declarar lista la versión 1.0.

## Próxima decisión

Resolver si la barra debe seguir al pane seleccionado y concentrar allí sus controles de IA. Recomendación: sí, con destino visible y operaciones pendientes ligadas al pane original. Después se eligen los controles esenciales, el comportamiento al cambiar de foco, la política de aplicación y finalmente su presentación visual.

## Comprobación de la retirada en el prototipo

Los tres scripts inline pasan la comprobación de sintaxis. Chrome remoto en la Mac mini comprobó las cinco variantes a 390×844: sin chat ni selector duplicado de MCPs en la barra, con el acceso del pane conservado y sin desbordamiento horizontal ni errores registrados. La variante E también se comprobó a 1440×1000. La ruta Tailscale devuelve HTTP 200 con la retirada aplicada.

La captura móvil se generó en la Mac mini, se transfirió explícitamente y se inspeccionó en /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/design/shots/sidebar/comandos-sidebar-without-chat-mobile.png. Es evidencia de la retirada, no una propuesta aprobada de la futura barra contextual. No se ejecutaron cambios de modelo, cuenta o harness en vivo.
