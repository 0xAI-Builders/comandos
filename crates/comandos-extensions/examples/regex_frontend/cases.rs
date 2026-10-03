use serde_json::{Value, json};
pub const FAMILIES: &[(&str, usize)] = &[
    ("token", 48),
    ("ascii-escape", 256),
    ("unicode-escape", 48),
    ("octal-reference", 48),
    ("class", 64),
    ("group", 48),
    ("conditional", 64),
    ("conditional-int", 64),
    ("repeat", 64),
    ("repeat-int", 48),
    ("flags", 80),
    ("verbose", 48),
    ("width", 64),
    ("compiler-order", 32),
    ("admission", 32),
    ("format-schema", 40),
    ("budget-depth", 64),
];
fn add(rows: &mut Vec<Value>, family: &str, pattern: Vec<u32>, recipe: String) {
    let n = rows.iter().filter(|r| r["family"] == family).count();
    rows.push(json!({"id":format!("frontend/{family}/{n:03}"),"family":family,"pattern":pattern,"recipe":recipe,"op":"compile","source_prediction":null,"compare":["class","nullable_position","flags","groups","names","warnings"],"source_site":match family {"token"=>"Lib/re/_parser.py Tokenizer240-315,_parse_sub453-515,parse963-1004","ascii-escape"|"unicode-escape"|"octal-reference"=>"Lib/re/_parser.py _class_escape317-374,_escape376-451","class"=>"Lib/re/_parser.py _parse553-638","group"|"conditional"=>"Lib/re/_parser.py State77-109,_parse716-877","conditional-int"=>"Lib/re/_parser.py _parse796-838;Objects/longobject.c int conversion","repeat"|"repeat-int"=>"Lib/re/_parser.py _parse640-711","flags"=>"Lib/re/_parser.py _parse_flags902-961,fix_flags963-1004","verbose"=>"Lib/re/_parser.py _parse517-552,comment746-759","width"=>"Lib/re/_parser.py getwidth178-234;Lib/re/_compiler.py _compile assertion/repeat","compiler-order"=>"Lib/re/_compiler.py _compile37-183;Lib/re/_parser.py parse963-1004","format-schema"=>"jsonschema/_format.py regex416-420;selected validator check_schema","admission"=>"private validated emission policy;Lib/re/_compiler.py","budget-depth"=>"private protective budgets;Lib/re/_parser.py nested structural grammar",_=>unreachable!()}}));
}
fn seeds(rows: &mut Vec<Value>, f: &str, ss: &[&str]) {
    for s in ss {
        add(
            rows,
            f,
            s.chars().map(u32::from).collect(),
            format!("literal {s:?}"),
        );
    }
}
pub fn generate() -> Vec<Value> {
    let mut r = Vec::new();
    seeds(
        &mut r,
        "token",
        &[
            "",
            "abc",
            "|",
            "a||b",
            "(",
            ")",
            "a)",
            "a\\",
            "é\\",
            "😀\\",
            "\\(\\)\\|",
            "\0",
            "a|\\q",
            "\\q|a",
            "(?#x)\\",
            "(?x)#x\\",
        ],
    );
    for c in 0..128 {
        for inside in [false, true] {
            let mut p = vec![92, c];
            if inside {
                p.insert(0, 91);
                p.push(93);
            }
            add(
                &mut r,
                "ascii-escape",
                p,
                format!("ASCII {c} inside={inside}"),
            );
        }
    }
    for s in [
        "\\x00",
        "\\xFF",
        "\\x0",
        "\\xGG",
        "\\x000",
        "\\u0000",
        "\\uFFFF",
        "\\uD800",
        "\\uDFFF",
        "\\u123",
        "\\u12G4",
        "\\U00000000",
        "\\U0010FFFF",
        "\\U00110000",
        "\\U0000D800",
        "\\U0000DFFF",
        "\\U0000000",
        "\\U0000000G",
        "\\é",
        "\\😀",
        "\\N{LATIN CAPITAL LETTER A}",
        "\\N{LF}",
        "\\N{CJK UNIFIED IDEOGRAPH-4E00}",
        "\\N{HANGUL SYLLABLE GA}",
        "\\N{KEYCAP DIGIT ONE}",
        "\\N{MISSING}",
        "\\N{}",
        "\\N{ABC",
        "\\N",
        "\\N{A\0B}",
    ] {
        seeds(&mut r, "unicode-escape", &[s]);
        if s.starts_with("\\N") {
            seeds(&mut r, "unicode-escape", &[&format!("[{s}]")]);
        }
    }
    for inside in [false, true] {
        for cp in [0xd800, 0xdfff] {
            let mut p = vec![92, 78, 123, cp, 125];
            if inside {
                p.insert(0, 91);
                p.push(93);
            }
            add(
                &mut r,
                "unicode-escape",
                p,
                format!("surrogate name {cp} inside={inside}"),
            );
        }
        let mut p = vec![92, 78, 123];
        p.extend(vec![65; 257]);
        p.push(125);
        if inside {
            p.insert(0, 91);
            p.push(93);
        }
        add(
            &mut r,
            "unicode-escape",
            p,
            format!("overlength name inside={inside}"),
        );
    }
    for s in [
        "0", "00", "000", "07", "077", "100", "377", "400", "777", "8", "9", "11", "118", "123",
        "999",
    ] {
        seeds(
            &mut r,
            "octal-reference",
            &[&format!("\\{s}"), &format!("[\\{s}]")],
        );
    }
    for n in [1, 11, 99] {
        seeds(
            &mut r,
            "octal-reference",
            &[&format!("{}\\{n}", "()".repeat(n))],
        );
    }
    seeds(
        &mut r,
        "octal-reference",
        &["(\\1)", "()\\118", "()\\11", "()\\19"],
    );
    seeds(
        &mut r,
        "class",
        &[
            "[]",
            "[^]",
            "[]]",
            "[^]]",
            "[-]",
            "[a-]",
            "[-a]",
            "[a-z]",
            "[z-a]",
            "[a-a]",
            "[\\d-a]",
            "[a-\\d]",
            "[aaa]",
            "[😀-😁]",
            "[[]",
            "[a--b]",
            "[a&&b]",
            "[a~~b]",
            "[a||b]",
            "[\\[a]",
            "[a\\-\\-b]",
            "[a\\&\\&b]",
            "[a\\~\\~b]",
            "[a\\|\\|b]",
            "[z--a]",
            "[a-",
            "[",
            "[^a]",
            "[aa]",
            "[a-a-a]",
        ],
    );
    add(
        &mut r,
        "class",
        vec![91, 0xd800, 45, 0xdfff, 93],
        "surrogate range".into(),
    );
    seeds(
        &mut r,
        "group",
        &[
            "()",
            "(?:)",
            "(?i:a)",
            "(?P<a>x)(?P=a)",
            "(?P<a>)(?P<a>)",
            "(?P<a",
            "(?P<>)",
            "(?P=x)",
            "(?P)",
            "(?",
            "(?)",
            "(?Q)",
            "(?P<1>a)",
            "(?P<_>a)",
            "(?P<α>a)",
            "(?P<á>a)",
            "(?P<́a>a)",
            "(?P<Ⅰ>a)",
            "(?P<class>a)",
            "(?P<K>)(?P<K>)",
            "(?P<K>)(?P=K)",
            "(?P<𞓐>a)",
            "(?P<a>)(?P=)",
            "(?P<a>(?P=a))",
        ],
    );
    add(
        &mut r,
        "group",
        vec![40, 63, 80, 60, 0xd800, 62, 41],
        "surrogate name".into(),
    );
    seeds(
        &mut r,
        "conditional",
        &[
            "(a)(?(1)b|c)",
            "(?(1)a)()",
            "(?(2)a)()",
            "(?P<a>(?(a)b))",
            "(?(x)a)",
            "(?(0)a)",
            "(?(-0)a)",
            "(?(-1)a)",
            "(?(1.0)a)",
            "()(?(1)a|b|c)",
            "()(?(1)|)",
            "()(?(1))",
            "(?(2)a)(?(1)b)",
            "(a)(?<=(?(1)b|c))",
            "(?<=(a)(?(1)b|c))",
        ],
    );
    for s in [
        "1",
        "١",
        "１",
        "1١",
        "+1",
        "0001",
        "1_0",
        "1__0",
        "_1",
        "1_",
        " 1 ",
        "\u{a0}1\u{a0}",
        "\u{1c}1",
        "1\0",
        "²",
        "1073741822",
        "1073741823",
        "1073741824",
        "+0",
        "-000",
        "-1",
        "+_1",
        "1\u{2003}",
    ] {
        seeds(&mut r, "conditional-int", &[&format!("(?({s})a)()")]);
    }
    for n in [4300, 4301] {
        for ending in ["1", "X"] {
            let s = format!("{}{}", "0".repeat(n - 1), ending);
            seeds(&mut r, "conditional-int", &[&format!("(?({s})a)()")]);
        }
    }
    seeds(
        &mut r,
        "repeat",
        &[
            "a*", "a+", "a?", "a{}", "a{,}", "a{,2}", "a{2,}", "a{2}", "a{2,1}", "a{X}", "a{2X}",
            "a{2", "*", "+", "?", "{2}", "a**", "a*?", "a*+", "a+?", "a++", "a??", "a?+",
            "a{1,2}?", "a{1,2}+", "(?:a*)*", "(a*)*", "^*", "$+", "(?=a)*", "(?<=a)*", "a*?+",
            "a{,x}",
        ],
    );
    for n in [4294967294u64, 4294967295, 4294967296] {
        seeds(
            &mut r,
            "repeat-int",
            &[&format!("a{{{n}}}"), &format!("a{{0,{n}}}")],
        );
    }
    for n in [4299, 4300, 4301] {
        for c in ['0', '9'] {
            let s = c.to_string().repeat(n);
            seeds(
                &mut r,
                "repeat-int",
                &[
                    &format!("a{{{s}}}"),
                    &format!("a{{0,{s}}}"),
                    &format!("a{{{s}"),
                ],
            );
        }
    }
    seeds(
        &mut r,
        "repeat-int",
        &["a{00000001}", "a{4294967295,0}", "a{4294967295,X}"],
    );
    for c in ['i', 'm', 's', 'x', 'a', 'u', 't', 'L'] {
        seeds(
            &mut r,
            "flags",
            &[
                &format!("(?{c})a"),
                &format!("(?{c}:a)"),
                &format!("(?-{c}:a)"),
            ],
        );
    }
    seeds(
        &mut r,
        "flags",
        &[
            "(?i",
            "(?-",
            "(?i-",
            "(?i-)",
            "(?i-a:a)",
            "(?au)",
            "(?aiu:a)",
            "(?i-i:a)",
            "(?t:a)",
            "(?-t:a)",
            "(?iQ)",
            "(?iα)",
            "(?i!)",
            "(?i)(?m)a",
            "(?a)(?u)",
            "(?a)(?u))",
            "|(?i)a",
            "(?#x)(?i)a",
            "a(?i)",
            "((?i)a)",
            "(?i:a(?m))",
            "(?x) #x\n(?i)a",
        ],
    );
    for c in [' ', '\t', '\n', '\r', '\u{b}', '\u{c}', '\u{a0}', '\u{1c}'] {
        seeds(&mut r, "verbose", &[&format!("(?x)a{c}b")]);
    }
    seeds(
        &mut r,
        "verbose",
        &[
            "(?x)a\\ b",
            "(?x)a\\#b",
            "(?x)a\\\nb",
            "(?x)a#end",
            "(?x)a#x\nb",
            "(?x)a#x\rb",
            "(?x)[ #]",
            "(?x:a(?-x: b)c)",
            "(?#comment)",
            "(?#comment",
            "(?x)#\\",
            "(?x)#x\\\ny",
            "(?x) |a",
            "(?x)#x\n(?i)a",
        ],
    );
    seeds(
        &mut r,
        "width",
        &[
            "(?<=a|bc)x",
            "(?<=ab|cd)x",
            "(a)(?<=\\1)b",
            "(?<=(a)\\1)b",
            "(?=a*)",
            "(?<=a{4294967294})x",
            "(?<=a{4294967294}aa)x",
            "(?<=(?:a{4294967294}){4294967294})x",
            "(?<=(?:(?:a{4294967294}){4294967294}){2})x",
            "(?<=(?:){4294967294})x",
            "(?<=a*)",
            "(?<=)",
            "(?<=a?)",
            "()(?(1)a)",
            "()(?(1)a|bb)",
            "(?<=(?>ab))",
            "(?<=a{2}+)",
            "(?<=(?=a))",
            "(ab)(?<=(?(1)ab|cd))",
            "(?<=a{4294967295})",
            "(a)(?<=\\1)",
            "(?<=(?<=a)b)",
        ],
    );
    seeds(
        &mut r,
        "compiler-order",
        &[
            "(?t)a*",
            "(?t)a*?",
            "(?t)a*+",
            "(?t)(?<=a*)",
            "(?t)a*(?<=b+)",
            "(?t)(?<=b+)a*",
            "(?<=(?<=a*)b+)",
            "(?<=a+)\\q",
            "(?t)a*\\q",
            "(?",
            "(?)",
            "(?P)",
            "(?a)(?u)",
            "(?a)(?u))",
        ],
    );
    for s in [
        "abc",
        "a.c",
        "^a$",
        "\\Aabc\\Z",
        "(?:abc)",
        "(?#x)a",
        "[aa]",
        "(a)",
        "a|b",
        "[ab]",
        "a*",
        "\\d",
        "(?=a)",
        "(?>a)",
        "(?i)a",
        "(?m)^a",
        "(?s).",
        "(?a)a",
        "(?u)a",
        "(?t)a",
        "(?:a*)",
        "(?<=a+)",
        "(a)\\q",
    ] {
        seeds(&mut r, "admission", &[s]);
        let last = r.last_mut().unwrap();
        last["search"] = json!([97, 98, 99]);
    }
    seeds(
        &mut r,
        "format-schema",
        &[
            "a",
            "a|b",
            "(?<=a+)",
            "(?a)(?u)",
            "a{4294967295}",
            "\\q",
            "(?P<n>a)",
        ],
    );
    for depth in [0, 1, 127, 128, 129] {
        for (open, close) in [("(", ")"), ("(?i:", ")"), ("(?=", ")"), ("(?(1)", ")")] {
            let p = format!(
                "{}a{}{}",
                open.repeat(depth),
                close.repeat(depth),
                if open == "(?(1)" { "()" } else { "" }
            );
            add(
                &mut r,
                "budget-depth",
                p.chars().map(u32::from).collect(),
                format!("depth {depth} open {open}"),
            );
        }
    }
    seeds(
        &mut r,
        "budget-depth",
        &[&"a".repeat(16384), &"a|".repeat(512)],
    );
    for s in [
        "(?<=a{4294967294}a)x",
        "(?<=(?:)*)x",
        "(?<=(a?))x",
        "(a)(?<=(?(1)a))x",
    ] {
        seeds(&mut r, "width", &[s]);
    }
    for n in [4300, 4301] {
        seeds(
            &mut r,
            "conditional-int",
            &[&format!("(?({})a)()", "0".repeat(n))],
        );
    }
    for s in ["(?i-:a)", "(?i-a)", "(?i:a", "(?x)(?x:a)"] {
        seeds(&mut r, "flags", &[s]);
    }
    for value in [json!(null), json!(false), json!(42), json!([]), json!({})] {
        add(
            &mut r,
            "format-schema",
            vec![],
            format!("nonstring format {value}"),
        );
        let x = r.last_mut().unwrap();
        x["op"] = json!("format");
        x["value"] = value;
    }
    for (draft, declaration) in [
        ("draft4", "http://json-schema.org/draft-04/schema#"),
        ("draft6", "http://json-schema.org/draft-06/schema#"),
        ("draft7", "http://json-schema.org/draft-07/schema#"),
        (
            "draft201909",
            "https://json-schema.org/draft/2019-09/schema",
        ),
        (
            "draft202012",
            "https://json-schema.org/draft/2020-12/schema",
        ),
    ] {
        for nonstring in [false, true] {
            add(
                &mut r,
                "format-schema",
                vec![97],
                format!("selected schema {draft} nonstring={nonstring}"),
            );
            let x = r.last_mut().unwrap();
            x["op"] = json!("schema");
            x["draft"] = json!(draft);
            x["schema_json"] = json!(json!({"$schema":declaration,"pattern":"a"}).to_string());
            x["content_json"] = json!(if nonstring {
                "42".into()
            } else {
                json!("a").to_string()
            });
        }
    }
    for (schema, content, recipe) in [
        (
            json!({"$schema":"http://json-schema.org/draft-07/schema","pattern":"a"}),
            json!("b"),
            "registered alias",
        ),
        (
            json!({"$schema":"https://example.invalid/unknown","pattern":"a"}),
            json!("a"),
            "unknown declaration",
        ),
        (json!({"pattern":"a|b"}), json!("a"), "matcher feature"),
        (
            json!({"patternProperties":{"a":{}}}),
            json!({"a":1}),
            "schema scope",
        ),
    ] {
        add(&mut r, "format-schema", vec![97], recipe.into());
        let x = r.last_mut().unwrap();
        x["op"] = json!("schema");
        x["draft"] = json!("draft7");
        x["schema_json"] = json!(schema.to_string());
        x["content_json"] = json!(content.to_string());
    }
    for budget in [
        json!({"work":0}),
        json!({"memory":0}),
        json!({"words":0}),
        json!({"work":1}),
        json!({"memory":1}),
        json!({"words":1}),
    ] {
        add(
            &mut r,
            "budget-depth",
            vec![97],
            format!("synthetic native budget {budget}"),
        );
        r.last_mut().unwrap()["native_policy"] = budget;
    }
    for (recipe, p) in [
        ("lookup budget propagation", vec![92, 78, 123, 76, 70, 125]),
        ("node/edge capacity", vec![97; 16384]),
        ("word exact below/at/above", vec![97]),
        ("allocation exact below/at/above", vec![97]),
        ("work exact below/at/above", vec![97]),
        ("format zero words", vec![97, 124, 98]),
        ("synthetic reduced MAXGROUPS", vec![40, 41]),
    ] {
        add(&mut r, "budget-depth", p, recipe.into());
        r.last_mut().unwrap()["native_policy"] = json!({"calibrate":recipe});
    }
    for s in [
        "a",
        "a|b",
        "(?<=a+)",
        "(?a)(?u)",
        "a{4294967295}",
        "\\q",
        "(?P<n>a)",
    ] {
        add(
            &mut r,
            "format-schema",
            s.chars().map(u32::from).collect(),
            format!("string regex format {s}"),
        );
        let x = r.last_mut().unwrap();
        x["op"] = json!("format");
        x["value"] = json!(s);
    }
    for cp in [0xd800, 0xdfff] {
        add(&mut r, "token", vec![cp], format!("literal surrogate {cp}"));
        add(
            &mut r,
            "unicode-escape",
            vec![92, cp],
            format!("escaped surrogate {cp}"),
        );
    }
    for d in [0, 1, 127, 128, 129] {
        add(
            &mut r,
            "budget-depth",
            format!("{}a{}", "(?<=".repeat(d), ")".repeat(d))
                .chars()
                .map(u32::from)
                .collect(),
            format!("lookbehind depth {d}"),
        );
    }
    for &(family, cap) in FAMILIES {
        assert!(
            r.iter().filter(|x| x["family"] == family).count() <= cap,
            "{family}"
        );
    }
    assert!(r.len() <= 1112);
    r
}
