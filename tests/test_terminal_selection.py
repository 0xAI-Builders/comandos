import json
import subprocess
from pathlib import Path
from test_remote_ui import extract_js_function

SOURCE = Path('dash/term.html').read_text()


def run_js(names, body):
    code = '\n'.join(extract_js_function(SOURCE, name) for name in names)
    return json.loads(subprocess.check_output(['node', '-e', code + '\n' + body], text=True))


def test_line_buttons_add_whole_lines_above_and_below():
    result = run_js(['extendSelectionByLine'], '''
const text='one\\nA🙂B\\nlast';
const slice=r=>text.slice(r.start,r.end);
const up=extendSelectionByLine(text,9,13,'up');
const upAgain=extendSelectionByLine(text,up.start,up.end,'up');
const down=extendSelectionByLine(text,0,3,'down');
const top=extendSelectionByLine(text,0,3,'up');
const bottom=extendSelectionByLine(text,9,13,'down');
console.log(JSON.stringify([slice(up),slice(upAgain),slice(down),slice(top),slice(bottom)]));
''')
    assert result == ['A🙂B\nlast', 'one\nA🙂B\nlast', 'one\nA🙂B', 'one', 'last']


def test_pane_names_use_folder_and_hide_tmux_ids():
    result = run_js(['paneDisplay'], '''
console.log(JSON.stringify([
  paneDisplay({id:'%78',title:'claude',path:'~/codebase/0xJesus/ComandOS'},0),
  paneDisplay({id:'%79',title:'zsh',path:'~'},1),
  paneDisplay({id:'%80',title:'node'},2),
]));
''')
    assert result == [
        {'path': '~/codebase/0xJesus/ComandOS', 'label': '1. ComandOS · claude'},
        {'path': '~', 'label': '2. ~ · terminal'},
        {'path': '', 'label': 'Panel 3 · node'},
    ]
