# Extensiones compartidas entre CLIs

`cc-extensions` comparte definiciones MCP y skills personales entre Claude,
Codex, Grok, OpenCode y Antigravity. Descubre también las cuentas ubicadas en
`~/.claude-accounts`, `~/.codex-accounts` y `~/.grok-accounts`. El resultado
verificado de la instalación está en la
[auditoría de paridad](audits/2026-09-25-extension-parity.md).

## Archivos y responsabilidades

| Ruta | Contenido |
|---|---|
| `~/.config/comandos/extensions/catalog.json` | Definiciones, transportes y estado habilitado de los MCPs |
| `~/.config/comandos/extensions/credentials.json` | Credenciales MCP reutilizadas, con permisos 0600 |
| `~/.agents/skills` | Skills personales compartidas y sus recursos |
| `~/.local/state/comandos/extensions/backups` | Copias privadas de configuraciones y skills reemplazadas |
| `~/.local/state/comandos/extensions/last-check.json` | Resultado de la última comprobación de servicios |
| `~/.local/share/comandos/extensions` | Código instalado del sincronizador y del puente |

El catálogo puede contener valores privados de las configuraciones originales.
No debe copiarse a Git, a mensajes ni a informes. Los comandos de estado solo
muestran nombres y resultados, nunca las credenciales.

## Propagación

El timer de usuario `comandos-extensions-sync.timer` ejecuta la sincronización
60 segundos después de terminar la anterior, con hasta cinco segundos de
variación y la precisión de systemd. Los cambios de nombre o altas aparecen en
los archivos nativos en la siguiente ejecución. Los procesos ya abiertos
conservan el catálogo que cargaron al iniciar.

Las definiciones nativas apuntan a `cc-extensions serve <nombre>`. El lanzador
lee la definición compartida al iniciar cada conexión. Los cambios explícitos
en una definición nativa se incorporan al catálogo y se propagan al resto.
Eliminar una entrada nativa desactiva su definición compartida, que sigue
guardada en el catálogo. Para reactivarla se cambia `enabled` en el catálogo.
Las reglas de aprobación de cada cliente se guardan incluso mientras un
servidor permanece desactivado.

Las skills se enlazan desde las carpetas de cada CLI. Editar sus archivos
modifica el contenido común. Reinstalar una skill sustituyendo el enlace por
una carpeta se detecta y se propaga si no hay cambios incompatibles en otra
copia. Para eliminar una skill de todos los clientes se elimina su carpeta
canónica de `~/.agents/skills`; los enlaces obsoletos se retiran en la siguiente
sincronización. Quitar solamente un enlace nativo hace que se reponga.

Si dos clientes cambian de forma incompatible la misma definición o skill,
la sincronización falla y conserva los cambios para resolver el conflicto.
Las comprobaciones de huellas detectan cambios de MCP entre lectura,
reconciliación, escritura y guardado del estado. Las escrituras privadas son
atómicas. No hay una transacción que abarque todos los archivos: un fallo
puede dejar algunos clientes actualizados y otros pendientes hasta reintentar.

Los endpoints específicos de proyectos se conservan cuando difieren del
catálogo global. Los plugins, las skills internas de cada proveedor y
`node_repl` de Codex conservan sus mecanismos nativos. Esta herramienta no
convierte esas capacidades propietarias en extensiones portables.

## Autenticación

Los servidores locales de Gmail, WhatsApp, Telegram y Threads0x conservan su
propio inicio de sesión. El lanzador comparte su transporte entre clientes.

Para HTTP, la importación busca credenciales de MCP en Claude, sus cuentas,
Grok y las cachés de `mcp-remote`, con coincidencia de endpoint. No importa
credenciales del modelo. Las renovaciones de los clientes que usan el puente
se serializan mediante un bloqueo de archivos. El token renovado se guarda
antes de que lo utilice otro proceso; también se actualiza el registro MCP de
origen sin modificar la autenticación del modelo. Una renovación posterior
de ese registro nativo puede reutilizarse.

Los procesos nativos que ya estaban abiertos no participan en ese bloqueo.
Conviene reanudarlos con la nueva configuración antes de depender de la
coordinación de renovaciones. No se importan almacenes OAuth propietarios de
Codex, Antigravity o plugins, ni credenciales nuevas de OpenCode. Un servicio
sin credenciales recuperables conserva su fallo de autenticación; no se abre
automáticamente un navegador ni se inventan permisos.

El puente HTTP conserva herramientas, recursos, prompts y respuestas
estructuradas. No anuncia soporte de sampling ni de suscripciones. La
autenticación de SSE admite el endpoint de mensajes en el mismo origen;
no se ha comprobado SSE contra un servicio real en esta instalación.

## Comandos comprobados

Desde este repositorio, `python3.11 scripts/install-extensions.py` instala las
dependencias fijadas, copia el código, sincroniza y activa el timer de usuario.
Repetirlo actualiza el código instalado y conserva el catálogo existente.

- `cc-extensions status` muestra el inventario compartido.
- `cc-extensions sync` propaga cambios inmediatamente.
- `cc-extensions check` inicia conexiones y lista herramientas. Devuelve código
  1 si algún servicio falla. Para Drive y Calendar también prueba una lectura
  mínima, porque estos servidores publican herramientas sin autenticar.
- `systemctl --user is-active comandos-extensions-sync.timer` comprueba el timer.
- `systemctl --user stop comandos-extensions-sync.timer` detiene la propagación
  automática. No restaura archivos por sí solo.

`check` no ejecuta herramientas de escritura. Omite navegadores y Teams para
evitar accesos interactivos. En los demás servicios, una conexión correcta no
demuestra que todas sus operaciones estén autorizadas. Los respaldos permiten
recuperar los archivos anteriores, pero no se ha ensayado una restauración
completa de esta instalación.

La implementación vive en [catálogo](../lib/extension_catalog.py),
[autenticación](../lib/extension_auth.py), [puente](../lib/extension_proxy.py)
y [comando](../bin/cc-extensions). Las pruebas usan hogares temporales y un
servidor HTTP simulado. La configuración nativa de Codex sigue su
[documentación de MCP](https://developers.openai.com/codex/mcp).
