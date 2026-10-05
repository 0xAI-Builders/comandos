use comandos_web_dom::i18n::{Lang, lang, lang_from_conf, set_lang, tf};
use serde_json::json;

#[test]
fn lang_comes_from_conf_like_the_inline_boot() {
    // `if(c._lang === "en") L = "en";` y si no, queda "es".
    assert_eq!(lang_from_conf(&json!({"_lang": "en"})), Lang::En);
    assert_eq!(lang_from_conf(&json!({"_lang": "EN"})), Lang::Es);
    assert_eq!(lang_from_conf(&json!({"_lang": "es"})), Lang::Es);
    assert_eq!(lang_from_conf(&json!({})), Lang::Es);
    assert_eq!(lang_from_conf(&json!(null)), Lang::Es);
}

#[test]
fn tf_picks_by_current_lang() {
    assert_eq!(lang(), Lang::Es, "por omisión, como `let L = \"es\"`");
    assert_eq!(tf("hola", "hello"), "hola");
    set_lang(Lang::En);
    assert_eq!(tf("hola", "hello"), "hello");
    assert_eq!(Lang::En.code(), "en");
    set_lang(Lang::Es);
}

#[test]
fn t_translates_with_t_en_like_the_inline_t() {
    use comandos_web_dom::i18n::t;
    // `const t = s => L === "en" ? (T_EN[s] ?? s) : s;`
    set_lang(Lang::Es);
    assert_eq!(t("Copiar"), "Copiar");
    set_lang(Lang::En);
    assert_eq!(t("Copiar"), "Copy");
    assert_eq!(t("Servidores SSH"), "SSH servers");
    assert_eq!(t("sin traducción"), "sin traducción");
    assert_eq!(
        t(
            "Viven en ~/.ssh/config: estandar y tuyo. Conectar abre una\n    sesion tmux reconectable. Para dejar de teclear passwords: corre cc-keys una vez."
        ),
        "They live in ~/.ssh/config: standard and yours. Connect opens a reconnectable tmux session. To stop typing passwords: run cc-keys once."
    );
    set_lang(Lang::Es);
}

/// Cadenas JS del bloque `const T_EN = {…};`, en orden: comillas dobles,
/// simples y plantillas sin huecos, con los escapes `\n`, `\t`, `\uXXXX`,
/// `\u{…}` y el carácter escapado tal cual. Una plantilla con `${` hace fallar
/// la prueba (no se puede comparar).
fn js_strings(block: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut it = block.chars().peekable();
    while let Some(q) = it.next() {
        if !matches!(q, '"' | '\'' | '`') {
            continue;
        }
        let mut s = String::new();
        while let Some(c) = it.next() {
            match c {
                c if c == q => break,
                '$' if q == '`' && it.peek() == Some(&'{') => panic!("plantilla con hueco en T_EN"),
                '\\' => match it.next() {
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some('u') => {
                        let hex: String = if it.peek() == Some(&'{') {
                            it.next();
                            it.by_ref().take_while(|c| *c != '}').collect()
                        } else {
                            it.by_ref().take(4).collect()
                        };
                        let n = u32::from_str_radix(&hex, 16).expect("\\u válido");
                        s.push(char::from_u32(n).expect("escalar"));
                    }
                    Some(o) => s.push(o),
                    None => {}
                },
                o => s.push(o),
            }
        }
        out.push(s);
    }
    out
}

#[test]
fn js_strings_reads_every_quote_style() {
    let got = js_strings(r#"{ "a\"b": 'c\'d', `e`: "\u00f1\u{1F345}\n" }"#);
    assert_eq!(got, ["a\"b", "c'd", "e", "ñ🍅\n"]);
}

#[test]
fn t_en_matches_the_inline_table() {
    // Vigilancia de deriva: la tabla Rust es la de `dash/index.html`, y `t`/`tf`
    // siguen siendo las funciones que copia `i18n`.
    let html = include_str!("../../../dash/index.html");
    assert!(html.contains("\nconst t = s => L === \"en\" ? (T_EN[s] ?? s) : s;\n"));
    assert!(html.contains("\nconst tf = (es, en) => L === \"en\" ? en : es;\n"));
    assert!(html.contains("\nlet L = \"es\";\n"));
    let start = html.find("const T_EN = {").expect("T_EN en index.html");
    let rest = &html[start..];
    let block = &rest[..rest.find("\n};").expect("fin de T_EN")];
    let lits = js_strings(&block[block.find('{').unwrap()..]);
    assert_eq!(lits.len() % 2, 0);
    let js: Vec<(&str, &str)> = lits
        .chunks(2)
        .map(|p| (p[0].as_str(), p[1].as_str()))
        .collect();
    assert_eq!(comandos_web_dom::i18n::T_EN, js.as_slice());
}
