#!/usr/bin/env bash
# Sistema de notificaciones y estado de Claude Code (global, todos los proyectos).
#
# Eventos que maneja (hooks en ~/.claude/settings.json):
#   UserPromptSubmit -> estado "working"  (silencioso)
#   Stop             -> estado "done"     + notificacion + sonido + preview de la respuesta
#   Notification     -> estado "waiting"  + notificacion critical + sonido de alerta
#   SessionEnd       -> borra el estado del proyecto (silencioso)
#
# Estado por proyecto:  ~/.claude/hooks/state/<proyecto>.json  (lo leen tmux y el dashboard)
# Timeline de eventos:  ~/.claude/hooks/events.jsonl
# Config editable:      ~/.claude/hooks/cc-notify.conf  (sonidos, canales on/off)
# Telegram opcional:    ~/.claude/hooks/telegram.env

HOOKS_DIR="$HOME/.claude/hooks"
STATE_DIR="$HOOKS_DIR/state"
EVENTS="$HOOKS_DIR/events.jsonl"
mkdir -p "$STATE_DIR"

# ---- Config editable (valores por defecto si no existe el archivo) ----
SOUND_DONE="/usr/share/sounds/freedesktop/stereo/complete.oga"
# window-attention: un toquecito discreto (dialog-warning=campanazo y
# message-new-instant=ding le resultaron molestos al oido con muchos claudes)
SOUND_ATTENTION="/usr/share/sounds/freedesktop/stereo/window-attention.oga"
DESKTOP_NOTIFY=1
SOUND_ENABLED=1
TELEGRAM_ENABLED=1
NOTIFY_ON_DONE=1
NOTIFY_ON_ATTENTION=1
SPEAK_ATTENTION=1
SPEAK_DONE=1
PIPER_VOICE=es_MX-ald-medium
VOLUME=60
[ -f "$HOOKS_DIR/cc-notify.conf" ] && . "$HOOKS_DIR/cc-notify.conf"
# Volumen GLOBAL (0-100, editable desde Ajustes del tablero): voz y chime lo respetan
case "$VOLUME" in ''|*[!0-9]*) VOLUME=60;; esac
[ "$VOLUME" -gt 100 ] && VOLUME=100
PAVOL=$(( VOLUME * 655 ))   # escala de paplay: 0..65536

input=$(cat)
event=$(jq -r '.hook_event_name // "Stop"' <<<"$input" 2>/dev/null)
cwd=$(jq -r '.cwd // ""' <<<"$input" 2>/dev/null)
msg=$(jq -r '.message // ""' <<<"$input" 2>/dev/null)
transcript=$(jq -r '.transcript_path // ""' <<<"$input" 2>/dev/null)
proj=$(basename "${cwd:-$PWD}")
proj_file=$(printf '%s' "$proj" | tr -c 'A-Za-z0-9._-' '-' | head -c 80)
now=$(date +%s)

write_state() { # $1=status $2=detalle
  jq -n --arg p "$proj" --arg s "$1" --arg d "$2" --arg c "$cwd" --argjson t "$now" \
    '{project:$p,status:$s,detail:$d,cwd:$c,ts:$t}' > "$STATE_DIR/$proj_file.json" 2>/dev/null
  jq -cn --arg p "$proj" --arg s "$1" --arg d "$2" --argjson t "$now" \
    '{project:$p,status:$s,detail:$d,ts:$t}' >> "$EVENTS" 2>/dev/null
  # Mantener el timeline acotado
  if [ "$(wc -l < "$EVENTS" 2>/dev/null || echo 0)" -gt 2000 ]; then
    tail -n 500 "$EVENTS" > "$EVENTS.tmp" && mv "$EVENTS.tmp" "$EVENTS"
  fi
}

