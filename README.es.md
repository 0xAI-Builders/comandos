# ComandOS — Centro de comando local-first para agentes de programación con IA

Corre Claude Code, OpenAI Codex CLI, Gemini CLI, OpenCode y otros agentes en
paralelo dentro de terminales tmux persistentes. Recibe notificaciones
accionables, administra conexiones SSH directas, monitorea el uso de tus planes
y continúa de forma segura desde tu escritorio, celular o tablet mediante la
PWA de terminal remota sobre Tailscale, sin un backend alojado de ComandOS, una
base de datos externa de ComandOS, telemetría ni un proxy SSH de ComandOS.

[![licencia](https://img.shields.io/badge/licencia-MIT-2EE59D?style=flat-square)](./LICENSE)
[![GitHub Sponsors](https://img.shields.io/badge/sponsor-0xAI--Builders-f38ba8.svg?style=flat-square&logo=github-sponsors)](https://github.com/sponsors/0xAI-Builders)
[![Buy Me a Coffee](https://img.shields.io/badge/buy%20me%20a%20coffee-0xjesus-yellow.svg?style=flat-square&logo=buymeacoffee)](https://buymeacoffee.com/0xjesus)

**[Read it in English →](./README.md)**

![ComandOS — mission control for parallel Claude Code agents](.github/media/dashboard.png)

## Por qué

Cuando corres muchos agentes a la vez, el problema no son las terminales: es
**saber cuál te necesita y volver a él sin fricción**. ComandOS convierte Claude
Code, Codex CLI, Gemini CLI, OpenCode y sesiones shell normales en un flujo por
eventos: el sistema te interrumpe (voz + popup accionable), respondes con una
tecla o una línea y sigues. No escaneas pestañas.

## Local-first por diseño

ComandOS corre en tu computadora. Su estado operativo vive en sesiones tmux,
archivos JSON y de configuración locales, y SQLite local en
`~/.claude/hooks/comandos-usage.sqlite`. No requiere un backend alojado de
ComandOS, una base de datos externa de ComandOS, un servicio de telemetría ni
un proxy SSH de ComandOS.

Los hosts SSH se leen desde `~/.ssh/config` y el cliente OpenSSH instalado se
conecta directamente desde tu computadora al servidor elegido. ComandOS no
sube tu configuración SSH ni tus llaves privadas a un servicio de ComandOS. Si
pides explícitamente configurar el acceso sin contraseña, `ssh-copy-id` copia
únicamente la llave pública seleccionada a ese servidor.

Algunas funciones usan la red solo cuando las utilizas o configuras:

- Los CLI de agentes se comunican con sus propios proveedores de IA según la
  configuración y los términos de cada proveedor.
- El monitoreo de uso puede consultar directamente los endpoints de Anthropic
  u OpenAI cuando existen las credenciales correspondientes.
- El acceso remoto mediante Tailscale Serve es opcional y permanece dentro de
  la configuración de tailnet que tú controlas.
- El control por Telegram es opcional; al activarlo, el contenido de las
  notificaciones, respuestas y comandos pasa por la API de bots de Telegram.
- La voz se reproduce localmente una vez instalada; `cc-doctor --fix` puede
  descargar un modelo de voz opcional de Piper cuando le pides reparar la voz.

## Qué hace

- **Tablero en vivo** (`127.0.0.1:4777`): lo que espera TU respuesta sale arriba y
  grande, con las opciones reales de cada pregunta como botones.
- **Popups accionables**: texto completo con markdown renderizado (tablas incluidas),
  botones 1/2/3, input, Copiar y "Ver TODO". Nunca respondas a ciegas.
- **App nativa** (GTK + VTE): una pestaña por sesión con punto de estado
  (ámbar espera · azul trabaja · verde listo), **Ctrl+K** salta a cualquier sesión,
  pestañas renombrables, splits y temas Noche/Día/Cálido.
- **Copia y exporta** cualquier respuesta: portapapeles, `.txt` o PDF con markdown
  renderizado — desde la terminal, el popup o el tablero.
- **Voz local** (piper, 100% offline) y chime, con UN volumen global.
- **Servidores SSH**: CRUD sobre `~/.ssh/config`, conexión de un click, detecta
  túneles multiplexados vivos (reconexión sin password).
- **Snippets**: guardá comandos shell reutilizables (una línea o scripts multilínea) y pegalos en la sesión activa con **Ctrl+Shift+K**. Bracketed paste — nada se ejecuta hasta que aprietes Enter.
- **Telegram**: botones opcionales en las notificaciones, responder por reply, `/ls /out /run`.
- **Estado local-first**: tmux, archivos JSON/de configuración y SQLite local
  sobreviven reinicios sin un servicio alojado de ComandOS.
- UI en **inglés y español** (auto-detectado por `$LANG`, cambiable en Ajustes).

## Instalar

```bash
git clone https://github.com/0xAI-Builders/comandos.git
cd comandos && ./install.sh
```

Requisitos: Linux con GNOME/X11, `python3-gi`, `tmux`, `jq`.
Recomendados: `xclip`, `wmctrl`, `piper` (voz), `kitty`.
Abre **ComandOS** desde el menú de apps o el tablero en <http://127.0.0.1:4777>.

## Agentes

| Agente | Integración | Qué obtienes |
|---|---|---|
| **Claude Code** | hooks nativos (auto vía `install.sh`) | working · waiting **con opciones reales** · done · respuesta completa |
| **Codex CLI** | hooks lifecycle + fallback `notify` (`cc-agents setup`) | working · waiting por permisos · done + ultimo mensaje |
| **OpenCode** | plugin (`cc-agents setup`) | working · waiting · errores · done |
| **Gemini CLI** | hooks (`cc-agents setup`) | working · waiting · done · respuesta completa |
| **Antigravity CLI** (`agy`) | hooks (`cc-agents setup`) | working · done — verificado con sesión real |

Un comando conecta todo lo que tengas instalado: **`cc-agents setup`**.
Si Codex te pide revisar hooks nuevos, abre `/hooks` en Codex y confia una vez
en el hook de ComandOS.
Cualquier otro agente entra con una llamada HTTP:
`POST 127.0.0.1:4777/event {"agent","event","cwd","msg?","full?"}`.

## Celular y tablet (seguro)

Sigue prompteando desde el celular — de forma segura, con un comando:

```bash
cc-mobile          # conecta por tu tailnet y muestra un QR para emparejar
```

Escanea el QR, abre el tablero, "Agregar a inicio" — se instala como app
(PWA), sin App Store. Responde prompts, contesta agentes, corre sesiones
desde la cama.

**¿Quieres la terminal COMPLETA e interactiva en el celular?** Instala `ttyd`
y corre `cc-webterm` — cada sesión tiene un botón **Terminal** que abre la
terminal real, viva (xterm.js attacheado a la misma sesión tmux). Tecleas,
scrolleas, corres `vim` — igual que sentado en tu escritorio, en vivo y
compartido. Se enruta en `/term` por el mismo Tailscale Serve. ComandOS pasa el
token existente como capacidad de arranque por conexión; el helper de attach
rechaza la conexión antes de abrir tmux si falta o es incorrecto. No aparece un
segundo login.

**Cuando activas el acceso remoto, la seguridad usa varias capas:**
- **Tailscale** (WireGuard): el acceso sigue la política de tu tailnet para usuarios y dispositivos autorizados, con cifrado extremo a extremo. ComandOS usa Serve, jamás Funnel público.
- **TLS automático** vía `tailscale serve` (https en tu tailnet).
- **Token de acceso**: protege el tablero remoto, la API de cc-dash y cada
  conexión a la terminal interactiva. `cc-dash` sigue escuchando solo en
  `127.0.0.1`; Serve hace de puente.
- **Frontera de la terminal interactiva**: ttyd también escucha únicamente en
  loopback. Su helper valida el mismo token antes de abrir tmux, mientras la
  política de Tailscale limita quién alcanza `/term` y `:8443`.
- **Anti DNS-rebinding**: allowlist de cabecera Host (loopback + `*.ts.net`).
- Los clientes locales reciben automáticamente la capacidad de terminal; las
  peticiones remotas/proxeadas del tablero y la API deben presentar el token.

`cc-mobile off` deja de exponerlo.

## Plataformas

| Plataforma | Estado |
|---|---|
| **Linux** (GNOME/X11) | Todo — es el daily driver ✓ |
| **Windows 11** | Vía **WSL2 + WSLg** (app GUI, audio y todo) ✓ |
| **macOS** | Motor + tablero web + **app nativa** (`cc-app`, PyObjC + WKWebView; requiere `pip install pyobjc-framework-Cocoa pyobjc-framework-WebKit` + `brew install tmux jq ttyd`) + notificaciones/voz nativas (`osascript`, `say`, `afplay`); popups pendientes — beta, [se buscan testers](https://github.com/0xAI-Builders/comandos/issues) |

### Setup Windows (WSL2 + WSLg)

1. En PowerShell (como Administrador): `wsl --install -d Ubuntu-24.04` y reinicia cuando te lo pida.
2. Abre Ubuntu desde el menú Inicio y define tu usuario/contraseña Linux.
3. Dentro de Ubuntu: `git clone https://github.com/0xAI-Builders/comandos.git && cd comandos && ./install.sh`
4. Si el instalador te pide habilitar systemd, sigue sus instrucciones: desde PowerShell corre `wsl --shutdown`, reabre Ubuntu y vuelve a correr `./install.sh`.
5. Busca **ComandOS** en el menú Inicio de Windows (WSLg lo publica automáticamente) o abre Edge en `http://127.0.0.1:4777`.

## Piezas

| Pieza | Qué es |
|---|---|
| `bin/cc-dash` | Motor: tablero + acciones sobre tmux/ssh (127.0.0.1:4777) |
| `bin/cc-app` | App nativa: tablero + pestañas de terminal |
| `bin/cc-notifyd` | Demonio de popups accionables |
| `bin/cc-telegram` | Puente Telegram |
| `hooks/cc-notify.sh` | Hook de Claude Code: estado + notificaciones |
| `bin/ccx` | Una sesión tmux por proyecto (`ccx nombre`, `ccx -a codex nombre`) |
| `bin/cc-agents` | Conecta Codex / OpenCode / Gemini / Antigravity |
| `bin/cc-doctor` | Diagnóstico: chequea plataforma, deps, servicios (`--fix` para arreglos batched) |
| `bin/cc-mobile` | Expone el tablero a tu celular por Tailscale (seguro) |
| `bin/cc-webterm` | Terminal web completa e interactiva (ttyd) de tus sesiones |

## Apóyalo

Si te ahorra tiempo, invítame un café:
[Buy Me a Coffee](https://buymeacoffee.com/0xjesus) ·
[GitHub Sponsors](https://github.com/sponsors/0xAI-Builders)

## Licencia

[MIT](./LICENSE)
