# Tarea 7b de la 2e — valores de borde, filas vivas y avisos de nivel por vuelta (latente)

Estado: DONE. Commit `c1fafc6` sobre `6b05d9f` (rama `migration/rust-fase2e`). Ninguna ruta
nueva: la Tarea 8 lo engancha.

## Qué se hizo (lo que faltaba tras la 7a)

- `usage/pane_models.rs`:
  - `live_rows(live_panes, state, cards: Option<&[Value]>) -> Option<Vec<Row>>`
    (`_pane_models_for_live_state` 286): ids vivos que empiezan por `%`; filas del memo con pane
    vivo; tarjeta con `model` verdadero → `model`, `agent = card.agent or row.agent` (ausente en
    las dos → `None` como el Python), `provider = agent`, `reasoning_effort` si hay `effort`.
    Tarjetas que el `except Exception` del Python vaciaría (una que no es objeto, un `pane`
    lista/objeto) → mapa vacío; la última tarjeta de cada pane gana. `cards = None` (el
    `/state` declinó) → `None`: esa vuelta no escribe bordes (D1).
  - `pane_values(&Native, rows, tmux_panes: Option<&BTreeSet<String>>) -> PaneOutcome`
    (`_pane_model_values` 4481) con `PaneOutcome::{Ready(PaneValues { values, file_text,
    alerts }), Skip, Raises(Vec<TierAlert>)}`. Un solo `spawn_blocking` por vuelta:
    `motor-results.json` con `gather::motor_results` (incierto → `Skip`),
    `catalogs::read_model_tiers` (ausente/incierto → `Skip`), `account_for_pid` con una
    `AccountCache` propia (`Native.pane_accounts`, nunca la del escaneo de `/state`). Las cuatro
    formas del valor (cambiando, modelo con color/símbolo del nivel, detectando, solo cabeza) y
    `None`; `PANE_MODEL_COLORS` con `244` por omisión; `file_text` ordenado por pane con `\n`
    final; una línea anterior del mismo pane se queda si luego el valor es `None` (como el
    `dict` del Python). Las observaciones de `_maybe_tier_alert` se calculan en el salto de
    bloqueo y se aplican después bajo el candado de `TierAlerts` (sin `await`), en el orden de
    los panes; al final `prune(tmux_panes, now)` solo si la vuelta terminó y la lista de tmux
    se conoce.
  - Qué lanza y qué es incierto, en el orden del Python: `tmux_pane` o `model` verdaderos que
    no son texto, agente lista/objeto (`AGENT_ACCOUNT_ENV.get`), `float(ts)` imposible de un
    cambio en curso y `tiers` verdadero que no es objeto (`tier_style`) → `Raises`, con los
    avisos ya decididos en los panes anteriores (el Python ya lanzó sus hilos). `repr` de un
    contenedor, sesión que no es texto con nivel, pid que no es entero con cuenta posible,
    patrón incierto de `model_tier`, correo incierto → `Skip` sin observar nada.
  - `PyFault { Raises, Unsure }`; `tier_style` devuelve `Raises` para `tiers` no objeto.
- Menores de la revisión de la 7a:
  1. `apply` sostiene el candado del archivo hasta haber anotado `desired` (el
     `_pane_model_lock` único del Python).
  2. `symbol`/`label` verdaderos que no son texto pasan por el `str()` de Python (`py_str`:
     `3` → `"3"`, `true` → `"True"`); un contenedor (su `repr`) → `Unsure`, no se avisa
     (diferencia aceptada: el Python mostraría el `repr`).
  3. La poda recibe todos los panes de tmux: `UsageStateReply.tmux_panes` (los ids de
     `list-panes -a` del paso 2 de `compute`; `None` si `list-panes` falló → no se poda).
- `usage/state.rs`: `UsageStateReply.tmux_panes` y un `struct Live` para el paso 2 (sin cambio
  de comportamiento).
- `native/mod.rs`: `Native.pane_accounts: Arc<Mutex<AccountCache>>`.

## Para la Tarea 8 (cambia la forma que daba su brief)

```rust
let reply = state::compute(native).await?;
let cards = native.states_cached().await.ok();
if native.options().usage_effects
    && let Some(rows) = pane_models::live_rows(&reply.live_panes, &reply.state,
                                               cards.as_ref().map(|c| c.items.as_slice()))
{
    match pane_models::pane_values(native, &rows, reply.tmux_panes.as_ref()).await {
        PaneOutcome::Ready(v) => { apply(v.values, v.file_text, …); spawn(send_alerts(v.alerts)) }
        PaneOutcome::Skip => {}
        PaneOutcome::Raises(alerts) => { spawn(send_alerts(alerts)); return 500 }
    }
}
```

`Raises` es el 500 del Python (`write_pane_models` corre antes de responder y no tiene `try`).
Si la Tarea 8 prefiere no dar ese 500, que lo anote como diferencia aceptada.

## Pruebas (`tests/dash_native_pane_models.rs`, 16; +5 nuevas)

