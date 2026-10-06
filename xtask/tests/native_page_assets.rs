use comandos_core::web_assets::{NATIVE_PAGE_FILE, NativePage, native_index_components};
#[test]
fn native_bundle_contains_existing_css_dependencies_and_pwa_without_legacy_scripts() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let files = xtask::web_build::native_page_files(root)
        .unwrap()
        .into_iter()
        .collect::<std::collections::BTreeMap<_, _>>();
    let page: NativePage = serde_json::from_slice(&files[NATIVE_PAGE_FILE]).unwrap();
    assert_eq!(page.components, native_index_components().unwrap());
    assert!(page.components.contains(&"analytics-inline".into()));
    assert!(
        !files
            .keys()
            .any(|p| p.ends_with(".js") || p.ends_with(".html"))
    );
    let regex = regex::Regex::new(r#"url\(['\"]?\./([^'\")]+)['\"]?\)"#).unwrap();
    for (name, bytes) in &files {
        if name.ends_with(".css") {
            let text = std::str::from_utf8(bytes).unwrap();
            assert!(
                !text.contains("url(/assets/")
                    && !text.contains("url('assets/")
                    && !text.contains("url(\"assets/")
            );
            for capture in regex.captures_iter(text) {
                assert!(
                    files.contains_key(&capture[1]),
                    "missing CSS dependency {}",
                    &capture[1]
                );
            }
        }
    }
    let pwa: serde_json::Value =
        serde_json::from_slice(&files[&page.assets["/manifest.webmanifest"]]).unwrap();
    assert_eq!(pwa["start_url"], "/?web=native");
    for icon in pwa["icons"].as_array().unwrap() {
        assert!(files.contains_key(icon["src"].as_str().unwrap().trim_start_matches("./")));
    }
}

#[test]
fn native_bundle_contains_every_switchable_pomodoro_sprite() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let files = xtask::web_build::native_page_files(root)
        .unwrap()
        .into_iter()
        .collect::<std::collections::BTreeMap<_, _>>();
    let page: NativePage = serde_json::from_slice(&files[NATIVE_PAGE_FILE]).unwrap();
    let catalog = comandos_web_view::pomodoro::catalog();
    let mut count = 0;
    for style in catalog["STYLES"].as_object().unwrap().values() {
        for asset in style["assets"].as_object().unwrap().values() {
            let url = format!("/assets/pomodoro/{}", asset["file"].as_str().unwrap());
            let logical = page
                .assets
                .get(&url)
                .unwrap_or_else(|| panic!("missing switchable sprite {url}"));
            assert!(
                files.contains_key(logical),
                "missing sprite bytes {logical}"
            );
            count += 1;
        }
    }
    assert_eq!(count, 36);
}

#[test]
fn terminal_bundle_uses_existing_fonts_and_dedicated_controller_inventory() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let files = xtask::native_term_page::files(root)
        .unwrap()
        .into_iter()
        .collect::<std::collections::BTreeMap<_, _>>();
    let page: NativePage =
        serde_json::from_slice(&files["comandos_native_term_page.json"]).unwrap();
    assert_eq!(page.components, ["term-main", "term-tail"]);
    assert_eq!(page.assets.len(), 6);
    for (url, name) in &page.assets {
        assert!(files.contains_key(name));
        if url.ends_with(".ttf") {
            assert!(files[name].len() > 10000);
        }
    }
    assert!(
        !files
            .keys()
            .any(|name| name.ends_with(".js") || name.ends_with(".html"))
    );
}
