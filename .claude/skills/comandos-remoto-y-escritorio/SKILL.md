---
name: comandos-remoto-y-escritorio
description: Paridad remoto + escritorio en ComandOS. Úsala SIEMPRE que vayas a cambiar cualquier cosa visible o funcional de ComandOS (dash/index.html, dash/*.js, dash/*.css, dash/term.html, bin/cc-app, bin/cc-dash, lib/), aunque el pedido solo nombre una de las dos versiones o no nombre ninguna: barra de comandos, terminales, pestañas, paneles, cabeceras de pane, modales, temas, iconos, atajos, copiar/pegar, foco, notificaciones. Explica dónde vive cada parte en el escritorio (cc-app GTK) y en el remoto (navegador), cómo implementar en ambos y cómo verificar cada uno antes de dar algo por terminado.
---

# ComandOS: todo cambio va en remoto y en escritorio

Jesús usa ComandOS de dos formas y espera lo mismo en las dos:

- **Escritorio**: `bin/cc-app`, una ventana GTK. A la izquierda un WebKit carga el tablero
  (`http://127.0.0.1:4777/?app=1`, `inApp()` es verdadero); a la derecha y debajo hay
  terminales **VTE nativas** con `tmux attach`, con cabecera de pane y marcos pintados por
  cc-app (`_attach_model_bar`, `_pane_frames`, `_refresh_tab_models`).
- **Remoto**: el mismo `dash/index.html` en un navegador (móvil o la Mac) servido por
  `bin/cc-dash`; `WEBTERM` es verdadero y las terminales son ttyd en iframes que cargan
  `dash/term.html` por `/term` (cabecera y marcos de pane hechos en web, `lib/terminal_panes.py`).

Un cambio que solo existe en una versión se siente como un bug en la otra: Jesús lo ha
pedido explícitamente («cualquier modificación sea siempre en remoto y en desktop»). Por eso,
antes de cerrar cualquier tarea, cada cambio tiene que estar hecho y comprobado en ambas, o
tener escrito por qué una de las dos no aplica.

## 1. Ubica la pieza en cada versión

Busca primero dónde vive hoy lo que vas a cambiar, en las dos versiones. Mapa de partes
que ya divergen (léelo como pistas, verifica en el código porque cambia rápido):

| Parte | Escritorio (cc-app) | Remoto (navegador) |
|---|---|---|
| Barra de comandos | `dash/command-sidebar.js` dentro del WebKit | el mismo módulo |
| Terminal de la barra | VTE nativa bajo el WebKit (`_side_paned`, `_side_term_show`; la web avisa con `{sidebarTerm: …}`) | iframe ttyd (`sidebarTermMount`, `.mini`) |
| Terminales de sesión | VTE + `tmux attach` (`open_tab`, `make_term`) | ttyd + `dash/term.html` |
| Cabecera y marcos de pane | `_pane_frames`, `_reposition_pills` en cc-app | capa `#pane-chrome` en `term.html` |
| Modal de Cadenas | ventana GTK modal (`open_chain_modal`, `?panel=chains`) | backdrop web (`chain-builder.js`) |
| Botones de la cabecera | `HEADER_ACTIONS` por el puente `centro` | handlers web |
| Copiar | puente tmux → portapapeles (`lib/tmux_clipboard.py`) + selección VTE | selección de xterm.js / navegador |
| Foco y destino de comandos | `/active-tab` que publica cc-app + `sidebarTermFocused` | `remotePaneFocus` en la web |
| Tema | `apply_theme` en cc-app + tokens CSS | tokens CSS (`data-theme`) |
| Esconder panel izquierdo | botón `panel-left` en la tira de pestañas | (pendiente si no existe: dilo) |

El puente web → app es `window.webkit.messageHandlers.centro.postMessage(JSON)` y se
atiende en `on_msg` de cc-app; app → web es `_dash_js` (despliega el panel) o
`_dash_js_quiet` (no lo despliega).

## 2. Implementa en las dos

- Si la pieza es web compartida (`dash/`), un solo cambio suele cubrir ambas; comprueba aun
  así las ramas `inApp()` / `WEBTERM` que haya en ese código y el CSS con `data-*` del modo.
- Si en escritorio la pieza es nativa (GTK/VTE), el cambio va en `bin/cc-app` **y** su
  equivalente web (`dash/term.html`, `sidebarTermMount`, etc.). Mismo comportamiento,
  mismos textos, misma jerarquía visual.
- Si de verdad no aplica a una versión (por ejemplo, un atajo de teclado GTK), escribe en el
  mensaje de commit y en la respuesta qué equivalente tiene la otra o por qué no lo necesita.
- Sube los cache-busters de `dash/index.html` (`?v=`) de cada archivo web tocado; si no,
  el WebKit del escritorio sigue con la versión vieja.
- Colores siempre con tokens del tema (`--panel`, `--text`, `--brand`…, o los `--cs-*`
  derivados); nada de hex sueltos que solo funcionan en un tema.

## 3. Prueba en las dos

Tests (Python del sistema, que trae GTK):

```bash
uv venv -q --python /usr/bin/python3 --system-site-packages /tmp/claude-1000/pyt   # una vez
uv pip install -q --python /tmp/claude-1000/pyt/bin/python pytest
DISPLAY=:1 XAUTHORITY=/run/user/1000/gdm/Xauthority /tmp/claude-1000/pyt/bin/python -m pytest -q -p no:cacheprovider tests/<archivos>
node tests/command_sidebar_checks.cjs; node tests/chain_builder_checks.cjs
```

Añade un test por versión cuando el cambio toque código propio de cada una (por ejemplo,
uno de cc-app y uno del módulo web). El aviso «the test session modified the real state
database» suele ser la app viva escribiendo su WAL, no tus tests.

Puesta en marcha:

1. `git pull --rebase origin main` antes de empujar (hay otra sesión trabajando en main).
2. `systemctl --user restart cc-dash` si tocaste `bin/cc-dash`, `lib/` que use, o config.
3. cc-app la relanza la otra sesión (`comandos-92`): pídeselo por `SendMessage` con el commit
   y los cache-busters, y espera su confirmación. Nunca mates sesiones tmux por patrón.

Verificación visual, una por versión:

- **Remoto**: MCP `chrome-bg` en la Mac mini. `cc-browser-expose start 4777`, abre
  `http://127.0.0.1:4777/` y, para el modo remoto, ejecuta
  `window.__COMANDOS_DEV_WEBTERM = true` antes de cargar o usa la URL ts.net. Revisa también
  390×844 (móvil). Cierra la página y `cc-browser-expose stop 4777` al terminar. Si
  `chrome-bg` no está, dilo; no abras un Chrome local.
- **Escritorio**: captura de la ventana sin hacer clics en los panes de Jesús:
  ```bash
  export DISPLAY=:1 XAUTHORITY=/run/user/1000/gdm/Xauthority
  W=$(wmctrl -l | awk '$NF=="ComandOS"{print $1}' | head -1); import -window "$W" /tmp/claude-1000/desk.png
  ```
  Y comprueba que el WebKit sirve la versión nueva (`curl -s http://127.0.0.1:4777/ | grep -o 'archivo.js?v=[0-9]*'`).

## 4. Informa por versión

En la respuesta final, una línea por versión: qué cambió y cómo lo verificaste (captura,
test, navegador). Si una versión quedó sin verificar en pantalla, dilo con la razón y cómo
puede probarlo Jesús. No digas «funciona» de una versión que no viste.