- `usage_state_borders_match_python_twin`: el `/usage/state` del Python
  (`write_pane_models(_pane_models_for_live_state(panes, state))`, hilos síncronos,
  `read_states_cached` y `usage_alert_send` sustituidos) y `live_rows` → `pane_values` →
  `apply` en Rust, seis vueltas, cada lado con su HOME y un **tmux falso** que anota cada argv:
  argv de tmux idéntico vuelta a vuelta (descubrimiento, `set-option` y `-u`, pane muerto,
  reintento), filas, `applied`, `pane-models.txt`, reintento, avisos (cambio a `high`, enfriamiento
  de una hora y aviso tras ella) y número de claves recordadas. Cuentas reales: tres `sleep`
  propios con `CLAUDE_CONFIG_DIR`, sin `CODEX_HOME` (→ `main`) y `GROK_HOME` con barra final.
  Comprobado que detecta una divergencia (color de «cambiando» alterado → falla en la vuelta 0).
- `live_rows_match_python`: seis casos con tarjetas raras contra `_pane_models_for_live_state`.
- `pane_values_raise_like_python`: ocho casos por rondas (`ok` con valores y texto, o
  excepción, y los avisos de cada ronda, también los previos a una excepción; agente numérico,
  `provider`, `alertTier` no texto).
- `pane_values_skip_when_inputs_are_uncertain` y
  `pane_values_bound_tier_memory_with_all_tmux_panes` (Rust).
- `dash_native_usage_state`: `tmux_panes` contiene el pane vivo; `None` si `list-panes` falla.

`cargo fmt --check` y `cargo clippy --workspace --all-targets -j 4 -- -D warnings` limpios.
Suite entera (`cargo test --workspace -j 4 --no-fail-fast`, `TMPDIR=/tmp/c22`, sin `DISPLAY`,
`WAYLAND_DISPLAY`, `DBUS_SESSION_BUS_ADDRESS` ni `TMUX`): **988 pasan, 0 fallan, 1 ignorada**
(983 + 5). En una corrida anterior de `-p comandos-server` falló una vez `dash_native_snippets`
(no tocado; pasó en las cuatro corridas siguientes): parece intermitente bajo carga.
`cargo xtask parity --fixture xtask/parity/frente.jsonl --hooks ~/.claude/hooks --state-db
~/.local/state/comandos/app-state.sqlite3 --usage-db ~/.claude/hooks/comandos-usage.sqlite`
(copias de solo lectura, netns, `comandos-cli` recompilado): **162 OK, 0 DIFF, 0 SKIP**.

## Desviaciones

1. **`pane_values` devuelve `PaneOutcome`, no `Option<(…)>`**, y recibe `tmux_panes`: hacen
   falta para el 500 del Python y para la poda con todos los panes (menor 3).
2. **`live_rows` devuelve `Option`**: `None` sin tarjetas (el brief lo describía en prosa).
3. **`Skip` no observa avisos.** Lo incierto en mitad de una vuelta deja `TierAlerts` como
   estaba: la vuelta siguiente (≤ 10 s) observa. Cambiar de nivel y volver dentro de una vuelta
   saltada no avisaría.
4. **`cc-notify.conf` ilegible al avisar** sigue en `send_alerts` (stderr, sin aviso), como en
   la 7a: en el Python sería un 500, pero `compute` ya da 500 antes con ese archivo
   (`cc_lang()?`), así que solo queda la carrera de cambiarlo entre medias.
5. **`AccountCache` de los bordes** se vacía a las 4096 entradas (la de la 2d); la regla 6 habla
   de 64 correos. Es la misma estructura que usa `/state`; no se cambió.

## Seguridad (tmux, avisos, servicios vivos)

- **tmux**: ningún `tmux` a mano. Las pruebas nuevas no arrancan ningún servidor: usan un
  `tmux` falso (guion `/bin/sh` escrito por la prueba en el HOME temporal, no versionado) que
  solo anota argv e imprime archivos; canario: el programa está dentro del HOME de la prueba y
  sin prefijo. En el oráculo Python ese mismo falso está primero en el `PATH`. Las pruebas de la
  7a con tmux real siguen con `Tmux::private` (`-S` al socket del HOME). Todo con
  `TMPDIR=/tmp/c22`, sin `TMUX`. `pgrep -a tmux` antes y después: solo las sesiones del usuario;
  ningún servidor ni socket propio bajo `/tmp/c22` (borrado al final), ningún `sleep 120` vivo.
- **Procesos**: los tres `sleep 120` de la prueba gemela se matan por su manejador
  (`Child::kill` + `wait`), nunca por patrón.
- **Avisos**: ninguno real. `usage_alert_send` sustituido en el oráculo (y `urlopen` desde el
  preludio de la 7a); en Rust las pruebas nuevas no llaman a `send_alerts` (las de la 7a usan
  `FakeNotify`). Sin `notify-send`, sonido ni ventanas; `COMANDOS_GTK_TESTS` sin fijar; sin red.
- **Producción**: nada llama a `live_rows`, `pane_values` ni `apply`; sin rutas nuevas. El único
  cambio en caliente es `UsageStateReply.tmux_panes` dentro de `compute`, que sigue sin ruta.
- **Archivos vivos**: ninguna lectura ni escritura en `~/.claude*`, `~/.local/state`,
  `~/.local/share/comandos`, systemd ni los puertos 4777–4782 (salvo las copias de solo lectura
  que hace `xtask parity`).
