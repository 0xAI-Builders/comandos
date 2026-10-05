//! Lógica de los popups sin pantalla, contra las funciones de `bin/cc-notifyd`
//! cargadas con un `gi` falso (Gtk/Gdk/GdkPixbuf/GLib grabadores escritos
//! aquí). Nada abre ventanas, suena ni toca tmux, el tablero o el puerto 4778.
mod support;

use comandos_notifyd::dash::{DashClient, NOTIF_POS_TTL, PrefsCache, THEME_TTL};
use comandos_notifyd::markup::{Block, blocks, fmt_table, markup_escape, md_to_pango};
use comandos_notifyd::model::{
    ARROW_CLOSED, CLOSE_GLYPH, ICON_FALLBACK, PopupModel, icon_svg, popup_model,
};
use comandos_notifyd::notice::{Lang, Notice};
use comandos_notifyd::position::{
    Configure, Geometry, anchor_from, clear_all_position, layout, load_anchor, on_configure,
    save_anchor,
};
use comandos_notifyd::stack::{PopupMeta, evict_candidate};
use comandos_notifyd::theme::{build_css, open_button_css, theme_name, themes_from_file, tokens};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use support::{python_eval, tempdir};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Literal de cadena válido en Python (una cadena JSON lo es).
fn py_lit(text: &str) -> String {
    serde_json::to_string(text).unwrap()
}

/// Sustitutos de `gi` que graban lo que hace `make_popup` y compañía, y un
/// `GLib.markup_escape_text` con la tabla de `g_markup_escape_text`.
const PRELUDE: &str = r#"
import types, time
def _esc(s):
    out = []
    for c in s:
        o = ord(c)
        if c == '&': out.append('&amp;')
        elif c == '<': out.append('&lt;')
        elif c == '>': out.append('&gt;')
        elif c == "'": out.append('&apos;')
        elif c == '"': out.append('&quot;')
        elif 1 <= o <= 8 or 11 <= o <= 12 or 14 <= o <= 31 or 127 <= o <= 132 or 134 <= o <= 159:
            out.append('&#x%x;' % o)
        else: out.append(c)
    return ''.join(out)
TIMERS = []
class _GLib:
    markup_escape_text = staticmethod(_esc)
    @staticmethod
    def timeout_add(ms, fn, *a):
        TIMERS.append(['ms', ms]); return 0
    @staticmethod
    def timeout_add_seconds(s, fn, *a):
        TIMERS.append(['s', s]); return 0
    @staticmethod
    def idle_add(fn, *a): return 0
class Const(int):
    def __call__(s, *a, **k): return None
    def __getattr__(s, n): return Const(0)
CREATED = []
class _Ctx:
    def __init__(s, w): s.w = w
    def add_class(s, c): s.w.classes.append(c)
    def remove_class(s, c):
        if c in s.w.classes: s.w.classes.remove(c)
    def add_provider(s, p, prio): s.w.providers.append(p)
class _Scr:
    def get_rgba_visual(s): return None
class W:
    def __init__(s, kind, *a, **k):
        s.kind = kind; s.classes = []; s.children = []; s.ends = []
        s.text = k.get('label'); s.markup = None; s.tip = None; s.providers = []
        s.data = k.get('data'); s.css = None; s.height = 50; s.moves = []
    def get_style_context(s): return _Ctx(s)
    def pack_start(s, c, *a): s.children.append(c)
    def pack_end(s, c, *a): s.ends.append(c)
    def add(s, c): s.children.append(c)
    def add_overlay(s, c): s.children.append(c)
    def set_markup(s, m): s.markup = m
    def set_text(s, t): s.text = t
    def set_label(s, t): s.text = t
    def set_tooltip_text(s, t): s.tip = t
    def load_from_data(s, d): s.css = d
    def get_allocated_height(s): return s.height
    def get_position(s): return (0, 0)
    def get_screen(s): return _Scr()
    def get_child(s): return None
    def connect(s, *a): return 0
    def move(s, x, y): s.moves.append([x, y])
    def __getattr__(s, n): return lambda *a, **k: None
class _Factory:
    def __init__(s, name): s.name = name
    def __call__(s, *a, **k):
        w = W(s.name, *a, **k); CREATED.append(w); return w
    def __getattr__(s, n):
        if s.name == 'Image' and n == 'new_from_pixbuf':
            return lambda pb: W('Image', data=pb.data)
        return Const(0)
class _NS:
    def __getattr__(s, n): return _Factory(n)
GEO = [0, 0, 1920, 1080]
class _Mon:
    def get_geometry(s):
        return types.SimpleNamespace(x=GEO[0], y=GEO[1], width=GEO[2], height=GEO[3])
class _Disp:
    def get_primary_monitor(s): return _Mon()
    def get_monitor(s, i): return _Mon()
class _Gdk(_NS):
    class Display:
        @staticmethod
        def get_default(): return _Disp()
class _PB:
    def __init__(s, d): s.data = d
