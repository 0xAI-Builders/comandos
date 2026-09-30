# Barra de comandos por CLI, cadenas y cabecera ordenada: plan de implementación

> **Para el agente implementador:** usar `superpowers:subagent-driven-development` (recomendado) o `superpowers:executing-plans` tarea por tarea. Las casillas `- [ ]` son trabajo futuro, no resultados. Aplican la preparación y las restricciones de /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/docs/superpowers/plans/2026-09-29-commandos-v1/README.md.

**Objetivo:** sustituir la barra izquierda del dashboard por un catálogo de comandos por CLI instalado que se escriben en el pane con un clic y sin Enter, con cadenas guardadas que se corren paso a paso, terminales rápidas en lugar del chat, y la cabecera "Ordenada" con el botón Servidores abriendo la interfaz SSH que ya existe.

**Arquitectura:** el servidor `bin/cc-dash` expone un catálogo curado (`config/cli-commands.json`) verificado contra la versión instalada de cada binario, una ruta que teclea texto literal en un pane de tmux letra por letra y nunca envía Enter, y un almacén de cadenas en archivos Markdown editables. El cliente web (`dash/`) pinta la barra con módulos JavaScript puros probados con Node, sin tocar el motor de configuración de sesiones ni Servidores. GTK sigue mostrando el mismo dashboard en su WebView.

**Tecnologías:** Python 3.10 (stdlib), tmux, JavaScript sin framework, CSS existente, pytest (`~/.local/bin/pytest`), Node para los módulos de cliente, Chrome remoto en la Mac mini vía MCP `chrome-bg`.

**Especificación:** veredictos "Fase 2" desde "Fase 2 · propuesta de Jesús y ronda 6" hasta "Cierre del grilling de fase 2" en /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/README-v1-grill.md; composición aprobada en /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/prototype-v2-barra.html (sin parámetros); sección "Barra izquierda: comandos por CLI y terminales rápidas" de /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/design/DESIGN.md; catálogo fuente /home/someguy/codebase/0xJesus/ComandOS/docs/tui-command-map.md.

## Restricciones globales

- Base: `main` en `0561687` (2026-09-30). Incluye la fase 1 (terminales rápidas E2 `733aa14`, avisos, Pomodoro, push, ediciones) **y el grill de fidelidad del 29–30 de septiembre ya implementado en main**: botones de cabecera de un solo tamaño con cinco estilos 3D (`3a172dc`, `b452cd1`), reloj de arena de Davitheoles con el tiempo real del Pomodoro (`afaf580`), avisos en un cajón que abre la campana (`9db3c1f`, `1e995a1`), Resúmenes junto a la terminal (`c47fcdf`), semáforos 8-bit, bandejas y «Ordenar una vez» en las tabs (`cf75733`, `71bee81`, `ae9e47b`), Sí/No en la barra móvil y en las píldoras de pane (`c11fd8a`, `15f1d6d`). Todas las líneas de este plan están resueltas contra `0561687`; verificar con `git -C /home/someguy/codebase/0xJesus/ComandOS log --oneline -1 main` antes de crear el checkout y, si main avanzó, volver a resolver los anclajes con `grep -n` antes de editar.
- Conservar intactas esas entregas del 29–30: `dash/buttons.css` y `html[data-btn-style]` (`tests/test_button_styles.py`), las reglas de cabecera `--hdr-key`/`.hdr-lbl` de `dash/workspace.css:584–624`, `#btn-pomo` con el reloj que pinta `dash/pomodoro.js:331`, `#btn-notif` + `#notif-badge` que alternan el cajón (`window.ComandosNotices.instance.toggleStrip()`), `#btn-news` (Resúmenes, `dash/news-reader.js` se monta en `#panes` y reparte ancho con `#term-area`), `#tab-sort` ⇅ en la fila de tabs, stickers y bandejas de `work-marks.js`/`workspace-dock.js`. Todo botón nuevo de cabecera lleva `class="hdr-btn"`, `data-icon` y `<span class="hdr-lbl">` para heredar el estilo elegido.
- Todo archivo nuevo bajo `dash/` se añade a la lista de symlinks de `install.sh:71–80`; sin esa línea el dashboard instalado (`~/.claude/hooks/dash`) responde 404. La puerta de token ya sirve `*.css`/`*.js` reales sin cambios (`PUBLIC_ASSET_RE`, `bin/cc-dash:8675–8692`).
- Checkout aislado propuesto: /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-fase2 en la rama `implementation/comandos-v1-fase2`. Todas las rutas de cambios apuntan ahí. No trabajar en el checkout principal ni en `.worktrees/comandos-v1-implementation` (candidato de fase 1 pendiente de activación R2/R3).
- Un clic **nunca** envía Enter. Ninguna ruta nueva puede llamar a `tmux send-keys … Enter`. El usuario da Enter en la terminal.
- Nada automático: sin esperas por "terminó", sin detección de turno para avanzar una cadena, sin cambios de modelo/cuenta/harness orquestados. Un paso avanza solo con Siguiente.
- El catálogo se ancla a versiones: Codex `0.154.0`, Claude Code `2.1.268`, Grok Build `1.0.25`, OpenCode `1.17.18`, Antigravity `1.1.25` (agy sin verificar). Versión instalada distinta = estado `drift` (ámbar, "sin verificar", clic disponible, nota "el CLI confirma"). Binario ausente = `missing`.
- Flags yolo desde /home/someguy/codebase/0xJesus/ComandOS/config/agent-roles.json (`dangerFlags`), nunca literales duplicados en JS.
- Servidores no se toca: `#ssh-bar`, `loadSsh`, `openSshTab`, `connectHost`, `setupSshKey`, `toggleSshManager` y las rutas `/ssh*` de `bin/cc-dash` quedan como están. Solo cambia dónde se muestra la fila.
- El chat de CommandOS se retira del cliente (revoca E4). Los datos de conversaciones en disco no se borran.
- Navegador solo con MCP `chrome-bg` en la Mac mini; exponer el puerto con /home/someguy/.local/bin/cc-browser-expose. `tests/test_sidebar_parity.py` se salta aquí por política (no hay Chrome local); su intención se cubre con checks Node y con la verificación remota registrada.
- Pruebas destructivas solo con sesiones tmux propias creadas por la prueba (socket privado, `unset TMUX`), nunca con panes reales. No relanzar la app viva.
- Commits pequeños con el mensaje indicado y la línea `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.

## Foco de revisión

Entradas que ninguna prueba de una tarea cubriría por sí sola; cada línea tiene su prueba en la tarea indicada.

1. Texto con salto de línea o carácter de control enviado a `/pane/type` (por ejemplo un comando pegado con `\n`): debe rechazarse con 400, no teclearse parcialmente ni convertirse en Enter. Prueba en T1.
2. Dos clics seguidos sobre el mismo pane mientras el primero aún teclea: el segundo responde 409 y no interleva letras. Prueba en T1.
3. El CLI del pane cambia mientras el catálogo está en pantalla (el usuario hizo `/exit`): la barra no debe seguir marcando "en este pane" ni ofrecer comandos `/…` como activos en una shell. Prueba en S1 (re-render con `cliInPane` vacío).
4. Archivo de cadena editado a mano con un paso mal escrito (`- foo: …`): la cadena se lista con error legible y no se corre; las demás cadenas siguen. Prueba en K1.
5. Pane de destino cerrado entre dos pasos de una cadena: Siguiente recibe 404 y la tarjeta muestra el error sin saltar de paso ni cambiar de pane. Prueba en S1 y T1.

---

## Preparación

- [ ] Comprobar Git y crear el checkout:

```sh
git -C /home/someguy/codebase/0xJesus/ComandOS status --short
git -C /home/someguy/codebase/0xJesus/ComandOS worktree list
git -C /home/someguy/codebase/0xJesus/ComandOS worktree add -b implementation/comandos-v1-fase2 /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-fase2 main
cd /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-fase2
~/.local/bin/pytest -q tests/test_quick_terminal.py tests/test_tui_state.py tests/test_button_styles.py tests/test_dashboard_security.py
~/.local/bin/pytest -q tests/ 2>&1 | tail -3 > /tmp/claude-1000/comandos-fase2-baseline.txt
```

Esperado: las cuatro suites pasan. La corrida completa fija la **línea base** de fallos preexistentes (main lleva pruebas de extensiones que requieren el paquete `mcp`, commit `d56c874`); anotar esa línea en el registro de aceptación y comparar contra ella en V1. `tests/conftest.py` aísla la base SQLite en `COMANDOS_STATE_DB`.

- [ ] Crear /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-fase2/docs/verification/commandos-v1-fase2.md con el encabezado `# CommandOS fase 2 — registro de aceptación` y una tabla `| Tarea | Commit | Pruebas | Navegador (Mac mini) | Revisión humana |`. Se rellena al cerrar cada tarea.

---

### Tarea C1: catálogo curado por CLI con verificación de versión

**Archivos:**
- Crear: `config/cli-commands.json`
- Crear: `lib/cli_catalog.py`
- Crear: `tests/test_cli_catalog.py`

**Interfaces:**
- Consume: `config/agent-roles.json` (`dangerFlags`), `config/providers.json` vía `lib/providers.load_registry()` y `lib/providers.which(binary)`, `lib/accounts.list_accounts(registry, provider)` y `lib/accounts.account_environment(registry, provider, alias)`.
- Produce: `load_catalog(path=None) -> dict`, `catalog_view(catalog, *, danger_flags, versions, accounts) -> dict`, `installed_versions(catalog, which=providers.which, run=subprocess.run) -> dict[str, str|None]`, `version_status(pinned, installed, verified) -> str`.

- [ ] **Paso 1: escribir el catálogo** en `config/cli-commands.json`. Fuente de nombres: /home/someguy/codebase/0xJesus/ComandOS/docs/tui-command-map.md (solo comandos listados ahí; aliases en `aliases`). Descripciones: las del mockup (`CAT` en prototype-v2-barra.html líneas 271–281 y `CAT.grok/opencode/agy`). Plantillas de arranque con `{bin}`, `{danger}` y `{resume}`:

```json
{
  "version": 1,
  "source": "docs/tui-command-map.md",
  "clis": [
    {
      "id": "claude", "label": "Claude Code", "binary": "claude",
      "pinnedVersion": "2.1.268", "verified": true,
      "launch": {"resume": "--resume <id>", "extra": ["{bin} {danger} --model claude-fable-5-1 --effort max"]},
      "groups": [
        {"title": "Modelo y esfuerzo", "icon": "brain", "commands": [
          {"text": "/model ", "description": "Cambia el modelo de esta conversación; sin argumento abre el selector.", "args": ["claude-fable-5-1", "claude-opus-5-5", "claude-sonnet-5-5"]},
          {"text": "/effort ", "description": "Nivel de razonamiento.", "args": ["low", "medium", "high", "max"]},
          {"text": "/fast", "description": "Salida rápida con Opus."},
          {"text": "/plan", "description": "Modo plan: propone antes de tocar código."}
        ]},
        {"title": "Conversación", "icon": "cycle", "commands": [
          {"text": "/resume", "description": "Elige una conversación anterior y la reanuda."},
          {"text": "/fork", "description": "Bifurca esta conversación en otra."},
          {"text": "/compact", "description": "Resume el contexto para liberar ventana."},
          {"text": "/clear", "description": "Limpia el historial de la sesión."},
          {"text": "/exit", "description": "Sale del CLI; el pane vuelve a la shell."}
        ]},
        {"title": "Cuenta", "icon": "key", "commands": [
          {"text": "/login", "description": "Inicia sesión solo en esta terminal."},
          {"text": "/logout", "description": "Cierra la sesión de esta terminal."},
          {"text": "/cost", "description": "Consumo de esta conversación."}
        ]},
        {"title": "Contexto y herramientas", "icon": "map", "commands": [
          {"text": "/context", "description": "Qué ocupa la ventana ahora."},
          {"text": "/add-dir ", "description": "Añade una carpeta al contexto."},
          {"text": "/mcp", "description": "Servidores MCP conectados."},
          {"text": "/agents", "description": "Subagentes disponibles."}
        ]}
      ]
    },
    {"id": "codex", "label": "Codex", "binary": "codex", "pinnedVersion": "0.154.0", "verified": true,
     "launch": {"resume": "resume <id>", "extra": ["{bin} --full-auto"]}, "groups": []},
    {"id": "grok", "label": "Grok Build", "binary": "grok", "pinnedVersion": "1.0.25", "verified": true,
     "launch": {"resume": "--resume <id>", "extra": []}, "groups": []},
    {"id": "opencode", "label": "OpenCode", "binary": "opencode", "pinnedVersion": "1.17.18", "verified": true,
     "launch": {"resume": "--session <id>", "extra": [], "yoloNote": "sin flag de arranque conocido; el modo se cambia dentro con /agents"}, "groups": []},
    {"id": "agy", "label": "Antigravity", "binary": "agy", "pinnedVersion": "1.1.25", "verified": false,
     "launch": {"resume": "--conversation <id>", "extra": []}, "groups": []}
  ]
}
```

Rellenar `groups` de codex, grok, opencode y agy con los comandos del mapa y las descripciones del mockup (mismo formato que claude). Los `args` son los del mockup; no inventar modelos que no estén en `config/providers.json` o en el mockup. La prueba del paso 2 exige que cada `text` aparezca en /home/someguy/codebase/0xJesus/ComandOS/docs/tui-command-map.md: si un comando del ejemplo (p. ej. `/fast` o `/cost`) no está en el mapa de su CLI, quitarlo del JSON en lugar de relajar la prueba.