case "$event" in
  UserPromptSubmit)
    write_state "working" ""
    exit 0
    ;;
  SessionEnd)
    rm -f "$STATE_DIR/$proj_file.json"
    exit 0
    ;;
  Notification)
    # Anti-spam: si ya estaba "waiting" hace poco, refresca estado pero no re-notifica
    prev=$(jq -r '[.status,(.ts|tostring)]|join(" ")' "$STATE_DIR/$proj_file.json" 2>/dev/null)
    prev_s=${prev%% *}; prev_t=${prev##* }
    title="🟡 [$proj] Claude necesita tu atencion"
    body="${msg:-Esperando input o permiso}"
    options=""
    full=""
    if [ -n "$transcript" ] && [ -f "$transcript" ]; then
      # Ultimo bloque del ultimo mensaje del asistente: si es una pregunta con
      # opciones (AskUserQuestion), usar la pregunta y los TEXTOS de las opciones
      lastblock=$(tail -80 "$transcript" 2>/dev/null \
        | jq -cs '[.[] | select(.type=="assistant")] | last | .message.content | last // empty' 2>/dev/null)
      if [ -n "$lastblock" ] && [ "$(jq -r '.name // ""' <<<"$lastblock")" = "AskUserQuestion" ]; then
        qfull=$(jq -r '.input.questions[0].question // ""' <<<"$lastblock")
        qtext=$(printf '%s' "$qfull" | head -c 260)
        options=$(jq -r '[.input.questions[0].options[]?.label] | join("")' <<<"$lastblock" 2>/dev/null)
        [ -n "$qtext" ] && body="$qtext"
        # Texto COMPLETO para "Ver TODO": pregunta entera + opciones numeradas
        # con su descripcion (para responder 1/2/3 con confianza)
        optlist=$(jq -r '[.input.questions[0].options[]?] | to_entries
          | map("\(.key+1). \(.value.label)"
                + (if (.value.description // "") != "" then "\n   " + .value.description else "" end))
          | join("\n")' <<<"$lastblock" 2>/dev/null)
        full="$qfull"
        [ -n "$optlist" ] && full="$qfull

Opciones:
$optlist"
      else
        # Ultimo mensaje de Claude: preview corto para el popup colapsado y
        # texto COMPLETO (con saltos de linea) para "Ver TODO"
        raw=$(tail -80 "$transcript" 2>/dev/null \
          | jq -rs '[.[] | select(.type=="assistant") | .message.content[]? | select(.type=="text") | .text] | last // ""' 2>/dev/null)
        preview=$(printf '%s' "$raw" | tr '\n' ' ' | head -c 260)
        [ -n "$preview" ] && body="$body
$preview"
        [ -n "$raw" ] && full=$(printf '%s' "$raw" | head -c 6000)
      fi
    fi
    # Prompt de permiso estandar: etiquetas con texto (mapean a 1/2/3)
    if [ -z "$options" ] && printf '%s' "$msg" | grep -qi "permi"; then
      options=$(printf 'Si\x1fSi, siempre\x1fNo')
    fi
    urgency="critical"
    sound="$SOUND_ATTENTION"
    write_state "waiting" "$(printf '%s' "${full:-$body}" | head -c 4000)"
    if [ "$prev_s" = "waiting" ] && [ $(( now - ${prev_t:-0} )) -lt 600 ]; then
      exit 0
    fi
    ;;
  *)
    title="✅ [$proj] Claude termino"
    # Preview de la ultima respuesta para saber QUE termino sin abrir la terminal
    preview=""
    full=""
    if [ -n "$transcript" ] && [ -f "$transcript" ]; then
      raw=$(tail -80 "$transcript" 2>/dev/null \
        | jq -rs '[.[] | select(.type=="assistant") | .message.content[]? | select(.type=="text") | .text] | last // ""' 2>/dev/null)
      preview=$(printf '%s' "$raw" | tr '\n' ' ' | head -c 180)
      [ -n "$raw" ] && full=$(printf '%s' "$raw" | head -c 6000)
    fi
    body="${preview:-Iteracion completada, listo para tu siguiente instruccion}"
    urgency="normal"
    sound="$SOUND_DONE"
    write_state "done" "$(printf '%s' "${full:-$body}" | head -c 4000)"
    ;;
esac

# El preview corto (popup colapsado, telegram) va SIN markdown ni markup
# peligroso. El texto COMPLETO ($full) viaja CRUDO: el popup lo renderiza con
# Pango y el tablero con su mini-markdown (cada uno escapa lo suyo).
mdclean() { sed -e 's/\*\*//g' -e 's/`//g' -e 's/^#\{1,6\} //g' \
  -e 's/\[\([^]]*\)\]([^)]*)/\1/g' -e 's/[&<>]/ /g'; }
body=$(mdclean <<<"$body")

# Filtro por tipo de evento (editable en cc-notify.conf)
kind="done"; [ "$event" = "Notification" ] && kind="waiting"
if [ "$kind" = "done" ] && [ "$NOTIFY_ON_DONE" != "1" ]; then exit 0; fi
if [ "$kind" = "waiting" ] && [ "$NOTIFY_ON_ATTENTION" != "1" ]; then exit 0; fi

if [ "$DESKTOP_NOTIFY" = "1" ]; then
  # Notificacion nativa ACCIONABLE via cc-notifyd (botones 1/2/3/Enter/Esc y Abrir);
  # si el demonio no responde, cae a notify-send plano.
  sess=$(printf '%s' "$proj" | tr '.:' '--' | head -c 60)
  payload=$(jq -cn --arg t "$title" --arg b "$body" --arg s "$sess" --arg k "$kind" --arg p "$proj" \
    --arg o "${options:-}" --arg f "${full:-}" \
    '{title:$t,body:$b,session:$s,kind:$k,project:$p,options:$o,full:$f}')
  # Solo popups propios (cc-notifyd). Nada de notificaciones GNOME, nunca.
  curl -s -m 2 -X POST http://127.0.0.1:4778/notify \
    -H 'Content-Type: application/json' -d "$payload" >/dev/null 2>&1
fi
# Voz local (piper con voz es_MX; fallback spd-say). Si habla, no suena el chime.
speak=""
[ "$kind" = "waiting" ] && [ "$SPEAK_ATTENTION" = "1" ] && speak="$proj necesita tu respuesta"
[ "$kind" = "done" ] && [ "$SPEAK_DONE" = "1" ] && speak="$proj terminó"
if [ -n "$speak" ]; then
  (
    PV="$HOME/.local/share/piper-voices/${PIPER_VOICE}.onnx"
    if command -v piper >/dev/null 2>&1 && [ -f "$PV" ]; then
      w=$(mktemp --suffix=.wav)
      printf '%s' "$speak" | piper -m "$PV" -f "$w" >/dev/null 2>&1 && paplay --volume="$PAVOL" "$w" 2>/dev/null
      rm -f "$w"
    elif command -v spd-say >/dev/null 2>&1; then
      spd-say -l es -i $(( VOLUME * 2 - 100 )) "$speak" 2>/dev/null
    fi
  ) &
elif [ "$SOUND_ENABLED" = "1" ]; then
  paplay --volume="$PAVOL" "$sound" >/dev/null 2>&1 &
fi

# Telegram opcional. Con CC_TELEGRAM_BOT_TOKEN (bot dedicado) las notificaciones
# llevan botones accionables y puedes responderles (reply) para operar la sesion
# (requiere el servicio cc-telegram corriendo). Sin el, texto plano con el bot normal.
tg="$HOOKS_DIR/telegram.env"
if [ "$TELEGRAM_ENABLED" = "1" ] && [ -f "$tg" ]; then
  # shellcheck disable=SC1090
  . "$tg"
  TG_TOKEN="${CC_TELEGRAM_BOT_TOKEN:-${TELEGRAM_BOT_TOKEN:-}}"
  if [ -n "$TG_TOKEN" ] && [ -n "${TELEGRAM_CHAT_ID:-}" ]; then
    sess=$(printf '%s' "$proj" | tr '.:' '--' | head -c 60)
    tg_title=$(sed 's/[&<>]/ /g' <<<"$title")
    if [ -n "${CC_TELEGRAM_BOT_TOKEN:-}" ] && [ "$event" = "Notification" ]; then
      kb='{"inline_keyboard":[[{"text":"1","callback_data":"k|'"$sess"'|1"},{"text":"2","callback_data":"k|'"$sess"'|2"},{"text":"3","callback_data":"k|'"$sess"'|3"}],[{"text":"Enter","callback_data":"k|'"$sess"'|Enter"},{"text":"Esc","callback_data":"k|'"$sess"'|Escape"}]]}'
      curl -s -m 5 "https://api.telegram.org/bot${TG_TOKEN}/sendMessage" \
        --data-urlencode chat_id="${TELEGRAM_CHAT_ID}" \
        --data-urlencode parse_mode="HTML" \
        --data-urlencode text="<b>${tg_title}</b>
$body" \
        --data-urlencode reply_markup="$kb" >/dev/null 2>&1 &
    else
      curl -s -m 5 "https://api.telegram.org/bot${TG_TOKEN}/sendMessage" \
        --data-urlencode chat_id="${TELEGRAM_CHAT_ID}" \
        --data-urlencode parse_mode="HTML" \
        --data-urlencode text="<b>${tg_title}</b>
$body" >/dev/null 2>&1 &
    fi
  fi
fi

exit 0