class _Loader:
    def __init__(s): s.buf = b''
    def set_size(s, w, h): s.size = (w, h)
    def write(s, d): s.buf += d
    def close(s): pass
    def get_pixbuf(s): return _PB(s.buf)
class _Pixbuf:
    class PixbufLoader:
        @staticmethod
        def new_with_type(t): return _Loader()
mod.GLib = _GLib
mod.Gtk = _NS()
mod.Gdk = _Gdk()
mod.GdkPixbuf = _Pixbuf()
mod.dash = lambda *a, **k: None
PREFS = {'theme': 'noche', 'notif_pos': 'free'}
def _dash_get(path, timeout=1.5):
    if PREFS is None: raise OSError('sin tablero')
    return PREFS
mod.dash_get = _dash_get
"#;

/// Ejecuta `PRELUDE + src` con `INPUT` (JSON) y devuelve `out`. `None` sin `python3`.
fn run_py(hooks: &Path, src: &str, input: &Value) -> Option<Value> {
    let code = format!("{PRELUDE}\n{src}");
    let expr = format!(
        "(lambda g: (exec({}, g), g['out'])[1])({{'mod': mod, 'json': json, 'INPUT': json.loads({})}})",
        py_lit(&code),
        py_lit(&input.to_string())
    );
    python_eval(hooks, &[], &expr)
}

fn block_json(blocks: &[Block]) -> Value {
    Value::Array(
        blocks
            .iter()
            .map(|b| match b {
                Block::Text { markup, .. } => json!(["text", markup]),
                Block::Table { markup, .. } => json!(["table", markup]),
            })
            .collect(),
    )
}

const MD_TEXTS: &[&str] = &[
    "Hola **mundo** y *cursiva* con `code` y [link](http://x.y)",
    "# Título\n## Sub **b**\n####### no\n##sin espacio\n# \n#",
    "- uno\n* dos\n  - tres\n-sin espacio\n\t* tab",
    "a < b && c > d \"q\" 'q'",
    "```\ncode <x> & **no**\n```\ntras **sí**",
    "| A | B |\n|---|:-:|\n| **1** | `2` |\n| largo valor | x |",
    "texto\n| a | b |\n| c |\nfin",
    "***a** b*",
    "**sin cierre y *otra",
    "(*paren*) y x*no*x y *fin*. *a*b *c*",
    "`a`b`c` `` vacío ``` no",
    "[a [b](c) [d](e f) [](x) [g]() [h](i)j",
    "línea\r\nwindows\rmac\u{b}vt\u{c}ff\u{1c}fs\u{2028}ls\u{2029}ps\u{85}nel",
    "control \u{1}\u{8}\u{1f}\u{7f}\u{84}\u{86}\u{9f}\u{a0} fin",
    "| sólo | una |",
    "|---|---|",
    "```python\nunclosed fence | x |\n| a |",
    "  * indent bullet *it*",
    "mezcla **negrita `code** fin`",
    "😀 | no tabla\n|😀|ñ|\n|a|b|\n\n**a**b**c**",
    "",
    "\n\n  \n",
    "tabla con | pipes | dentro de texto\n||\n|",
    "x\n```\n| a | b |\n```\n| c | d |\n```",
];

/// El `markup_escape_text` falso del oráculo es la libglib real del sistema.
#[test]
fn escape_matches_glib() {
    let hooks = tempdir();
    let mut samples: Vec<String> = MD_TEXTS.iter().map(|s| s.to_string()).collect();
    samples.push((1u32..0x200).filter_map(char::from_u32).collect());
    samples.push("\u{2028}\u{feff}\u{1f600}<&>'\"".into());
    let Some(expected) = run_py(
        hooks.path(),
        "out = [_esc(t) for t in INPUT]",
        &json!(samples),
    ) else {
        return;
    };
    let got: Vec<String> = samples.iter().map(|s| markup_escape(s)).collect();
    assert_eq!(json!(got), expected);
}

#[test]
fn md_to_pango_matches_python() {
    let hooks = tempdir();
    let src = "out = [mod.md_to_pango(t) for t in INPUT]";
    let Some(expected) = run_py(hooks.path(), src, &json!(MD_TEXTS)) else {
        eprintln!("sin python3: se omite");
        return;
    };
    let got: Vec<String> = MD_TEXTS.iter().map(|t| md_to_pango(t)).collect();
    assert_eq!(json!(got), expected);
}

#[test]
fn fmt_table_matches_python() {
    let hooks = tempdir();
    let cases: Vec<Vec<&str>> = vec![
        vec!["| a | b |", "|---|---|", "| ccc | d |"],
        vec!["|x|", "| y | z | w |"],
        vec!["  | **b** | `c` |  ", "| : | - |"],
        vec!["|---|", "| :-: |"],
        vec!["||", "| | |"],
        vec!["| ñandú | 😀 |", "| a<b | & |"],
        vec!["|| a ||", "| trailing |\t"],
        vec![],
    ];
    let src = "out = [mod._fmt_table(rows) for rows in INPUT]";
    let Some(expected) = run_py(hooks.path(), src, &json!(cases)) else {
        return;
    };
    let got: Vec<Vec<String>> = cases.iter().map(|rows| fmt_table(rows)).collect();
    assert_eq!(json!(got), expected);
}

