# Reparto de cuota: analizar, proponer, curar y aplicar — Diseño

Pestaña nueva **Reparto** en el modal Analytics, en lugar de Optimizar. Deja al usuario redistribuir sus sesiones vivas entre cuentas, motores, modelos y efforts en función de la cuota que le queda, y aplicar el resultado en un clic sin que ninguna sesión se rompa.

## Problema

Hoy Analytics dice "98 % de Grok, 25 % de Codex" y nada más. Con eso no se puede decidir:

- El porcentaje no dice si la cuota llega al reset. Fable main al 63 % con el reset en 3 días va a 1.3x del ritmo y se acaba en menos de dos; Grok al 6 % sobra. Ninguna de las dos lecturas sale del número.
- La pestaña Optimizar aplica planes de IDs de modelo congelados (`config/optimization-plans.json`), sin mirar cuota ni cuenta, en un bucle del navegador sin `requestId`, sin estado por sesión y con el resultado reducido a "3 iniciados, 2 fallaron".
- El cambio de cuenta se queda parado en "Do you trust the files in this folder?". `lib/claude_trust.ensure_cwd_trusted` existe, está probado y está desconectado a propósito (audit del 13 sep). El coordinador detecta el diálogo con un regex en inglés y deja la sesión en `awaiting_confirmation`, que además bloquea el pane para cambios futuros.
- La cuota de Codex y Grok es pasiva (se lee de rollouts y logs y desaparece si no se ha usado el CLI), y el pronóstico de agotamiento (`token_guard_with_forecast`) solo existe para Claude main semanal.

Lo que el usuario necesita, en sus palabras: un botón que analice y **proponga** una configuración, que curarla sea "sumamente sencillo", y que al aplicar "realmente no se rompa y sea smooth".

## Decisiones

Validadas en 8 rondas de `grill-design` (prototipo en la rama `prototype/reparto-tanques`, `dash/prototypes/reparto-round8-final.html`).

1. **Flujo en cuatro pasos, una sola vista:** panorama de cuotas → **Analizar** rellena una propuesta → el usuario la **cura** tocando lo que quiera → **Aplicar**. No hay asistente ni pantallas intermedias.
2. **La importancia la decide el usuario; el reparto lo decide el motor.** El motor no adivina qué sesión importa. Toma la importancia (implícita en el tanque y el effort donde el usuario deja cada ficha) y elige el resto para que ninguna cuota se acabe antes de su reset.
3. **Aplicar reinicia ya.** Todas las sesiones afectadas se reinician en el momento con la nueva configuración, conservando la conversación (`--resume`). Las que están a mitad de turno pierden ese turno. Decisión explícita del usuario frente a "esperar a que terminen".
4. **Trust heredado.** Si la carpeta ya estaba aceptada en la cuenta origen (HOME o config dir de origen), ComandOS la marca aceptada en la cuenta destino antes de lanzar. Nunca acepta una carpeta que el usuario no haya aceptado antes en ninguna cuenta. Sustituye la política "dejar el diálogo al usuario" solo para este caso.
5. **Diseño: tanques.** Cada cuota es un tanque cuyo líquido es lo que habrá gastado cuando se renueve. Las sesiones son fichas arrastrables. Las cuotas reaccionan **durante** el arrastre, no al soltar.
6. **Lenguaje llano.** Nada de códigos (F/M/L, "aguanta"). Cada cuota dice "llega al reset" o "se acaba en 1d 22h", más "ritmo 1.3x" con su etiqueta. Cada ficha muestra modelo y effort en texto, siempre.
7. **Determinista.** Mismas entradas, misma propuesta. Sin LLM en el motor. La propuesta se congela con un hash del estado y aplicar rechaza lo que cambió.

## Interfaz

### Tanques (cabecera)

Cuatro tanques en fila, uno por cuota que hoy tiene fuente: Claude main, Claude relotto, Codex, Grok. Si aparece una cuenta o proveedor con cuota, aparece un tanque; sin fuente de cuota no hay tanque, y sus sesiones van en un grupo "sin cuota conocida" no arrastrable como destino.

