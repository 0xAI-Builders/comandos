//! Shared dashboard theme parsing plus desktop-only CSS.
use comandos_core::pomodoro::python_str;
use serde_json::{Map, Value};

pub use comandos_notifyd::theme::{themes_from_file, tokens};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeTokens {
    pub values: Map<String, Value>,
    pub ansi: Vec<String>,
}

const PALETTE: [&str; 16] = [
    "#15161E", "#F7768E", "#9ECE6A", "#E0AF68", "#7AA2F7", "#BB9AF7", "#7DCFFF", "#A9B1D6",
    "#414868", "#F7768E", "#9ECE6A", "#E0AF68", "#7AA2F7", "#BB9AF7", "#7DCFFF", "#C0CAF5",
];
const PAL_DIA: [&str; 16] = [
    "#1F2328", "#C33846", "#2F7D32", "#9A6700", "#1D55C7", "#8250DF", "#1B7C83", "#D0D7DE",
    "#57606A", "#D1242F", "#1A7F37", "#BF8700", "#0969DA", "#A371F7", "#3192AA", "#F6F8FA",
];
const PAL_CALIDO: [&str; 16] = [
    "#20130A", "#D86A4B", "#7FA35A", "#E0A458", "#5D8AA8", "#B47EB3", "#6CA6A6", "#F2E5D0",
    "#8A7A5F", "#E07755", "#9DBB73", "#FFB454", "#79A6C9", "#C998C8", "#8CC9C9", "#FFF1D6",
];
const PAL_BRUNO: [&str; 16] = [
    "#1A1A1A", "#CC6666", "#B5BD68", "#F0C674", "#81A2BE", "#B294BB", "#8ABEB7", "#CCCCCC",
    "#666666", "#D54E53", "#B9CA4A", "#E7C547", "#7AA6DA", "#C397D8", "#70C0B1", "#FFFFFF",
];
const PAL_UBUNTU: [&str; 16] = [
    "#2E3436", "#CC0000", "#4E9A06", "#C4A000", "#3465A4", "#75507B", "#06989A", "#D3D7CF",
    "#555753", "#EF2929", "#8AE234", "#FCE94F", "#729FCF", "#AD7FA8", "#34E2E2", "#EEEEEC",
];

pub fn desktop_theme(name: &str, themes: &Value) -> Option<ThemeTokens> {
    let values = tokens(themes, &Value::String(name.to_string()))?;
    Some(ThemeTokens {
        ansi: ansi_for(name).iter().map(|s| (*s).to_string()).collect(),
        values,
    })
}

pub fn theme_css(theme: &ThemeTokens) -> String {
    let t = &theme.values;
    let mut css = header_css(theme);
    css.push_str(&format!(
        "
window {{ background-color: {}; color: {}; }}
.terminal {{ background-color: {}; color: {}; }}
.selection {{ background-color: {}; color: {}; }}
",
        token(t, "bg"),
        token(t, "text"),
        token(t, "bg"),
        token(t, "fg"),
        token(t, "brand"),
        token(t, "bg")
    ));
    css
}

pub fn header_css(theme: &ThemeTokens) -> String {
    let t = &theme.values;
    format!(
        "notebook > header {{ background: {}; border: none; box-shadow: none; }}\
.tabstrip {{ background: {}; border-bottom: 1px solid {}; min-height: 44px; }}\
.strip-tab {{ background-color: {}; color: {}; }}\
.strip-tab:hover,.strip-tab.cur {{ color: {}; }}\
.strip-tab.cur {{ background-color: mix({}, {}, 0.35); box-shadow: inset 0 -2px 0 0 {}; }}",
        token(t, "bar"),
        token(t, "bar"),
        token(t, "line"),
        token(t, "bar"),
        token(t, "dim"),
        token(t, "text"),
        token(t, "bar"),
        token(t, "line2"),
        token(t, "brand")
    )
}

