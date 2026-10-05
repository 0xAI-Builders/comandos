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