- [ ] **Paso 2: escribir las pruebas que fallan** en `tests/test_cli_catalog.py`:

```python
import json, subprocess
from pathlib import Path
import pytest

ROOT = Path(__file__).resolve().parents[1]
import sys; sys.path.insert(0, str(ROOT / "lib"))
import cli_catalog

DANGER = {"claude": "--dangerously-skip-permissions", "codex": "--dangerously-bypass-approvals-and-sandbox",
          "opencode": "", "grok": "--always-approve", "agy": "--dangerously-skip-permissions"}
VERSIONS = {"claude": "2.1.268", "codex": "0.154.0", "grok": "1.0.25", "opencode": "1.17.18", "agy": "1.1.25"}

def test_catalog_file_matches_repo_danger_flags_and_map_versions():
    cat = cli_catalog.load_catalog()
    roles = json.loads((ROOT / "config" / "agent-roles.json").read_text())["dangerFlags"]
    ids = [c["id"] for c in cat["clis"]]
    assert ids == ["claude", "codex", "grok", "opencode", "agy"]
    assert set(ids) <= set(roles)
    text = (ROOT / "docs" / "tui-command-map.md").read_text()
    for cli in cat["clis"]:
        assert cli["pinnedVersion"] in text, cli["id"]
        for grp in cli["groups"]:
            for cmd in grp["commands"]:
                assert cmd["text"].strip() in text, (cli["id"], cmd["text"])
                assert "\n" not in cmd["text"] and cmd["description"]

def test_yolo_launches_come_first_and_use_danger_flag():
    cat = cli_catalog.load_catalog()
    view = cli_catalog.catalog_view(cat, danger_flags=DANGER, versions=VERSIONS, accounts={})
    claude = next(c for c in view["clis"] if c["id"] == "claude")
    assert claude["launch"]["yolo"][:2] == [
        "claude --dangerously-skip-permissions",
        "claude --dangerously-skip-permissions --resume <id>"]
    assert claude["launch"]["normal"][:2] == ["claude", "claude --resume <id>"]
    codex = next(c for c in view["clis"] if c["id"] == "codex")
    assert codex["launch"]["yolo"][1] == "codex --dangerously-bypass-approvals-and-sandbox resume <id>"
    opencode = next(c for c in view["clis"] if c["id"] == "opencode")
    assert opencode["launch"]["yolo"] == [] and "sin flag" in opencode["launch"]["yoloNote"]

def test_account_launches_use_config_dir_env():
    cat = cli_catalog.load_catalog()
    accounts = {"claude": [{"alias": "relotto", "env": {"CLAUDE_CONFIG_DIR": "/home/u/.claude-accounts/relotto"}}]}
    view = cli_catalog.catalog_view(cat, danger_flags=DANGER, versions=VERSIONS, accounts=accounts)
    claude = next(c for c in view["clis"] if c["id"] == "claude")
    assert "CLAUDE_CONFIG_DIR=/home/u/.claude-accounts/relotto claude --dangerously-skip-permissions" in claude["launch"]["normal"]

@pytest.mark.parametrize("pinned,installed,verified,expected", [
    ("2.1.268", "2.1.268", True, "ok"), ("2.1.268", "2.1.270", True, "drift"),
    ("2.1.268", None, True, "missing"), ("1.1.25", "1.1.25", False, "unverified")])
def test_version_status(pinned, installed, verified, expected):
    assert cli_catalog.version_status(pinned, installed, verified) == expected

def test_installed_versions_runs_resolved_path_and_tolerates_failures():
    cat = cli_catalog.load_catalog()
    calls = []
    def which(b): return {"claude": "/opt/claude", "codex": "/opt/codex"}.get(b)
    def run(cmd, **kw):
        calls.append(cmd)
        if cmd[0] == "/opt/codex": raise subprocess.TimeoutExpired(cmd, 1)
        return subprocess.CompletedProcess(cmd, 0, stdout="2.1.268 (Claude Code)\n", stderr="")
    versions = cli_catalog.installed_versions(cat, which=which, run=run)
    assert versions == {"claude": "2.1.268", "codex": "?", "grok": None, "opencode": None, "agy": None}
    assert calls[0] == ["/opt/claude", "--version"]

def test_view_never_contains_enter_or_newlines():
    cat = cli_catalog.load_catalog()
    view = cli_catalog.catalog_view(cat, danger_flags=DANGER, versions=VERSIONS, accounts={})
    for cli in view["clis"]:
        for text in cli["launch"]["yolo"] + cli["launch"]["normal"]:
            assert "\n" not in text and "\r" not in text
```

- [ ] **Paso 3: correr para ver el fallo**

Run: `~/.local/bin/pytest -q tests/test_cli_catalog.py`
Esperado: FAIL con `ModuleNotFoundError: cli_catalog`.

- [ ] **Paso 4: implementar `lib/cli_catalog.py`**

```python
"""Catálogo curado de comandos por CLI, verificado contra el binario instalado.

El catálogo nunca teclea nada: solo describe. Los arranques yolo salen de
config/agent-roles.json; las cuentas, de los directorios reales de cuentas.
"""
from __future__ import annotations
import json, os, re, subprocess
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
CATALOG_FILE = REPO_ROOT / "config" / "cli-commands.json"
_VERSION_RE = re.compile(r"\d+\.\d+[\w.-]*")


class CatalogError(ValueError):
    pass


def load_catalog(path=None):
    p = Path(path) if path else CATALOG_FILE
    with open(p, encoding="utf-8") as fh:
        cat = json.load(fh)
    if not isinstance(cat, dict) or cat.get("version") != 1 or not isinstance(cat.get("clis"), list):
        raise CatalogError("catálogo inválido: se esperaba {version: 1, clis: [...]}")
    seen = set()
    for cli in cat["clis"]:
        for key in ("id", "label", "binary", "pinnedVersion", "launch", "groups"):
            if key not in cli:
                raise CatalogError(f"cli sin '{key}': {cli.get('id')}")
        if cli["id"] in seen:
            raise CatalogError(f"cli repetido: {cli['id']}")
        seen.add(cli["id"])
        for grp in cli["groups"]:
            for cmd in grp.get("commands", []):
                if not cmd.get("text") or not cmd.get("description") or "\n" in cmd["text"]:
                    raise CatalogError(f"comando inválido en {cli['id']}: {cmd}")
    return cat


def version_status(pinned, installed, verified=True):
    if installed is None:
        return "missing"
    if not verified:
        return "unverified"
    return "ok" if str(installed) == str(pinned) else "drift"


def installed_versions(catalog, which=None, run=subprocess.run):
    if which is None:
        from providers import which as _which
        which = _which
    out = {}
    for cli in catalog["clis"]:
        exe = which(cli["binary"])
        if not exe:
            out[cli["id"]] = None
            continue
        try:
            r = run([exe, "--version"], capture_output=True, text=True, timeout=15)
            m = _VERSION_RE.search((r.stdout or "") + (r.stderr or ""))
            out[cli["id"]] = m.group(0) if m else "?"
        except (subprocess.SubprocessError, OSError):
            out[cli["id"]] = "?"
    return out


def _launches(cli, danger, accounts):
    b, resume = cli["binary"], cli["launch"].get("resume", "--resume <id>")
    yolo, normal = [], [f"{b}", f"{b} {resume}"]
    if danger:
        yolo = [f"{b} {danger}", f"{b} {danger} {resume}"]
    for extra in cli["launch"].get("extra", []):
        text = extra.replace("{bin}", b).replace("{danger}", danger or "").replace("{resume}", resume)
        (yolo if danger and "{danger}" in extra else normal).append(re.sub(r"\s{2,}", " ", text).strip())
    for acc in accounts.get(cli["id"], []):
        env = " ".join(f"{k}={v}" for k, v in sorted(acc.get("env", {}).items()))
        if env:
            normal.append(f"{env} {b} {danger}".strip() if danger else f"{env} {b}")
    return {"yolo": yolo, "normal": normal, "yoloNote": cli["launch"].get("yoloNote", "")}


def catalog_view(catalog, *, danger_flags, versions, accounts):
    clis = []
    for cli in catalog["clis"]:
        installed = versions.get(cli["id"])
        clis.append({
            "id": cli["id"], "label": cli["label"], "binary": cli["binary"],
            "version": {"pinned": cli["pinnedVersion"], "installed": installed,
                        "status": version_status(cli["pinnedVersion"], installed, cli.get("verified", True))},
            "launch": _launches(cli, danger_flags.get(cli["id"], ""), accounts),
            "groups": [{"title": g["title"], "icon": g.get("icon", ""),
                        "commands": [{"text": c["text"], "description": c["description"], "args": c.get("args", [])}
                                     for c in g.get("commands", [])]} for g in cli["groups"]],
        })
    return {"version": 1, "clis": clis}
```

- [ ] **Paso 5: correr las pruebas**

Run: `~/.local/bin/pytest -q tests/test_cli_catalog.py`
Esperado: PASS (6 pruebas).

- [ ] **Paso 6: commit**

```bash
git add config/cli-commands.json lib/cli_catalog.py tests/test_cli_catalog.py
git commit -m "feat: curated per-CLI command catalog verified against installed versions"
```

---

### Tarea C2: ruta `GET /commands/catalog` con CLI del pane y versiones en caché

**Archivos:**
- Modificar: `bin/cc-dash` (imports junto a `import tui_state` en la línea 1175; `API_GET` en 8670–8673; el cuerpo de rutas GET es `_do_GET` en 8698 — `do_GET` 8688 solo aplica la puerta; `agent_info_for_pane` en 1284)
- Crear: `tests/test_commands_catalog_endpoint.py`

**Interfaces:**
- Consume: `cli_catalog.load_catalog/installed_versions/catalog_view` (C1), `load_agent_roles()` (`bin/cc-dash:3817`), `load_provider_registry()` (`:1197`), `account_registry.list_accounts/account_environment` (`lib/accounts.py`, importado en `:1180`), `agent_info_for_pane(pane)` (`:1284`).
- Produce: `GET /commands/catalog?session=<s>&pane=<%n>[&refresh=1]` → `{ "cliInPane": "codex"|"" , "target": {"session","pane"}, "catalog": <catalog_view> , "versionsAt": <epoch> }`. Token requerido (añadir `/commands/catalog` a `API_GET`).

- [ ] **Paso 1: prueba de la ruta** en `tests/test_commands_catalog_endpoint.py`, siguiendo el arranque de servidor de `tests/test_snippets_endpoints.py` (fixture `dash` con `HOME` en `tmp_path`, puerto libre, token leído de `~/.claude/hooks/dash-token`):

```python
def test_catalog_route_reports_pane_cli_and_versions(dash):
    body = dash.get("/commands/catalog?session=demo&pane=%1")
    assert body["target"] == {"session": "demo", "pane": "%1"}
    assert body["cliInPane"] in ("", "claude", "codex", "grok", "opencode", "agy")
    ids = [c["id"] for c in body["catalog"]["clis"]]
    assert ids == ["claude", "codex", "grok", "opencode", "agy"]
    for cli in body["catalog"]["clis"]:
        assert cli["version"]["status"] in ("ok", "drift", "missing", "unverified")
    again = dash.get("/commands/catalog?session=demo&pane=%1")
    assert again["versionsAt"] == body["versionsAt"]          # caché: no vuelve a ejecutar --version

def test_catalog_route_requires_token(dash):
    assert dash.get_status("/commands/catalog", token=False) == 401
```

Añadir a la fixture un helper `get(path)` que envía `X-Comandos-Token` y decodifica JSON, y `get_status(path, token)` que devuelve el código HTTP (copiar el patrón de urllib de `tests/test_snippets_endpoints.py`).

- [ ] **Paso 2: correr para ver el fallo**

Run: `~/.local/bin/pytest -q tests/test_commands_catalog_endpoint.py`
Esperado: FAIL con 404 en `/commands/catalog`.

- [ ] **Paso 3: implementar en `bin/cc-dash`**

Junto a los imports de la línea 1175:

```python
import cli_catalog
_CLI_CATALOG = {"catalog": None, "versions": {}, "at": 0.0}
_CLI_CATALOG_TTL = 1800.0


def cli_catalog_payload(refresh=False):
    now = time.time()
    if _CLI_CATALOG["catalog"] is None:
        _CLI_CATALOG["catalog"] = cli_catalog.load_catalog()
    if refresh or now - _CLI_CATALOG["at"] > _CLI_CATALOG_TTL:
        _CLI_CATALOG["versions"] = cli_catalog.installed_versions(_CLI_CATALOG["catalog"])
        _CLI_CATALOG["at"] = now
    registry = load_provider_registry()
    accounts = {}
    for cli in _CLI_CATALOG["catalog"]["clis"]:
        try:
            accounts[cli["id"]] = [
                {"alias": a["alias"], "env": accounts_lib.account_environment(registry, cli["id"], a["alias"])}
                for a in accounts_lib.list_accounts(registry, cli["id"]) if a.get("alias") != "main"]
        except Exception:
            accounts[cli["id"]] = []
    view = cli_catalog.catalog_view(_CLI_CATALOG["catalog"],
                                    danger_flags=load_agent_roles().get("dangerFlags") or {},
                                    versions=_CLI_CATALOG["versions"], accounts=accounts)
    return view, _CLI_CATALOG["at"]
```