const BLOCKS_PY: &str = r#"
def _blocks(box):
    res = []
    for c in box.children:
        if c.kind == 'Label': res.append(['text', c.markup])
        else: res.append(['table', c.children[0].markup])
    return res
out = [_blocks(mod.build_full_widget(t)) for t in INPUT]
"#;

#[test]
fn blocks_match_build_full_widget() {
    let hooks = tempdir();
    let Some(expected) = run_py(hooks.path(), BLOCKS_PY, &json!(MD_TEXTS)) else {
        return;
    };
    let got: Vec<Value> = MD_TEXTS.iter().map(|t| block_json(&blocks(t))).collect();
    assert_eq!(json!(got), expected);
}

const THEME_PY: &str = r#"
import json as _j
res = []
for case in INPUT:
    PREFS = case['prefs']
    if case['file'] is not None:
        mod.THEMES_FILE = case['file']
    mod._theme_cache.update(at=0.0, tokens=None)
    try:
        t = mod.theme_tokens()
        res.append({'tokens': {k: str(v) for k, v in t.items()}, 'css': mod.build_css(t).decode()})
    except Exception as e:
        res.append({'error': type(e).__name__})
out = res
"#;

#[test]
fn theme_tokens_and_css_match_python() {
    let hooks = tempdir();
    let themes_file = repo_root().join("config/themes.json");
    let raw = std::fs::read(&themes_file).unwrap();
    let names: Vec<String> = serde_json::from_slice::<Value>(&raw).unwrap()["themes"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert!(names.len() >= 5);
    let custom = |name: &str, body: &str| -> String {
        let path = hooks.path().join(name);
        std::fs::write(&path, body).unwrap();
        path.to_string_lossy().into_owned()
    };
    let mut cases: Vec<Value> = names
        .iter()
        .map(|n| json!({"prefs": {"theme": n}, "file": null}))
        .collect();
    let repo_file = themes_file.to_string_lossy().into_owned();
    for prefs in [
        json!({"theme": "no-existe"}),
        Value::Null,
        json!({"theme": ""}),
        json!({"theme": 5}),
        json!({"theme": [1]}),
        json!([1]),
        json!({}),
    ] {
        cases.push(json!({"prefs": prefs, "file": repo_file}));
    }
    for (i, body) in [
        "",
        "[]",
        r#"{"themes": []}"#,
        r#"{"themes": [1]}"#,
        r#"{"themes": {"noche": "x"}}"#,
        r#"{"themes": {"noche": {"panel": 5, "text": null, "brand": [1, "a"], "extra": "e"}}}"#,
        r#"{"themes": {"x": {}}}"#,
        r#"{"themes": {"noche": {"bg": NaN, "dim": 1.5e3}}}"#,
    ]
    .iter()
    .enumerate()
    {
        let file = custom(&format!("themes-{i}.json"), body);
        cases.push(json!({"prefs": {"theme": "x"}, "file": file}));
    }
    cases.push(json!({"prefs": {"theme": "noche"}, "file": hooks.path().join("missing.json").to_string_lossy()}));
    let Some(expected) = run_py(hooks.path(), THEME_PY, &json!(cases)) else {
        return;
    };
    let got: Vec<Value> = cases
        .iter()
        .map(|case| {
            let file = case["file"]
                .as_str()
                .map_or(themes_file.clone(), PathBuf::from);
            let raw = std::fs::read(&file).ok();
            let prefs = (!case["prefs"].is_null()).then_some(&case["prefs"]);
            match tokens(&themes_from_file(raw.as_deref()), &theme_name(prefs)) {
                Some(t) => {
                    let strs: serde_json::Map<String, Value> = t
                        .iter()
                        .map(|(k, v)| (k.clone(), json!(comandos_core::pomodoro::python_str(v))))
                        .collect();
                    json!({"tokens": strs, "css": build_css(&t)})
                }
                None => json!({"error": "raised"}),
            }
        })
        .collect();
    let expected = expected.as_array().unwrap();
    assert_eq!(got.len(), expected.len());
    for (i, (g, e)) in got.iter().zip(expected).enumerate() {
        if e.get("error").is_some() {
            assert!(g.get("error").is_some(), "caso {i}: {:?} / {e:?}", cases[i]);
        } else {
            assert_eq!(g, e, "caso {i}: {:?}", cases[i]);
        }
    }
}

const POPUP_PY: &str = r#"
def _blocks(box):
    res = []
    for c in box.children:
        if c.kind == 'Label': res.append(['text', c.markup])
        else: res.append(['table', c.children[0].markup])
    return res
res = []
for case in INPUT:
    PREFS = case['prefs']
    mod._theme_cache.update(at=0.0, tokens=None)
    mod.UI_LANG = case['lang']
    mod.popups.clear(); mod.by_session.clear()
    CREATED.clear(); TIMERS.clear()
    n = case['notice']
    mod.make_popup(n['title'], n['body'], n['session'], n['kind'], n['project'],
                   n['options'], n['full'], False, n['pane'])
    win = [w for w in CREATED if w.kind == 'Window'][-1]
    ovl = win.children[0]
    card, cl2 = ovl.children
    title_ev, rev = card.children
    hd = title_ev.children[0]
    hdv = hd.children[0]
    hdi, inprev = hdv.children
    ar_ev, kd, pev2, spacer = hdi.children
    content = rev.children[0]
    tools = [c for c in content.children if c.ends][0]
    first = content.children[0]
    sws = [c for c in content.children if c.kind == 'ScrolledWindow']
    ob = tools.ends[0]
    res.append({
        'key': [k for k, v in mod.by_session.items() if v is win][0],
        'hd_classes': hd.classes,
        'arrow': ar_ev.children[0].text, 'arrow_classes': ar_ev.children[0].classes,
        'arrow_tip': ar_ev.tip,
        'icon': kd.data.decode() if kd.kind == 'Image' else None,
        'icon_fallback': kd.text if kd.kind == 'Label' else None,
        'kbadge_classes': kd.classes, 'kind_tip': kd.tip,
        'project': pev2.children[0].text, 'project_tip': pev2.tip,
        'first_line': inprev.text,
        'preview': _blocks(first) if first is not tools else None,
        'tools': [b.text for b in tools.children] + [b.text for b in tools.ends],
        'tool_classes': [b.classes for b in tools.children] + [b.classes for b in tools.ends],
        'see_all': _blocks(sws[0].children[0]) if sws else None,
        'close': cl2.text, 'close_tip': cl2.tip,
        'open_css': ob.providers[0].css.decode() if ob.providers else None,
        'seconds': [t[1] for t in TIMERS if t[0] == 's'],
        'window_classes': win.classes,
    })
out = res
"#;

fn model_json(m: &PopupModel, t: &comandos_notifyd::theme::Tokens) -> Value {
    let icons = repo_root().join("dash/icons");
    let svg = icon_svg(&icons, m).map(|b| String::from_utf8(b).unwrap());
    let mut tools = Vec::new();
    let mut tool_classes = Vec::new();
    if m.copy.is_some() {
        tools.push(m.copy_label);
        tool_classes.push(json!(["vermas"]));
    }
    if m.see_all.is_some() {
        tools.push(m.see_all_label);
        tool_classes.push(json!(["vermas"]));
    }
    tools.push(m.open_label);
    tool_classes.push(json!(["btn", "primary"]));
    json!({
        "key": m.key,
        "hd_classes": if m.waiting { json!(["hd", "waiting"]) } else { json!(["hd"]) },
        "arrow": ARROW_CLOSED, "arrow_classes": ["close"], "arrow_tip": m.arrow_tip,
        "icon_fallback": if svg.is_none() { Some(ICON_FALLBACK) } else { None },
        "icon": svg,
        "kbadge_classes": if m.waiting { json!(["kbadge", "waiting"]) } else { json!(["kbadge"]) },
        "kind_tip": m.kind_tip,
        "project": m.project, "project_tip": m.project_tip,
        "first_line": m.first_line,
        "preview": m.preview.as_deref().map(block_json),
        "tools": tools, "tool_classes": tool_classes,
        "see_all": m.see_all.as_deref().map(block_json),
        "close": CLOSE_GLYPH, "close_tip": m.close_tip,
        "open_css": open_button_css(t),
        "seconds": if m.auto_close { json!([10]) } else { json!([]) },
        "window_classes": ["ccpop"],
    })
}

fn notice(kind: &str, body: &str, full: &str, project: &str, pane: &str) -> Notice {
    Notice {
        title: "Claude Code".into(),
        body: body.into(),
        session: "proyecto-x".into(),
        kind: kind.into(),
        project: project.into(),
        options: "Sí\u{1f}No\u{1f}Quizá".into(),
        full: full.into(),
        pane: pane.into(),
    }
}

#[test]
fn popup_model_matches_make_popup() {
    let hooks = tempdir();
    let long: String = (0..40)
        .map(|i| format!("línea {i} con **negrita** y | tabla | {i} |\n"))
        .collect();
    let notices = [
        notice("waiting", "¿Puedo ejecutar `rm`?", "", "ComandOS", "%12"),
        notice(
            "done",
            "",
            "Listo.\n\n| a | b |\n|---|---|\n| 1 | 2 |",
            "",
            "",
        ),
        notice("done", "corto", &long, "Proy", "%9999999"),
        notice("waiting", "", "", "", "%abc"),
        notice("done", "  \n  primera  \nsegunda", "", "P", "12"),
        notice("done", "x", "   x   ", "P", ""),
        notice("raro", "b", &"y".repeat(17_000), "", "%1"),
        notice(
            "waiting",
            "b",
            "```\ncódigo\n```\n# Título\n- uno",
            "Z",
            "%3",
        ),
    ];
    let themes = repo_root().join("config/themes.json");
    let mut cases = Vec::new();
    for (i, n) in notices.iter().enumerate() {
        let lang = if i % 2 == 0 { "es" } else { "en" };
        let prefs = match i % 3 {
            0 => json!({"theme": "dia"}),
            1 => json!({"theme": "neon"}),
            _ => Value::Null,
        };
        cases.push(json!({
            "lang": lang,
            "prefs": prefs,
            "notice": {
                "title": n.title, "body": n.body, "session": n.session, "kind": n.kind,
                "project": n.project, "options": n.options, "full": n.full, "pane": n.pane,
            },
        }));
    }
    let Some(expected) = run_py(hooks.path(), POPUP_PY, &json!(cases)) else {
        return;
    };
    let raw = std::fs::read(&themes).unwrap();
    for (i, (n, e)) in notices.iter().zip(expected.as_array().unwrap()).enumerate() {
        let lang = if i % 2 == 0 { Lang::Es } else { Lang::En };
        let prefs = (!cases[i]["prefs"].is_null()).then_some(&cases[i]["prefs"]);
        let t = tokens(&themes_from_file(Some(&raw)), &theme_name(prefs)).unwrap();
        let m = popup_model(n, lang, &t);
        assert_eq!(model_json(&m, &t), *e, "aviso {i}");
    }
}

#[test]
fn evict_candidate_matches_python() {
    let hooks = tempdir();
    let stacks: Vec<Vec<(&str, f64)>> = vec![
        vec![],
        vec![("waiting", 5.0)],
        vec![("waiting", 3.0), ("waiting", 1.0), ("waiting", 2.0)],
        vec![
            ("waiting", 1.0),
            ("done", 4.0),
            ("done", 2.0),
            ("waiting", 0.5),
        ],
        vec![("done", 2.0), ("done", 2.0), ("raro", 1.0)],
        vec![("done", 9.0); 8],
    ];
    let src = r#"
NS = types.SimpleNamespace
res = []
for stack in INPUT:
    wins = [NS(_kind=k, _born=b) for k, b in stack]
    w = mod.evict_candidate(wins)
    res.append(None if w is None else wins.index(w))
out = res
"#;
    let Some(expected) = run_py(hooks.path(), src, &json!(stacks)) else {
        return;
    };
    let got: Vec<Option<usize>> = stacks
        .iter()
        .map(|s| {
            let metas: Vec<PopupMeta> = s
                .iter()
                .map(|(k, b)| PopupMeta {
                    kind: (*k).into(),
                    session: "s".into(),
                    pane: String::new(),
                    born: *b,
                })
                .collect();
            evict_candidate(&metas)
        })
        .collect();
    assert_eq!(json!(got), expected);
}

const LAYOUT_PY: &str = r#"
res = []
for case in INPUT:
    GEO[:] = case['geo']
    mod._notif_pos = (lambda m: (lambda: m))(case['mode'])
    mod.ANCHOR = tuple(case['anchor']) if case['anchor'] else None
    wins = []
    for h in case['heights']:
        w = W('Window'); w.height = h; wins.append(w)
    mod.popups[:] = wins
    seen = []
    mod._sync_clear_all = lambda x, y, up: seen.append([x, y, up])
    mod.reposition()
    res.append({'positions': [list(w._want) for w in wins], 'end': seen[0][:2], 'up': seen[0][2]})
out = res
"#;

#[test]
fn layout_matches_reposition() {
    let hooks = tempdir();
    let geos = [
        [0, 0, 1920, 1080],
        [1920, 30, 2560, 1440],
        [-1280, 0, 1280, 800],
    ];
    let modes = [
        json!("tl"),
        json!("tr"),
        json!("bl"),
        json!("br"),
        json!("free"),
        json!(5),
    ];
    let anchors = [
        None,
        Some([300, 200]),
        Some([99_999, -50]),
        Some([-99_999, 99_999]),
    ];
    let heights: [&[i64]; 3] = [&[], &[100], &[120, 80, 200]];
    let mut cases = Vec::new();
    for geo in geos {
        for mode in &modes {
            for anchor in anchors {
                for h in heights {
                    cases.push(json!({"geo": geo, "mode": mode, "anchor": anchor, "heights": h}));
                }
            }
        }
    }
    let Some(expected) = run_py(hooks.path(), LAYOUT_PY, &json!(cases)) else {
        return;
    };
    for (case, e) in cases.iter().zip(expected.as_array().unwrap()) {
        let g = case["geo"].as_array().unwrap();
        let geo = Geometry {
            x: g[0].as_i64().unwrap(),
            y: g[1].as_i64().unwrap(),
            width: g[2].as_i64().unwrap(),
            height: g[3].as_i64().unwrap(),
        };
        let anchor = case["anchor"]
            .as_array()
            .map(|a| (a[0].as_i64().unwrap(), a[1].as_i64().unwrap()));
        let hs: Vec<i64> = serde_json::from_value(case["heights"].clone()).unwrap();
        let l = layout(geo, case["mode"].as_str(), anchor, &hs);
        let got = json!({
            "positions": l.positions.iter().map(|(x, y)| json!([x, y])).collect::<Vec<_>>(),
            "end": [l.end.0, l.end.1],
            "up": l.up,
        });
        assert_eq!(got, *e, "{case}");
    }
}

#[test]
fn clear_all_position_matches_python() {
    let hooks = tempdir();
    let cases = json!([
        [100, 500, false, 20],
        [100, 500, true, 20],
        [5, 9, true, 60],
        [5, 9, false, 38]
    ]);
    let src = r#"
res = []
for x, y, up, h in INPUT:
    win = W('Window'); win.height = h
    mod.CLEAR_ALL.update(win=win, btn=W('Button'))
    mod.popups[:] = [W('Window'), W('Window')]
    mod._sync_clear_all(x, y, up)
    res.append(win.moves[-1])
out = res
"#;
    let Some(expected) = run_py(hooks.path(), src, &cases) else {
        return;
    };
    let got: Vec<Value> = cases
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            let (x, y) = clear_all_position(
                (c[0].as_i64().unwrap(), c[1].as_i64().unwrap()),
                c[2].as_bool().unwrap(),
                c[3].as_i64().unwrap(),
            );
            json!([x, y])
        })
        .collect();
    assert_eq!(json!(got), expected);
}

