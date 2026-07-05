# ComandOS

**Mission control for running many Claude Code agents in parallel — it tells you who needs you.**
Offline voice alerts, actionable popups with full rendered markdown, tabbed terminals,
and Telegram remote control. You answer with one keystroke and move on.

[![license](https://img.shields.io/badge/license-MIT-2EE59D?style=flat-square)](./LICENSE)
[![GitHub Sponsors](https://img.shields.io/badge/sponsor-0xAI--Builders-f38ba8.svg?style=flat-square&logo=github-sponsors)](https://github.com/sponsors/0xAI-Builders)
[![Buy Me a Coffee](https://img.shields.io/badge/buy%20me%20a%20coffee-0xjesus-yellow.svg?style=flat-square&logo=buymeacoffee)](https://buymeacoffee.com/0xjesus)

**[Léeme en español →](./README.es.md)**

![ComandOS — mission control for parallel Claude Code agents](.github/media/dashboard.png)

## Why

When you run 10 Claude Codes at once, the problem isn't terminals — it's **knowing
which one needs you and getting back to it without friction**. ComandOS turns that
into an event-driven flow: the system interrupts you (voice + actionable popup),
you answer with a key or a line, and you keep going. No tab-scanning.

## What you get

- **Live dashboard** (`127.0.0.1:4777`): whatever waits for YOUR answer shows up big,
  with the question's real options as buttons. Respond without switching windows.
- **Actionable popups**: full text with rendered markdown (tables included),
  1/2/3 buttons, inline reply, Copy, and "View ALL". Never answer blind.
- **Native app** (GTK + VTE): one tab per session with a status dot
  (amber = waiting · blue = working · green = done), **Ctrl+K** jumps to any session,
  renameable tabs, splits, and Night/Day/Warm themes.
- **Copy & export** any response: clipboard, `.txt`, or PDF with rendered markdown —
  from the terminal, the popup, or the dashboard.
- **Local voice** (piper, 100% offline) + chime, one global volume that everything respects.
- **SSH manager**: CRUD over `~/.ssh/config`, one-click connect, detects live
  multiplexed tunnels (no-password reconnect).
- **Telegram**: buttons on notifications, reply to answer, `/ls /out /run`.
- Everything is **files** (tmux, JSON, ssh config). No cloud, no DB. Survives reboots.
- UI in **English and Spanish** (auto-detected from `$LANG`, switchable in Settings).

## Install

```bash
git clone https://github.com/0xAI-Builders/comandos.git
cd comandos && ./install.sh
```

Requires Linux with GNOME/X11, `python3-gi`, `tmux`, `jq`.
Recommended: `xclip`, `wmctrl`, `piper` (voice), `kitty`.
Open **ComandOS** from your app menu, or the dashboard at <http://127.0.0.1:4777>.

## Agents

| Agent | Integration | What you get |
|---|---|---|
| **Claude Code** | native hooks (auto via `install.sh`) | working · waiting **with real options** · done · full reply |
| **Codex CLI** | `notify` hook (`cc-agents setup`) | turn done + last message |
| **OpenCode** | plugin (`cc-agents setup`) | working · waiting · errors · done |
| **Gemini CLI** | hooks (`cc-agents setup`) | working · waiting · done · full reply |
| **Antigravity CLI** | hooks (`cc-agents setup`) — experimental | same as Gemini |

One command connects everything you have installed: **`cc-agents setup`**.
Any other agent can join with a single HTTP call:
`POST 127.0.0.1:4777/event {"agent","event","cwd","msg?","full?"}`.

## Platforms

| Platform | Status |
|---|---|
| **Linux** (GNOME/X11) | Everything — this is the daily driver ✓ |
| **Windows 11** | Via **WSL2 + WSLg** (GUI app, audio and all) — beta |
| **macOS** | Engine + web dashboard + native notifications/voice (`osascript`, `say`, `afplay`); GTK app/popups pending — beta, [testers welcome](https://github.com/0xAI-Builders/comandos/issues) |

## Pieces

| Piece | What it is |
|---|---|
| `bin/cc-dash` | Engine: dashboard + tmux/ssh actions (127.0.0.1:4777) |
| `bin/cc-app` | Native app: dashboard + terminal tabs |
| `bin/cc-notifyd` | Actionable popup daemon |
| `bin/cc-telegram` | Telegram bridge |
| `hooks/cc-notify.sh` | Claude Code hook: state + notifications |
| `bin/ccx` | One tmux session per project (`ccx name`, `ccx -a codex name`) |
| `bin/cc-agents` | Connect Codex / OpenCode / Gemini / Antigravity |

## Roadmap

Support for more agents is planned — see
[Codex CLI](https://github.com/0xAI-Builders/comandos/issues/1) and
[Antigravity](https://github.com/0xAI-Builders/comandos/issues/2). PRs welcome.

## Support

If it saves you time, buy me a coffee — it keeps the project alive:
[Buy Me a Coffee](https://buymeacoffee.com/0xjesus) ·
[GitHub Sponsors](https://github.com/sponsors/0xAI-Builders)

## License

[MIT](./LICENSE)