Cada tanque (estilo "dos capas"):

- **Capa sólida:** porcentaje ya gastado (`limits[].percent`).
- **Capa translúcida encima:** lo que las sesiones colocadas en ese tanque añadirán hasta el reset, según la proyección (ver Motor). Sube y baja con animación al soltar una ficha y, en punteado, mientras una ficha está encima.
- **Desborde:** si la proyección pasa de 100 %, espuma y borde en rojo.
- **Texto:** nombre de la cuenta, "llega al reset" (verde) o "se acaba en X" (rojo), "ritmo N.Nx", y "reset en X". El ritmo es `usado ÷ fracción de ventana transcurrida`; 1.0x significa gastar justo lo que llega al reset.
- Cada proveedor con ventana corta (Claude y Codex, 5 h) muestra la ventana corta como línea fina bajo el nombre, sin segundo tanque.

En angosto (panel < 620 px, container query sobre el panel, no sobre la ventana) el tanque cambia a **barra horizontal**, una fila por cuenta, misma información. A < 440 px las fichas van en una sola lista con el nombre de la cuenta como separador.

### Fichas (una por pane vivo)

Estilo "antes → después con selectores":

```
● C  SAVA ⫽10
     fable-5-1 · xhigh  →  [fable-5-1|opus-5|sonnet-5]  [low|medium|high]
```

- Punto de estado (trabajando, esperando, terminó, inactiva), glifo de proveedor, nombre de sesión y pane.
- "Antes": modelo y effort actuales, tachados si cambian.
- "Después": dos mini segmentados con **todas** las opciones válidas para el motor del tanque donde está la ficha. Un toque cambia el valor. El segmentado cuyo valor difiere del actual va en color de marca; el que no cambia, en gris.
- Las opciones salen de `config/providers.json` (`motors[m].models[].efforts`) filtradas por `validate_selection` y `session_change_support`. Lo que no se puede en caliente no aparece.
- Arrastrar una ficha a otro tanque cambia motor y cuenta; el modelo pasa al de la misma capa en el motor nuevo (ver Motor) y el effort se conserva. La ficha queda con borde punteado ("movida por ti"). Clic en la ficha la arma y clic en un tanque la coloca, para pantallas táctiles y como respaldo del drag.
- En angosto, al empezar a arrastrar aparece una **barra de destinos** pegada al pie del panel con los cuatro tanques en miniatura; sirve cuando los tanques ya no están a la vista.

### Pie

- Resumen: "11 cambios · 5 igual · 3 movidas por ti".
- **Propuesta original**: descarta la curación y vuelve a lo que propuso el motor.
- **Analizar**: solo aparece antes de la primera propuesta. Después, cada cambio del usuario recalcula en vivo y el botón no vuelve.
- **Aplicar N**: botón simple. Al pulsar, el pie se convierte en el **panel de progreso**.

### Panel de progreso

- Barra global "5 / 11" y cada sesión afectada como un paso con su estado: en cola, aplicando, lista, detenida (con motivo: trust, login, timeout, identidad cambió).
- Las fichas de los tanques muestran el mismo estado.
- **Reintentar** por paso detenido. **Revertir todo** aparece solo cuando el lote terminó (todas listas o detenidas); revierte escribiendo un plan inverso, no deshaciendo el historial.
- Al terminar sin detenidas: aviso verde "Listo: 11 sesiones reconfiguradas" y los tanques muestran ya la nueva proyección como capa sólida.

## Motor de propuesta (`lib/allocation.py`, puro)

Entradas: sesiones vivas (`/state`: session, pane, agent, model, effort, account, status, cwd), cuotas (`limits[]` con `percent`, `resets_at`, `window`, `daily`), matriz de rutas (`provider_registry`), cuentas seleccionables (`list_accounts`).

Salida: `Plan` con `planId`, `stateHash`, `createdAt`, y por pane `{from, to, reason, risk}` más `impact` por cuota (`burnNow`, `burnAfter`, `runsOutIn`, `reachesReset`).

Reglas, en orden:

