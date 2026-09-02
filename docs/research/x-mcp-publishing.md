# MCP para publicar en X (formerly Twitter)

Verificado: 2026-07-20.

## Conclusión

Sí existen MCPs para X. X mantiene un MCP oficial y Composio ofrece uno
administrado. Sin embargo, el catálogo público actual del MCP oficial de X no
documenta la creación de posts normales: permite leer posts y publicar
Articles. Para publicar un post normal hoy, las opciones verificadas son el MCP
local `x-playwright` ya instalado en esta máquina, Composio, o una integración
propia con `POST /2/tweets` mediante la API oficial.

## Opciones verificadas

### 1. X MCP oficial

- Endpoint alojado: `https://api.x.com/mcp`.
- Puente oficial: `@xdevplatform/xurl`, con OAuth 2.0 PKCE y renovación de
  tokens.
- X exige una app de desarrollador propia para el flujo con contexto de
  usuario. El bridge guarda y renueva el token localmente.
- Su tabla pública documenta lectura de posts, búsqueda, usuarios, bookmarks,
  tendencias, noticias y creación/publicación de Articles.
- La tabla no lista una herramienta para crear un post normal. Esto debe
  comprobarse de nuevo antes de adoptarlo como publicador, porque el catálogo
  puede cambiar.

Fuentes:

- https://docs.x.com/tools/mcp
- https://github.com/xdevplatform/xurl

### 2. MCP local `x-playwright`

Ya está configurado en `~/.codex/config.toml` y disponible en la sesión actual.
Su código vive en:

`/home/someguy/metaverse-dev/mcp/x-playwright-mcp`

Herramientas relevantes:

- `x_draft_post`: abre el compositor y prepara el borrador.
- `x_publish_current_draft`: publica únicamente si recibe `confirm: "POST"`.

Ventajas: no entrega credenciales a terceros, usa un perfil Chrome local y
mantiene borrador y publicación como acciones separadas. Desventajas: depende
de la interfaz web y sus selectores; X puede cambiar la UI o mostrar retos de
login. Las 15 pruebas locales del servidor pasaron el 2026-07-20, pero no se
realizó una publicación de prueba.

### 3. Composio Twitter MCP

- Documenta 78 herramientas, incluida `Create a post`.
- Administra OAuth, rotación de tokens, cuentas y auditoría.
- Ofrece un endpoint MCP alojado para Codex y otros clientes.
- El costo operativo y la confianza cambian: Composio ejecuta la herramienta y
  conserva la conexión OAuth. Conviene limitar el servidor a las herramientas
  necesarias y conservar aprobación humana para publicar.

Fuentes:

- https://composio.dev/toolkits/twitter
- https://docs.composio.dev/docs/sessions-via-mcp

## Costos actuales de la API oficial

X usa créditos pay-per-use. Al 2026-07-20:

- Crear un post normal: USD 0.015 por solicitud.
- Crear un post con URL: USD 0.200 por solicitud.
- La consola permite límites de gasto y seguimiento en tiempo real.

Fuente: https://docs.x.com/x-api/getting-started/pricing

## Recomendación para ComandOS

1. Para uso inmediato, conservar `x-playwright`: borrador visible y botón o
   confirmación explícita antes de publicar.
2. No usar el MCP oficial como publicador de posts normales hasta que su
   catálogo documente esa herramienta.
3. Para una función estable de producto, reemplazar la automatización web por
   un MCP local pequeño que use OAuth mediante `xurl` y llame a
   `POST /2/tweets`. Mantener `draft -> preview -> confirm -> publish`, un
   allowlist de cuenta y un límite de gasto.
4. Usar Composio solo si se prefiere delegar autenticación y mantenimiento a un
   tercero; restringir el Tool Router a crear posts y consultar su resultado.

