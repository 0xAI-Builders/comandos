# Control de MCPs y skills por sesión en los cinco CLIs

Verificado: 2026-09-24. Versiones probadas: Claude Code 2.1.282, Codex CLI
0.155.0, OpenCode 1.17.18, Grok Build 1.0.41 y Antigravity CLI (`agy`) 1.2.9,
en Linux (Ubuntu 22.04, kernel 6.8).

## Conclusión

Los cinco CLIs permiten fijar qué MCPs y qué skills carga **un solo proceso**
sin modificar la configuración global, y reanudar la misma conversación con un
set distinto. Claude, Codex y OpenCode lo hacen con mecanismos nativos (flags o
variables de entorno). Grok y Antigravity no tienen mecanismo por sesión; la vía
probada es sustituir su archivo de configuración solo para ese proceso con un
montaje en un *user namespace* de Linux.

Tres límites afectan a todo diseño que se construya encima:

1. **Apagar skills al reanudar no las retira del historial** en Claude, Codex y
   Grok: el listado inicial queda en la conversación, sigue costando tokens y el
   modelo las ve aunque ya no pueda llamarlas. Apagar skills rinde al crear la
   sesión, o reanudando con `--fork-session`. OpenCode y Antigravity rearman el
   listado en cada proceso.
2. **El ahorro en tokens depende del agente, no solo del MCP.** Codex y Grok
   cargan las definiciones de MCP bajo demanda: apagar un MCP ahorra
   aproximadamente 0 tokens por turno y solo evita arrancar su proceso. Claude
   las envía completas cuando la búsqueda de herramientas está apagada, como en
   esta máquina por el proxy local (38.9k tokens de MCPs medidos). OpenCode las
   envía completas.
