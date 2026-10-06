//! Extract only approved Python functions/dictionaries; never import GTK or run tmux.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::disallowed_methods
)]
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    process::Command,
};
const SOURCE: &str = "2674f366bb01b9db42f6f64b728929b3acfe8b83:bin/cc-app";
const SOURCE_SHA256: &str = "1b535bc6f4463959cc3ab992fa5a4ab859fbdaec43fcf628123cfede68b4ca6b";
const LOADER: &str = r#"
import ast,json,re,sys,types
from urllib.parse import quote
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
if mode=='xterm':want.add('open_xterm_tab')
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
elif mode=='xterm':
    class Box:
        def __init__(self,**kw):pass
        def pack_start(self,*a):pass
        def show_all(self):pass
    class WebView:
        def get_settings(self):return types.SimpleNamespace(set_property=lambda *a:None)
        def load_uri(self,uri):calls.append(['load_uri',uri])
    class Response:
        def __enter__(self):return self
        def __exit__(self,*a):pass
        def read(self):return b'{"token":"private-token"}'
    class Thread:
        def __init__(self,target,daemon):self.target=target
        def start(self):self.target()
    ns.update({'quote':quote,'threading':types.SimpleNamespace(Thread=Thread),'urllib':types.SimpleNamespace(request=types.SimpleNamespace(urlopen=lambda *a,**kw:Response())),'Gtk':types.SimpleNamespace(Box=Box,Orientation=types.SimpleNamespace(VERTICAL=1)),'WebKit2':types.SimpleNamespace(WebView=WebView),'_ensure_cc_webterm':lambda:None,'TERM_HTML':'/private/term.html','BASE_URL':'http://private.invalid','THEMES':{},'THEME':{},'tab_label':lambda *a:None,'save_tabs':stub('save_tabs'),'ws_select':stub('ws_select')})
    for session in cases:
        ns['tabs']={};ns['_XTERM_PENDING']=set();calls.clear()
        ns['nb']=types.SimpleNamespace(append_page=lambda *a:0,set_tab_reorderable=lambda *a:None,set_current_page=stub('select_page'))
        try:
            ns['APP_COMMANDS']['open_xterm_tab']({'session':session})
            out.append({'keys':list(ns['tabs']),'calls':list(calls)})
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
/// Frozen AST oracle. Default replay never reads source or starts Python/git.
/// The central helper also records the private HOME tree, so filesystem effects
/// cannot disappear silently even though the approved stubs currently create none.
pub fn oracle(mode: &str, cases: &Value) -> Value {
    let mut nonce = [0u8; 12];
    getrandom::fill(&mut nonce).unwrap();
    let root = std::env::temp_dir().join(format!(
        "comandos-t13-oracle-{}",
        nonce.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ));
    let home = root.join("home");
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&home)
        .unwrap();
    let golden_root = std::env::var_os("COMANDOS_APP_T13_GOLDEN_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| comandos_oracle::golden_root(Path::new(env!("CARGO_MANIFEST_DIR"))));
    let input = json!({
        "source": SOURCE, "source_sha256": SOURCE_SHA256,
        "loader_sha256": format!("{:x}", Sha256::digest(LOADER.as_bytes())),
        "mode": mode, "cases": cases,
    });
    let result = comandos_oracle::text_with_tree_at(
        &golden_root,
        "app-t13",
        &input,
        &home,
        &[("$T13_HOME", &home), ("$T13_ROOT", &root)],
        || {
            // Deliberately inside record/check: neither git nor source access is
            // a replay prerequisite. A moving checkout or environment override
            // cannot change the approved original.
            let source = Command::new("git")
                .args(["show", SOURCE])
                .current_dir(env!("CARGO_MANIFEST_DIR"))
                .output()
                .map_err(|e| e.to_string())?;
            if !source.status.success() {
                return Err("T13 original source unavailable".into());
            }
            if format!("{:x}", Sha256::digest(&source.stdout)) != SOURCE_SHA256 {
                return Err("T13 original source checksum differs".into());
            }
            let source_path = root.join("original-cc-app");
            std::fs::write(&source_path, source.stdout).map_err(|e| e.to_string())?;
            let result = Command::new(
                std::env::var_os("COMANDOS_APP_ORACLE_PYTHON")
                    .unwrap_or_else(|| "/usr/bin/python3".into()),
            )
            .env_clear()
            .env("HOME", &home)
            .env("PATH", "/usr/bin:/bin")
            .env("LC_ALL", "C.UTF-8")
            .env("COMANDOS_APP_TMUX_SOCKET", root.join("private-tmux.sock"))
            .arg("-c")
            .arg(LOADER)
            .arg(&source_path)
            .arg(mode)
            .arg(cases.to_string())
            .output()
            .map_err(|e| e.to_string())?;
            if !result.status.success() {
                return Err(format!(
                    "T13 original failed: {}",
                    String::from_utf8_lossy(&result.stderr)
                ));
            }
            String::from_utf8(result.stdout).map_err(|e| e.to_string())
        },
    );
    std::fs::remove_dir_all(root).unwrap();
    serde_json::from_str(&result.unwrap()).unwrap()
}
