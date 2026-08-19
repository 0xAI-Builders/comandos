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
# Los .oga de freedesktop no existen en macOS y afplay tampoco los reproduce:
# sin este branch el chime NUNCA suena en Mac (falla en silencio).
if [ "$(uname -s)" = "Darwin" ]; then
  SOUND_DONE="/System/Library/Sounds/Glass.aiff"
  SOUND_ATTENTION="/System/Library/Sounds/Tink.aiff"
else
  SOUND_DONE="/usr/share/sounds/freedesktop/stereo/message.oga"
  # window-attention: un toquecito discreto (dialog-warning=campanazo y
  # message-new-instant=ding le resultaron molestos al oido con muchos claudes)
  SOUND_ATTENTION="/usr/share/sounds/freedesktop/stereo/window-attention.oga"
fi
DESKTOP_NOTIFY=1
SOUND_ENABLED=1
TELEGRAM_ENABLED=1
NOTIFY_ON_DONE=1
NOTIFY_ON_ATTENTION=1
SPEAK_ATTENTION=1
SPEAK_DONE=1
PIPER_VOICE=es_MX-ald-medium
VOLUME=60
CC_LANG=auto
[ -f "$HOOKS_DIR/cc-notify.conf" ] && . "$HOOKS_DIR/cc-notify.conf"
# Idioma de la UI: CC_LANG=es|en explicito, o auto por $LANG del sistema
case "$CC_LANG" in
  es|en) UI_LANG="$CC_LANG" ;;
  *) case "${LANG:-}" in es*|ES*) UI_LANG=es ;; *) UI_LANG=en ;; esac ;;
esac
if [ "$UI_LANG" = "es" ]; then
  T_ATTN="necesita tu atencion";         T_WAITBODY="Esperando input o permiso"
  T_DONE="termino";                      T_DONEBODY="Iteracion completada, listo para tu siguiente instruccion"
  T_OPTS="Opciones:";                    T_YESALW=$(printf 'Si\x1fSi, siempre\x1fNo')
  T_SPKWAIT="necesita tu respuesta";     T_SPKDONE="terminó";  SPD_LANG=es
else
  T_ATTN="needs your attention";         T_WAITBODY="Waiting for input or permission"
  T_DONE="finished";                     T_DONEBODY="Turn complete — ready for your next instruction"
  T_OPTS="Options:";                     T_YESALW=$(printf 'Yes\x1fYes, always\x1fNo')
  T_SPKWAIT="needs your answer";         T_SPKDONE="finished";  SPD_LANG=en
fi
# Volumen GLOBAL (0-100, editable desde Ajustes del tablero): voz y chime lo respetan
case "$VOLUME" in ''|*[!0-9]*) VOLUME=60;; esac
[ "$VOLUME" -gt 100 ] && VOLUME=100
PAVOL=$(( VOLUME * 655 ))   # escala de paplay: 0..65536

# OJO: pipewire-pulse (medido en 0.3.48) IGNORA el --volume de paplay; el
# stream sale al 100%. pw-play con volumen flotante SI atenua las muestras.
play_snd() { # $1 = archivo
  if command -v pw-play >/dev/null 2>&1; then
    pw-play --volume="$(awk "BEGIN{printf \"%.2f\", $VOLUME/100}")" "$1" >/dev/null 2>&1
  elif command -v paplay >/dev/null 2>&1; then
    paplay --volume="$PAVOL" "$1" >/dev/null 2>&1
  elif command -v afplay >/dev/null 2>&1; then   # macOS
    afplay -v "$(awk "BEGIN{printf \"%.2f\", $VOLUME/100}")" "$1" >/dev/null 2>&1
  fi
}