3. **Ningún mecanismo puede enlazar archivos que el CLI reescribe con renombrado
   atómico.** Grok y Antigravity reemplazan un enlace simbólico por un archivo
   propio al guardar (credenciales OAuth, cachés). Un "hogar sombra" con
   credenciales enlazadas separa el token de renovación del real; eso ocurrió
   durante esta investigación (ver [Incidentes](#incidentes-durante-la-investigación)).

## Matriz por agente

| | Claude | Codex | OpenCode | Grok | Antigravity |
|---|---|---|---|---|---|
| MCPs por sesión | `--strict-mcp-config --mcp-config <archivo>`: solo quedan los del archivo | `-c mcp_servers.<nombre>.enabled=false` por cada uno que sobra | `OPENCODE_CONFIG_CONTENT` con `{"mcp":{"<n>":{"enabled":false}}}` | Config sustituida en el proceso: `disabled_mcp_servers=[...]` | Config sustituida en el proceso: `mcp_config.json` filtrado |
| Skills por sesión | `--settings` con `skillOverrides: {"<n>":"off"}` | `-c 'skills.config=[{name="<n>",enabled=false}]'` | `"permission":{"skill":{"<n>":"deny"}}` | Config sustituida: `[skills] disabled=[...]` | `skills.json` con `exclude`, o carpeta de skills elegidas |
| Skills de plugins | Solo el plugin entero: `enabledPlugins` en `--settings` | Por nombre, o el plugin entero con `plugins.<id>.enabled=false` | Por nombre | Por nombre | Por plugin en `config.json` |
| Modelo del set | Lista blanca | Lista negra | Lista negra | Lista negra | Lista blanca (archivo filtrado) |
| Reanudar con otro set | `--resume <id>` repitiendo todos los flags | `codex resume <id>` con los nuevos `-c` | `opencode -s <id>` con el nuevo JSON | `grok -r <id>` en el proceso con la nueva config | `agy --conversation <id>` con la nueva config |
| Skills tras reanudar | Nuevas se anuncian; apagadas siguen listadas | Se añade el nuevo catálogo; el viejo sigue | Listado nuevo | Listado congelado desde la creación | Listado nuevo |
| Costo por MCP | Completo (búsqueda de herramientas apagada tras proxy) | ~0 (herramientas diferidas) | Completo | ~0 (`search_tool`/`use_tool`) | UNVERIFIED |
| Costo de skills | Descripciones, tope 1% del contexto | Catálogo con tope 2% del contexto | Descripciones completas | Descripciones | Solo nombre y descripción |
| Medir costo sin llamar al modelo | `claude -p "/context"`: exacto por MCP y skill | `codex debug prompt-input`: skills exactas | No encontrado; hay que capturar la petición | `events.jsonl`: número de herramientas por MCP | No encontrado |
| Registro de uso | Transcript: `tool_use` `mcp__<srv>__<tool>` y `Skill` con `input.skill` | Rollout: `McpToolCall` con `server`/`tool`; skill como lectura de `SKILL.md` | `opencode.db`, tabla `part`: `<srv>_<tool>` y `tool="skill"` | `events.jsonl`: `mcp_tool_call_completed`; skill como `read_file` de `SKILL.md` | `transcript_full.jsonl`: `call_mcp_tool`; skill como `view_file` de `SKILL.md` |
| Saber qué cargó el proceso vivo | `/proc/<pid>/cmdline`; el transcript no guarda los flags | `/proc/<pid>/cmdline`; el rollout no guarda los MCPs | `/proc/<pid>/environ` | `events.jsonl`: `mcp_config_resolved` | UNVERIFIED |

Cada celda está respaldada en el archivo de evidencia del agente, con su fuente
(documentación, código fuente o prueba empírica):
[Claude](extensiones-por-sesion/claude.md),
[Codex](extensiones-por-sesion/codex.md),
[OpenCode](extensiones-por-sesion/opencode.md),
[Grok](extensiones-por-sesion/grok.md) y
[Antigravity](extensiones-por-sesion/agy.md).

## Mecanismo recomendado por agente

- **Claude.** Lanzar con `--strict-mcp-config --mcp-config <mcp.json>` y
  `--settings '<json>'` que combine `enabledPlugins` (plugins apagados) y
  `skillOverrides` (skills propias y de proyecto en `off`). El prompt inicial,
  si lo hay, va antes de `--mcp-config` porque ese flag acepta varios valores.
  Reanudar repitiendo todos los flags; sin ellos, la sesión vuelve al set global.
- **Codex.** Enumerar los servidores con `codex mcp list --json` y pasar
  `-c mcp_servers.<nombre>.enabled=false` por cada uno que sobra, sin comillas
  en el nombre. Las skills se apagan por `name` o por la ruta al archivo
  `SKILL.md` (una ruta a carpeta no tiene efecto). Pasar siempre algún `-c`: sin
  él, una TUI puede engancharse a un servidor de Codex compartido e ignorar la
  configuración del lanzamiento.
- **OpenCode.** Todo en `OPENCODE_CONFIG_CONTENT`, que no escribe nada a disco.
  No usar `OPENCODE_CONFIG` (OpenCode reescribe ese archivo) ni
  `OPENCODE_CONFIG_DIR` (instala un paquete npm ahí y se colgó más de 400 s en
  la prueba).
- **Grok.** Ejecutar `grok` en un *user namespace* privado donde un archivo
  generado se monta sobre `~/.grok/config.toml` solo para ese proceso
  ([`grok-ns.py`](extensiones-por-sesion/grok-ns.py), prueba de concepto de 15
  líneas). Credenciales, sesiones y cachés siguen siendo las reales. Dentro del
  namespace, `/mcps` y `grok mcp disable` fallan al escribir, así que un cambio
  hecho desde la TUI no puede volverse global. Añadir
  `GROK_{CLAUDE,CURSOR}_{MCPS,SKILLS}_ENABLED=false` si no se quieren los MCPs y
  skills que Grok importa de Claude y Cursor.
- **Antigravity.** El mismo montaje sobre `~/.gemini/config/mcp_config.json` y
  `~/.gemini/config/skills.json`. La prueba empírica usó `HOME=<sombra>`, que
  funciona pero filtra el `HOME` falso a todos los comandos que ejecuta el
  agente (git, ssh, npm); el montaje evita eso. El montaje en Antigravity
  **no está probado**.

## Consecuencias para el diseño de ComandOS

- **Un adaptador por agente** que traduzca el set deseado a argumentos, variables
  o archivos montados. Los tres modelos de set (lista blanca en Claude y
  Antigravity, lista negra en el resto) obligan a enumerar el catálogo completo
  en cada lanzamiento; si no, un MCP añadido después a la configuración global
  entra sin que nadie lo pida.
- **Manifiesto de lanzamiento propio.** Claude y Codex no guardan en sus
  archivos de sesión con qué MCPs arrancaron. ComandOS debe registrar el set de
  cada lanzamiento y compararlo con `/proc/<pid>/cmdline` o `environ` para
  confirmar lo cargado.
- **Costo mostrado por agente.** Las burbujas de MCP de Codex y Grok deben decir
  "bajo demanda" y no prometer ahorro. En Claude conviene medir con
  `claude -p "/context"` en lugar de estimar.
- **Skills de plugins de Claude.** Se mueven como grupo (todo el plugin).
- **Skills de Codex.** El catálogo tiene un tope de presupuesto: apagar skills
  sueltas casi no ahorra, porque las restantes ocupan el espacio. Lo que ahorra
  es un interruptor general (`skills.include_instructions=false`) o bajar
  `skills.max_context_tokens`.
- **Apagar skills en caliente** debe avisar que el listado anterior sigue en la
  conversación, y ofrecer continuar en una conversación derivada
  (`--fork-session`) cuando el objetivo sea ahorrar tokens.
- **Plataforma.** El montaje de Grok y Antigravity requiere Linux con *user
  namespaces* sin privilegios (`kernel.apparmor_restrict_unprivileged_userns=0`
  en esta máquina). En macOS esos dos agentes necesitarían un hogar sombra con
  sincronización de credenciales al terminar, o quedarían sin control por sesión.
- **Datos sensibles.** `opencode debug config` y `GET /config` imprimen los
  tokens *bearer* de la configuración del usuario en texto plano; nunca deben ir
  a logs ni a la UI.

## Defectos encontrados en ComandOS

- `lib/session_profiles.py:428` genera `-c mcp_servers."<nombre>".enabled=...`.
  Codex 0.155 divide la clave en cada punto y conserva las comillas como parte
  del nombre, así que falla con `invalid transport`. Los nombres con guion
  funcionan sin comillas; los nombres con punto no se pueden direccionar con
  `-c`.
- `adapters/agy-hooks.sh` identifica el proyecto por `workspacePaths[0]`, que va
  vacío cuando Antigravity arranca sin workspace registrado; en ese caso el
  adaptador no reporta nada.
- En Antigravity, un hook `PreToolUse` que devuelve `{}` **bloquea** la
  herramienta; debe devolver `{"decision":"allow"}` o `"ask"`. ComandOS no
  registra hoy ese hook; aplica si se añade.

## Incidentes durante la investigación

- **Token de Grok.** Una prueba con hogar sombra renovó el token OAuth de Grok,
  que ya estaba vencido. Grok guardó el par nuevo solo en la carpeta temporal y
  el `~/.grok/auth.json` real conserva el token de renovación anterior, que el
  servidor probablemente invalidó. Se guardó una copia del archivo renovado
  (permisos 0600) fuera del repositorio; el usuario decide si restaurarla o
  hacer `grok login`.
- **Saldo de Grok.** Todas las llamadas de Grok respondieron 402 *Grok Build
  usage balance exhausted*.
- **Codex.** El login de `~/.codex` ya tenía el token de renovación revocado
  (`refresh_token_invalidated`) antes de las pruebas; las de Codex se hicieron
  con un modelo simulado local.
- **Residuos de prueba** en los historiales reales, sin borrar:
  - Claude: 12 transcripciones en el proyecto `-tmp-claude-1000-ext-research-claude-work`.
  - Codex: 6 sesiones del 2026-09-24.
  - OpenCode: la sesión `ses_f2a212d84fferYYYMNZG0kZ2E1`.
  - Grok: una sesión en la carpeta de sesiones de `/tmp/claude-1000/ext-research-grok/cwd`.
  - Antigravity: la conversación `6efccada-3f30-4cb7-86b6-2747940e3a06`, que
    contiene la salida de `get_status` de WhatsApp.
- **Configuración.** Las huellas SHA-256 de `~/.claude/settings.json`,
  `~/.codex/config.toml`, `~/.config/opencode/opencode.json(c)`,
  `~/.grok/config.toml` y `~/.gemini/config/mcp_config.json` son idénticas
  antes y después. `~/.claude.json` cambió por las sesiones de Claude abiertas;
  sus secciones de MCPs y plugins son idénticas.

## Pendiente de verificar

- Claude: que apagar con `--settings` un plugin instalado que trae un MCP
  también detiene ese servidor, y el comportamiento en sesiones interactivas
  (solo se probó `-p`).
- Codex: reanudar desde la TUI (verificado solo en el código fuente).
- Grok: si `--fork-session` vuelve a generar el listado de skills.
- Antigravity: el montaje del archivo de configuración en lugar de `HOME`
  sombra, el costo por MCP y cómo saber qué cargó un proceso vivo.
