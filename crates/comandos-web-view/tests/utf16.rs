use comandos_web_view::utf16::{decode, encode, json_to_javascript, json_to_unicode};

#[test]
fn all_utf16_units_and_private_use_collisions_roundtrip() {
    for unit in 0..=u16::MAX {
        assert_eq!(decode(&encode([unit])), [unit]);
    }
    let units = vec![
        0x61, 0xd800, 0x62, 0xdc00, 0xd83c, 0xdf45, 0xe000, 0xdb80, 0xdc00,
    ];
    assert_eq!(decode(&encode(units.clone())), units);
}

#[test]
fn json_keeps_keys_fields_literals_pairs_and_lone_surrogates() {
    let source = r#"{"\ud800":"prefix\udc00suffix","literal":"\\ud800","paired":"\ud83c\udf45","clock":2000}"#;
    let unicode = json_to_unicode(source);
    let model: serde_json::Value = serde_json::from_str(&unicode).unwrap();
    assert_eq!(model["clock"], 2000);
    let serialized = json_to_javascript(&model.to_string());
    assert_eq!(
        serialized,
        source.replace(r#""paired":"\ud83c\udf45""#, "\"paired\":\"🍅\"")
    );
}