1. **Capa por sesión.** Cada sesión tiene una capa 3/2/1 que sale de su effort actual (`xhigh|max → 3`, `high → 3`, `medium → 2`, `low → 1`) y del estado: `done` e `idle` bajan una capa. El usuario la cambia al arrastrar o tocar effort. Capa → effort destino: 3 `high`, 2 `medium`, 1 `low`. Capa → modelo por motor: tabla `config/model-tiers.json` (`tiers.high/mid/low` ya existen); nunca IDs congelados en código.
2. **Candidatos.** Para cada sesión, los pares (motor, cuenta) que `session_change_support` marca cambiables en caliente desde su harness actual, más quedarse donde está.
3. **Carga.** Peso por effort: `low .5, medium 1, high 1.6, xhigh 2.2, max 3`. El ritmo proyectado de una cuota escala con la suma de pesos que le caen frente a la que tenía: `burnAfter = burnNow × pesoNuevo ÷ pesoActual`. Para una cuota casi sin uso (`percent < 3`) se usa `burnAfter = pesoNuevo × 0.06` para no dividir por ruido.
4. **Asignación.** Sesiones en orden de capa descendente y luego por nombre (determinismo). Cada una va al candidato cuya cuota más apretada, tras colocarla, tiene más margen hasta el reset. Empates: quedarse igual, luego mismo motor, luego la cuenta con reset más lejano.
5. **Restricción dura.** Ninguna asignación puede dejar una cuota con `burnAfter > 1.0` si existe un candidato que no lo haga. Si no existe, se elige el que menos se pasa y la propuesta lo dice en `reason`.
6. **La capa 3 no cambia de motor.** Una sesión que el usuario marcó como la más importante se reparte entre las cuentas de su propio motor, no salta a otro. Cambiar de cuenta reparte la misma familia de modelos; cambiar de motor cambia el modelo debajo del trabajo más importante, y eso lo decide el usuario arrastrando la ficha. Las capas 2 y 1 son la válvula de escape del reparto. Si su motor no ofrece ningún candidato seleccionable, se permite el salto.
7. **La cuota que paga es la del motor.** Una cuota se identifica por `(motor, motorAccount)`, no por la cuenta del harness: en una ruta gateway (`claude:codex`) los tokens los gasta la suscripción de Codex bajo `motorAccount`, que siempre es `main`. Agrupar por la cuenta del harness inventaría cuotas que no existen.
8. **Una cuota sin sesiones vivas se escala contra la carga media de la flota**, no contra cero: su ritmo actual se midió con una carga que ya no está, y escalarlo desde cero la haría parecer varias veces peor de lo que es, con lo que ninguna sesión se movería nunca a ella.
9. **Razón y riesgo por fila.** `reason` en una línea ("Fable relotto tiene más margen: 65 %, reset en 3d 4h"). `risk`: bajo si solo cambia effort, medio si cambia modelo en el mismo motor, alto si cambia motor o cuenta.

El mismo módulo calcula `impact()` para la vista previa en vivo con una sesión colocada tentativamente. Es la única fuente de las cifras que muestra la UI; el navegador no recalcula por su cuenta.

## Endpoints (`bin/cc-dash`)

- `POST /allocation/propose` `{overrides?: {[pane]: {layer|to}}}` → `Plan`. Idempotente sobre el mismo estado: mismo `stateHash`, mismo plan.
- `POST /allocation/preview` `{planId, pane, target}` → `impact` recalculado sin persistir. Lo usa el arrastre.
- `POST /allocation/apply` `{planId, panes?: [...]}` → `202 {batchId}`. Rechaza con `409 plan_stale` si `stateHash` ya no coincide (una sesión cambió de config o murió), listando qué panes cambiaron.
- `GET /allocation/status?batchId=` → `{state, done, total, items: [{session, pane, state, error?, recoveryAllowed}]}`.
- `POST /allocation/revert` `{batchId}` → nuevo lote con el plan inverso.

### Lote sobre el coordinador existente

