import json
import subprocess
from pathlib import Path

from test_remote_ui import extract_js_function

HTML = Path('dash/index.html').read_text()


def test_remote_visible_cards_use_same_interval_as_desktop():
    script = extract_js_function(HTML, 'remotePollSeconds') + '''
    const S={cfg:{poll:2}},document={hidden:false};
    let WEBTERM=false,REMOTE={remoteOn:false};
    const desktop=remotePollSeconds();
    WEBTERM=true;REMOTE.remoteOn=true;
    const remote=remotePollSeconds();
    document.hidden=true;
    console.log(JSON.stringify({desktop,remote,background:remotePollSeconds()}));
    '''
    out = subprocess.run(['node', '-e', script], text=True, capture_output=True, check=True)
    assert json.loads(out.stdout) == {'desktop': 2, 'remote': 2, 'background': 15}


def test_slow_state_poll_does_not_accumulate_requests_and_failure_releases_lock():
    script = extract_js_function(HTML, 'tick') + '''
    let statePollInFlight=false,calls=0,release,renders=0;
    let api=()=>{calls++;return new Promise(resolve=>release=resolve);};
    function render(){renders++;}function tickUsage(){}function tickRecent(){}
    function refreshFavorites(){}
    (async()=>{
      const first=tick();await tick();await tick();
      if(calls!==1)throw Error('duplicate in-flight requests');
      release([]);await first;
      api=async()=>{calls++;throw Error('offline');};await tick();
      api=async()=>{calls++;return [];};await tick();
      console.log(JSON.stringify({calls,renders,statePollInFlight}));
    })().catch(e=>{console.error(e);process.exit(1);});
    '''
    out = subprocess.run(['node', '-e', script], text=True, capture_output=True, check=True)
    assert json.loads(out.stdout) == {'calls': 3, 'renders': 2, 'statePollInFlight': False}