pub fn button_style_css(style: &str, theme: &ThemeTokens) -> String {
    let t = &theme.values;
    let line = token(t, "line");
    let text = token(t, "text");
    let dim = token(t, "dim");
    let bar = token(t, "bar");
    let face = t.get("line2").map_or_else(|| line.clone(), python_str);
    let edge = t
        .get("bg")
        .map_or_else(|| "#0A0D13".to_string(), python_str);
    let sel = "button.cc-key";
    let base = format!(
        "{sel}{{min-width:34px;min-height:34px;padding:0;margin:4px 3px 6px;background-image:none;background-color:{face};color:{text};border:0;}}"
    );
    let key = match style {
        "arcade" | "tecla" | "pixel" | "consola" => style,
        _ => "sutil",
    };
    let specific = match key {
        "arcade" => format!(
            "{sel}{{border-radius:999px;background-image:radial-gradient(circle at 50% 30%,shade({face},1.45),{face} 65%);box-shadow:0 0 0 3px {edge},0 5px 0 3px {edge},inset 0 -3px 0 rgba(0,0,0,0.25);}}{sel}:hover{{background-image:radial-gradient(circle at 50% 30%,shade({face},1.6),shade({face},1.1) 65%);}}{sel}:active{{margin-top:8px;margin-bottom:2px;box-shadow:0 0 0 3px {edge},0 1px 0 3px {edge};}}"
        ),
        "tecla" => format!(
            "{sel}{{border-radius:10px;background-image:linear-gradient(to bottom,shade({face},1.25),{face});box-shadow:inset 0 1px 0 rgba(255,255,255,0.14),0 5px 0 {edge};}}{sel}:active{{margin-top:9px;margin-bottom:1px;box-shadow:inset 0 1px 0 rgba(255,255,255,0.1);}}"
        ),
        "pixel" => format!(
            "{sel}{{border-radius:0;border:3px solid #000;box-shadow:inset 3px 3px 0 rgba(255,255,255,0.16),inset -3px -3px 0 rgba(0,0,0,0.3),4px 4px 0 rgba(0,0,0,0.55);}}{sel}:active{{margin-top:8px;margin-bottom:2px;box-shadow:inset 3px 3px 0 rgba(0,0,0,0.3);}}"
        ),
        "consola" => format!(
            "{sel}{{border-radius:12px;color:#ffffff;background-image:linear-gradient(to bottom,shade({face},1.35),{face});box-shadow:0 5px 0 {edge},inset 0 2px 0 rgba(255,255,255,0.3);}}{sel}:active{{margin-top:9px;margin-bottom:1px;box-shadow:inset 0 2px 0 rgba(255,255,255,0.2);}}"
        ),
        _ => format!(
            "{sel}{{border-radius:10px;background-color:{bar};border:1px solid {line};color:{dim};box-shadow:0 3px 0 {line};}}{sel}:hover{{color:{text};}}{sel}:active{{margin-top:6px;margin-bottom:4px;box-shadow:none;}}"
        ),
    };
    let gen_sel = "button:not(.cc-key):not(.cc-winctl):not(.titlebutton):not(.tabfav):not(.tabclose):not(.tab-suggest):not(.tabnav):not(.side-btn)";
    let generic = match key {
        "arcade" => format!(
            "{gen_sel}{{border:0;border-radius:999px;background-image:radial-gradient(circle at 50% 30%,shade({face},1.4),{face} 65%);box-shadow:0 0 0 2px {edge},0 3px 0 2px {edge};}}{gen_sel}:active,{gen_sel}:checked{{box-shadow:0 0 0 2px {edge},inset 0 3px 0 rgba(0,0,0,0.35);}}"
        ),
        "tecla" => format!(
            "{gen_sel}{{border:0;border-radius:8px;background-image:linear-gradient(to bottom,shade({face},1.25),{face});box-shadow:inset 0 1px 0 rgba(255,255,255,0.14),0 3px 0 {edge};}}{gen_sel}:active,{gen_sel}:checked{{box-shadow:inset 0 3px 0 rgba(0,0,0,0.35);}}"
        ),
        "pixel" => format!(
            "{gen_sel}{{border:2px solid #000;border-radius:0;background-image:none;background-color:{face};box-shadow:inset 2px 2px 0 rgba(255,255,255,0.16),inset -2px -2px 0 rgba(0,0,0,0.3),3px 3px 0 rgba(0,0,0,0.55);}}{gen_sel}:active,{gen_sel}:checked{{box-shadow:inset 3px 3px 0 rgba(0,0,0,0.35);}}"
        ),
        "consola" => format!(
            "{gen_sel}{{border:0;border-radius:12px;background-image:linear-gradient(to bottom,shade({face},1.35),{face});box-shadow:0 3px 0 {edge},inset 0 2px 0 rgba(255,255,255,0.25);}}{gen_sel}:active,{gen_sel}:checked{{box-shadow:inset 0 3px 0 rgba(0,0,0,0.35);}}"
        ),
        _ => format!(
            "{gen_sel}{{border:1px solid {line};box-shadow:0 2px 0 {face};}}{gen_sel}:active,{gen_sel}:checked{{box-shadow:inset 0 2px 0 rgba(0,0,0,0.3);}}"
        ),
    };
    format!(
        "{base}{specific}{generic}scrollbar button{{border:0;box-shadow:none;background-image:none;}}"
    )
}

fn token(t: &Map<String, Value>, key: &str) -> String {
    t.get(key).map_or_else(String::new, python_str)
}

fn ansi_for(name: &str) -> &'static [&'static str; 16] {
    match name {
        "dia" => &PAL_DIA,
        "calido" => &PAL_CALIDO,
        "bruno" => &PAL_BRUNO,
        "ubuntu" => &PAL_UBUNTU,
        _ => &PALETTE,
    }
}
