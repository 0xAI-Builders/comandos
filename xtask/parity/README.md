# xtask parity

Compara `bin/cc-dash` (oráculo Python) con `comandos dash` (frente Rust) sobre copias
privadas de `~/.claude/hooks`.

```
cargo build -p comandos-cli
cargo run -p xtask -- parity --fixture xtask/parity/frente.jsonl --hooks ~/.claude/hooks [--keep] [--comandos RUTA]
```

El arnés copia `--hooks` (`cp -a`, symlinks intactos) a dos HOME temporales bajo
`std::env::temp_dir()`; el original es de solo lectura. Aborta antes de lanzar nada si
`--hooks` no es un directorio con `state/` o si un HOME hijo no cuelga de `temp_dir()`.
Ambos servidores corren con `TMUX_TMPDIR` privado, sin `$TMUX`, puertos libres elegidos por
el SO y el mismo directorio estático (symlinks a `<repo>/dash` más `assets`). El frente usa
un `--legacy-port` muerto: una ruta reenviada responde 502 (o 404 mientras el reenvío no
exista) y se marca `SKIP (reenviada)`. Nunca toca 4777/4778/4781.

Salida: tabla `OK`/`DIFF`/`SKIP` por petición y resumen; código 1 si hay `DIFF`. Los pares
que difieren quedan en `<tmp>/comandos-parity-<t>/parity-<t>/<n>.{py,rs}`.

## Fixture JSONL (una petición por línea)

| Campo | Significado |
|---|---|
| `name`, `method`, `path` | identificación y petición |
| `headers` | objeto; `{{token}}` se sustituye por el `dash-token` de la copia |
| `body` | `null`, texto literal o JSON (se serializa) |
| `volatile` | punteros JSON (con `*` para índices) sustituidos por `"<volátil>"` en ambos lados |
| `expect` | `same` (estado + `content-type`/`content-length`/`cache-control` + cuerpo), `static-accepted` (estado + cuerpo), `status-only` (solo estado), `skip` |
| `forwarded` | `true` si el frente la reenvía: con el heredado muerto, 502/404 propio del frente → `SKIP` |
| `reason` | texto para `skip` |

Los cuerpos JSON se comparan semánticamente (el espaciado de `json.dumps` no cuenta); los
demás, byte a byte.
