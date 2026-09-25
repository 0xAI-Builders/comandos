"""Exercise shelf resizing in Node without starting a browser."""
import subprocess
from pathlib import Path


def test_shelf_clamps_against_live_window_and_visual_viewport():
    source = Path('dash/extensions.js').read_text()
    start = source.index("  const SHELF_KEY=")
    end = source.index("  function shelfGrip()", start)
    listener_start = source.index("  // A smaller window")
    listener_end = source.index("  // Dashboard and desktop", listener_start)
    script = """
const assert=require('node:assert/strict');
const listeners={}, visualListeners={}, props={};
global.window={innerHeight:900,visualViewport:{height:900,addEventListener:(k,f)=>visualListeners[k]=f},addEventListener:(k,f)=>listeners[k]=f};
global.innerHeight=900;
global.localStorage={getItem:()=> '650'};
global.getComputedStyle=()=>({getPropertyValue:()=> '900px'});
global.document={documentElement:{clientHeight:900,style:{setProperty:(k,v)=>props[k]=v}},body:{classList:{contains:()=>false}},getElementById:id=>id==='pane-extensions-frame'?{}:null};
""" + source[start:end] + source[listener_start:listener_end] + """
window.innerHeight=500;global.innerHeight=500;document.documentElement.clientHeight=500;
listeners.resize();
assert.equal(props['--pane-shelf-height'],'380px');
window.visualViewport.height=350;
assert.equal(typeof visualListeners.resize,'function');
visualListeners.resize();
assert.equal(props['--pane-shelf-height'],'230px');
window.innerHeight=900;document.documentElement.clientHeight=900;window.visualViewport.height=900;
listeners.resize();
assert.equal(props['--pane-shelf-height'],'650px');
"""
    subprocess.run(['node', '-e', script], check=True, capture_output=True, text=True)
