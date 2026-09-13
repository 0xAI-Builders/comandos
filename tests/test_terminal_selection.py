import json
import subprocess
from pathlib import Path
from test_remote_ui import extract_js_function


def selection_js(body):
    source = Path('dash/term.html').read_text()
    code = extract_js_function(source, 'moveTextSelection')
    return json.loads(subprocess.check_output(['node', '-e', code + '\n' + body], text=True))


def test_bottom_arrows_adjust_only_selected_edge_and_preserve_emoji():
    result = selection_js('''
const text='one\\nA🙂B\\nlast';
const left=moveTextSelection(text,5,8,'end','left');
const emoji=moveTextSelection(text,left.start,left.end,'end','left');
const up=moveTextSelection(text,4,8,'start','up');
const crossing=moveTextSelection(text,4,5,'start','right');
console.log(JSON.stringify({left,emoji,up,crossing}));
''')
    assert result['left'] == {'start':5,'end':7}
    assert result['emoji'] == {'start':5,'end':5}
    assert result['up'] == {'start':0,'end':8}
    assert result['crossing'] == {'start':5,'end':5}


def test_navigation_bounds_empty_text_and_short_lines():
    result = selection_js('''
console.log(JSON.stringify([
 moveTextSelection('',0,0,'end','down'),
 moveTextSelection('abcd\\nx\\nlast',0,4,'end','down'),
 moveTextSelection('text',0,4,'end','right'),
 moveTextSelection('text',0,4,'start','left')
]));
''')
    assert result == [{'start':0,'end':0},{'start':0,'end':6},{'start':0,'end':4},{'start':0,'end':4}]
