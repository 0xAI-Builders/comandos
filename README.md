# ComandOS

**Comando central para trabajar muchos proyectos de Claude Code a la vez** —
sin perder el hilo, con notificaciones que te hablan, terminales en pestañas,
gestor de servidores SSH y control remoto desde Telegram.

> Un solo lugar para ver qué sesión te necesita, responderle sin cambiar de
> ventana, saltar a su terminal, y operar tus servidores. Sobrevive reinicios.

![licencia](https://img.shields.io/badge/licencia-PolyForm%20Noncommercial-8B7CFF)

---

## Por qué existe

Cuando corres 10 Claude Code en paralelo, el problema no es la terminal: es
**saber cuál te necesita y volver a él sin fricción**. ComandOS convierte eso en
un flujo por eventos: el sistema te avisa (voz + popup accionable), tú respondes
con una tecla o una línea, y sigues. No escaneas — te interrumpe solo lo urgente.

## Qué hace

- **Tablero** con jerarquía: lo que espera tu respuesta arriba y grande; el resto
  en una lista compacta y escaneable. Buscador y favoritos.
- **Popups accionables** de escritorio (diseño propio, no los del sistema): botones
  con el texto real de cada opción, input para responder, y "abrir".
- **Voz local** (piper, 100% offline): "LifeOS necesita tu respuesta".
- **App nativa** (GTK + VTE) con **pestañas de terminal**: cada proyecto/servidor
  en su pestaña, splits en las 4 direcciones por click derecho, clipboard
  compartido, se restauran al reabrir.
- **Gestor de servidores SSH**: CRUD sobre `~/.ssh/config`, conectar de un click,
  multiplexing (un password y ya), estado de conexión honesto.
- **Telegram**: opera todo desde el celular — botones en las notificaciones,
  responder por reply, `/ls /out /run /servers`. Restringible a un solo usuario.
- **Atajos globales** (Super+N a lo más urgente) y panel de ayuda (`?`).

Todo es **archivos** (ssh config, JSON, conf, tmux) — sin base de datos, sin nube.

## Instalación

Requisitos: Linux con GNOME/X11, `python3-gi`, `tmux`, `jq`. Opcionales pero
recomendados: `xclip` (clipboard), `wmctrl` (traer ventanas al frente),
`piper` + una voz (avisos hablados), `kitty` (terminal externa).

```bash
git clone https://github.com/0xJesus/comandos.git
cd comandos
./install.sh
```

Abre **ComandOS** desde el menú de apps, o el tablero en <http://127.0.0.1:4777>.

## Componentes

| Pieza | Qué es |
|---|---|
| `bin/cc-dash` | Motor: sirve el tablero y ejecuta acciones sobre tmux/ssh (127.0.0.1:4777) |
| `bin/cc-app` | App nativa GTK: tablero + pestañas de terminal |
| `bin/cc-telegram` | Puente Telegram (botones y comandos) |
| `hooks/cc-notify.sh` | Hook de Claude Code: estado + notificaciones |
| `hooks/cc-notifyd` | Demonio de popups propios accionables |
| `bin/ccx` | Una sesión tmux por proyecto (`ccx nombre`) |
| `bin/cc-keys` | Instala tu llave en los servidores (adiós passwords) |

Los hooks se conectan en `~/.claude/settings.json` (eventos `Stop`,
`Notification`, `UserPromptSubmit`, `SessionEnd`).

## Apóyalo

Si te sirve, invítame un café — mantiene el proyecto vivo:

- GitHub Sponsors: <https://github.com/sponsors/0xJesus>
- Buy Me a Coffee: *(próximamente)*

## Licencia

**PolyForm Noncommercial 1.0.0** — código a la vista (*source-available*).
Puedes usar, forkear y modificar libremente para fines **no comerciales**.
El uso comercial está reservado al titular del copyright. Ver [LICENSE](LICENSE.md).

> Nota: es *source-available*, no "open source" según la OSI (que no permite
> restringir el uso comercial). La diferencia es intencional.