const CONFIGURE_PY: &str = r#"
res = []
for case in INPUT:
    TIMERS.clear()
    saved = []
    mod._anchor_save = lambda x, y: saved.append([x, y])
    mod.ANCHOR = None
    above = []
    for h in case['above']:
        w = W('Window'); w.height = h; above.append(w)
    win = W('Window')
    if case['want'] is not None: win._want = tuple(case['want'])
    else: win._want = None
    win._dragging = {'x': 1} if case['dragging'] else None
    if case['age'] is not None: win._drag_ts = time.time() - case['age']
    else: win._drag_ts = 0
    win._fix = case['fix']
    mod.popups[:] = above + [win]
    ev = types.SimpleNamespace(x=case['ev'][0], y=case['ev'][1])
    mod._on_configure(win, ev)
    if saved: res.append(['anchor', list(mod.ANCHOR)])
    elif TIMERS: res.append(['reassert', win._fix])
    else: res.append(['ignore', win._fix])
out = res
"#;

#[test]
fn on_configure_matches_python() {
    let hooks = tempdir();
    let mut cases = Vec::new();
    for want in [None, Some([100, 200])] {
        for dragging in [false, true] {
            for age in [None, Some(1.0), Some(30.0)] {
                for ev in [[100, 200], [102, 198], [103, 200], [100, 260]] {
                    for fix in [0, 4, 5] {
                        cases.push(json!({"want": want, "dragging": dragging, "age": age,
                                          "ev": ev, "fix": fix, "above": [120, 80]}));
                    }
                }
            }
        }
    }
    let Some(expected) = run_py(hooks.path(), CONFIGURE_PY, &json!(cases)) else {
        return;
    };
    for (case, e) in cases.iter().zip(expected.as_array().unwrap()) {
        let want = case["want"]
            .as_array()
            .map(|a| (a[0].as_i64().unwrap(), a[1].as_i64().unwrap()));
        let ev = (
            case["ev"][0].as_i64().unwrap(),
            case["ev"][1].as_i64().unwrap(),
        );
        let fix = case["fix"].as_u64().unwrap() as u32;
        let got = match on_configure(
            want,
            case["dragging"].as_bool().unwrap(),
            case["age"].as_f64(),
            ev,
            fix,
        ) {
            Configure::Anchor => {
                let (x, y) = anchor_from(ev, &[120, 80]);
                json!(["anchor", [x, y]])
            }
            Configure::Reassert => json!(["reassert", fix + 1]),
            Configure::Ignore => json!(["ignore", fix]),
        };
        assert_eq!(got, *e, "{case}");
    }
}

