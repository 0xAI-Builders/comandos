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
for f in index.html sw.js manifest.webmanifest icon-192.png icon-512.png; do
  ln -sf "$REPO/dash/$f" "$HOOKS/dash/$f"
done
[ -f "$HOOKS/cc-notify.conf" ] || cp "$REPO/hooks/cc-notify.conf.example" "$HOOKS/cc-notify.conf"
[ -f "$HOOKS/telegram.env" ]   || cp "$REPO/hooks/telegram.env.example"   "$HOOKS/telegram.env"
# Secretos (token de bot, config): solo el dueno (0600)
chmod 600 "$HOOKS/telegram.env" "$HOOKS/cc-notify.conf" 2>/dev/null || true

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

# Servicios: systemd (Linux) o launchd (macOS)
if [ "$(uname -s)" = "Darwin" ]; then
  mkdir -p "$HOME/Library/LaunchAgents"
  cat > "$HOME/Library/LaunchAgents/com.0xai.cc-dash.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>com.0xai.cc-dash</string>
  <key>ProgramArguments</key><array>
    <string>$BIN/cc-dash</string><string>--no-open</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
</dict></plist>
PLIST
  launchctl unload "$HOME/Library/LaunchAgents/com.0xai.cc-dash.plist" 2>/dev/null || true
  launchctl load "$HOME/Library/LaunchAgents/com.0xai.cc-dash.plist" 2>/dev/null || true
  echo "  macOS: cc-dash como LaunchAgent. App nativa y popups GTK: por ahora solo Linux;"
  echo "  usa el tablero en el navegador (las notificaciones van por 'osascript'/'say')."
else
  for s in cc-dash cc-notifyd cc-telegram; do
    ln -sf "$REPO/systemd/$s.service" "$HOME/.config/systemd/user/$s.service"
  done
  systemctl --user daemon-reload
  systemctl --user enable --now cc-dash.service cc-notifyd.service 2>/dev/null || true
  # cc-telegram solo si hay token configurado
  grep -q "^CC_TELEGRAM_BOT_TOKEN=." "$HOOKS/telegram.env" 2>/dev/null \
    && systemctl --user enable --now cc-telegram.service 2>/dev/null || true
fi

# Conectar otros agentes instalados (codex, opencode, gemini, agy)
"$BIN/cc-agents" setup || true

echo ""
echo "Listo. Abre el tablero en http://127.0.0.1:4777 o la app 'ComandOS'."
echo "Agentes conectados: corre 'cc-agents' para ver el estado."
echo "Dependencias sugeridas: tmux jq xclip wmctrl piper (voz) kitty (terminal)."
echo "Para el celular (seguro): tailscale + qrencode, y corre cc-mobile."
echo "Para operar por Telegram: llena ~/.claude/hooks/telegram.env y reinicia cc-telegram."
