use comandos_server::dash::web::markdown::{Profile, render};
#[test]
fn original_uri_provenance_baselines() {
    let rows: serde_json::Value = serde_json::from_str(&comandos_web_view::utf16::json_to_unicode(
        include_str!("../../../xtask/web/fixtures/b8/uri-source-provenance-baselines.json"),
    ))
    .unwrap();
    let mut differences = Vec::new();
    for row in rows["cases"].as_array().unwrap().iter() {
        let text = row["text"].as_str().unwrap();
        let expected = row["baseline"].as_str().unwrap();
        let actual = render(text, Profile::News);
        if let Some(difference) = comandos_domdiff::first_difference(expected, &actual) {
            differences.push(format!("{text:?}: {difference}"));
        }
    }
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

#[test]
fn original_uri_label_provenance_baselines() {
    let rows: serde_json::Value = serde_json::from_str(&comandos_web_view::utf16::json_to_unicode(
        include_str!("../../../xtask/web/fixtures/b8/uri-label-provenance-baselines.json"),
    ))
    .unwrap();
    let mut differences = Vec::new();
    for row in rows["cases"].as_array().unwrap() {
        let text = row["text"].as_str().unwrap();
        let expected = row["baseline"].as_str().unwrap();
        let actual = render(text, Profile::News);
        if let Some(difference) = comandos_domdiff::first_difference(expected, &actual) {
            differences.push(format!("{text:?}: {difference}"));
        }
    }
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

#[test]
fn original_browser_uri_provenance_baselines() {
    let rows: serde_json::Value = serde_json::from_str(&comandos_web_view::utf16::json_to_unicode(
        include_str!("../../../xtask/web/fixtures/b8/uri-browser-provenance-baselines.json"),
    ))
    .unwrap();
    let mut differences = Vec::new();
    for row in rows["cases"].as_array().unwrap() {
        let text = row["text"].as_str().unwrap();
        let expected = row["original"].as_str().unwrap();
        let actual = render(text, Profile::News);
        if let Some(difference) = comandos_domdiff::first_difference(expected, &actual) {
            differences.push(format!("{text:?}: {difference}"));
        }
    }
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

#[test]
fn all_original_legacy_uri_baselines() {
    let rows: serde_json::Value = serde_json::from_str(&comandos_web_view::utf16::json_to_unicode(
        include_str!("../../../xtask/web/fixtures/b8/uri-provenance-baselines.json"),
    ))
    .unwrap();
    let browser: serde_json::Value =
        serde_json::from_str(&comandos_web_view::utf16::json_to_unicode(include_str!(
            "../../../xtask/web/fixtures/b8/uri-browser-legacy-baselines.json"
        )))
        .unwrap();
    let mut differences = Vec::new();
    for row in rows["cases"].as_array().unwrap() {
        let text = row["text"].as_str().unwrap();
        let expected = browser["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["text"] == row["text"])
            .map_or_else(
                || row["baseline"].as_str().unwrap(),
                |r| r["original"].as_str().unwrap(),
            );
        let actual = render(text, Profile::News);
        if let Some(difference) = comandos_domdiff::first_difference(expected, &actual) {
            differences.push(format!("{text:?}: {difference}"));
        }
    }
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

#[test]
fn reviewer_all_36_node_cases() {
    let rows: serde_json::Value = serde_json::from_str(&comandos_web_view::utf16::json_to_unicode(
        include_str!("../../../xtask/web/fixtures/b8/uri-review36-node-baselines.json"),
    ))
    .unwrap();
    let mut differences = Vec::new();
    for row in rows["cases"].as_array().unwrap().iter() {
        let actual = render(row["text"].as_str().unwrap(), Profile::News);
        if let Some(difference) =
            comandos_domdiff::first_difference(row["originalNode"].as_str().unwrap(), &actual)
        {
            differences.push(format!("{}: {difference}", row["id"]));
        }
    }
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

#[test]
fn reviewer_all_36_browser_cases() {
    let rows: serde_json::Value = serde_json::from_str(&comandos_web_view::utf16::json_to_unicode(
        include_str!("../../../xtask/web/fixtures/b8/uri-review36-browser-baselines.json"),
    ))
    .unwrap();
    let mut differences = Vec::new();
    for row in rows["cases"].as_array().unwrap() {
        let actual = render(row["text"].as_str().unwrap(), Profile::News);
        if let Some(difference) =
            comandos_domdiff::first_difference(row["original"].as_str().unwrap(), &actual)
        {
            differences.push(format!("{}: {difference}", row["id"]));
        }
    }
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}