Cada pane del lote es un `POST /session/configure` interno con `interrupt: true`, `requestId = "{batchId}:{session}:{pane}"` (idempotente: reintentar el lote no duplica), y `expectedIdentity` / `expectedConversationId` del snapshot del plan. Concurrencia 3, sin `max-errors`: un pane detenido no frena a los demás. El estado por pane es el del coordinador (`validating → applying → verifying → confirmed | awaiting_confirmation | failed`), traducido a las cuatro palabras de la UI. `awaiting_confirmation` se muestra como "detenida" con el motivo detectado en pantalla.

El coordinador se toca en tres puntos:

1. **Trust heredado** en `SessionConfiguration.prepare()`: si `to.harnessAccount != from.harnessAccount` y la cwd está en `projects[]` con `hasTrustDialogAccepted` en HOME o en el config dir de origen, llamar `ensure_cwd_trusted(cwd, config_dir=destino)` antes de `apply()`. Misma llamada en `/session-new`, `/up` y `/recover-tab`, que hoy lanzan a ciegas. Se registra en el `usage_changes` como `trust_inherited`.
2. **Detección de diálogos** en `_verify`: el regex pasa a una lista en `config/detectors.json` (trust, login, onboarding, en inglés y español) y `pending_confirmation` también la aplica, para que una sesión parada en un diálogo se reporte como "detenida: trust" y no como timeout opaco.
3. **Ventana de verificación**: 30 s → 90 s cuando el destino declara MCPs (arranque frío medido en 8.8 s sin MCPs).

## Cuotas: fuente y proyección

- `limits[]` sigue siendo la fuente. Se añade `pace`, `burn`, `runsOutIn`, `reachesReset` calculados en `cc_usage` para todas las cuotas, no solo Claude main semanal; `token_guard_with_forecast` los consume en vez de recalcular.
- Codex: la fila no se descarta al 1.5× de su ventana; se marca `stale: true` con `captured_at` real y el tanque lo dice ("cuota de hace 2 d"). Grok igual con `unified.jsonl`.
- Una cuota `stale` se puede usar en el reparto; la UI lo advierte en el tanque.

## Errores

- Plan caducado al aplicar: 409, la UI marca las fichas cambiadas y ofrece "Volver a analizar".
- Pane con operación no terminal (`awaiting_confirmation`, `recovery_required`): el plan lo excluye y lo muestra con candado "bloqueada: pendiente de recuperar", con enlace a `/session/recover`.
- Ruta no cambiable en caliente (opencode, agy, 4 de 5 ACP): la ficha no se puede soltar en tanques de otro motor; el tanque lo rechaza en el arrastre con el motivo de `session_change_support`.
- Fallo a mitad de lote: los panes ya confirmados quedan; los detenidos se reintentan uno a uno; "Revertir todo" genera el plan inverso solo para los confirmados.

## Pruebas

- `tests/test_allocation.py`: motor puro. Determinismo (mismo input, mismo plan, 100 iteraciones), restricción dura, empates, capas por estado, impacto en vivo igual al plan, cuota stale, sesión sin cuota conocida.
- `tests/test_allocation_batch.py`: lote sobre `session_operations` con el `Boundary` de `test_session_route_matrix`: idempotencia por `requestId`, `plan_stale`, concurrencia 3 con un pane detenido, revert como plan inverso, exclusión de panes bloqueados.
- `tests/test_claude_trust.py`: nuevos casos de herencia: acepta solo si origen lo tenía, nunca en `$HOME`, registra `trust_inherited`; los casos existentes "launch never accepts trust" pasan a "launch accepts trust only when inherited".
- `tests/test_dashboard_reparto.py` + `tests/e2e_reparto.cjs`: render de tanques con `limits` reales, arrastre con vista previa, angosto a 560 y 400 px con barra de destinos, progreso y revert, cero errores JS.

## Fuera de alcance

- Cambiar cuenta en rutas gateway (`claude:codex`, `claude:grok`) y en ACP: siguen sin exportador; la ficha no ofrece esas cuentas.
- Nuevas fuentes de cuota (Gemini, Antigravity, OpenCode): sin tanque hasta que exista fuente.
- Aprender de resultados (ratings, experimentos): el motor no usa `usage_experiments`.
- Sustituir la pestaña Comparar ni tocar Resumen.