# ---- Entrada: hook de Claude Code (JSON por stdin) o MODO ADAPTADOR ----
# Cualquier agente (codex, opencode, gemini, agy...) puede disparar el mismo
# pipeline (estado + popup + voz + telegram) con flags:
#   cc-notify.sh --agent codex --event done|waiting|working|end \
#                --cwd DIR [--msg TXT] [--full TXT] [--options "a\x1fb"]
AGENT=claude
full_arg=""; options_arg=""
if [ "${1:-}" = "--agent" ] || [ "${1:-}" = "--event" ]; then
  event=""; cwd=""; msg=""; transcript=""
  while [ $# -gt 0 ]; do
    case "$1" in
      --agent)   AGENT="$2"; shift 2 ;;
      --event)   event="$2"; shift 2 ;;
      --cwd)     cwd="$2"; shift 2 ;;
      --msg)     msg="$2"; shift 2 ;;
      --full)    full_arg="$2"; shift 2 ;;
      --options) options_arg="$2"; shift 2 ;;
      *) shift ;;
    esac
  done
  case "$event" in
    working) event=UserPromptSubmit ;;
    waiting) event=Notification ;;
    end)     event=SessionEnd ;;
    *)       event=Stop ;;
  esac
  input=""
else
  input=$(cat)
  event=$(jq -r '.hook_event_name // "Stop"' <<<"$input" 2>/dev/null)
  cwd=$(jq -r '.cwd // ""' <<<"$input" 2>/dev/null)
  msg=$(jq -r '.message // ""' <<<"$input" 2>/dev/null)
  transcript=$(jq -r '.transcript_path // ""' <<<"$input" 2>/dev/null)
fi
case "$AGENT" in
  claude) AGENT_NAME=Claude ;; codex) AGENT_NAME=Codex ;;
  opencode) AGENT_NAME=OpenCode ;; gemini) AGENT_NAME=Gemini ;;
  agy) AGENT_NAME=Antigravity ;; *) AGENT_NAME="$AGENT" ;;
esac

# La respuesta COMPLETA del turno: TODO el texto de Claude desde el ultimo
# prompt real del usuario (los tool_result son entradas 'user' falsas y se
# ignoran). No solo el ultimo mensaje: tambien los avances intermedios.
turn_text() {
  tail -800 "$transcript" 2>/dev/null | jq -rs '
    def is_prompt: .type == "user"
      and ((.message.content | type) == "string"
           or ([.message.content[]? | select(.type == "tool_result")] | length) == 0);
    . as $a
    | ([range(0; ($a | length)) | select($a[.] | is_prompt)] | max // -1) as $u
    | [$a[($u + 1):][] | select(.type == "assistant")
       | .message.content[]? | select(.type == "text") | .text | select(length > 0)]
    | join("\n\n")' 2>/dev/null
}

proj=$(basename "${cwd:-$PWD}")
proj_file=$(printf '%s' "$proj" | tr -c 'A-Za-z0-9._-' '-' | head -c 80)
now=$(date +%s)
PANE_HINT=""
SESSION_HINT=""
if printf '%s' "${TMUX_PANE:-}" | grep -Eq '^%[0-9]+$' && command -v tmux >/dev/null 2>&1; then
  PANE_HINT="$TMUX_PANE"
  SESSION_HINT=$(tmux display-message -p -t "$PANE_HINT" '#S' 2>/dev/null || true)
fi
if ! printf '%s' "$SESSION_HINT" | grep -Eq '^[A-Za-z0-9._-]{1,80}$'; then
  SESSION_HINT=$(printf '%s' "$proj" | tr '.:' '--' | head -c 60)
fi

write_state() { # $1=status $2=detalle $3=opciones (labels \x1f). LAST=respuesta previa
  jq -n --arg p "$proj" --arg s "$1" --arg d "$2" --arg c "$cwd" --arg o "${3:-}" \
    --arg L "${LAST:-}" --arg a "$AGENT" --arg sess "$SESSION_HINT" --arg pane "$PANE_HINT" \
    --argjson t "$now" \
    '{project:$p,status:$s,detail:$d,cwd:$c,ts:$t,options:$o,last:$L,agent:$a,session:$sess}
     + (if $pane != "" then {pane:$pane} else {} end)' > "$STATE_DIR/$proj_file.json" 2>/dev/null
  # el timeline solo necesita una probadita, no la respuesta entera
  jq -cn --arg p "$proj" --arg s "$1" --arg d "$(printf '%s' "$2" | head -c 280)" --argjson t "$now" \
    '{project:$p,status:$s,detail:$d,ts:$t}' >> "$EVENTS" 2>/dev/null
  # Mantener el timeline acotado
  if [ "$(wc -l < "$EVENTS" 2>/dev/null || echo 0)" -gt 2000 ]; then
    tail -n 500 "$EVENTS" > "$EVENTS.tmp" && mv "$EVENTS.tmp" "$EVENTS"
  fi
  usage_capture "$1" >/dev/null 2>&1 || true
}

usage_num() {
  local v="${!1:-0}"
  case "$v" in ''|*[!0-9.]* ) printf 0 ;; *) printf '%s' "$v" ;; esac
}