#[test]
fn anchor_file_matches_python() {
    let hooks = tempdir();
    let pos = hooks.path().join("notifyd-pos.json");
    // Lo que escribe el Python y lo que escribe el Rust, byte a byte.
    let Some(_) = python_eval(hooks.path(), &[], "mod._anchor_save(12, -34)") else {
        return;
    };
    let py_bytes = std::fs::read(&pos).unwrap();
    save_anchor(&pos, (12, -34)).unwrap();
    assert_eq!(std::fs::read(&pos).unwrap(), py_bytes);
    assert!(!hooks.path().join("notifyd-pos.json.tmp").exists());
    for body in [
        r#"{"x": 10, "y": 20}"#,
        r#"{"x": 1.9, "y": -2.7}"#,
        r#"{"x": "12", "y": " -3 "}"#,
        r#"{"x": "1_000", "y": "+7"}"#,
        r#"{"x": "0x10", "y": 1}"#,
        r#"{"x": true, "y": false}"#,
        r#"{"x": 1}"#,
        r#"[1, 2]"#,
        "garbage",
        r#"{"x": null, "y": 1}"#,
        r#"{"x": 1e400, "y": 1}"#,
        r#"{"x": NaN, "y": 1}"#,
        r#"{"y": 2, "x": 3, "x": 4}"#,
    ] {
        std::fs::write(&pos, body).unwrap();
        let expected = python_eval(hooks.path(), &[], "mod._anchor_load()").unwrap();
        let got = load_anchor(&pos)
            .map(|(x, y)| json!([x, y]))
            .unwrap_or(Value::Null);
        assert_eq!(got, expected, "{body}");
    }
    std::fs::remove_file(&pos).unwrap();
    assert_eq!(load_anchor(&pos), None);
}

