# Verificación de extensiones compartidas

La instalación comparte 31 MCPs habilitados y 81 skills personales entre los
cinco CLIs y la cuenta `relotto`. La autenticación total sigue incompleta:
siete servicios fallan. El [manual](../extension-parity.md) describe el alcance,
los archivos privados y la propagación automática.

## Evidencia local

- Siete archivos nativos contienen los mismos 31 nombres compartidos. OpenCode
  tiene archivos JSON y JSONC; ambos se sincronizan. Codex conserva además su
  `node_repl` nativo.
- Ocho carpetas de skills exponen los mismos 81 nombres personales. No se
  encontraron enlaces rotos en el árbol canónico.
- Claude, Codex, Grok, OpenCode y Antigravity leyeron sus configuraciones con
  código de salida cero. No se hicieron llamadas a sus modelos.
- Una segunda sincronización informó cero configuraciones y cero enlaces
  modificados. El timer está activo y el servicio informó `Result=success` y
  `ExecMainStatus=0`.
- El lanzador instalado completó `initialize` y `tools/list` para Gmail,
  Mobbin y ClickUp, con 15, 3 y 61 herramientas respectivamente.
- Las pruebas de catálogo, credenciales, puente y migración de navegador
  completaron 41 casos. Cubren renovación concurrente, respuestas estructuradas,
  edición concurrente de configuración, conflictos entre copias de skills,
  enlaces, nuevas cuentas y conservación de reglas de aprobación.

La comprobación de los 39 registros del catálogo obtuvo 21 conexiones,
siete fallos, tres omisiones y ocho definiciones desactivadas. El resultado
privado, sin credenciales ni contenidos de usuario, queda en
`~/.local/state/comandos/extensions/last-check.json`. El comando devolvió 1
debido a los fallos; no se interpreta como verificación completa.

## Servicios pendientes

| Servicio | Resultado observado |
|---|---|
| Google Drive | Anuncia herramientas; la lectura mínima responde HTTP 401 |
| Google Calendar | Anuncia herramientas; la lectura mínima responde HTTP 401 |
| Linear | Falló la renovación de la credencial MCP importada |
| Quercu SharePoint | El servidor local responde HTTP 401; el registro de Claude no contiene token |
| Radek SharePoint | El servidor local responde HTTP 401; el registro de Claude no contiene token |
| Odoo | El servidor termina con error de configuración: el modo configurado requiere usuario y API key; falta `ODOO_USER` |
| Near | `npx` no puede obtener el paquete configurado `@nearai/near-mcp`; falla antes de inicializar MCP |

El token de Gmail solo tiene scopes de correo. Las credenciales guardadas de
gcloud tampoco incluyen scopes de Drive o Calendar. No se reutilizaron esos
tokens para afirmar un acceso que no tienen. No se solicitaron nuevos logins.

Chrome remoto, Chrome personal y Teams no se iniciaron en la comprobación.
Playwright, X Playwright, Lightpanda, Obscura y Screenwright quedan desactivados
porque sus runtimes de navegador no se verificaron en el Mac. Godot, Unreal y
el alias antiguo Jira Rovo también permanecen desactivados.

## Revisión y límites

La revisión independiente reprodujo problemas de enlaces de skills, pérdida
de políticas al desactivar servidores, renovación nativa posterior a la
importación y sobrescritura de ediciones simultáneas. Las pruebas de regresión
incluyen esas correcciones. La verificación no cubre las operaciones de
negocio de todos los MCPs, los plugins propietarios ni una restauración total.

No se modificó el proyecto Lola ni su infraestructura. Los secretos se
mantienen fuera del repositorio; catálogo y credenciales tienen permisos 0600.
