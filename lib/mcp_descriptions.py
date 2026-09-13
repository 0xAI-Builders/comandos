"""Short UI summaries from advertised tools; no servers are started to read them.

Sources and review date are recorded in docs/mcp-descriptions.md. These describe
what an integration offers, not which tools an existing agent has loaded.
"""
import re

DESCRIPTIONS = {
    'atlassian': 'Busca y gestiona tareas de Jira, páginas de Confluence y componentes de Compass.',
    'azure-devops': 'Consulta proyectos, repositorios, tareas y alertas de seguridad de Azure DevOps.',
    'chrome-bg': 'Inspecciona y controla páginas de Chrome en el navegador configurado para segundo plano.',
    'chrome-devtools': 'Inspecciona páginas de Chrome, su consola, red y rendimiento; permite interactuar con ellas.',
    'claude-in-chrome': 'Controla pestañas de Chrome mediante su extensión: navegación, formularios y capturas.',
    'clickup': 'Busca y gestiona tareas de ClickUp, sus etiquetas, dependencias y relaciones.',
    'context7': 'Busca documentación y ejemplos actuales de bibliotecas, frameworks y APIs.',
    'gmail': 'Busca y lee correo de Gmail, descarga adjuntos y prepara o envía mensajes.',
    'godot': 'Crea y modifica escenas y nodos de proyectos Godot, y exporta recursos.',
    'google-calendar': 'Consulta calendarios y crea, actualiza o elimina eventos.',
    'google-drive': 'Busca, descarga, crea y copia archivos de Google Drive.',
    'jira-rovo': 'Busca y gestiona contenido de Jira y Confluence mediante Atlassian Rovo.',
    'lightpanda': 'Navega páginas sin interfaz gráfica, extrae su contenido e interactúa con formularios.',
    'mcp-image': 'Genera imágenes a partir de texto o edita una imagen existente y guarda el resultado.',
    'mobbin': 'Busca pantallas y flujos de productos reales como referencia para diseñar interfaces.',
    'moneyhacktracker': 'Busca hackathones y bounties de Web3 e IA y compara las oportunidades encontradas.',
    'near-mcp': 'Consulta cuentas y saldos de NEAR e interactúa con transacciones y contratos.',
    'node-repl': 'Ejecuta JavaScript en una sesión persistente y permite reutilizar variables y módulos.',
    'obscura': 'Navega e inspecciona páginas web y permite interactuar con elementos y cookies.',
    'openaideveloperdocs': 'Busca documentación oficial de OpenAI y consulta referencias de sus APIs.',
    'playwright': 'Automatiza el navegador: abre páginas, interactúa con elementos y consulta la consola.',
    'qcdr-search': 'Busca en la web mediante QCDR y extrae texto de las páginas encontradas.',
    'radek': 'Lista y descarga documentos de RADEK en SharePoint usando la sesión autorizada.',
    'screenwright': 'Graba recorridos de navegador o Android y genera videos con subtítulos sincronizados.',
    'slack': 'Busca contenido de canales, envía mensajes y gestiona documentos canvas de Slack.',
    'solana-dev': 'Busca documentación de Solana y consulta ayuda técnica para desarrollar en su ecosistema.',
    'supabase': 'Consulta bases de datos Supabase mediante SQL, genera tipos y revisa diagnósticos del proyecto.',
    'telegram': 'Consulta conversaciones y adjuntos de Telegram y permite enviar o gestionar mensajes.',
    'unreal-mcp': 'Controla Unreal Editor: actores, Blueprints, materiales, secuencias y otros recursos del proyecto.',
    'whatsapp': 'Consulta conversaciones de WhatsApp y gestiona mensajes, contactos y grupos.',
    'x-playwright': 'Abre X en el navegador, lee la página y prepara borradores de publicaciones.',
    'x-suite': 'Conecta una cuenta de X y ofrece herramientas para trabajar con ella mediante su API.',
}


def metadata(name, description=None):
    if isinstance(description, str):
        clean = re.sub(r'[\x00-\x1f\x7f]', ' ', description)
        clean = ' '.join(clean.split())[:600]
        if clean:
            return {'description': clean, 'descriptionSource': 'configuration'}
    text = DESCRIPTIONS.get(name.casefold().replace('_', '-'), '')
    return {'description': text, 'descriptionSource': 'catalog' if text else 'unavailable'}