(`bin/cc-dash:1180` ya importa `lib/accounts.py` como `account_registry`; sustituir `accounts_lib` por `account_registry` en el bloque anterior.) En `_do_GET` (:8698), junto a `GET /snippets` (:9013):

```python
        if self.path.startswith("/commands/catalog"):
            q = urllib.parse.parse_qs(urllib.parse.urlparse(self.path).query)
            sess = (q.get("session") or [""])[0][:80]
            pane = (q.get("pane") or [""])[0]
            info = agent_info_for_pane(pane) if PANE_RE.match(pane or "") else None
            view, at = cli_catalog_payload(refresh=(q.get("refresh") or ["0"])[0] == "1")
            return self._json(200, {"cliInPane": (info or {}).get("agent", "") or "",
                                    "target": {"session": sess, "pane": pane},
                                    "catalog": view, "versionsAt": at})
```

Añadir `"/commands/catalog"` a la tupla `API_GET` (líneas 8670–8672).

- [ ] **Paso 4: correr las pruebas**

Run: `~/.local/bin/pytest -q tests/test_commands_catalog_endpoint.py tests/test_cli_catalog.py`
Esperado: PASS.

- [ ] **Paso 5: commit**

```bash
git add bin/cc-dash tests/test_commands_catalog_endpoint.py
git commit -m "feat: serve the per-CLI command catalog with the pane's CLI and cached versions"
```

---

### Tarea T1: teclear texto literal letra por letra sin Enter (`POST /pane/type`)

**Archivos:**
- Crear: `lib/pane_typing.py`
- Crear: `tests/test_pane_typing.py`
- Modificar: `bin/cc-dash` (ruta POST junto a `/send` en la línea 10050; import junto a la línea 1175)
- Crear: `tests/test_pane_type_endpoint.py`

**Interfaces:**
- Consume: `tmux(*args)` de `bin/cc-dash:6222` (patrón `send-keys -t <pane> -l -- <texto>` ya usado en `_send_shell_line` `:1545` y en `_pane_exit_current`), resolución de pane de `do_POST` (`sess` validado con `SESSION_RE` en `:9935–9937`; `target`/`pane` resueltos en `:10013–10024`, con el pane exacto verificado mediante `display-message -p -t <pane> #{pane_id}`).
- Produce: `pane_typing.type_literal(tmux_fn, pane, text, *, sleep=time.sleep, delay=0.022, budget=1.2) -> dict`, `pane_typing.PaneTypingLocks.acquire(pane) -> bool / release(pane)`, y `POST /pane/type {session, pane, text, requestId}` → 200 `{ok, typed, durationMs, requestId}`; 400 texto inválido; 404 sesión/pane inexistente; 409 `typing_in_progress`; misma `requestId` repetida → mismo resultado sin volver a teclear.

- [ ] **Paso 1: pruebas de la biblioteca** en `tests/test_pane_typing.py`:

```python
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "lib"))
import pytest
import pane_typing

class FakeTmux:
    def __init__(self, fail_at=None):
        self.calls, self.fail_at = [], fail_at
    def __call__(self, *args, **kw):
        self.calls.append(args)
        class R: returncode = 0; stderr = ""
        if self.fail_at is not None and len(self.calls) == self.fail_at:
            R.returncode, R.stderr = 1, "no such pane"
        return R()

def test_types_every_character_literally_and_never_sends_enter():
    t, slept = FakeTmux(), []
    out = pane_typing.type_literal(t, "%3", "/model claude-fable-5-1", sleep=slept.append, delay=0.02, budget=10)
    typed = "".join(c[-1] for c in t.calls)
    assert typed == "/model claude-fable-5-1"
    assert all(c[:5] == ("send-keys", "-t", "%3", "-l", "--") for c in t.calls)
    assert not any("Enter" in c for c in t.calls)
    assert out == {"typed": 23}
    assert len(slept) == 22 and slept[0] == pytest.approx(0.02)

def test_long_text_speeds_up_to_fit_the_budget():
    t, slept = FakeTmux(), []
    pane_typing.type_literal(t, "%3", "x" * 200, sleep=slept.append, delay=0.022, budget=1.2)
    assert sum(slept) <= 1.2 + 1e-6 and len(t.calls) == 200

@pytest.mark.parametrize("bad", ["", "  ", "ls\n", "a\rb", "\x1b[A", "x" * 2001])
def test_rejects_newlines_control_chars_empty_and_too_long(bad):
    with pytest.raises(pane_typing.TypingError):
        pane_typing.type_literal(FakeTmux(), "%3", bad, sleep=lambda s: None)

def test_stops_at_first_tmux_failure_and_reports_progress():
    t = FakeTmux(fail_at=3)
    with pytest.raises(pane_typing.TypingError) as exc:
        pane_typing.type_literal(t, "%3", "abcdef", sleep=lambda s: None)
    assert exc.value.typed == 2 and "no such pane" in str(exc.value)

def test_lock_refuses_a_second_typing_on_the_same_pane_only():
    locks = pane_typing.PaneTypingLocks()
    assert locks.acquire("%3") and not locks.acquire("%3") and locks.acquire("%4")
    locks.release("%3")
    assert locks.acquire("%3")
```

- [ ] **Paso 2: correr para ver el fallo**

Run: `~/.local/bin/pytest -q tests/test_pane_typing.py`
Esperado: FAIL con `ModuleNotFoundError: pane_typing`.

- [ ] **Paso 3: implementar `lib/pane_typing.py`**

```python
"""Teclea texto literal en un pane de tmux, letra por letra, sin Enter jamás."""
from __future__ import annotations
import threading, time

MAX_CHARS = 2000


class TypingError(RuntimeError):
    def __init__(self, message, *, typed=0, code="invalid"):
        super().__init__(message)
        self.typed, self.code = typed, code


def validate(text):
    if not isinstance(text, str) or not text.strip():
        raise TypingError("Texto vacío")
    if len(text) > MAX_CHARS:
        raise TypingError(f"Texto demasiado largo (máx. {MAX_CHARS})")
    if any(ch in "\r\n" or (ord(ch) < 32 and ch != "\t") or ord(ch) == 127 for ch in text):
        raise TypingError("El texto no puede contener saltos de línea ni caracteres de control")
    return text


def type_literal(tmux_fn, pane, text, *, sleep=time.sleep, delay=0.022, budget=1.2):
    text = validate(text)
    step = min(delay, budget / max(len(text) - 1, 1))
    typed = 0
    for i, ch in enumerate(text):
        r = tmux_fn("send-keys", "-t", pane, "-l", "--", ch)
        if getattr(r, "returncode", 1) != 0:
            raise TypingError((getattr(r, "stderr", "") or "tmux send-keys falló").strip(), typed=typed, code="tmux")
        typed += 1
        if i < len(text) - 1:
            sleep(step)
    return {"typed": typed}


class PaneTypingLocks:
    def __init__(self):
        self._busy, self._guard = set(), threading.Lock()

    def acquire(self, pane):
        with self._guard:
            if pane in self._busy:
                return False
            self._busy.add(pane)
            return True

    def release(self, pane):
        with self._guard:
            self._busy.discard(pane)
```

- [ ] **Paso 4: correr las pruebas**

Run: `~/.local/bin/pytest -q tests/test_pane_typing.py`
Esperado: PASS (5 pruebas, 6 parametrizaciones).

- [ ] **Paso 5: prueba de la ruta** en `tests/test_pane_type_endpoint.py`, con un servidor tmux privado (patrón de `tests/test_quick_terminal.py`: `TMUX_TMPDIR` en `tmp_path`, `unset TMUX`, `tmux -L <socket> new-session -d -s demo`; el fixture `dash` del test anterior debe recibir ese socket en `env["TMUX_TMPDIR"]` y el mismo `-L` que usa `bin/cc-dash`, o exportar `TMUX_TMPDIR` para que el socket por defecto sea el privado):

```python
def test_type_route_writes_literal_text_without_enter(dash, private_tmux):
    r = dash.post("/pane/type", {"session": "demo", "pane": private_tmux.pane, "text": "/effort max", "requestId": "r1"})
    assert r["ok"] and r["typed"] == 11
    screen = private_tmux.capture()
    assert "/effort max" in screen and screen.rstrip().endswith("/effort max")   # sigue en el prompt, sin ejecutar

def test_type_route_is_idempotent_per_request_id(dash, private_tmux):
    dash.post("/pane/type", {"session": "demo", "pane": private_tmux.pane, "text": "abc", "requestId": "r2"})
    dash.post("/pane/type", {"session": "demo", "pane": private_tmux.pane, "text": "abc", "requestId": "r2"})
    assert private_tmux.capture().count("abc") == 1

def test_type_route_rejects_newline_and_unknown_pane(dash, private_tmux):
    assert dash.post_status("/pane/type", {"session": "demo", "pane": private_tmux.pane, "text": "ls\n"}) == 400
    assert dash.post_status("/pane/type", {"session": "demo", "pane": "%999", "text": "ls"}) == 404
```

`private_tmux` es un fixture que crea la sesión `demo` en el socket privado, expone `.pane` (`tmux display-message -p '#{pane_id}'`) y `.capture()` (`capture-pane -p`), y la mata al terminar (`kill-server` solo en ese socket).

- [ ] **Paso 6: correr para ver el fallo**

Run: `~/.local/bin/pytest -q tests/test_pane_type_endpoint.py`
Esperado: FAIL con 404 en `/pane/type`.

- [ ] **Paso 7: implementar la ruta en `bin/cc-dash`**

Junto a los imports (línea 1175): `import pane_typing`; `bin/cc-dash` no importa `collections` (línea 25), añadir `import collections` y

```python
_PANE_TYPING_LOCKS = pane_typing.PaneTypingLocks()
_PANE_TYPING_RESULTS = collections.OrderedDict()   # requestId -> (status, body); 256 entradas
```

Justo antes de `if self.path == "/send":` (línea 10050), donde `sess`, `target` y `pane` ya están resueltos (`:10013–10024`):

```python
        if self.path == "/pane/type":
            rid = str(data.get("requestId") or "")[:64]
            if rid and rid in _PANE_TYPING_RESULTS:
                return self._json(*_PANE_TYPING_RESULTS[rid])
            if tmux("has-session", "-t", target).returncode != 0:
                return self._json(404, {"error": f"No hay sesion tmux '{sess}'. Levantala primero."})
            want = str(data.get("pane") or "")
            if want and pane != want:
                return self._json(404, {"error": f"El pane {want} ya no existe", "code": "pane_gone"})
            try:
                text = pane_typing.validate(data.get("text"))
            except pane_typing.TypingError as exc:
                return self._json(400, {"error": str(exc), "code": "invalid"})
            if not _PANE_TYPING_LOCKS.acquire(pane):
                return self._json(409, {"error": "Ya se está escribiendo en ese pane; espera a que termine.", "code": "typing_in_progress"})
            t0 = time.monotonic()
            try:
                out = pane_typing.type_literal(tmux, pane, text)
                res = (200, {"ok": True, "typed": out["typed"], "durationMs": int((time.monotonic() - t0) * 1000), "requestId": rid})
            except pane_typing.TypingError as exc:
                res = (502, {"error": str(exc), "code": exc.code, "typed": exc.typed, "requestId": rid})
            finally:
                _PANE_TYPING_LOCKS.release(pane)
            if rid:
                _PANE_TYPING_RESULTS[rid] = res
                while len(_PANE_TYPING_RESULTS) > 256:
                    _PANE_TYPING_RESULTS.popitem(last=False)
            return self._json(*res)
```

Nota: la ruta es síncrona (≤ 1.3 s). Nunca hay `send-keys … Enter`.

- [ ] **Paso 8: correr las pruebas**

Run: `~/.local/bin/pytest -q tests/test_pane_typing.py tests/test_pane_type_endpoint.py`
Esperado: PASS.

- [ ] **Paso 9: commit**

```bash
git add lib/pane_typing.py tests/test_pane_typing.py bin/cc-dash tests/test_pane_type_endpoint.py
git commit -m "feat: type literal text into a pane letter by letter without Enter"
```

---

### Tarea K1: cadenas como archivos Markdown editables

**Archivos:**
- Crear: `lib/command_chains.py`
- Crear: `tests/test_command_chains.py`

**Interfaces:**
- Consume: nada del servidor. Directorio por defecto `$XDG_CONFIG_HOME/comandos/cadenas` (o `~/.config/comandos/cadenas`), coherente con `~/.config/comandos/extensions/` de `lib/extension_catalog.py:41`.
- Produce: `default_dir()`, `list_chains(dir) -> list[dict]`, `save_chain(dir, name, steps, slug=None) -> dict`, `delete_chain(dir, slug) -> bool`, `parse(text) -> dict`, `serialize(name, steps) -> str`, `slugify(name) -> str`. Un `step` es `{"kind": "shell"|"pane", "text": str}`. Un archivo inválido se lista con `{"slug", "name": <nombre de archivo>, "error": "..."}` y sin `steps`.

Formato del archivo `<slug>.md`:

```markdown
# Arrancar Codex yolo y fijar modelo

1. shell: codex --dangerously-bypass-approvals-and-sandbox
2. pane: /model
```

- [ ] **Paso 1: pruebas** en `tests/test_command_chains.py`:

```python
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "lib"))
import pytest
import command_chains as cc

STEPS = [{"kind": "shell", "text": "codex --dangerously-bypass-approvals-and-sandbox"}, {"kind": "pane", "text": "/model"}]

def test_round_trip_is_deterministic(tmp_path):
    saved = cc.save_chain(tmp_path, "Arrancar Codex yolo y fijar modelo", STEPS)
    assert saved["slug"] == "arrancar-codex-yolo-y-fijar-modelo"
    text = (tmp_path / "arrancar-codex-yolo-y-fijar-modelo.md").read_text(encoding="utf-8")
    assert text == "# Arrancar Codex yolo y fijar modelo\n\n1. shell: codex --dangerously-bypass-approvals-and-sandbox\n2. pane: /model\n"
    assert cc.list_chains(tmp_path) == [{"slug": saved["slug"], "name": "Arrancar Codex yolo y fijar modelo", "steps": STEPS}]

def test_hand_edited_file_with_bad_step_is_listed_with_error_and_others_survive(tmp_path):
    cc.save_chain(tmp_path, "Buena", STEPS)
    (tmp_path / "rota.md").write_text("# Rota\n\n- foo: bar\n", encoding="utf-8")
    listed = {c["slug"]: c for c in cc.list_chains(tmp_path)}
    assert "steps" in listed["buena"]
    assert listed["rota"]["error"].startswith("Paso inválido") and "steps" not in listed["rota"]

def test_parse_accepts_dashes_numbers_and_ignores_blank_lines():
    chain = cc.parse("# Dos\n\n- shell: claude\n\n2) pane: /effort max\n")
    assert chain["steps"] == [{"kind": "shell", "text": "claude"}, {"kind": "pane", "text": "/effort max"}]

@pytest.mark.parametrize("bad", [[], [{"kind": "x", "text": "a"}], [{"kind": "pane", "text": "a\nb"}], [{"kind": "pane", "text": ""}]])
def test_save_rejects_invalid_steps(tmp_path, bad):
    with pytest.raises(cc.ChainError):
        cc.save_chain(tmp_path, "N", bad)

def test_same_name_twice_gets_a_distinct_slug_and_explicit_slug_overwrites(tmp_path):
    a = cc.save_chain(tmp_path, "Dup", STEPS)
    b = cc.save_chain(tmp_path, "Dup", STEPS)
    assert (a["slug"], b["slug"]) == ("dup", "dup-2")
    c = cc.save_chain(tmp_path, "Dup renombrada", STEPS[:1], slug="dup")
    assert c["slug"] == "dup" and cc.list_chains(tmp_path)[0]["name"] == "Dup renombrada"

def test_delete_only_touches_that_slug(tmp_path):
    cc.save_chain(tmp_path, "A", STEPS); cc.save_chain(tmp_path, "B", STEPS)
    assert cc.delete_chain(tmp_path, "a") and not cc.delete_chain(tmp_path, "a")
    assert [c["slug"] for c in cc.list_chains(tmp_path)] == ["b"]
    with pytest.raises(cc.ChainError):
        cc.delete_chain(tmp_path, "../b")
```

- [ ] **Paso 2: correr para ver el fallo**

Run: `~/.local/bin/pytest -q tests/test_command_chains.py`
Esperado: FAIL con `ModuleNotFoundError: command_chains`.

- [ ] **Paso 3: implementar `lib/command_chains.py`**

```python
"""Cadenas de comandos: archivos Markdown que el usuario puede editar a mano."""
from __future__ import annotations
import os, re, unicodedata
from pathlib import Path

KINDS = ("shell", "pane")
_STEP_RE = re.compile(r"^\s*(?:\d+[.)]|[-*])\s*(\w+)\s*:\s*(.+?)\s*$")
_SLUG_RE = re.compile(r"^[a-z0-9][a-z0-9-]{0,59}$")


class ChainError(ValueError):
    pass


def default_dir():
    base = os.environ.get("XDG_CONFIG_HOME") or os.path.join(os.path.expanduser("~"), ".config")
    return Path(base) / "comandos" / "cadenas"


def slugify(name):
    s = unicodedata.normalize("NFKD", str(name or "")).encode("ascii", "ignore").decode()
    s = re.sub(r"[^a-z0-9]+", "-", s.lower()).strip("-")[:60]
    return s or "cadena"


def _check_steps(steps):
    if not isinstance(steps, list) or not steps:
        raise ChainError("La cadena necesita al menos un paso")
    out = []
    for i, st in enumerate(steps, 1):
        kind, text = (st or {}).get("kind"), str((st or {}).get("text") or "")
        if kind not in KINDS:
            raise ChainError(f"Paso inválido {i}: tipo '{kind}' (usa shell o pane)")
        if not text.strip() or any(ch in "\r\n" or ord(ch) < 32 for ch in text):
            raise ChainError(f"Paso inválido {i}: texto vacío o con saltos de línea")
        out.append({"kind": kind, "text": text.strip()})
    return out


def serialize(name, steps):
    steps = _check_steps(steps)
    lines = [f"# {str(name).strip()}", ""]
    lines += [f"{i}. {s['kind']}: {s['text']}" for i, s in enumerate(steps, 1)]
    return "\n".join(lines) + "\n"


def parse(text):
    name, steps = "", []
    for raw in str(text or "").splitlines():
        line = raw.rstrip()
        if not line.strip():
            continue
        if not name and line.startswith("# "):
            name = line[2:].strip()
            continue
        m = _STEP_RE.match(line)
        if not m:
            raise ChainError(f"Paso inválido: {line.strip()!r}")
        steps.append({"kind": m.group(1), "text": m.group(2)})
    return {"name": name, "steps": _check_steps(steps)}


def _path(directory, slug):
    if not _SLUG_RE.match(str(slug or "")):
        raise ChainError(f"Slug inválido: {slug!r}")
    return Path(directory) / f"{slug}.md"


def list_chains(directory):
    directory = Path(directory)
    out = []
    if not directory.is_dir():
        return out
    for p in sorted(directory.glob("*.md")):
        slug = p.stem
        try:
            chain = parse(p.read_text(encoding="utf-8"))
            out.append({"slug": slug, "name": chain["name"] or slug, "steps": chain["steps"]})
        except (ChainError, OSError, UnicodeDecodeError) as exc:
            out.append({"slug": slug, "name": slug, "error": str(exc)})
    return out


def save_chain(directory, name, steps, slug=None):
    directory = Path(directory)
    directory.mkdir(parents=True, exist_ok=True)
    body = serialize(name, steps)
    if slug is None:
        base = slugify(name)
        slug, n = base, 2
        while _path(directory, slug).exists():
            slug, n = f"{base}-{n}", n + 1
    path = _path(directory, slug)
    tmp = path.with_suffix(".md.tmp")
    tmp.write_text(body, encoding="utf-8")
    os.replace(tmp, path)
    return {"slug": slug, "name": str(name).strip(), "steps": _check_steps(steps)}


def delete_chain(directory, slug):
    path = _path(directory, slug)
    if not path.exists():
        return False
    path.unlink()
    return True
```

- [ ] **Paso 4: correr las pruebas**

Run: `~/.local/bin/pytest -q tests/test_command_chains.py`
Esperado: PASS.

- [ ] **Paso 5: commit**

```bash
git add lib/command_chains.py tests/test_command_chains.py
git commit -m "feat: store command chains as editable markdown files"
```

---

### Tarea K2: rutas `/chains`

**Archivos:**
- Modificar: `bin/cc-dash` (import junto a 1175; `API_GET` 8670–8672; `_do_GET` 8698 junto a `GET /snippets` 9013; `do_POST` junto a `/snippets` 9620, `/snippets/update` 9634 y `/snippets/delete` 9653)
- Crear: `tests/test_chains_endpoints.py`

**Interfaces:**
- Consume: `command_chains` (K1).
- Produce: `GET /chains` → `{chains: [...]}`; `POST /chains {name, steps, slug?}` → `{ok, chain}`; `POST /chains/delete {slug}` → `{ok, deleted: bool}`. Directorio: `command_chains.default_dir()` (respeta `XDG_CONFIG_HOME`, así el fixture aísla con `HOME`/`XDG_CONFIG_HOME` en `tmp_path`).

- [ ] **Paso 1: pruebas** (`tests/test_chains_endpoints.py`, mismo fixture `dash` que C2 con `env["XDG_CONFIG_HOME"] = str(tmp_path / "cfg")`):

```python
STEPS = [{"kind": "shell", "text": "claude --dangerously-skip-permissions"}, {"kind": "pane", "text": "/effort max"}]

def test_chain_crud_round_trip(dash, tmp_path):
    saved = dash.post("/chains", {"name": "Claude yolo máximo", "steps": STEPS})
    assert saved["chain"]["slug"] == "claude-yolo-maximo"
    assert (tmp_path / "cfg" / "comandos" / "cadenas" / "claude-yolo-maximo.md").exists()
    assert dash.get("/chains")["chains"][0]["steps"] == STEPS
    assert dash.post("/chains/delete", {"slug": "claude-yolo-maximo"})["deleted"] is True
    assert dash.get("/chains")["chains"] == []

def test_chain_save_rejects_bad_steps_with_400(dash):
    assert dash.post_status("/chains", {"name": "x", "steps": [{"kind": "pane", "text": "a\nb"}]}) == 400
```

- [ ] **Paso 2: correr para ver el fallo** — Run: `~/.local/bin/pytest -q tests/test_chains_endpoints.py` — Esperado: FAIL 404.

- [ ] **Paso 3: implementar**. Import `command_chains` junto a 1175. En `_do_GET`:

```python
        if self.path == "/chains":
            return self._json(200, {"chains": command_chains.list_chains(command_chains.default_dir())})
```

En `do_POST`, junto a las rutas `/snippets*` (antes de la validación de `session` de la línea 9935, porque estas rutas no llevan sesión):

```python
        if self.path == "/chains":
            try:
                chain = command_chains.save_chain(command_chains.default_dir(), data.get("name"),
                                                  data.get("steps"), slug=data.get("slug") or None)
            except command_chains.ChainError as exc:
                return self._json(400, {"error": str(exc)})
            return self._json(200, {"ok": True, "chain": chain})
        if self.path == "/chains/delete":
            try:
                deleted = command_chains.delete_chain(command_chains.default_dir(), data.get("slug"))
            except command_chains.ChainError as exc:
                return self._json(400, {"error": str(exc)})
            return self._json(200, {"ok": True, "deleted": deleted})
```

Añadir `"/chains"` a `API_GET`.

- [ ] **Paso 4: correr** — Run: `~/.local/bin/pytest -q tests/test_chains_endpoints.py tests/test_command_chains.py` — Esperado: PASS.

- [ ] **Paso 5: commit**

```bash
git add bin/cc-dash tests/test_chains_endpoints.py
git commit -m "feat: list, save and delete command chains over HTTP"
```

---

### Tarea S1: módulo `dash/command-sidebar.js` (render puro + controlador)

**Archivos:**
- Crear: `dash/command-sidebar.js`
- Crear: `tests/command_sidebar_checks.cjs`
- Crear: `tests/test_command_sidebar_ui.py`

**Interfaces:**
- Consume: respuesta de `GET /commands/catalog` (C2), `GET /chains` (K2), `POST /pane/type` (T1) a través de `api(path, body)` (`dash/index.html:2659`), `storage` (localStorage) y `makeId()`.
- Produce: `createCommandSidebar({api, root, storage, makeId, getTarget, focusTarget, openBuilder, toast, terminals})` con `refresh()`, `render()`, `insert(text, kind)`, `startChain(slug)`, `next()`, `stop()`, `state` (solo lectura). `getTarget()` devuelve `{session, pane, kind: 'pane'|'term', title}`; `terminals()` devuelve la lista de terminales rápidas abiertas `[{tabId, paneKey, session, pane, label, cwd}]`; `focusTarget(target)` selecciona el pane/tab en el workspace.

Estructura del DOM que pinta (ids/clases que S2 y las pruebas usan):

```
#command-sidebar
  .cs-head            → .cs-target (título del destino) · button.cs-chains[data-open-builder] · input.cs-search
  .cs-saved.open?     → .cli-h[data-toggle=saved] · .cs-saved-item[data-run=<slug>] (button Correr) · error si `chain.error`
  .cs-runner.done?    → .r-h (nombre · paso i/n · Parar) · .steps .step.done|.cur · button.cs-next[data-run-next]
  .cs-cli[data-cli=<id>].open?.here?.drift?.missing? → .cli-h[data-toggle=<id>] (nombre · versión · "en este pane" · chevron)
     .launch.yolo.open? → .lab3[data-toggle=<id>:yolo] · .cmd[data-cmd][data-kind=shell]
     .launch.normal.open? → .lab3[data-toggle=<id>:normal] · .cmd[data-cmd][data-kind=shell]
     .grp.open? → .lab3[data-toggle=<id>:<n>] · .cmd.dis?[data-cmd][data-kind=pane] > code + small + .opts button[data-cmd]
  .cs-terms           → .t[data-focus-term=<tabId>] · .t.plus[data-new-term]
```

