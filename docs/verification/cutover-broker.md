# Cutover del broker de MCP — 4 de octubre de 2026

`comandos-broker.service` (unidad enlazada desde `systemd/comandos-broker.service` del worktree
`migration/rust-full`) está activo desde las 05:31 CST con el binario `153c06e`+`cd9b1bf`
(`comandos install --stage`). Desde entonces, cada `cc-extensions serve <stdio>` de una sesión nueva se
cuelga del daemon por `/run/user/1000/comandos/broker.sock` y comparte el proceso del servidor con las
demás sesiones del mismo proyecto (clave: nombre, cwd, entorno del spec). Los servidores HTTP, los que
llevan filtros de herramientas y los `DEDICATED` (navegadores, teams, claude-codex) siguen en proxy directo.

## Primer intento (05:13–05:20) y corrección

El Mux repetía a todos los clientes el `InitializeResult` del `initialize` interno (`2025-06-18`);
Claude Code 2.1.289 sondea con `2025-11-25` y reconectaba «pinned legacy» (supabase, samuel-reputacion).
Se desactivó el servicio (los `serve` vuelven solos al proxy directo) y la Task 12b hizo que el broker
caliente con la última versión y negocie por cliente como lo haría el servidor real: pedida si la
soporta y no es más nueva que la del upstream; si no, la del upstream. Verificado en sombra con socket
temporal: supabase contesta `2025-11-25` al sondeo y `2025-06-18` a un cliente viejo sobre el mismo
proceso; x-suite igual que en directo.

## Verificación con el servicio real

| Prueba | Resultado |
|---|---|
| Sesión `claude --debug -p` | 27/30 MCP conectados; los 3 restantes por el cierre a los 3.5 s de `-p` (uvx/ssh lentos) y el sondeo rmcp de `teams` (directo, comportamiento del propio servidor) |
| Dos sesiones concurrentes en el mismo proyecto | 17 attaches «(2 clientes)», 2 spawns nuevos: ambas comparten los 15 upstreams ya vivos |
| Clientes finos | ≈ 4 MiB Pss de media (el mayor, higgsfield HTTP directo, 24 MiB) frente a 51 MiB por proxy Python en una sesión viva |
| Daemon | 3 MiB solo; cgroup 622–818 MiB con 15–17 upstreams (npx, uvx, ssh a macmini arrancados con el PATH del cliente, Task 12a) |
| Reglas de oro | 22 sesiones tmux, 316 proxies Python intactos, ningún servicio reiniciado |

Hallazgo del arnés: un socket Unix con ruta > 108 bytes falla con mensaje claro y los clientes caen al
proxy directo; en producción la ruta es `/run/user/1000/comandos/broker.sock`.

## Pendiente de observar en uso real

- Servidores que pidan `roots/list`, `sampling` o `elicitation` a través del broker (el `initialize`
  interno no anuncia capacidades; los que las necesiten se marcan `"shared": false` en el catálogo).
- Upstreams que cambian de estado por sesión (x-suite guarda operaciones pendientes en memoria): hoy
  comparten proceso entre sesiones del mismo proyecto.
- Ahorro real con el patrón de Jesús (223 pares servidor|cwd distintos entre 316 proxies): el reparto es
  por proyecto, así que la compartición solo se nota con varias sesiones en el mismo directorio.

## Reversión

```sh
systemctl --user disable --now comandos-broker.service   # los serve nuevos vuelven al proxy directo
```

Los clientes ya colgados del daemon pierden su servidor al pararlo; las sesiones lo recuperan al
reconectar (Claude Code relanza el proxy). Hacerlo con las sesiones nuevas cerradas o avisando.