#[test]
fn prefs_cache_follows_python_ttls() {
    let t0 = Instant::now();
    let mut cache = PrefsCache::default();
    assert!(cache.theme_stale(t0));
    assert!(cache.notif_pos_stale(t0));
    assert_eq!(cache.notif_pos(), Some("free"));
    // El tema que lanzó en el Python no se guarda: sigue caducado.
    cache.store_theme(t0, None);
    assert!(cache.theme_stale(t0));
    cache.store_theme(t0, Some(serde_json::Map::new()));
    assert!(!cache.theme_stale(t0 + THEME_TTL - Duration::from_millis(1)));
    assert!(cache.theme_stale(t0 + THEME_TTL));
    cache.mark_notif_pos(t0);
    assert!(!cache.notif_pos_stale(t0 + NOTIF_POS_TTL));
    assert!(cache.notif_pos_stale(t0 + NOTIF_POS_TTL + Duration::from_millis(1)));
    cache.store_notif_pos(Some(&json!({"notif_pos": "br"})));
    assert_eq!(cache.notif_pos(), Some("br"));
    // Fallo de /prefs o respuesta que no es objeto: se queda el valor anterior.
    cache.store_notif_pos(None);
    cache.store_notif_pos(Some(&json!([1])));
    assert_eq!(cache.notif_pos(), Some("br"));
    cache.store_notif_pos(Some(&json!({"notif_pos": ""})));
    assert_eq!(cache.notif_pos(), Some("free"));
    cache.store_notif_pos(Some(&json!({"notif_pos": 7})));
    assert_eq!(cache.notif_pos(), None);
    // Tras un arrastre: «libre» sin consultar durante 30 + 5 s.
    cache.pin_free(t0);
    assert_eq!(cache.notif_pos(), Some("free"));
    assert!(!cache.notif_pos_stale(t0 + Duration::from_secs(35)));
    assert!(cache.notif_pos_stale(t0 + Duration::from_secs(36)));
}