usage_capture() {
  local status="${1:-}"
  local script="$HOME/.local/bin/cc_usage.py"
  [ -r "$script" ] || return 0
  local input_tok output_tok cache_read cache_write cost started
  input_tok=$(usage_num COMANDOS_USAGE_INPUT_TOKENS)
  output_tok=$(usage_num COMANDOS_USAGE_OUTPUT_TOKENS)
  cache_read=$(usage_num COMANDOS_USAGE_CACHE_READ_TOKENS)
  cache_write=$(usage_num COMANDOS_USAGE_CACHE_WRITE_TOKENS)
  cost=$(usage_num COMANDOS_USAGE_COST_USD)
  started=$(usage_num COMANDOS_USAGE_TURN_STARTED_AT)
  [ "$started" = "0" ] && started="$now"
  [ "$input_tok$output_tok$cache_read$cache_write$cost" = "00000" ] && return 0
  jq -cn \
    --arg provider "$AGENT" \
    --arg agent "$AGENT" \
    --arg session "$SESSION_HINT" \
    --arg pane "$PANE_HINT" \
    --arg cwd "$cwd" \
    --arg model "${COMANDOS_USAGE_MODEL:-}" \
    --arg effort "${COMANDOS_USAGE_REASONING_EFFORT:-}" \
    --arg source "hook:$status" \
    --argjson input "$input_tok" \
    --argjson output "$output_tok" \
    --argjson cache_read "$cache_read" \
    --argjson cache_write "$cache_write" \
    --argjson cost "$cost" \
    --argjson started "$started" \
    --argjson finished "$now" \
    '{provider:$provider,agent:$agent,tmux_session:$session,tmux_pane:$pane,
      pane_pwd:$cwd,git_root:$cwd,model:$model,reasoning_effort:$effort,
      input_tokens:$input,output_tokens:$output,cache_read_tokens:$cache_read,
      cache_write_tokens:$cache_write,cost_usd:$cost,turn_started_at:$started,
      turn_finished_at:$finished,source:$source,confidence:"exact"}' \
    | python3 "$script" capture-hook >/dev/null 2>&1 &
}

