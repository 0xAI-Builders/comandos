# Cutover de `cc-extensions` a Rust — 4 de octubre de 2026

`~/.local/bin/cc-extensions` apunta desde las 02:35 CST al binario Rust
`~/.local/share/comandos/bin/comandos` (commit `8871ca6` de `migration/rust-full`).
Las sesiones que ya estaban abiertas siguen con sus proxies Python (no se tocó ningún
proceso); toda sesión nueva arranca sus MCP con `comandos ext serve <nombre>`.

## Paridad en sombra antes de cambiar el enlace (catálogo real, HOME real solo lectura)

| Prueba | Resultado |
|---|---|
| 13 servidores HTTP reales (`initialize` → `tools/list` → `ping`) | 13/13 byte a byte iguales a Python; ids 0, 1, 2; rc 0/0 (mobbin 18 968 B … higgsfield 806 073 B) |
| 21 servidores stdio (argv, cwd y entorno del `exec`) | 21/21 idénticos (18 con fakes en PATH; claude-in-chrome, deepsearch y higgsfield-use-after-effects con catálogo temporal) |
| `status` | 6 512 bytes idénticos |
| `sync` sobre dos copias de 27 GB del HOME | configs de 4 harnesses + 3 cuentas idénticas; 678 enlaces de skills idénticos; `snapshot.json` 331 huellas = fórmula Python; stdout idéntico tras Task 7a |
| `sync` sobre HOME temporal con el catálogo real | stdout y árbol idénticos (35 entradas) |
| x-suite (node, 347 548 B) y teams (npx, 29 295 B) en vivo | idénticos byte a byte |

Detalle de las series HTTP/stdio: `docs/verification/serve-parity.md`.

## Pasos ejecutados

1. `comandos install --stage` → `~/.local/share/comandos/bin/comandos` (12 030 744 B, `comandos 0.1.0`).
2. Prueba del binario instalado contra mobbin real: idéntico al Python.
3. `comandos install --link cc-extensions` → `~/.local/bin/cc-extensions -> ~/.local/share/comandos/bin/comandos`;
   registro de reversión `~/.local/share/comandos/rollback/cc-extensions.target` =
   `LINK:/home/someguy/codebase/0xJesus/ComandOS/bin/cc-extensions`.
4. Primer tick de `comandos-extensions-sync.timer` con Rust: `success`, 
   `{"configurations_changed": 0, "skill_links_changed": 0}`, `~/.claude.json` y el catálogo sin reescribir.
5. Sesión de prueba `claude --debug -p 'Responde solo: hola' --max-turns 1`: respondió `hola`;
   31/34 MCP «Successfully connected» por el proxy Rust (whatsapp 18 ms, telegram 16 ms,
   threads0x 23 ms, supabase 1 052 ms…). Los 3 restantes no son del proxy: `teams` (npx) y
   `higgsfield-use-after-effects` (ssh a macmini) seguían arrancando cuando `-p` cerró la sesión;
   `x-suite` recibe el sondeo de versión de Claude Code («rmcp-class pre-init hard close») que
   es comportamiento del propio servidor node, al que ambos proxies hacen `exec` con el mismo argv.

## Reglas de oro, evidencia

- Proxies Python vivos antes/después del enlace: 317/317; 22 sesiones tmux intactas; ningún
  `cc-app`, `cc-dash` ni `tmux` reiniciado.
- Enlaces no tocados: `cc-dash`, `cc-app`, `~/.claude/hooks/cc-{notify,status,usage-tool}.sh`.

## Memoria

- Proxy HTTP: Pss 45 349 KiB (Python) → 5 929 KiB (Rust), `docs/verification/rss.jsonl`.
- Instantánea real al momento del cutover: 320 proxies Python = 14 117 MiB Pss (44 MiB cada uno).
  A medida que las sesiones se renueven, la misma cantidad en Rust ocupará ≈ 1 850 MiB.

## Reversión

```sh
comandos install --rollback cc-extensions   # restaura el enlace al bin/cc-extensions Python
readlink ~/.local/bin/cc-extensions         # debe volver a …/ComandOS/bin/cc-extensions
```

Solo afecta a sesiones nuevas; las abiertas conservan el proxy con el que arrancaron.