Reglas de estado (todas deterministas, sin temporizadores salvo la animación de tecleo que hace el servidor):
- `state.open` (Set de claves `saved`, `<cli>`, `<cli>:yolo`, `<cli>:normal`, `<cli>:<n>`) se persiste en `storage` bajo `comandos.commands.open`. Al cargar por primera vez abre `saved`, el CLI de `cliInPane` (o el `preferredCli` de la terminal rápida guardado en `comandos.commands.preferred.<paneKey>`), su bloque `yolo` y sus grupos.
- Todo plegable, incluido el CLI del pane.
- Con destino de tipo `term` sin CLI: filas `.cmd[data-kind=pane]` llevan `.dis` y `insert()` las ignora; los arranques siguen activos. Tras insertar un arranque `shell` cuyo primer token es un `binary` del catálogo, se guarda `comandos.commands.preferred.<paneKey>=<cli>`.
- `insert(text, kind)`: si hay un tecleo en vuelo (`state.typing`) no hace nada y muestra toast "Espera a que termine de escribir"; captura el destino en ese momento y llama `api('/pane/type', {session, pane, text, requestId: makeId()})`; en error muestra el `error` del servidor. Nunca envía Enter ni `/send`.
- `startChain(slug)`: fija `state.run = {slug, name, steps, step: 0, target: getTarget()}`; el destino no cambia aunque el usuario seleccione otro pane. `next()` teclea `steps[step]` en `run.target` y avanza `step` solo si el servidor respondió 200; en 404 `pane_gone` muestra el error y deja `step` igual. Al completar, la tarjeta muestra "completa" con Cerrar. `stop()` borra `state.run`.
- `version.status` → clase `drift` (ámbar, texto "sin verificar · el CLI confirma"), `missing` ("no instalado", arranques con `.dis`), `unverified` (texto "sin verificar").
- Búsqueda `.cs-search` filtra filas por `text`/`description` sin cambiar `state.open`.

- [ ] **Paso 1: checks Node** en `tests/command_sidebar_checks.cjs` (DOM mínimo como en /home/someguy/codebase/0xJesus/ComandOS/.worktrees/comandos-v1-grill/dash/prototypes/prototype-v2-smoke.cjs: un `el()` con `innerHTML`, `querySelector(All)` por selector simple, `classList`, `dataset`, `addEventListener`; el módulo debe renderizar con `root.innerHTML = html` y delegar clics en `root`):

```js
const assert = require('node:assert/strict');
const { createCommandSidebar } = require(process.cwd() + '/dash/command-sidebar.js');
// … stub DOM `mkRoot()` que registra el último innerHTML y permite `click(selector)` emulando delegación …
const catalog = JSON.parse(require('node:fs').readFileSync(process.cwd() + '/tests/fixtures/command-catalog.json'));
(async () => {
  const calls = [], toasts = [];
  const store = new Map(), storage = { getItem: k => store.get(k) ?? null, setItem: (k, v) => store.set(k, v), removeItem: k => store.delete(k) };
  let target = { session: 'demo', pane: '%2', kind: 'pane', title: 'demo %2' }, ids = 0, typeStatus = 200;
  const api = async (path, body) => { calls.push([path, body]);
    if (path.startsWith('/commands/catalog')) return { cliInPane: 'codex', target: { session: 'demo', pane: '%2' }, catalog, versionsAt: 1 };
    if (path === '/chains') return { chains: [{ slug: 'yolo', name: 'Codex yolo', steps: [{ kind: 'shell', text: 'codex --dangerously-bypass-approvals-and-sandbox' }, { kind: 'pane', text: '/model' }] }, { slug: 'rota', name: 'rota', error: 'Paso inválido: - foo: bar' }] };
    if (path === '/pane/type') { if (typeStatus !== 200) { const e = new Error('El pane %2 ya no existe'); e.code = 'pane_gone'; throw e; } return { ok: true, typed: body.text.length, requestId: body.requestId }; }
    throw new Error('ruta inesperada ' + path); };
  const root = mkRoot();
  const sb = createCommandSidebar({ api, root, storage, makeId: () => 'id-' + (++ids), getTarget: () => target, focusTarget: () => {}, openBuilder: () => {}, toast: (m, e) => toasts.push([m, e]), terminals: () => [] });
  await sb.refresh();
  // 1) el CLI del pane abre solo, marcado, con yolo primero y todo plegable
  assert.equal(root.querySelector('.cs-cli.here').dataset.cli, 'codex');
  assert.equal(root.querySelector('.cs-cli.here .launch').classList.contains('yolo'), true);
  assert.equal(root.querySelector('.cs-cli.here .launch.yolo .cmd').dataset.cmd, 'codex --dangerously-bypass-approvals-and-sandbox');
  root.click('.cs-cli.here .cli-h'); assert.equal(root.querySelector('.cs-cli.here').classList.contains('open'), false);
  root.click('.cs-cli.here .cli-h'); assert.equal(root.querySelector('.cs-cli.here').classList.contains('open'), true);
  assert.equal(JSON.parse(store.get('comandos.commands.open')).includes('codex'), true);
  // 2) un clic teclea sin Enter en el pane capturado, con requestId
  root.click('.cs-cli.here .grp .cmd');
  await sb.state.typing; 
  const typed = calls.filter(c => c[0] === '/pane/type');
  assert.deepEqual(typed[0][1], { session: 'demo', pane: '%2', text: '/model', requestId: 'id-1' });
  assert.equal(calls.some(c => c[0] === '/send'), false);
  // 3) chips de argumento
  root.click('.cs-cli[data-cli="claude"] .cli-h'); root.click('.cs-cli[data-cli="claude"] .opts button');
  await sb.state.typing; assert.equal(calls.at(-1)[1].text, '/model claude-fable-5-1');
  // 4) cadena guardada: correr fija el destino; Siguiente avanza solo con 200; pane cerrado no salta de paso
  root.click('.cs-saved-item[data-run="yolo"] button');
  assert.equal(root.querySelector('.cs-runner .r-h').textContent.includes('paso 1 de 2'), true);
  target = { session: 'demo', pane: '%9', kind: 'pane', title: 'otro' };            // el usuario cambió de pane
  root.click('.cs-next'); await sb.state.typing;
  assert.equal(calls.at(-1)[1].pane, '%2');                                       // destino fijado al arrancar
  assert.equal(sb.state.run.step, 1);
  typeStatus = 404; root.click('.cs-next'); await sb.state.typing.catch(() => {});
  assert.equal(sb.state.run.step, 1); assert.equal(toasts.at(-1)[0], 'El pane %2 ya no existe');
  typeStatus = 200; root.click('.cs-next'); await sb.state.typing;
  assert.equal(root.querySelector('.cs-runner').classList.contains('done'), true);
  // 5) cadena rota se lista con error y sin botón Correr
  assert.equal(root.querySelector('.cs-saved-item[data-run="rota"]'), null);
  assert.equal(root.querySelector('.cs-saved-item.error').textContent.includes('Paso inválido'), true);
  // 6) terminal rápida sin CLI: comandos /… atenuados, arranques activos; al arrancar se recuerda el CLI
  target = { session: 'term-q1', pane: '%7', kind: 'term', title: 'Terminal 14:32' };
  sb.render(); const before = calls.length; root.click('.cs-cli[data-cli="claude"] .grp .cmd'); assert.equal(calls.length, before);   // atenuado: no teclea
  assert.equal(root.querySelector('.cs-cli[data-cli="claude"] .grp .cmd').classList.contains('dis'), true);
  root.click('.cs-cli[data-cli="claude"] .launch.yolo .cmd'); await sb.state.typing;
  assert.equal(store.get('comandos.commands.preferred.%7'), 'claude');
  // 7) el CLI sale del pane: la barra deja de marcar "en este pane"
  target = { session: 'demo', pane: '%2', kind: 'pane', title: 'demo %2' };
  sb.applyCatalog({ cliInPane: '', target: { session: 'demo', pane: '%2' }, catalog, versionsAt: 2 });
  assert.equal(root.querySelector('.cs-cli.here'), null);
  // 8) drift y missing
  const drift = JSON.parse(JSON.stringify(catalog)); drift.clis[1].version.status = 'drift'; drift.clis[4].version.status = 'missing';
  sb.applyCatalog({ cliInPane: 'codex', target: { session: 'demo', pane: '%2' }, catalog: drift, versionsAt: 3 });
  assert.equal(root.querySelector('.cs-cli[data-cli="codex"]').classList.contains('drift'), true);
  assert.equal(root.querySelector('.cs-cli[data-cli="codex"]').textContent.includes('el CLI confirma'), true);
  assert.equal(root.querySelector('.cs-cli[data-cli="agy"] .launch .cmd').classList.contains('dis'), true);
  console.log('command-sidebar checks ok');
})().catch(e => { console.error(e); process.exit(1); });
```

Crear `tests/fixtures/command-catalog.json` con la salida real de `cli_catalog.catalog_view` (generarla con un `python3 -c` sobre C1 usando `DANGER`/`VERSIONS` de la prueba y sin cuentas) para que el JS use el mismo contrato.

- [ ] **Paso 2: envoltorio pytest** `tests/test_command_sidebar_ui.py` que ejecuta `node tests/command_sidebar_checks.cjs` desde la raíz del checkout (patrón de `tests/test_work_marks_ui.py`) y comprueba que la salida termina en `command-sidebar checks ok`.

- [ ] **Paso 3: correr para ver el fallo** — Run: `~/.local/bin/pytest -q tests/test_command_sidebar_ui.py` — Esperado: FAIL (`Cannot find module dash/command-sidebar.js`).

- [ ] **Paso 4: implementar `dash/command-sidebar.js`** con el mismo envoltorio UMD que `dash/quick-terminal.js` (`root.ComandosCommandSidebar = { createCommandSidebar }` y `module.exports`). Estructura obligatoria:

```js
(function (root) {
  const KEY_OPEN = 'comandos.commands.open', KEY_PREF = 'comandos.commands.preferred.';
  const esc = s => String(s ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  const STATUS_TEXT = { ok: '', drift: 'sin verificar · el CLI confirma', missing: 'no instalado', unverified: 'sin verificar' };

  function createCommandSidebar(opts) {
    const { api, root: el, storage, makeId, getTarget, focusTarget, openBuilder, toast, terminals } = opts;
    const state = { catalog: null, cliInPane: '', chains: [], open: new Set(), run: null, typing: null, q: '', firstRender: true };
    // …readOpen()/writeOpen() con try/catch sobre storage…
    function target() { return getTarget(); }
    function launchCli(text) { const exe = text.replace(/^(?:[A-Z_]+=\S+\s+)+/, '').split(' ')[0]; return (state.catalog?.clis || []).find(c => c.binary === exe)?.id || ''; }
    function applyCatalog(payload) { state.catalog = payload.catalog; state.cliInPane = payload.cliInPane || ''; if (state.firstRender) { /* abrir saved, cli del pane o preferido, yolo y grupos */ state.firstRender = false; } render(); }
    async function refresh() { const t = target(); const [cat, ch] = await Promise.all([api(`/commands/catalog?session=${encodeURIComponent(t.session)}&pane=${encodeURIComponent(t.pane)}`), api('/chains')]); state.chains = ch.chains || []; applyCatalog(cat); }
    function insert(text, kind, fixedTarget) { /* reglas de la tarea; devuelve la promesa guardada en state.typing */ }
    function startChain(slug) { /* fija state.run con target() */ }
    function next() { /* teclea run.steps[run.step] en run.target; avanza solo con 200 */ }
    function stop() { state.run = null; render(); }
    function render() { el.innerHTML = headHTML() + savedHTML() + runnerHTML() + (state.catalog ? state.catalog.clis.map(cliHTML).join('') : '') + termsHTML(); }
    el.addEventListener('click', e => { /* delegación por data-toggle, data-cmd (+data-kind), data-run, data-run-next, data-run-stop, data-open-builder, data-focus-term, data-new-term */ });
    el.addEventListener('input', e => { if (e.target.classList?.contains('cs-search')) { state.q = e.target.value; renderKeepingSearch(); } });
    return { refresh, render, insert, startChain, next, stop, applyCatalog, get state() { return state; } };
  }
  root.ComandosCommandSidebar = { createCommandSidebar };
  if (typeof module !== 'undefined' && module.exports) module.exports = { createCommandSidebar };
})(typeof window !== 'undefined' ? window : globalThis);
```

Cada función de HTML (`headHTML`, `savedHTML`, `runnerHTML`, `cliHTML`, `launchHTML`, `groupHTML`, `rowHTML`, `termsHTML`) usa `esc()` en todo texto del catálogo y de las cadenas. `rowHTML` es la fila de dos líneas aprobada: `<div class="cmd" data-cmd="…" data-kind="pane"><code>/model <em>…</em></code><small>descripción</small><span class="opts"><button data-cmd="/model claude-fable-5-1">…</button></span></div>` (el `<em>…</em>` solo si `text` termina en espacio). El icono por grupo usa `data-icon="<icon>"` como el resto del dashboard (`data-icon` en `index.html`), no las hojas de píxeles del laboratorio.

- [ ] **Paso 5: correr** — Run: `~/.local/bin/pytest -q tests/test_command_sidebar_ui.py` — Esperado: PASS y salida `command-sidebar checks ok`.

- [ ] **Paso 5b: registrar el archivo en la instalación**: añadir `command-sidebar.js` a la lista `for f in …` de `install.sh:71` (la que ya contiene `quick-terminal.js`). Añadir a `tests/test_command_sidebar_ui.py`:

```python
def test_install_links_the_module():
    assert "command-sidebar.js" in (ROOT / "install.sh").read_text()
```