case "$event" in
  UserPromptSubmit)
    # Conservar la ULTIMA respuesta (para Copiar/TXT/PDF mientras trabaja)
    LAST=$(jq -r 'if ((.status=="done" or .status=="waiting") and ((.detail//"")!=""))
                  then .detail else (.last//"") end' \
           "$STATE_DIR/$proj_file.json" 2>/dev/null)
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
    title="🟡 [$proj] $AGENT_NAME $T_ATTN"
    body="${msg:-$T_WAITBODY}"
    options="$options_arg"
    full="$full_arg"
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

$T_OPTS
$optlist"
      else
        raw=$(turn_text)
        preview=$(printf '%s' "$raw" | tr '\n' ' ' | head -c 260)
        [ -n "$preview" ] && body="$body
$preview"
        [ -n "$raw" ] && full=$(printf '%s' "$raw" | head -c 60000)
      fi
    fi
    # Prompt de permiso estandar: etiquetas con texto (mapean a 1/2/3)
    if [ -z "$options" ] && printf '%s' "$msg" | grep -qi "permi"; then
      options="$T_YESALW"
    fi
    urgency="critical"
    sound="$SOUND_ATTENTION"
    write_state "waiting" "$(printf '%s' "${full:-$body}" | head -c 60000)" "$options"
    if [ "$prev_s" = "waiting" ] && [ $(( now - ${prev_t:-0} )) -lt 600 ]; then
      exit 0
    fi
    ;;
  *)
    title="✅ [$proj] $AGENT_NAME $T_DONE"
    # Preview de la ultima respuesta para saber QUE termino sin abrir la terminal
    preview=""
    full="$full_arg"
    [ -n "$full_arg" ] && preview=$(printf '%s' "$full_arg" | tr '\n' ' ' | head -c 180)
    if [ -n "$transcript" ] && [ -f "$transcript" ]; then
      raw=$(turn_text)
      preview=$(printf '%s' "$raw" | tr '\n' ' ' | head -c 180)
      [ -n "$raw" ] && full=$(printf '%s' "$raw" | head -c 60000)
    fi
    body="${preview:-$T_DONEBODY}"
    urgency="normal"
    sound="$SOUND_DONE"
    write_state "done" "$(printf '%s' "${full:-$body}" | head -c 60000)"
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
  sess="$SESSION_HINT"
  payload=$(jq -cn --arg t "$title" --arg b "$body" --arg s "$sess" --arg k "$kind" --arg p "$proj" \
    --arg o "${options:-}" --arg f "${full:-}" \
    '{title:$t,body:$b,session:$s,kind:$k,project:$p,options:$o,full:$f}')
  # Solo popups propios (cc-notifyd). Nada de notificaciones GNOME, nunca.
  if ! curl -s -m 2 -X POST http://127.0.0.1:4778/notify \
      -H 'Content-Type: application/json' -d "$payload" >/dev/null 2>&1; then
    # macOS sin cc-notifyd (GTK): notificacion nativa del sistema
    if command -v osascript >/dev/null 2>&1; then
      osascript -e "display notification \"$(printf '%s' "$body" | head -c 200 | tr '"' "'")\" with title \"$(printf '%s' "$title" | tr '"' "'")\"" >/dev/null 2>&1
    fi
  fi
fi
# Voz local (piper con voz es_MX; fallback spd-say). Si habla, no suena el chime.
speak=""
[ "$kind" = "waiting" ] && [ "$SPEAK_ATTENTION" = "1" ] && speak="$proj $T_SPKWAIT"
[ "$kind" = "done" ] && [ "$SPEAK_DONE" = "1" ] && speak="$proj $T_SPKDONE"
if [ -n "$speak" ]; then
  (
    PV="$HOME/.local/share/piper-voices/${PIPER_VOICE}.onnx"
    if command -v piper >/dev/null 2>&1 && [ -f "$PV" ]; then
      w=$(mktemp --suffix=.wav)
      printf '%s' "$speak" | piper -m "$PV" -f "$w" >/dev/null 2>&1 && play_snd "$w"
      rm -f "$w"
    elif command -v say >/dev/null 2>&1; then      # macOS: voz del sistema
      say "$speak" 2>/dev/null
    elif command -v spd-say >/dev/null 2>&1; then
      spd-say -l "$SPD_LANG" -i $(( VOLUME * 2 - 100 )) "$speak" 2>/dev/null
    fi
  ) &
elif [ "$SOUND_ENABLED" = "1" ]; then
  play_snd "$sound" &
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
    sess="$SESSION_HINT"
    tg_title=$(sed 's/[&<>]/ /g' <<<"$title")
    # Texto COMPLETO con markdown renderizado (HTML de Telegram: negritas,
    # codigo, tablas alineadas en <pre>). Fallback: preview plano.
    if [ -n "${full:-}" ] && [ -x "$HOOKS_DIR/md2tg.py" ]; then
      tg_body=$(printf '%s' "$full" | "$HOOKS_DIR/md2tg.py" 2>/dev/null)
      [ -n "$tg_body" ] && body="$tg_body"
    fi
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
