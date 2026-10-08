use comandos_server::dash::web::markdown::{Profile, render};

#[test]
fn commented_script_does_not_hide_the_actual_script() {
    let entry = comandos_server::dash::web::registry::Entry::script(
        "vendor-purify",
        "dash/vendor/purify-3.4.16.min.js",
        "",
        &[],
        &[],
    );
    let comment = "<!-- <script src='/vendor/purify-3.4.16.min.js'></script> -->";
    assert_eq!(
        entry.cut(&format!(
            "{comment}<script src='/vendor/purify-3.4.16.min.js'></script>"
        )),
        comment
    );
    let unclosed = "<!-- <script src='/vendor/purify-3.4.16.min.js'></script>";
    assert_eq!(entry.cut(unclosed), unclosed);
}

#[test]
fn independent_original_cases_preserve_destinations_and_markup() {
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "../../../xtask/web/fixtures/b8/review-baselines.json"
    ))
    .unwrap();
    for row in cases["cases"].as_array().unwrap() {
        let text = row["text"].as_str().unwrap();
        let baseline = row["baseline"].as_str().unwrap();
        let actual = render(text, Profile::News);
        assert_eq!(
            comandos_domdiff::first_difference(baseline, &actual),
            None,
            "{text}: {actual}"
        );
    }
}

#[test]
fn raw_html_paragraphs_and_soft_breaks_match_original_exactly() {
    assert_eq!(
        render("<script>\nalert(1)\n</script>", Profile::News),
        "<p>&lt;script&gt;\nalert(1)\n&lt;/script&gt;</p>\n"
    );
    assert_eq!(
        render("~~gone~~ ~single~", Profile::News),
        "<p><s>gone</s> ~single~</p>\n"
    );
}

#[test]
fn additional_original_grammar_preserves_code_links_and_literal_masks() {
    let cases: serde_json::Value =
        serde_json::from_str(&comandos_web_view::utf16::json_to_unicode(include_str!(
            "../../../xtask/web/fixtures/b8/repair-grammar.json"
        )))
        .unwrap();
    let mut differences = Vec::new();
    for row in cases["cases"].as_array().unwrap() {
        let source = row["text"].as_str().unwrap();
        let html = render(source, Profile::News);
        if let Some(difference) =
            comandos_domdiff::first_difference(row["baseline"].as_str().unwrap(), &html)
        {
            differences
                .push(serde_json::json!({"text":source,"difference":difference,"candidate":html}));
        }
    }
    println!(
        "additional grammar differences: {}",
        serde_json::to_string(&differences).unwrap()
    );
    assert!(differences.is_empty(), "{differences:?}");
}

#[test]
fn independent_entity_masks_preserve_original_text_and_destinations() {
    let cases: serde_json::Value =
        serde_json::from_str(&comandos_web_view::utf16::json_to_unicode(include_str!(
            "../../../xtask/web/fixtures/b8/review-v2-baselines.json"
        )))
        .unwrap();
    let mut differences = Vec::new();
    for row in cases["cases"].as_array().unwrap() {
        let source = row["text"].as_str().unwrap();
        let candidate = render(source, Profile::News);
        let difference =
            comandos_domdiff::first_difference(row["baseline"].as_str().unwrap(), &candidate);
        if let Some(difference) = difference {
            differences.push(
                serde_json::json!({"text":source,"difference":difference,"candidate":candidate}),
            );
        }
    }
    assert!(differences.is_empty(), "{differences:?}");
}

#[test]
fn nul_normalization_matches_original_in_all_markdown_contexts() {
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "../../../xtask/web/fixtures/b8/review-nul-baselines.json"
    ))
    .unwrap();
    for row in cases["cases"].as_array().unwrap() {
        let text = row["text"].as_str().unwrap();
        let actual = render(text, Profile::News);
        assert_eq!(
            comandos_domdiff::first_difference(row["baseline"].as_str().unwrap(), &actual),
            None,
            "{}: {actual}",
            row["id"]
        );
    }
}

#[test]
fn unicode_href_normalization_preserves_original_bytes() {
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "../../../xtask/web/fixtures/b8/review-unicode-href-baselines.json"
    ))
    .unwrap();
    let differences = cases["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|row| {
            let candidate = render(row["text"].as_str().unwrap(), Profile::News);
            comandos_domdiff::first_difference(row["baseline"].as_str().unwrap(), &candidate)
                .map(|difference| serde_json::json!({"id":row["id"],"difference":difference}))
        })
        .collect::<Vec<_>>();
    assert!(differences.is_empty(), "{differences:?}");
}

#[test]
fn numeric_entity_url_boundaries_match_original_context() {
    let cases: serde_json::Value =
        serde_json::from_str(&comandos_web_view::utf16::json_to_unicode(include_str!(
            "../../../xtask/web/fixtures/b8/review-v3-baselines.json"
        )))
        .unwrap();
    let differences = cases["cases"].as_array().unwrap().iter().filter_map(|row| {
        let source = row["text"].as_str().unwrap();
        let candidate = render(source, Profile::News);
        comandos_domdiff::first_difference(row["baseline"].as_str().unwrap(), &candidate)
            .map(|difference| serde_json::json!({"text":source,"candidate":candidate,"difference":difference}))
    }).collect::<Vec<_>>();
    assert!(differences.is_empty(), "{differences:?}");
}

#[test]
fn unicode_autolink_delimiters_match_original_without_swallowing_suffixes() {
    let cases: serde_json::Value =
        serde_json::from_str(&comandos_web_view::utf16::json_to_unicode(include_str!(
            "../../../xtask/web/fixtures/b8/review-v4-baselines.json"
        )))
        .unwrap();
    let differences = cases["cases"].as_array().unwrap().iter().filter_map(|row| {
        let source = row["text"].as_str().unwrap();
        let candidate = render(source, Profile::News);
        comandos_domdiff::first_difference(row["baseline"].as_str().unwrap(), &candidate)
            .map(|difference| serde_json::json!({"text":source,"candidate":candidate,"difference":difference}))
    }).collect::<Vec<_>>();
    assert!(differences.is_empty(), "{differences:?}");
}