- [ ] **Paso 6: commit**

```bash
git add dash/command-sidebar.js tests/command_sidebar_checks.cjs tests/fixtures/command-catalog.json tests/test_command_sidebar_ui.py install.sh
git commit -m "feat: command sidebar module with per-CLI accordion, chains runner and quick-terminal targets"
```

---

### Tarea S2: montar la barra en `dash/index.html` y retirar el contenido anterior

**Archivos:**
- Modificar: `dash/index.html` (`#side-top` 2003–2086, `#op-split` 2087, `#op-chat` 2088–2115, `render()` 7082–7316, `renderSidebarInsights` 7762–7778 y su llamada 8377, `renderEvents` 7342, chat JS 8636–9034, `initOpChatSplit` 6081–6123, el handler de `#tab-new` 6720–6727, el script tag de `quick-terminal.js` 1945)
- Modificar: `install.sh:71` (lista de symlinks), `tests/test_usage_ui.py:214–220`, `tests/test_dashboard_layout.py:104`, `tests/test_quick_terminal_client.py:50–56`
- Modificar: `dash/workspace.js` (`setChatVisible` 140–147, `initSessionWorkspace` 148–164, `openOperatorActions` 166, `opApplyActions` 176–220)
- Modificar: `dash/workspace.css` (reglas de `#op-chat`, `html.chat-hidden`, `@container sidebar`)
- Modificar: `tests/e2e_sidebar_parity.js` y `tests/test_sidebar_parity.py`
- Modificar: `tests/test_js_parses.sh` no cambia; debe seguir pasando.

**Interfaces:**
- Consume: `createCommandSidebar` (S1), `createQuickTerminal` (`dash/quick-terminal.js`, instanciado en `initTabNavigation` `:6762`), `openTerm(tabId, label)` (localizar con `grep -n "function openTerm" dash/index.html`), estado de pane activo (el dato con el que `render()` escribe `#op-target` en `:7085–7086`), `WorkspaceDock.stripEntries()` para listar terminales rápidas (`kind === 'scratch'` con `paneKey` `pane-q…`). El cajón de avisos (`N.install({host: #panes})`, `:9059–9127`) y el lector de Resúmenes (`#news-reader` montado en `#panes`) no dependen de nada dentro de `#content`; no se tocan.
- Produce: `#command-sidebar` como único contenido de `#content`; `window.commandSidebar` para que la cabecera (H1) y GTK puedan llamar `refresh()`.

- [ ] **Paso 1: escribir la prueba de texto** en `tests/test_command_sidebar_mount.py` (patrón grep de `tests/test_snippets_ui.py`):

```python
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
HTML = (ROOT / "dash" / "index.html").read_text(encoding="utf-8")

def test_sidebar_mounts_command_module_and_drops_chat_and_old_sections():
    assert 'id="command-sidebar"' in HTML
    assert 'src="command-sidebar.js"' in HTML or "command-sidebar.js" in HTML
    for gone in ('id="op-chat"', 'id="toggle-chat"', 'id="sidebar-insights"', 'id="centro"', 'id="rows"', 'id="op-target"',
                 '/operator/chat/stream', 'opSend(', 'initOpChatSplit('):
        assert gone not in HTML, gone
    assert 'id="newsess"' in HTML          # el formulario de Nueva sesión se conserva

def test_workspace_js_has_no_chat_helpers():
    js = (ROOT / "dash" / "workspace.js").read_text(encoding="utf-8")
    for gone in ("setChatVisible", "openOperatorActions", "opApplyActions", "cc-chat-visible", "cc-chat-draft"):
        assert gone not in js, gone
```

- [ ] **Paso 2: correr para ver el fallo** — Run: `~/.local/bin/pytest -q tests/test_command_sidebar_mount.py` — Esperado: FAIL.

- [ ] **Paso 3: sustituir el contenido de `#content`**. Dentro de `#side-top` (:2003–2086) conservar únicamente el diálogo `#newsess` (:2029–2076); eliminar `section#centro-wrap` (:2004), `section#sessions-wrap` (:2007, incluidos `#btn-newsess`, `#workspace-tools`, `#session-overview`, `#rows`), `section#sidebar-insights` (:2014), `section#recent-wrap` (:2077), `#tl-wrap` (:2082); eliminar `#op-split` (:2087) y `section#op-chat` (:2088–2115). Añadir tras `#newsess`:

```html
<div id="command-sidebar" aria-label="Comandos por CLI"></div>
```

Cargar el módulo justo después de `quick-terminal.js` (:1945): `<script src="/command-sidebar.js?v=1"></script>` (misma forma que los demás script tags de la página). Añadir `command-sidebar.js` a `install.sh:71` si S1 no lo hizo. Como `#tab-new` (:6720–6727) hacía `getElementById("btn-newsess").click()` y ese botón desaparece, cambiar ese handler para llamar `nsOpen()` directamente (H1 moverá el botón a la cabecera con el mismo id `btn-newsess`).

- [ ] **Paso 4: retirar el JS del chat y de las secciones**: borrar el bloque 8636–9034 (`const OP` … la llamada `initOpChatSplit()`), `initOpChatSplit` (6081–6123), `renderSidebarInsights` (7762–7778) y su llamada con `setInterval` en 8377, `renderEvents` (7342) y el handler de `#tl-toggle`, `tickRecent`/`renderRecent`, `renderCentro`/`renderBrain` y `favBtn` (localizar cada una con `grep -n`), **solo si ningún otro código las usa** (`grep -n "<nombre>" dash/*.js bin/cc-app`). El handler `centro` de `bin/cc-app` (`on_msg` :5769) recibe mensajes del web (`extensions`, `reader`, `buttonStyle`, `theme`, `openUrl`, `rename`, y `open_tab` por defecto) y no depende de la tarjeta Centro; no tocarlo. En `render(list)` (:7082–7316): eliminar la construcción de `.row` en `#rows` y las líneas :7085–7086 (`#op-target`), conservar los contadores `#n-waiting/#n-done/#n-working` (:7301–7304) y las llamadas `refreshDesktopTabs()`/`renderTabbar()` (:7313–7314). En `dash/workspace.js` quitar `setChatVisible`, `openOperatorActions`, `opApplyActions` y las referencias de `initSessionWorkspace` a `#workspace-tools`, `#op-hide`, `#op-action-history`, `#op-in`; dejar `openExtensionUsage`, `openSessionProfiles` y `renderSessionOverview` (los reutiliza H1 desde ☰).

- [ ] **Paso 5: montar el módulo** al final del script principal (junto a la creación del `quickTerminal` existente):

```js
window.commandSidebar = ComandosCommandSidebar.createCommandSidebar({
  api, root: $('#command-sidebar'), storage: localStorage, makeId: () => 'ct-' + Date.now().toString(36) + '-' + Math.random().toString(36).slice(2, 8),
  getTarget: () => { const t = activePaneTarget(); return t; },
  focusTarget: t => { openTerm(t.session, t.label || t.session); },
  openBuilder: () => window.chainBuilder && window.chainBuilder.open(),
  toast, terminals: () => WorkspaceDock.stripEntries().filter(e => e.kind === 'scratch' && String(e.paneKey || '').startsWith('pane-q'))
});
commandSidebar.refresh();
```

`activePaneTarget()` es una función nueva de 10 líneas que devuelve `{session, pane, kind, title}` a partir del pane activo que `render()` ya conoce (el mismo dato que antes alimentaba `#op-target`): `kind = 'term'` cuando la tab es `scratch` con `paneKey` `pane-q…`, si no `'pane'`. Llamar `commandSidebar.refresh()` cuando cambia el pane activo (donde antes se actualizaba `#op-target`) y en el `setInterval` de `/state` cada 2 s solo si cambió `cliInPane` (comparar con `commandSidebar.state.cliInPane` usando el `agent` que `/state` ya trae por pane, sin pedir el catálogo entero: llamar `refresh()` únicamente cuando difiera).

- [ ] **Paso 6: CSS** en `dash/workspace.css`: borrar reglas de `#op-chat`, `html.chat-hidden`, `#op-split`; añadir estilos de `#command-sidebar` reutilizando tokens existentes (`--panel`, `--line`, `--dim`, `--brand`, `--amber`): `.cs-cli` tarjeta con borde, `.cli-h` 40 px, `.launch.yolo .lab3` en ámbar, `.cmd` grid `1fr auto` dos líneas, `.cmd.dis{opacity:.45;pointer-events:none}`, `.cs-runner .cs-next{min-height:44px}`, `.cs-terms` al pie con `border-top`. En el bloque `@container sidebar (max-width:760px)` mantener una sola columna; en `html.only-panel` (móvil) el módulo ocupa todo `#content` con scroll.

- [ ] **Paso 6b: actualizar las pruebas que fijaban ids del chat y del Centro**: en `tests/test_usage_ui.py:214–220` sustituir las aserciones de `btn-newsess` en la etiqueta de sesiones y de `id="op-chat"` después de `#tl-wrap` por `assert 'id="command-sidebar"' in HTML`; en `tests/test_dashboard_layout.py:104` cambiar `$("#centro-wrap")` por `$("#command-sidebar")`; en `tests/test_quick_terminal_client.py:50–56` cambiar la aserción `getElementById("btn-newsess").click()` por `nsOpen()` (los ids `tab-terminal`/`tab-new` siguen ahí hasta H1).

- [ ] **Paso 7: actualizar `tests/e2e_sidebar_parity.js`** para el nuevo contrato (stubs de `/commands/catalog`, `/chains`, `/pane/type`; asserts: `#command-sidebar .cs-cli` × 5, `.cs-cli.here` según el `agent` del stub, clic en `.cmd` produce un POST a `/pane/type` y ninguno a `/send`, `.cs-terms` visible, sin desbordamiento horizontal en 390/320, ids iguales desktop/remoto). Mantener el skip de `tests/test_sidebar_parity.py` (aquí no hay Chrome local por política).

- [ ] **Paso 8: correr**

Run: `~/.local/bin/pytest -q tests/test_command_sidebar_mount.py tests/test_command_sidebar_ui.py tests/test_quick_terminal_client.py tests/test_app_quick_terminal.py tests/test_usage_ui.py tests/test_dashboard_layout.py tests/test_dashboard_security.py tests/test_button_styles.py tests/test_remote_ui.py && bash tests/test_js_parses.sh`
Esperado: PASS; `test_js_parses.sh` sin errores de sintaxis.

- [ ] **Paso 9: commit**

```bash
git add dash/index.html dash/workspace.js dash/workspace.css install.sh tests/test_command_sidebar_mount.py tests/e2e_sidebar_parity.js tests/test_usage_ui.py tests/test_dashboard_layout.py tests/test_quick_terminal_client.py
git commit -m "feat: replace the sessions sidebar and chat with the per-CLI command sidebar"
```

---

### Tarea M1: los chips de modelo y las versiones salen del watcher vivo (pedido de Jesús 2026-09-30)

Jesús: "deja un proceso back sync que siempre esté pulleando los modelos más nuevos". Ya existe el watcher de modelos en `bin/cc-dash` (`_model_watch_cycle`, `_model_watch_loop`, snapshot `MODEL_WATCH_FILE = ~/.claude/hooks/model-watch.json`, lib `lib/model_watch.py`: `installed_versions`, `discover_models`, `watch_models`, `_family_ver`, `_norm`; rutas `GET /models/latest`, `POST /models/refresh`). Esta tarea lo conecta con el catálogo de la barra y lo hace más frecuente. No se crea un segundo poller.

**Archivos:**
- Modificar: `lib/model_watch.py` (`installed_versions` también para `opencode` y `agy`, mismo patrón de ruta resuelta y regex).
- Modificar: `bin/cc-dash` (`_model_watch_loop`: versiones cada 600 s en lugar de 1800; escaneo completo de binarios sigue siendo al cambiar versión o cada 6 h; `cli_catalog_payload`: deja de ejecutar `--version` por su cuenta y usa `versions` y `discovered`/`newSince` del snapshot del watcher; la caché del catálogo se invalida cuando cambia `checkedAt` del snapshot; `?refresh=1` fuerza un ciclo del watcher (`_model_watch_cycle(force=True)`) y luego relee).
- Modificar: `lib/cli_catalog.py` (`catalog_view(..., models=None, new_models=None)`), `config/cli-commands.json` (los comandos `/model ` de claude y grok llevan `"argsFrom": "models"`; sus `args` fijos quedan como respaldo si el watcher no tiene datos).
- Modificar/crear pruebas: `tests/test_cli_catalog.py`, `tests/test_commands_catalog_endpoint.py`, `tests/test_model_watch*.py` si existen (grep).
- Modificar: `dash/command-sidebar.js` solo para marcar un chip con clase `new` y texto accesible "nuevo" cuando el catálogo lo indique (`cmd.newArgs: [...]`), y `tests/command_sidebar_checks.cjs` + fixture si el contrato cambia.

