//! Static terminal assets and descriptor packaged with its own WASM artifact.
use comandos_core::web_assets::NativePage;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path};
pub fn files(root: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut assets = BTreeMap::new();
    let mut files = Vec::new();
    for url in [
        "/buttons.css",
        "/icon-192.png",
        "/assets/fonts/JetBrainsMono/JetBrainsMonoNerdFontMono-Regular.ttf",
        "/assets/fonts/JetBrainsMono/JetBrainsMonoNerdFontMono-Italic.ttf",
        "/assets/fonts/JetBrainsMono/JetBrainsMonoNerdFontMono-Bold.ttf",
        "/assets/fonts/JetBrainsMono/JetBrainsMonoNerdFontMono-BoldItalic.ttf",
    ] {
        let rel = url.trim_start_matches('/');
        let source = if rel.starts_with("assets/") {
            root.join(rel)
        } else {
            root.join("dash").join(rel)
        };
        let bytes =
            std::fs::read(source).map_err(|e| format!("native terminal asset {url}: {e}"))?;
        let name = format!(
            "native-term-{}-{}",
            &format!("{:x}", Sha256::digest(url.as_bytes()))[..12],
            rel.rsplit('/').next().unwrap()
        );
        assets.insert(url.into(), name.clone());
        files.push((name, bytes));
    }
    let page = NativePage {
        version: 1,
        template_sha256: format!(
            "{:x}",
            Sha256::digest(
                comandos_web_view::term_page::shell(&Default::default())
                    .into_string()
                    .as_bytes()
            )
        ),
        components: vec!["term-main".into(), "term-tail".into()],
        assets,
    };
    files.push((
        "comandos_native_term_page.json".into(),
        serde_json::to_vec(&page).map_err(|e| e.to_string())?,
    ));
    Ok(files)
}