/// El cliente del tablero contra un servidor de la prueba (nunca el 4777).
#[test]
fn dash_client_get_and_post() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let mut seen = Vec::new();
        for reply in [
            "HTTP/1.0 200 OK\r\nContent-Type: application/json\r\n\r\n{\"theme\": \"dia\", \"notif_pos\": \"tl\"}",
            "HTTP/1.0 500 Internal\r\n\r\n{}",
            "HTTP/1.0 200 OK\r\n\r\n{\"ok\": true}",
            "HTTP/1.0 404 Not Found\r\n\r\n",
        ] {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = vec![0u8; 4096];
            let n = stream.read(&mut buf).unwrap();
            seen.push(String::from_utf8_lossy(&buf[..n]).into_owned());
            stream.write_all(reply.as_bytes()).unwrap();
        }
        seen
    });
    let dash = DashClient::parse(&format!("http://127.0.0.1:{port}")).unwrap();
    let timeout = Duration::from_secs(5);
    assert_eq!(
        dash.get_json("/prefs", timeout),
        Some(json!({"theme": "dia", "notif_pos": "tl"}))
    );
    assert_eq!(dash.get_json("/prefs", timeout), None);
    assert!(dash.post_json("/prefs-set", &json!({"notif_pos": "free"}), timeout));
    assert!(!dash.post_json("/prefs-set", &json!({}), timeout));
    let seen = server.join().unwrap();
    assert!(seen[0].starts_with("GET /prefs HTTP/1.0\r\n"));
    assert!(seen[2].starts_with("POST /prefs-set HTTP/1.0\r\n"));
    assert!(seen[2].ends_with("\r\n\r\n{\"notif_pos\":\"free\"}"));
    assert!(seen[2].contains("Content-Type: application/json\r\n"));
    // Sin nadie escuchando: falla rápido, sin colgarse.
    drop(dash);
    let closed = DashClient::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
    assert_eq!(closed.get_json("/prefs", Duration::from_millis(300)), None);
    assert_eq!(DashClient::parse("https://x"), None);
    assert_eq!(DashClient::parse("http://h:1/ruta"), None);
}

#[test]
fn close_all_label_and_texts() {
    use comandos_notifyd::model::close_all_label;
    assert_eq!(close_all_label(3, Lang::Es), "Cerrar todas · 3");
    assert_eq!(close_all_label(12, Lang::En), "Close all · 12");
}

