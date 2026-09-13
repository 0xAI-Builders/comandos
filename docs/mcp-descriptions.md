# Descripciones del inventario MCP

Las cards y los perfiles muestran una descripción visible debajo del nombre. El inventario conserva `description` declarada en la configuración del servidor. Si falta, usa el resumen integrado de ComandOS y lo identifica mediante `descriptionSource: catalog`. Un servidor desconocido muestra que no tiene descripción disponible; no se deducen capacidades de sus credenciales, argumentos o nombre.

El catálogo describe funciones disponibles en la integración, no confirma herramientas cargadas en la sesión actual. No inicia procesos MCP ni consulta servicios para pintar una lista. No añade dependencias. Las descripciones se limitan a 600 caracteres, se eliminan controles y el navegador las presenta como texto escapado.

La lectura TOML distingue un servidor con nombre entre comillas de sus tablas `env` y `http_headers`. Estas tablas ya no aparecen como servidores. La configuración de usuario tampoco se vuelve a contar como configuración de proyecto cuando ambas rutas apuntan al mismo archivo.

## Fuentes consultadas el 11 de septiembre de 2026

Los resúmenes de Atlassian, Azure DevOps, Chrome en segundo plano, Claude in Chrome, ClickUp, Context7, Gmail, Godot, Calendar, Drive, Jira Rovo, Image, MoneyHackTracker, Node REPL, Obscura, OpenAI Developer Docs, Playwright, QCDR, RADEK, Screenwright, Solana, Supabase, Telegram, WhatsApp, X Playwright y X Suite proceden de las definiciones de herramientas publicadas por sus MCPs en esta sesión. No se ejecutaron herramientas de esas cuentas para redactarlos. El resumen de Chrome DevTools utiliza las mismas herramientas de inspección publicadas por Chrome en segundo plano.

| Integración | Fuente adicional | Uso |
|---|---|---|
| NEAR | [Repositorio oficial](https://github.com/nearai/near-mcp) | Cuentas, saldos, transacciones y contratos. |
| Slack | [Descripción oficial del servidor MCP](https://docs.slack.dev/ai/mcp-overview/) | Canales, mensajes y canvas. |
| Lightpanda | [Referencia oficial de herramientas](https://lightpanda.io/docs/reference/mcp-tools) | Navegación, extracción e interacción con páginas. |
| Mobbin | [Presentación oficial de su MCP](https://mobbin.com/mcp) | Pantallas y flujos como referencias de diseño. No se consultaron diseños de Mobbin para modificar ComandOS. |
| Unreal | `/home/someguy/metaverse-dev/mcp/unreal-engine-skills-for-claude-code/README.md` | Integración local instalada; control de Unreal Editor y sus recursos. |

Las pruebas de `tests/test_mcp_descriptions.py` comprueban tablas anidadas, nombres entre comillas, descripciones multilínea, prioridad de configuración, servidores desconocidos y exclusión de secretos. El recorrido de navegador comprueba que la descripción se lea en las dos vistas y que HTML incrustado no se ejecute.
