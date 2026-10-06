# Cutover de terminal: procedimiento pendiente para el controlador

Este cambio no activa terminales por omisión y no ejecuta despliegue. El frente
con `--term ttyd` conserva los bytes de la página xterm instalada; `native` sirve
la página A9, cuyo arranque WASM se completa en A11. No activar `native` todavía.

Antes de activar, registrar las unidades cc-dash, cc-dash-legacy, cc-webterm y
cc-webterm-path, la release y `tailscale serve status`. Preparar las grabaciones
A12 con tmux privado, HOME temporal y configuración vacía. La herramienta A12 y
la comparación de capturas aún son dependencias pendientes: este procedimiento
no declara aprobado ese gate.

Ensayar el binario de la release desde su ruta absoluta verificada con:
`dash 4782 --no-open --shadow-readonly --no-usage-effects --term ttyd
--term-replay-dir /home/someguy/.local/share/comandos/term-replays`.
No usar sesiones reales para grabar ni conectar el navegador a 4777. El modo
sombra devuelve JSON simulado para escrituras y usa grabaciones para WebSocket;
los GET conservan el reenvío heredado previsto en D6.

Exponer 4782 con `cc-browser-expose start 4782`. La comparación con ttyd debe
usar exclusivamente el navegador remoto de Mac mini mediante MCP chrome-bg,
ambos motores con las mismas grabaciones privadas. Si falta ese runtime, parar
el gate visual y documentar la capacidad faltante, sin navegador local.

Tras aprobar A12 y la comparación, el controlador puede instalar los enlaces
cc-webterm y cc-webterm-attach y crear el override
`/home/someguy/.config/systemd/user/cc-dash.service.d/term.conf` con:

```ini
[Service]
Environment=COMANDOS_DASH_TERM=ttyd
Environment=COMANDOS_DASH_WEBTERM_COMPAT=4780
```

Detener exclusivamente cc-webterm-path.service, recargar systemd y reiniciar
cc-dash.service. El oyente 4780 debe pertenecer al frente. Su respuesta anuncia
`X-Comandos-Term-Compat: 4780`; el CLI omite ttyd únicamente para los puertos
anunciados. Sin ese anuncio, conserva el fallback de 4780 para tailscale.
Verificar teléfono, GBoard, rotación, historial y Paneles; confirmar tailscale
sin cambios y Pss con cuatro terminales dentro del presupuesto aprobado.

Para revertir, retirar únicamente ese override, recargar systemd, reiniciar el
frente y restaurar los enlaces de la release registrada. Ejecutar el controlador
legacy de terminales desde su ruta absoluta verificada. No matar tmux ni sesiones.
