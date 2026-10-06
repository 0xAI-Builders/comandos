//! Extract only approved Python functions/dictionaries; never import GTK or run tmux.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::disallowed_methods
)]
use serde_json::Value;
use std::{path::PathBuf, process::Command};
const LOADER: &str = r#"
import ast,json,re,sys,types
tree=ast.parse(open(sys.argv[1],encoding='utf-8').read())
mode=sys.argv[2];cases=json.loads(sys.argv[3]);calls=[]
def stub(name):
    def f(*args,**kw):calls.append([name,list(args),kw])
    return f
ns={'json':json,'re':re,'print':lambda *a:None}
names=['_open_extension_shelf','reader_open','reader_close','reader_terminal','apply_button_style','_side_term_show','_left_panel_set','_chain_modal_msg','apply_theme','open_web_modal','open_web_tab','set_tab_label','_split_cmd','_kill_cur_pane','feed','_select_pane_cmd','_cycle_tab','_mru_toggle','tab_reorder','_mosaic_open','_mosaic_close','_mosaic_toggle','_mosaic_zoom','open_switcher','open_tabs_overview','show_help','open_snippets_dialog','new_local_tab','open_xterm_tab','_open_wizard','open_ai_session_here','copy_vte_selection','exit_copy_mode','copy_claude_reply','raise_main_window','_dash_call']
ns.update({name:stub(name) for name in names})
ns['HEADER_ACTIONS']={name:stub(name) for name in ['quickTerminal','newSession','sortMenu','notices','chains','analytics']}
ns['GLib']=types.SimpleNamespace(idle_add=lambda fn,*args:fn(*args))
ns['_SIDE']={};ns['tabs']={'safe':'safe'}
ns['_cur_term']=lambda:'current-terminal'
ns['_page_for']=lambda session:(2,'page')
ns['nb']=types.SimpleNamespace(set_current_page=stub('nb.set_current_page'),set_visible=stub('nb.set_visible'))
ns['wv']=types.SimpleNamespace(load_uri=stub('wv.load_uri'));ns['URL']='private-dashboard-url'
ns['win']=types.SimpleNamespace(iconify=stub('win.iconify'),maximize=stub('win.maximize'),unmaximize=stub('win.unmaximize'))
ns['paned']=types.SimpleNamespace(set_position=stub('paned.set_position'))
ns['Gtk']=types.SimpleNamespace(main_quit=stub('Gtk.main_quit'))
ns['set_font_scale']=stub('set_font_scale')
ns['_cur_term']=lambda:types.SimpleNamespace(get_font_scale=lambda:1.0,paste_clipboard=stub('paste_clipboard'))
ns['copy_vte_selection']=lambda term:calls.append(['copy_vte_selection',[],{}])
ns['copy_claude_reply']=lambda term:calls.append(['copy_claude_reply',[],{}])
ns['exit_copy_mode']=lambda term:calls.append(['exit_copy_mode',[],{}])
ns['feed']=lambda term,bytes:calls.append(['feed',[list(bytes)],{}])
def open_tab(session,window,label=None):
    if not ns['WIN_RE'].match(window or ''):window='claude'
    if not ns['SESSION_RE'].match(session or ''):return
    calls.append(['open_tab',[session,window],{'label':label}])
ns['open_tab']=open_tab
want={'on_msg','APP_COMMANDS','SESSION_RE','WIN_RE','_side_tabs_from_web','report_presence','ssh_host_from_session'}
if mode=='split':want.add('_split_cmd')
if mode=='fonts':want.add('set_font_scale')
for node in tree.body:
    names=[node.name] if isinstance(node,ast.FunctionDef) else [t.id for t in node.targets if isinstance(t,ast.Name)] if isinstance(node,ast.Assign) else []
    if want.intersection(names):exec(compile(ast.Module([node],[]),'original-ast','exec'),ns)
out=[]
if mode=='keys':out=list(ns['APP_COMMANDS'])
elif mode=='bridge':
    for case in cases:
        calls.clear();ns['_SIDE']={}
        res=types.SimpleNamespace(get_js_value=lambda:types.SimpleNamespace(to_string=lambda:json.dumps(case,ensure_ascii=False)))
        ns['on_msg'](None,res);out.append(list(calls))
elif mode in ('commands','fonts'):
    if mode=='fonts':ns['_cur_term']=lambda:types.SimpleNamespace(get_font_scale=lambda:1.0,set_font_scale=stub('term.set_font_scale'))
    for name,args in cases:
        calls.clear()
        try:ns['APP_COMMANDS'][name](args);out.append({'calls':list(calls)})
        except Exception as e:out.append({'error':type(e).__name__})
elif mode=='presence':
    ns['_PRESENCE']={'last':0.0,'visible':True};ns['WS_DEVICE']='desktop-private'
    ns['_in_background']=lambda work,done:work()
    for now,interaction,visible in cases:
        ns['time']=types.SimpleNamespace(monotonic=lambda:now/1000)
        ns['_PRESENCE']['visible']=visible;calls.clear()
        ns['report_presence'](interaction);out.append(list(calls))
elif mode=='ssh':out=[ns['ssh_host_from_session'](case) for case in cases]
elif mode=='split':
    for session,side,ssh,pane,cwd in cases:
        calls.clear();ns['_cur_term']=lambda:'terminal';ns['term_session']=lambda term:session
        ns['_cur_page']=lambda:types.SimpleNamespace(_key=session);ns['_cur_pane_id']=lambda:pane
        def tmuxc(*args):
            calls.append(list(args));return types.SimpleNamespace(stdout=cwd if args[-1]=='#{pane_current_path}' else '',returncode=0)
        ns['tmuxc']=tmuxc
        try:ns['_split_cmd'](side,ssh);out.append({'calls':list(calls)})
        except Exception as e:out.append({'error':type(e).__name__})
print(json.dumps(out,ensure_ascii=False))
"#;
pub fn oracle(mode: &str, cases: &Value) -> Value {
    let mut nonce = [0u8; 12];
    getrandom::fill(&mut nonce).unwrap();
    let root = std::env::temp_dir().join(format!(
        "comandos-t13-oracle-{}",
        nonce.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ));
    std::fs::create_dir_all(root.join("home")).unwrap();
    let result = Command::new("/usr/bin/python3")
        .env_clear()
        .env("HOME", root.join("home"))
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C.UTF-8")
        .env("COMANDOS_APP_TMUX_SOCKET", root.join("private-tmux.sock"))
        .arg("-c")
        .arg(LOADER)
        .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bin/cc-app"))
        .arg(mode)
        .arg(cases.to_string())
        .output()
        .unwrap();
    std::fs::remove_dir_all(root).unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&result.stdout).unwrap()
}