**Contrato:**
- `models` = `{cli: [ids]}`: para cada familia (`_family_ver`) de los ids descubiertos por el watcher para ese CLI (`claude`→`claude`, `codex`→`codex`, `grok`→`grok`), solo el MÁS NUEVO por familia, sin variantes con fecha (`-YYYYMMDD`) ni con corchetes (`[1m]`); orden: familias por versión descendente del modelo. Unir con los ids del registro (`config/providers.json` motors) con el mismo criterio.
- En `catalog_view`, un comando con `argsFrom: "models"` usa `models[cli]` si no está vacío; si está vacío conserva sus `args` del JSON. `newArgs` = intersección con `newSince[cli].models` del snapshot.
- `version.installed` de cada CLI sale del snapshot; si el snapshot no tiene ese CLI, `None` (→ `missing` sólo si tampoco hay binario: el watcher ya lo resuelve).
- Nunca se inventa un modelo: todo chip viene del binario instalado, del registro o del JSON curado. Nunca Enter.

**Pruebas (TDD):** vista con `models` y `newSince` inyectados produce los chips más nuevos por familia sin `[1m]` ni fechas, en orden; sin datos del watcher conserva los `args` del JSON; el endpoint no ejecuta `--version` (inyectar un snapshot en `HOME/.claude/hooks/model-watch.json` del harness y comprobar que `version.installed` sale de él); cambiar `checkedAt` invalida la caché; `installed_versions` incluye `opencode` y `agy` con `which`/`run` falsos (refactor mínimo para inyectarlos si hace falta).

**Commit:** `feat: sidebar model chips and CLI versions follow the live model watcher`

---

### Tarea S3: modal Cadenas (constructor con el mismo acordeón)

**Archivos:**
- Crear: `dash/chain-builder.js`
- Crear: `tests/chain_builder_checks.cjs`, `tests/test_chain_builder_ui.py`
- Modificar: `dash/index.html` (montaje junto a S2 paso 5), `dash/workspace.css`

**Interfaces:**
- Consume: `commandSidebar.state.catalog` (misma vista), `api('/chains', {...})` (K2), `commandSidebar.refresh()` al guardar.
- Produce: `createChainBuilder({api, root, catalog: () => view, onSaved, toast})` con `open(slug?)`, `close()`, `state` (`{name, steps, dragging}`). DOM: `.backdrop[data-mclose] > .modal.chain-only` con `.m-head` (nombre, Cerrar), cuerpo con el acordeón por CLI en 3 columnas (`.cli` con `.cli-h[data-toggle]`, `.cmd[data-add][data-kind]` con botón "+ cadena"), y `.slots` con `.step[draggable][data-i]` (kind · texto · asa), botones Guardar (`[data-save]`) y Correr (`[data-run]`, guarda y luego `commandSidebar.startChain(slug)`).

Reglas: un clic en un comando **añade** un paso y no teclea nada; arrastrar reordena (`dragstart`/`dragover`/`drop` sobre `.slots`, misma lógica que `wireDnD` del laboratorio); el fondo cierra solo si `e.target === backdrop` (no `stopPropagation` en el modal); Guardar exige nombre no vacío y ≥ 1 paso; error del servidor se muestra en `.m-error`; `open(slug)` precarga una cadena existente para editarla (`slug` se envía al guardar).

- [ ] **Paso 1: checks Node** en `tests/chain_builder_checks.cjs`: abrir, abrir el CLI Codex, añadir un arranque yolo y `/model`, comprobar `state.steps` en orden, reordenar por drop (`drop` con `dataTransfer` stub de 1→0), Guardar → POST `/chains` con `{name, steps}`; Correr → guarda y llama `onSaved(chain, {run: true})`; guardar sin nombre no llama a la API y pinta `.m-error`; `open('yolo')` precarga y envía `slug`.

- [ ] **Paso 2: correr para ver el fallo** — Run: `~/.local/bin/pytest -q tests/test_chain_builder_ui.py` — Esperado: FAIL.

- [ ] **Paso 3: implementar `dash/chain-builder.js`** con el envoltorio UMD; el HTML del acordeón reutiliza las mismas funciones de fila que `command-sidebar.js` exporta (`ComandosCommandSidebar.rowHTML`, `cliHTML`) con la opción `{mode: 'build'}` que cambia `data-cmd` por `data-add` y añade el botón "+ cadena". Montaje en `index.html`: `window.chainBuilder = ComandosChainBuilder.createChainBuilder({ api, root: document.body, catalog: () => commandSidebar.state.catalog, onSaved: (chain, o) => { commandSidebar.refresh().then(() => { if (o.run) commandSidebar.startChain(chain.slug); }); }, toast });`

- [ ] **Paso 3b: instalación y carga**: añadir `chain-builder.js` a `install.sh:71` y cargarlo con `<script src="/chain-builder.js?v=1"></script>` justo después de `command-sidebar.js`; añadir a `tests/test_chain_builder_ui.py` la aserción `assert "chain-builder.js" in (ROOT / "install.sh").read_text()`.

- [ ] **Paso 4: CSS**: `.modal.chain-only{width:min(1040px,100%);height:min(700px,100%)}`, columnas `grid-template-columns:repeat(3,minmax(0,1fr))` ≥ 900 px y una columna en móvil, `.slots` fija al pie con scroll horizontal.

- [ ] **Paso 5: correr** — Run: `~/.local/bin/pytest -q tests/test_chain_builder_ui.py tests/test_command_sidebar_ui.py && bash tests/test_js_parses.sh` — Esperado: PASS.

- [ ] **Paso 6: commit**

```bash
git add dash/chain-builder.js tests/chain_builder_checks.cjs tests/test_chain_builder_ui.py dash/index.html dash/workspace.css install.sh
git commit -m "feat: chain builder modal that assembles command chains without typing into the pane"
```

---

### Tarea S4: retirar el backend del chat sin borrar datos

**Archivos:**
- Modificar: `bin/cc-dash` (rutas GET `/operator/action-results` :9015 y `startswith("/operator")` :9018; rutas POST `/operator/action-result` :9158, `/operator/chat/stream` :9184, `/operator/chat` :9211, `/operator/new` :9222, `/operator/model` :9229; bloque de helpers `operator_*` :5430–6221; `import operator_receipts` en :5924, :9016, :9159; `"/operator"` en `API_GET` :8672)
- Eliminar: `lib/operator_chat.py`, `lib/operator_stream.py`, `tests/test_operator_chat.py`, `tests/test_operator_stream.py`
- **Conservar**: `lib/operator_catalog.py`, `lib/operator_dispatch.py` (usa `operator_receipts` en su línea 135), `lib/operator_receipts.py`, `tests/test_operator_dispatch.py`, `tests/test_operator_recovery.py` y la ruta `POST /app/command` (`bin/cc-dash:9924`), porque el puente de acciones de la app (`app-command.json`, `bin/cc-app:6931`) sigue usándolos y no forma parte del chat.
- Crear: `tests/test_operator_retired.py`

**Interfaces:**
- Consume: nada. **Produce:** toda ruta `/operator*` responde 410 `{"error": "El chat de CommandOS se retiró; usa la barra de comandos", "code": "retired"}` desde una sola rama en `_do_GET` y otra en `do_POST`; `bin/cc-dash` no importa `operator_chat` ni `operator_stream`; `operator_pane` (`:5507`) conserva su nombre porque `tmux_paste` (`:6259`) lo usa. Los archivos de conversaciones bajo `operator_store_root()` (`:5468`; leer la ruta que devuelve antes de tocar nada) no se abren ni se borran.

- [ ] **Paso 1: prueba** en `tests/test_operator_retired.py` (mismo fixture `dash` que C2):

```python
from pathlib import Path

def test_operator_routes_answer_410(dash):
    for path in ("/operator?id=x", "/operator/action-results"):
        assert dash.get_status(path) == 410
    for path in ("/operator/chat", "/operator/chat/stream", "/operator/new", "/operator/model", "/operator/action-result"):
        assert dash.post_status(path, {}) == 410

def test_app_command_bridge_survives(dash):
    assert dash.post_status("/app/command", {}) != 410

def test_no_chat_module_is_loaded():
    src = (Path(__file__).resolve().parents[1] / "bin" / "cc-dash").read_text()
    assert "operator_chat" not in src and "operator_stream" not in src and "operator_handle_chat" not in src
```

- [ ] **Paso 2: correr para ver el fallo** — Run: `~/.local/bin/pytest -q tests/test_operator_retired.py` — Esperado: FAIL (200/401/404 en lugar de 410).

- [ ] **Paso 3: implementar**: al principio de la cadena de rutas de `_do_GET` (:8698) y de `do_POST` (después de leer `data`, :9041–9059) añadir `if self.path.startswith("/operator"): return self._json(410, {"error": "El chat de CommandOS se retiró; usa la barra de comandos", "code": "retired"})`; borrar los siete handlers listados; quitar `"/operator"` de `API_GET` (:8672). Dentro del bloque :5430–6221 borrar solo las funciones cuyos únicos llamadores eran esas rutas o el propio chat: comprobar cada nombre con `grep -n "<nombre>(" bin/cc-dash lib/*.py bin/cc-app` y borrar únicamente cuando el único uso sea su definición o una función que también se borra (`operator_snapshot`, `operator_set_model`, `operator_handle_chat`, `operator_handle_chat_stream`, `_operator_request_context`, `operator_sse_frame`, `operator_build_dispatcher`, `operator_snippet_send`, `_operator_http_error` son los candidatos; `operator_pane`, `operator_store_root`, `operator_tabs_payload` y cualquier función que use `/app/command` se quedan). Borrar los módulos y pruebas de chat indicados. Después: `grep -rn "operator_chat\|operator_stream" bin lib dash tests` debe devolver vacío.

- [ ] **Paso 4: correr** — Run: `~/.local/bin/pytest -q tests/test_operator_retired.py tests/test_operator_dispatch.py tests/test_operator_recovery.py tests/test_snippets_paste.py tests/test_session_route_matrix.py tests/test_dashboard_security.py` — Esperado: PASS.

- [ ] **Paso 5: commit**

```bash
git add -A bin/cc-dash lib tests
git commit -m "refactor: retire the CommandOS operator chat; conversations on disk stay untouched"
```

---

### Tarea H1: cabecera "Ordenada" y botón Servidores

**Archivos:**
- Modificar: `dash/index.html` (`<header>` :1960–1993; `nav#app-navigation` :1948–1957; `#ssh-bar` :1995–2000 y su CSS en :250–259 y :1923; IIFE de `#ssh-toggle` :8107–8113; `initTabNavigation` :6711–6770 con el handler de `#tab-new` :6720–6727, el de `#tab-terminal` :6759, `createQuickTerminal(` :6762 y `termBtn.onclick` :6765; el mapa `ICON` :7615–7640 para añadir el icono `menu`)
- Modificar: `dash/workspace.css` (bloque de cabecera :584–624; añadir caras `consola` para los ids nuevos junto a :617–623)
- Crear: `tests/test_header_layout.py`
- Modificar: `tests/test_quick_terminal_client.py:50–56` (ids del botón Terminal), `tests/test_button_styles.py` (añadir los ids nuevos a la comprobación de `data-icon` + `hdr-lbl`)
- `bin/cc-app`: sin cambios previstos. `_dash_click` (:5062) solo se usa con `btn-notif` (:5221) y `btn-settings` (:5230), ids que se conservan; la cabecera GTK tiene sus propios botones nativos (`_plus`, `_quick_term_btn`, `_sort_btn`, `_news_btn`, `_pomo_btn`).

**Interfaces:**
- Consume: `quickTerminal.open()` (instancia creada en `:6762`), `nsOpen()`, `loadSsh()` (:8114), `toggleSshManager` (:8154) sin cambios, `renderSessionOverview`, `openSessionProfiles`, `openExtensionUsage` (`dash/workspace.js`), el sistema de estilos `html[data-btn-style]` + `header .hdr-btn` (`workspace.css:584–624`), `window.ComandosNotices.instance.toggleStrip()` (handler de `#btn-notif` :5670).
- Produce: fila 1 `header.hdr-ordered`, todos `class="hdr-btn"` con `data-icon` y `<span class="hdr-lbl">`, en este orden: `#btn-menu` (☰, icono `menu`), `.counts` (`#n-waiting`/`#n-done`/`#n-working`), `#btn-terminal` (icono `terminal`), `#btn-newsess` (`+ Nueva sesión`, icono `plus`, clase `primary`), y `.hdr-right`: `#btn-switch`, `#btn-snippets`, `#btn-usage`, `#btn-remote`, `#btn-servers` (nuevo, icono `server`, `title="Servidores"`, `aria-expanded`), `#btn-news`, `#btn-notif` (+ `#notif-badge`, `#notif-panel`), `#btn-pomo` (+ `#pomo-panel`), `#btn-settings`, `#clock`. Fila 2: `#app-navigation` con `#tab-panel`, `#tab-prev`, `#tabbar`, `#tab-next`, `#tab-sort`, `#tab-open`; `#tab-terminal` y `#tab-new` desaparecen (pasan a la fila 1). `#menu-panel` (☰, `class="hidden" role="dialog"`) contiene, con sus ids y handlers actuales: `#btn-theme` (Apariencia), `#btn-sov` (Soberanía), `#snd-ctl` (`#btn-mute`, `#vol-top`), `#limits-strip`, `#toggle-overview` (Todas las sesiones), `#open-session-profiles` (Perfiles), `#open-extension-usage` (Uso de herramientas). `#ssh-bar` sale de `#topbar` y vive dentro de `<div id="servers-panel" class="hidden">` justo bajo la cabecera; `#btn-servers` lo alterna. Contenido, funciones y `localStorage cc-ssh-open` de Servidores no cambian.

