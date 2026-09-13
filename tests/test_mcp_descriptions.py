import json
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'lib'))
import capabilities


def test_toml_metadata_keeps_quoted_names_and_ignores_nested_tables(tmp_path):
    config = tmp_path / 'config.toml'
    config.write_text('''[mcp_servers."docs.env"]
description = "Consulta documentación técnica."
enabled = false
[mcp_servers."docs.env".env]
description = "PRIVATE_ENV_VALUE"
TOKEN = "PRIVATE_TOKEN"
[mcp_servers.browser]
description = """Navega páginas.
Extrae contenido."""
[mcp_servers.browser.http_headers]
Authorization = "PRIVATE_HEADER"
''')
    rows = capabilities._toml_mcps(config, 'codex', 'user', 'test')
    assert [row['name'] for row in rows] == ['docs.env', 'browser']
    assert rows[0]['description'] == 'Consulta documentación técnica.'
    assert rows[0]['enabled'] is False
    assert rows[1]['description'] == 'Navega páginas. Extrae contenido.'
    assert 'PRIVATE' not in json.dumps(rows)


def test_json_description_exposes_no_command_arguments_or_credentials(tmp_path):
    config = tmp_path / '.mcp.json'
    config.write_text(json.dumps({'mcpServers': {'custom': {
        'description': 'Busca documentos del equipo.', 'command': 'PRIVATE_COMMAND',
        'args': ['PRIVATE_ARG'], 'env': {'TOKEN': 'PRIVATE_TOKEN'},
        'headers': {'Authorization': 'PRIVATE_HEADER'}}}}))
    row = capabilities._json_mcps(config, 'claude', 'project', 'test')[0]
    assert row['description'] == 'Busca documentos del equipo.'
    assert row['descriptionSource'] == 'configuration'
    assert 'PRIVATE' not in json.dumps(row)


def test_catalog_fallback_is_explicit_and_unknown_server_is_not_invented(tmp_path):
    config = tmp_path / '.mcp.json'
    config.write_text(json.dumps({'mcpServers': {'playwright': {}, 'mystery-custom': {}}}))
    rows = {r['name']: r for r in capabilities._json_mcps(config, 'claude', 'user', 'test')}
    assert 'navegador' in rows['playwright']['description'].lower()
    assert rows['playwright']['descriptionSource'] == 'catalog'
    assert rows['mystery-custom']['description'] == ''
    assert rows['mystery-custom']['descriptionSource'] == 'unavailable'


def test_project_description_takes_precedence_over_catalog(tmp_path):
    user = tmp_path / 'user.json'
    project = tmp_path / 'project.json'
    user.write_text(json.dumps({'mcpServers': {'playwright': {}}}))
    project.write_text(json.dumps({'mcpServers': {'playwright': {'description': 'Navegador del proyecto.'}}}))
    rows = capabilities._merge(capabilities._json_mcps(user, 'claude', 'user', 'user') +
                               capabilities._json_mcps(project, 'claude', 'project', 'project'))
    assert rows[0]['description'] == 'Navegador del proyecto.'
    assert rows[0]['descriptionSource'] == 'configuration'


def test_metadata_size_and_control_characters_are_bounded(tmp_path):
    config = tmp_path / '.mcp.json'
    config.write_text(json.dumps({'mcpServers': {'custom': {'description': 'Text\x00\x1b\n' + 'a' * 4000}}}))
    row = capabilities._json_mcps(config, 'claude', 'project', 'test')[0]
    assert 1 <= len(row['description']) <= 600
    assert not any(ord(c) < 32 for c in row['description'])
