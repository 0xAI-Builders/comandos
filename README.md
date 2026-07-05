# ComandOS

**Centro de comando para correr muchos Claude Code en paralelo sin perder el hilo.**
El sistema te avisa cuál sesión te necesita (voz + popup accionable), tú respondes
con una tecla — desde el tablero, el popup o Telegram — y sigues.

[![licencia](https://img.shields.io/badge/licencia-PolyForm%20Noncommercial-8B7CFF?style=flat-square)](./LICENSE.md)
[![GitHub Sponsors](https://img.shields.io/badge/sponsor-0xAI--Builders-f38ba8.svg?style=flat-square&logo=github-sponsors)](https://github.com/sponsors/0xAI-Builders)
[![Buy Me a Coffee](https://img.shields.io/badge/buy%20me%20a%20coffee-0xjesus-yellow.svg?style=flat-square&logo=buymeacoffee)](https://buymeacoffee.com/0xjesus)

## Qué hace

- **Tablero** (`127.0.0.1:4777`): lo que espera TU respuesta arriba y grande, con las
  opciones reales de cada pregunta como botones. Responde sin cambiar de ventana.
- **Popups accionables** propios: texto completo con markdown renderizado (tablas
  incluidas), botones 1/2/3, input, Copiar y "Ver TODO". Nada de recortes.
- **App nativa** (GTK + VTE): una pestaña por sesión con punto de estado
  (ámbar espera · azul trabaja · verde listo), Ctrl+K salta a cualquier sesión,
  pestañas renombrables, splits, temas Noche/Día/Cálido.
- **Copia y exporta** cualquier respuesta: portapapeles, `.txt` o PDF con markdown
  renderizado, desde la terminal, el popup o el tablero.
- **Voz local** (piper, 100% offline) y chime, con UN volumen global.
- **Servidores SSH**: CRUD sobre `~/.ssh/config`, conexión de un click, detecta
  túneles multiplexados vivos (reconexión sin password).
- **Telegram**: botones en las notificaciones, responder por reply, `/ls /out /run`.
- Todo es **archivos** (tmux, JSON, ssh config). Sin nube, sin DB. Sobrevive reinicios.

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

Si te ahorra tiempo, invítame un café — mantiene el proyecto vivo:
[Buy Me a Coffee](https://buymeacoffee.com/0xjesus) ·
[GitHub Sponsors](https://github.com/sponsors/0xAI-Builders)

## Licencia

**PolyForm Noncommercial 1.0.0** — úsalo, fórkealo y modifícalo libremente para
fines no comerciales. Uso comercial reservado al titular. Ver [LICENSE](LICENSE.md).