Reconciliación con el grill del 29 (posterior al mockup de la ronda 9): la campana `#btn-notif` se queda en la fila 1 porque desde `9db3c1f` es lo único que abre el cajón de avisos; los botones ya son de un solo tamaño con etiqueta oculta (`.hdr-lbl`), así que la regla "solo icono bajo 1600 px" del mockup ya está cubierta por el sistema de estilos y no se reimplementa.

- [ ] **Paso 1: prueba de texto** `tests/test_header_layout.py`:

```python
from pathlib import Path
import re
ROOT = Path(__file__).resolve().parents[1]
HTML = (ROOT / "dash" / "index.html").read_text(encoding="utf-8")
CSS = (ROOT / "dash" / "workspace.css").read_text(encoding="utf-8")

def test_row_one_order_and_servers_button():
    head = HTML[HTML.index('<header class="hdr-ordered"'):HTML.index('</header>')]
    order = re.findall(r'id="(btn-menu|n-waiting|btn-terminal|btn-newsess|btn-switch|btn-snippets|btn-usage|btn-remote|btn-servers|btn-news|btn-notif|btn-pomo|btn-settings|clock)"', head)
    assert order == ["btn-menu", "n-waiting", "btn-terminal", "btn-newsess", "btn-switch", "btn-snippets", "btn-usage", "btn-remote", "btn-servers", "btn-news", "btn-notif", "btn-pomo", "btn-settings", "clock"]
    for tag_id, icon in (("btn-menu", "menu"), ("btn-terminal", "terminal"), ("btn-newsess", "plus"), ("btn-servers", "server")):
        tag = head[head.index(f'id="{tag_id}"') - 40: head.index(f'id="{tag_id}"') + 260]
        assert 'class="hdr-btn' in tag and f'data-icon="{icon}"' in tag and 'class="hdr-lbl"' in tag, tag_id
    topbar = HTML[HTML.index('<div id="topbar">'):HTML.index('<div id="content">')]
    assert 'id="ssh-bar"' not in topbar                       # ya no es fila permanente
    assert 'id="servers-panel"' in HTML and 'id="ssh-bar"' in HTML and 'id="ssh-chips"' in HTML and 'id="ssh-manage"' in HTML

def test_tabs_row_keeps_sort_and_drops_terminal_buttons():
    nav = HTML[HTML.index('<nav id="app-navigation"'):HTML.index('</nav>')]
    assert 'id="tab-sort"' in nav and 'id="tab-panel"' in nav
    assert 'id="tab-terminal"' not in nav and 'id="tab-new"' not in nav

def test_relocated_controls_keep_their_ids_inside_the_menu():
    start = HTML.index('id="menu-panel"'); menu = HTML[start:start + 6000]
    for i in ("btn-theme", "btn-sov", "btn-mute", "vol-top", "limits-strip", "toggle-overview", "open-session-profiles", "open-extension-usage"):
        assert f'id="{i}"' in menu, i

def test_ssh_functions_untouched_and_styles_cover_new_buttons():
    for fn in ("async function loadSsh(", "async function openSshTab(", "async function connectHost(", "async function setupSshKey(", "function toggleSshManager("):
        assert fn in HTML, fn
    assert "--hdr-key" in CSS and "#btn-servers" in CSS
```

- [ ] **Paso 2: correr para ver el fallo** — Run: `~/.local/bin/pytest -q tests/test_header_layout.py` — Esperado: FAIL.

- [ ] **Paso 3: implementar el HTML**: reescribir `<header>` (:1960–1993) como `<header class="hdr-ordered">` con el orden anterior, reutilizando los botones existentes tal cual (mismos ids, `data-icon`, `hdr-lbl`, `title`) y añadiendo:

```html
<button class="hdr-btn" id="btn-menu" aria-label="Más" title="Más" aria-haspopup="dialog"><span data-icon="menu" data-size="17"></span><span class="hdr-lbl">Más</span></button>
<button class="hdr-btn" id="btn-terminal" aria-label="Terminal" title="Terminal rápida en una carpeta nueva"><span data-icon="terminal" data-size="17"></span><span class="hdr-lbl">Terminal</span></button>
<button class="hdr-btn primary" id="btn-newsess" aria-label="Nueva sesión" title="Nueva sesión"><span data-icon="plus" data-size="17"></span><span class="hdr-lbl">Nueva sesión</span></button>
<button class="hdr-btn" id="btn-servers" aria-label="Servidores" title="Servidores" aria-expanded="false"><span data-icon="server" data-size="17"></span><span class="hdr-lbl">Servidores</span></button>
```

Añadir al mapa `ICON` (:7615–7640) la entrada `menu` (tres líneas horizontales, mismo formato que `smartphone`). Quitar `#tab-terminal` (:1953) y `#tab-new` (:1954) del `nav`. En `initTabNavigation` (:6711–6770): el handler de `#tab-new` (:6720–6727) pasa a `#btn-newsess` llamando `nsOpen()`; `termBtn` (:6759, :6765) pasa a `document.getElementById("btn-terminal")`; `createQuickTerminal` (:6762) no cambia. Crear `<div id="menu-panel" class="hidden" role="dialog" aria-label="Más">` justo después de `</header>` con los controles reubicados (mover los nodos existentes, no duplicarlos). Mover `<div id="ssh-bar">` (:1995–2000) a `<div id="servers-panel" class="hidden">` después de `#menu-panel`. Handlers:

```js
$('#btn-servers').addEventListener('click', () => {
  const p = $('#servers-panel'); const open = !p.classList.toggle('hidden');
  $('#btn-servers').setAttribute('aria-expanded', String(open));
  if (open) { $('#ssh-bar').classList.add('open'); loadSsh(); }
});
$('#btn-menu').addEventListener('click', () => $('#menu-panel').classList.toggle('hidden'));
```

`#ssh-bar.open` muestra todos los chips y "gestionar" por la regla existente `#ssh-bar:not(.open) …` (:258–259); `loadSsh` (:8114) y la IIFE de `#ssh-toggle` (:8107–8113) no se tocan.

- [ ] **Paso 4: CSS** en `dash/workspace.css`: `.hdr-ordered{display:flex;align-items:center;gap:8px;flex-wrap:nowrap}` y `.hdr-ordered .hdr-right{margin-left:auto}` dentro del bloque de cabecera (:584–624) para que los botones nuevos hereden `--hdr-key` y `.hdr-lbl`; en las caras `consola` (:617–623) añadir `#btn-servers`, `#btn-terminal`, `#btn-newsess` y `#btn-menu` con un color por función; `#servers-panel` con el `max-width:1360px` que ya usa `#ssh-bar` (`index.html:250`); `#menu-panel` como popover bajo ☰ (`position:absolute; z-index` igual a `#notif-panel`). En `@media (max-width:640px)` (`index.html:1916` y siguientes): contadores abreviados (`esperan`→`esp.`) y `#btn-newsess` sin etiqueta visible (ya lo está por `.hdr-lbl`).

- [ ] **Paso 5: pruebas existentes**: en `tests/test_quick_terminal_client.py:50–56` cambiar `tab-terminal` por `btn-terminal` y `tab-new` por `btn-newsess`; en `tests/test_button_styles.py` añadir `("btn-servers","server"),("btn-terminal","terminal"),("btn-newsess","plus"),("btn-menu","menu")` a la lista de `data-icon` + `hdr-lbl`. `tests/test_remote_ui.py:1839` (`id="btn-remote"`) sigue verde sin cambios.

- [ ] **Paso 6: correr** — Run: `~/.local/bin/pytest -q tests/test_header_layout.py tests/test_button_styles.py tests/test_quick_terminal_client.py tests/test_command_sidebar_mount.py tests/test_app_quick_terminal.py tests/test_remote_ui.py tests/test_operator_dispatch.py && bash tests/test_js_parses.sh` — Esperado: PASS.

- [ ] **Paso 7: commit**

```bash
git add dash/index.html dash/workspace.css tests/test_header_layout.py tests/test_button_styles.py tests/test_quick_terminal_client.py
git commit -m "feat: ordered two-row header with Servidores opening the existing SSH bar"
```

---

### Tarea V1: verificación en la Mac mini y registro

**Archivos:**
- Modificar: `docs/verification/commandos-v1-fase2.md`

- [ ] **Paso 1: suite completa** — Run: `~/.local/bin/pytest -q tests/ 2>&1 | tail -5 && bash tests/test_js_parses.sh` — Esperado: exactamente los mismos fallos que la línea base registrada en Preparación (`/tmp/claude-1000/comandos-fase2-baseline.txt`); cualquier fallo nuevo bloquea.

- [ ] **Paso 2: arrancar un cc-dash de ensayo** con `HOME` y `TMUX_TMPDIR` aislados en un puerto libre (mismo arranque que el fixture `dash`), crear una sesión tmux privada `demo` con `claude` **no** lanzado (una shell) y exponerlo: `/home/someguy/.local/bin/cc-browser-expose start <puerto>`.

- [ ] **Paso 3: en la Mac mini con `chrome-bg`** (`new_page` → `http://127.0.0.1:<puerto>/`), a 1440×1000 y 390×844: capturar la barra (acordeón con 5 CLI, yolo primero), clic en un arranque yolo y comprobar con `tmux capture-pane` en el socket privado que el texto está en el prompt y no se ejecutó; plegar y reabrir el primer CLI; abrir Cadenas, armar dos pasos, guardar, correr con Siguiente dos veces (capturar el pane entre pasos); cabecera: ☰ abre el menú con Apariencia/Soberanía/volumen; Servidores abre la fila SSH real; la campana sigue abriendo el cajón de avisos; el reloj de arena sigue en `#btn-pomo`; cambiar el estilo de botones en Ajustes → Apariencia restyla también los botones nuevos; Resúmenes abre el lector con la barra de comandos visible; sin desbordamiento horizontal en móvil; consola sin errores (`list_console_messages`). Guardar capturas con `take_screenshot` (ruta de la Mac) y transferirlas con `scp` a `docs/verification/shots/fase2/`, citando ruta absoluta local.

- [ ] **Paso 4: cerrar** la sesión tmux de ensayo (`kill-server` solo en el socket privado), `cc-browser-expose stop <puerto>`, `close_page`.

- [ ] **Paso 5: registrar** en `docs/verification/commandos-v1-fase2.md` cada tarea con commit, comando de prueba, resultado, capturas, limitaciones (p. ej. `test_sidebar_parity` saltado por política) y veredicto humano pendiente. Commit `docs: record phase-2 verification`.

---

## Decisiones pequeñas que faltan dentro de este bloque

| ID | Decisión pendiente de Jesús | Recomendación implementada | Momento de preguntar |
| --- | --- | --- | --- |
| D7 | Terminales rápidas al pie de la barra: lista de destinos que enfoca la tab (E2 ya las hizo tabs del workspace) o terminal embebida como en el mockup. | Lista de destinos con "+ nueva"; una terminal embebida duplicaría el pane. | Demostración S2. |
| D8 | Dónde viven Todas las sesiones, Perfiles, Uso de herramientas, Apariencia, Soberanía, volumen y límites al salir de la barra y de la cabecera. | Menú ☰ de la fila 1 con los ids existentes. La campana no entra aquí: se queda en la fila 1 porque abre el cajón de avisos (decisión del 29-sep). | Demostración H1. |
| D9 | Un clic con texto ya escrito en el prompt: añadir al final (implementado) o limpiar antes. | Añadir; limpiar borraría lo que el usuario escribió. | Demostración S1. |
| D10 | Retirar el código del chat (rutas 410 y módulos) o solo esconderlo. | Retirar código; conservar conversaciones en disco. | Antes de S4. |

## Autorrevisión

- Cobertura de la especificación: catálogo por CLI con versión y "en este pane" (C1, C2, S1); todo plegable y yolo primero (S1); fila de dos líneas con chips (S1 `rowHTML`); clic escribe letra por letra sin Enter (T1, S1); cadenas guardadas y tarjeta corriendo con Siguiente/Parar (K1, K2, S1); modal Cadenas con el mismo acordeón (S3); chat sustituido por terminales rápidas (S2, S4); cabecera Ordenada sin fila SSH permanente y Servidores intacto (H1); drift/missing/unverified (C1, S1); móvil (S2 paso 6, H1 paso 4, V1).
- Nombres consistentes: `createCommandSidebar`, `applyCatalog`, `startChain`, `next`, `stop`, `insert`; `type_literal`, `PaneTypingLocks`, `TypingError`; `list_chains`, `save_chain`, `delete_chain`, `ChainError`; `catalog_view`, `installed_versions`, `version_status`; rutas `/commands/catalog`, `/pane/type`, `/chains`, `/chains/delete`.
- Foco de revisión: 1 y 2 en T1; 3 en S1 (punto 7); 4 en K1 y S1 (punto 5); 5 en S1 (punto 4) y T1 (`pane_gone`).
- Reconciliación con main `0561687` (grill de fidelidad del 29–30): el sistema de botones 3D, el reloj de arena, el cajón de avisos tras la campana, Resúmenes en `#panes`, `#tab-sort`, stickers y bandejas se conservan sin cambios (restricciones globales, H1); los archivos nuevos entran en `install.sh` (S1, S3); `/app/command` y `lib/operator_dispatch/catalog/receipts` sobreviven al retiro del chat (S4); las pruebas que fijaban `#op-chat`, `#centro-wrap`, `#tab-new`/`#tab-terminal` se actualizan en S2 y H1.
