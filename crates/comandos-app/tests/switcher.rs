#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::ui::switcher::fuzzy_score;
#[test]
fn fuzzy_is_subsequence_with_gap_and_length_order() {
    assert_eq!(fuzzy_score("abc", "abcdef"), 6);
    assert_eq!(fuzzy_score("ac", "abcdef"), 106);
    assert_eq!(fuzzy_score("zc", "abcdef"), -1);
    assert_eq!(fuzzy_score("", "ñandú"), 5);
}
#[test]
fn help_rows_match_actual_original_ast() {
    use comandos_app::proc::{ProcSpec, run};
    let path = std::env::var("COMANDOS_CC_APP_ORACLE")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../bin/cc-app").into());
    let script = "import ast,json,sys\nns={}\nfor n in ast.parse(open(sys.argv[1]).read()).body:\n if isinstance(n,ast.Assign) and any(isinstance(t,ast.Name) and t.id in {'SHORTCUTS_ES','SHORTCUTS_EN'} for t in n.targets):exec(compile(ast.Module([n],[]),sys.argv[1],'exec'),ns)\nprint(json.dumps([ns['SHORTCUTS_ES'],ns['SHORTCUTS_EN']]))";
    let out = run(&ProcSpec {
        program: "python3".into(),
        args: vec!["-c".into(), script.into(), path.into()],
        stdin: None,
        env: vec![],
        clear_env: false,
        env_remove: vec![],
        cwd: None,
        timeout: std::time::Duration::from_secs(5),
    })
    .unwrap();
    assert_eq!(out.code, Some(0));
    let expected: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    for (i, english) in [false, true].into_iter().enumerate() {
        assert_eq!(
            serde_json::json!(comandos_app::ui::help::shortcuts(english)),
            expected[i]
        );
    }
}

#[test]
fn fuzzy_scores_and_status_palette_match_actual_original_ast() {
    use comandos_app::proc::{ProcSpec, run};
    let mut cases = Vec::new();
    for q in ["", "a", "ac", "ZZ", "á", "Ñ", "İ", "🇲🇽", "o x", "  "] {
        for name in [
            "alpha",
            "A Codex",
            "árbol",
            "Ñandú",
            "İstanbul",
            "other xyz",
            "🇲🇽 mexico",
            "",
        ] {
            cases.push((q, name));
        }
    }
    let out = run(&ProcSpec {
        program: "python3".into(),
        args: vec![
            "-c".into(),
            include_str!("support/t15_original.py").into(),
            std::env::var("COMANDOS_CC_APP_ORACLE")
                .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../bin/cc-app").into())
                .into(),
            "fuzzy".into(),
        ],
        stdin: Some(serde_json::to_vec(&cases).unwrap()),
        env: vec![],
        clear_env: false,
        env_remove: vec![],
        cwd: None,
        timeout: std::time::Duration::from_secs(5),
    })
    .unwrap();
    assert_eq!(
        out.code,
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let expected: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    for (i, (q, name)) in cases.iter().enumerate() {
        assert_eq!(
            serde_json::json!(fuzzy_score(q, name)),
            expected["scores"][i],
            "{q:?}/{name:?}"
        );
    }
    for (state, color) in expected["dots"].as_object().unwrap() {
        assert_eq!(
            comandos_app::ui::switcher::dot_color(state),
            color.as_str().unwrap()
        );
    }
    assert_eq!(
        comandos_app::ui::switcher::dot_color("unknown"),
        expected["idle"]
    );
}

#[test]
fn candidates_priority_hints_and_twelve_rows_match_original_nested_callbacks() {
    use comandos_app::{
        proc::{ProcSpec, run},
        ui::switcher::{Candidate, candidates, dot_color, hint, search},
    };
    use std::collections::BTreeMap;
    let open = vec![Candidate {
        key: "open".into(),
        label: "Zulu Open".into(),
        state: "waiting".into(),
        open: true,
    }];
    let mut items = BTreeMap::new();
    let mut states = BTreeMap::new();
    states.insert("open".into(), "waiting".into());
    for i in 0..20 {
        let k = format!("s{i:02}");
        let st = ["working", "waiting", "done", "idle"][i % 4];
        items.insert(
            k.clone(),
            serde_json::json!({"project":format!("Alpha {i}"),"status":st}),
        );
        states.insert(k, st.into());
    }
    for (k, v) in [
        ("local", serde_json::json!({})),
        ("term-orphan", serde_json::json!({})),
        ("dead", serde_json::json!({"alive":false})),
        ("zombie", serde_json::json!({"zombie":true})),
        (
            "term-agent",
            serde_json::json!({"agent":true,"project":"Agent"}),
        ),
        (
            "term-tabbed",
            serde_json::json!({"tabbed":true,"project":"Tabbed"}),
        ),
        ("open", serde_json::json!({"project":"duplicate"})),
    ] {
        items.insert(k.into(), v);
    }
    for english in [false, true] {
        for query in ["", "alp", "agent", "missing", " z "] {
            let input = serde_json::json!({"open":open.iter().map(|r|serde_json::json!({"key":r.key,"label":r.label})).collect::<Vec<_>>(),"items":items,"states":states,"query":query,"english":english});
            let out = run(&ProcSpec {
                program: "python3".into(),
                args: vec![
                    "-c".into(),
                    include_str!("support/t15_original.py").into(),
                    std::env::var("COMANDOS_CC_APP_ORACLE")
                        .unwrap_or_else(|_| {
                            concat!(env!("CARGO_MANIFEST_DIR"), "/../../bin/cc-app").into()
                        })
                        .into(),
                    "switcher".into(),
                ],
                stdin: Some(serde_json::to_vec(&input).unwrap()),
                env: vec![],
                clear_env: false,
                env_remove: vec![],
                cwd: None,
                timeout: std::time::Duration::from_secs(5),
            })
            .unwrap();
            assert_eq!(
                out.code,
                Some(0),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            let expected: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
            let native = search(query, &candidates(&open, &items, &states));
            let actual=serde_json::json!(native.iter().map(|r|serde_json::json!({"key":r.key,"label":r.label,"open":r.open,"hint":hint(&r.state,r.open,english),"dot":format!("<span size=\"9000\" foreground=\"{}\">●</span>",dot_color(&r.state))})).collect::<Vec<_>>());
            assert_eq!(actual, expected, "{english}/{query}");
        }
    }
}