/// Sin `--headless` y sin pantalla, GTK no arranca y el binario sale con 1.
/// Entorno vacío: ni `DISPLAY` ni `WAYLAND_DISPLAY`, `XDG_RUNTIME_DIR` propio
/// (el backend Wayland no encuentra `wayland-0`) y `GDK_BACKEND=x11`.
#[test]
fn gtk_mode_without_display_exits_with_error() {
    let home = tempdir();
    let runtime = home.path().join("run");
    std::fs::create_dir_all(&runtime).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_comandos-notifyd"))
        .args(["--port", "0", "--hooks-dir"])
        .arg(home.path())
        .args(["--dash-url", "http://127.0.0.1:9"])
        .env_clear()
        .env("HOME", home.path())
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("GDK_BACKEND", "x11")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("GTK no pudo arrancar"));
    assert!(out.stdout.is_empty(), "no anuncia el puerto sin popups");
}

/// Ruling: si Pango rechaza el markup, la etiqueta lleva el texto crudo
/// (el Python la dejaba en blanco). `parse_markup` no necesita pantalla.
#[test]
fn invalid_markup_falls_back_to_raw_text() {
    use comandos_notifyd::markup::{LabelText, valid_markup};
    let crossed = "mezcla **negrita `code** fin`";
    let got = blocks(crossed);
    assert_eq!(got.len(), 1);
    assert!(!valid_markup(got[0].markup()), "{}", got[0].markup());
    assert_eq!(got[0].label_text(), LabelText::Plain(crossed));
    let fine = blocks("**hola** y `x`");
    assert_eq!(
        fine[0].label_text(),
        LabelText::Markup("<b>hola</b> y <tt>x</tt>")
    );
    let table = blocks("| a | b |\n|---|---|\n| 1 | 2 |");
    assert!(matches!(table[0], Block::Table { .. }));
    assert!(matches!(table[0].label_text(), LabelText::Markup(_)));
    if let Block::Table { raw, .. } = &table[0] {
        assert_eq!(raw, "| a | b |\n|---|---|\n| 1 | 2 |");
    }
}

/// El checkout: `--repo-root`, luego la regla de `comandos dash`
/// (`COMANDOS_DASH_REPO` o `<hooks>/dash/index.html` resuelto) y el
/// ejecutable solo al final. Todo en directorios temporales.
#[test]
fn repo_root_follows_dash_rule() {
    use comandos_notifyd::dash::resolve_repo_root;
    let tmp = tempdir();
    let repo = tmp.path().join("checkout");
    std::fs::create_dir_all(repo.join("dash")).unwrap();
    std::fs::create_dir_all(repo.join("config")).unwrap();
    std::fs::write(repo.join("dash/index.html"), "<html>").unwrap();
    std::fs::write(repo.join("config/themes.json"), "{}").unwrap();
    let hooks = tmp.path().join("home/.claude/hooks");
    std::fs::create_dir_all(hooks.join("dash")).unwrap();
    std::os::unix::fs::symlink(repo.join("dash/index.html"), hooks.join("dash/index.html"))
        .unwrap();
    let release = tmp.path().join("share/comandos/bin");
    std::fs::create_dir_all(&release).unwrap();
    let exe = release.join("comandos-notifyd");
    std::fs::write(&exe, "").unwrap();
    let canon = std::fs::canonicalize(&repo).unwrap();

    let flag = tmp.path().join("flag");
    assert_eq!(
        resolve_repo_root(Some(&flag), Some("/env"), &hooks, Some(&exe)),
        Some(flag)
    );
    assert_eq!(
        resolve_repo_root(None, Some("/env"), &hooks, Some(&exe)),
        Some(PathBuf::from("/env"))
    );
    assert_eq!(
        resolve_repo_root(None, Some(""), &hooks, Some(&exe)),
        Some(canon.clone())
    );
    assert_eq!(
        resolve_repo_root(None, None, &hooks, Some(&exe)),
        Some(canon)
    );
    // Sin enlace del tablero: el ejecutable, como último recurso.
    let bare = tmp.path().join("bare/hooks");
    std::fs::create_dir_all(&bare).unwrap();
    assert_eq!(
        resolve_repo_root(None, None, &bare, Some(&exe)),
        Some(std::fs::canonicalize(tmp.path().join("share/comandos")).unwrap())
    );
    assert_eq!(resolve_repo_root(None, None, &bare, None), None);
}

/// Un solo escritor de `notifyd-pos.json`: gana la última posición.
#[test]
fn anchor_writer_keeps_last() {
    use comandos_notifyd::position::AnchorWriter;
    let tmp = tempdir();
    let file = tmp.path().join("notifyd-pos.json");
    let writer = AnchorWriter::spawn(file.clone()).unwrap();
    for i in 0..200 {
        writer.store((i, -i));
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    let want = r#"{"x": 199, "y": -199}"#;
    while std::fs::read_to_string(&file).ok().as_deref() != Some(want) {
        assert!(
            Instant::now() < deadline,
            "{:?}",
            std::fs::read_to_string(&file)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(load_anchor(&file), Some((199, -199)));
    assert!(!tmp.path().join("notifyd-pos.json.tmp").exists());
}
