# ComandOS

**Centro de comando para correr muchos Claude Code en paralelo — te dice quién te necesita.**
Voz local, popups accionables con markdown renderizado, terminales en pestañas y
control remoto desde Telegram. Respondes con una tecla y sigues.

[![licencia](https://img.shields.io/badge/licencia-MIT-2EE59D?style=flat-square)](./LICENSE)
[![GitHub Sponsors](https://img.shields.io/badge/sponsor-0xAI--Builders-f38ba8.svg?style=flat-square&logo=github-sponsors)](https://github.com/sponsors/0xAI-Builders)
[![Buy Me a Coffee](https://img.shields.io/badge/buy%20me%20a%20coffee-0xjesus-yellow.svg?style=flat-square&logo=buymeacoffee)](https://buymeacoffee.com/0xjesus)

**[Read it in English →](./README.md)**

## Por qué

Cuando corres 10 Claude Codes a la vez, el problema no son las terminales: es
**saber cuál te necesita y volver a él sin fricción**. ComandOS lo convierte en un
flujo por eventos: el sistema te interrumpe (voz + popup accionable), respondes con
una tecla o una línea, y sigues. No escaneas pestañas.

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
- **Telegram**: botones en las notificaciones, responder por reply, `/ls /out /run`.
- Todo es **archivos** (tmux, JSON, ssh config). Sin nube, sin DB. Sobrevive reinicios.
- UI en **inglés y español** (auto-detectado por `$LANG`, cambiable en Ajustes).

## Instalar

```bash
git clone https://github.com/0xAI-Builders/comandos.git
cd comandos && ./install.sh
```

Requisitos: Linux con GNOME/X11, `python3-gi`, `tmux`, `jq`.
Recomendados: `xclip`, `wmctrl`, `piper` (voz), `kitty`.
Abre **ComandOS** desde el menú de apps o el tablero en <http://127.0.0.1:4777>.

## Piezas

| Pieza | Qué es |
|---|---|
| `bin/cc-dash` | Motor: tablero + acciones sobre tmux/ssh (127.0.0.1:4777) |
| `bin/cc-app` | App nativa: tablero + pestañas de terminal |
| `bin/cc-notifyd` | Demonio de popups accionables |
| `bin/cc-telegram` | Puente Telegram |
| `hooks/cc-notify.sh` | Hook de Claude Code: estado + notificaciones |
| `bin/ccx` | Una sesión tmux por proyecto (`ccx nombre`) |

## Apóyalo

Si te ahorra tiempo, invítame un café:
[Buy Me a Coffee](https://buymeacoffee.com/0xjesus) ·
[GitHub Sponsors](https://github.com/sponsors/0xAI-Builders)

## Licencia

[MIT](./LICENSE)
