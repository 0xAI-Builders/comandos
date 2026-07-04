#!/usr/bin/env bash
# Instalador de ComandOS. Enlaza los binarios y hooks a su lugar, registra los
# servicios systemd de usuario y la app de escritorio. Idempotente.
set -euo pipefail
REPO="$(cd "$(dirname "$0")" && pwd)"
BIN="$HOME/.local/bin"
HOOKS="$HOME/.claude/hooks"

echo "ComandOS -> instalando desde $REPO"
mkdir -p "$BIN" "$HOOKS/dash" "$HOOKS/state" \
         "$HOME/.config/systemd/user" \
         "$HOME/.config/kitty" \
         "$HOME/.local/share/applications" \
         "$HOME/.local/share/icons/hicolor/scalable/apps"

# Binarios (symlink: los updates del repo se reflejan solos)
for f in "$REPO"/bin/*; do ln -sf "$f" "$BIN/$(basename "$f")"; done
chmod +x "$REPO"/bin/* "$REPO"/hooks/cc-notify.sh "$REPO"/hooks/cc-status.sh

# Hooks + dashboard
for f in cc-notify.sh cc-status.sh md2tg.py; do ln -sf "$REPO/hooks/$f" "$HOOKS/$f"; done
ln -sf "$REPO/dash/index.html" "$HOOKS/dash/index.html"
[ -f "$HOOKS/cc-notify.conf" ] || cp "$REPO/hooks/cc-notify.conf.example" "$HOOKS/cc-notify.conf"
[ -f "$HOOKS/telegram.env" ]   || cp "$REPO/hooks/telegram.env.example"   "$HOOKS/telegram.env"

# Config de terminal
ln -sf "$REPO/config/kitty.conf" "$HOME/.config/kitty/kitty.conf"
if [ ! -e "$HOME/.tmux.conf" ]; then
  ln -sf "$REPO/config/tmux.conf" "$HOME/.tmux.conf"
else
  echo "  (ya tienes ~/.tmux.conf; revisa config/tmux.conf y mezcla a mano)"
fi

# App de escritorio (Exec/Icon apuntando a este usuario)
sed "s|__HOME__|$HOME|g" "$REPO/dash/comandos.desktop.in" \
  > "$HOME/.local/share/applications/comandos.desktop" 2>/dev/null || true
cp "$REPO/dash/comandos.svg" "$HOME/.local/share/icons/hicolor/scalable/apps/centro-claude.svg" 2>/dev/null || true

# Servicios systemd de usuario
for s in cc-dash cc-notifyd cc-telegram; do
  ln -sf "$REPO/systemd/$s.service" "$HOME/.config/systemd/user/$s.service"
done
systemctl --user daemon-reload
systemctl --user enable --now cc-dash.service cc-notifyd.service 2>/dev/null || true
# cc-telegram solo si hay token configurado
grep -q "^CC_TELEGRAM_BOT_TOKEN=." "$HOOKS/telegram.env" 2>/dev/null \
  && systemctl --user enable --now cc-telegram.service 2>/dev/null || true

echo ""
echo "Listo. Abre el tablero en http://127.0.0.1:4777 o la app 'ComandOS'."
echo "Dependencias sugeridas: tmux jq xclip wmctrl piper (voz) kitty (terminal)."
echo "Para operar por Telegram: llena ~/.claude/hooks/telegram.env y reinicia cc-telegram."
