# Operador de ComandOS: streaming + catálogo completo de acciones — Diseño

## Problema

El chat del operador (`/operator/chat`) hoy:

- No hace streaming: un solo POST bloqueante; el navegador espera a que terminen hasta 4 rondas de tools (hasta 12 llamadas HTTP encadenadas con timeouts de 60 s cada una).
- Es lento por diseño: relee credenciales y memoria de disco en cada intento, reenvía el esquema de 29 tools en cada llamada, cae en cascada a otros dos modelos ante cualquier respuesta vacía, y ejecuta las tools en serie.
- Mapea 29 tools. La UX/UI real de ComandOS tiene ~120 endpoints en cc-dash, ~250 controles en el tablero remoto, ~90 acciones en la app GTK (splits, mosaico, zoom, pestañas, ventana) y una toolbar táctil en el terminal web. Casi nada de eso es alcanzable desde el chat.

## Decisiones

1. **Un catálogo único de acciones** en `lib/operator_catalog.py`: cada acción de la UX es una `ToolSpec` con nombre, grupo, descripción, esquema de parámetros y un *target* que dice cómo ejecutarla. Los esquemas Anthropic/OpenAI se generan desde ahí. Un test cruza el catálogo con el código fuente (paths de cc-dash, selectores del tablero, comandos de cc-app) para que no haya deriva.
2. **Cuatro tipos de target**:
   - `api`: llama un endpoint de cc-dash por loopback (`127.0.0.1:4777`) con el token. No se refactorizan los handlers.
   - `ui`: devuelve una *action* al navegador; `opApplyActions` la ejecuta (click a un selector, llamada a una función global, postMessage al iframe del terminal).
   - `app`: escribe `~/.claude/hooks/app-command.json`; cc-app lo observa (mismo patrón que `app-focus.json`) y ejecuta el handler GTK (splits, mosaico, zoom, pestañas, ventana).
   - `local`: los callbacks Python que ya existen (focus, rename, split, pause, key…).
3. **Streaming por SSE** en `POST /operator/chat/stream`. El servidor pide `stream: true` al proveedor, reenvía deltas de texto y anuncia cada tool call y su resultado. El frontend lee el body con `ReadableStream` (EventSource no admite POST). `/operator/chat` sigue existiendo para compatibilidad y tests.
4. **Latencia**: caché de credenciales (60 s), esquema de tools serializado una vez por proceso, `cache_control` de Anthropic sobre system+tools, timeout 30 s por llamada, respaldo a otro modelo solo ante 401/429/5xx/red (nunca ante “respuesta vacía”), tools de un mismo turno ejecutadas en paralelo cuando son de solo lectura.
5. **Seguridad**: las acciones destructivas (matar sesión, borrar servidor SSH, borrar snippet, apagar remoto, matar split, cerrar la app) exigen `confirm: true`; el system prompt obliga a pedir confirmación en el chat antes. Las claves de teclado siguen limitadas por `ALLOWED_KEYS`. Nada nuevo se expone fuera de loopback/tailnet.
6. **Cobertura obligatoria**: la tabla del catálogo (sección "Mapa") es el contrato. Un control de la UI sin tool es un defecto.

## Mapa (resumen; el detalle vive en el catálogo)

| Grupo | Fuente | Cantidad aprox. |
|---|---|---|
| Sesiones y pestañas | /tabs /state /tab-* /session-new /focus /up /ensure /shell /kill /export /new /open-with-account /recover-tab | 22 |
| Panes e input | /send /paste /key /pause /tmux-mouse /tmux-scroll /model/switch-cancel + splits locales | 12 |
| Motor, modelo, cuentas, skills, MCP | /proxy /model/* /harness/switch /account/* /providers /model-tiers /opencode/models /skill-toggle /mcp-toggle /optimization/* /sovereignty | 20 |
| Uso, guardia, analytics | /usage/* /dedication /ui-log/summary | 18 |
| Pomodoro | /pomodoro + botones del panel | 8 |
| Ajustes y preferencias | /conf-set /prefs-set /test /conf /prefs + switches del tablero | 20 |
| Notificaciones | /notifs/count + panel (localStorage) | 8 |
| Remoto y SSH | /remote-* /ssh-* | 13 |
| Snippets | /snippets* | 5 |
| Archivos y enlaces | /fs/* /open-path /open-url | 4 |
| Noticias y catálogo de modelos | /news/* /models/* | 4 |
| Navegación del tablero | paneles, tabs internas, switcher, wizard, centro, layout | 16 |
| Toolbar del terminal web | Esc/Enter/Tab/flechas/Ctrl/pegar/seleccionar | 5 |
| App de escritorio (GTK) | splits, mosaico, zoom, pestañas, ventana, fuente, atajos | 28 |
| Chat/operador | memoria, conversación, modelo del chat, copiar | 7 |

## Fuera de alcance

- Cambiar el proveedor del chat o añadir modelos con razonamiento.
- Reescribir handlers de cc-dash como funciones (se llaman por loopback).
- Acciones de depuración por señal (SIGUSR2/RTMIN) y arrastre de ventana.
