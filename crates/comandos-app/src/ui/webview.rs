use crate::config::{AppConfig, RunMode};
use gtk::prelude::*;
use webkit2gtk::{SettingsExt, WebContext, WebViewExt};

pub type WebView = webkit2gtk::WebView;

#[derive(Debug)]
pub enum WebError {
    Gtk(String),
}

pub fn dashboard_uri(base: Option<&str>, version: &str) -> Option<String> {
    let base = base?.trim_end_matches('/');
    Some(format!(
        "{base}/?app=1&anwin=1&version={}",
        encode_query(version)
    ))
}

pub fn create(cfg: &AppConfig) -> Result<WebView, WebError> {
    let manager = match cfg.mode() {
        RunMode::Shadow | RunMode::Sandbox => webkit2gtk::WebsiteDataManager::builder()
            .base_data_directory(cfg.web_data_dir().to_string_lossy().as_ref())
            .base_cache_directory(cfg.web_cache_dir().to_string_lossy().as_ref())
            .build(),
        RunMode::Live => webkit2gtk::WebsiteDataManager::builder()
            .base_data_directory(cfg.web_data_dir().to_string_lossy().as_ref())
            .base_cache_directory(cfg.web_cache_dir().to_string_lossy().as_ref())
            .build(),
    };
    let context = WebContext::with_website_data_manager(&manager);
    let webview = WebView::with_context(&context);
    webview.set_hexpand(true);
    webview.set_vexpand(true);
    if let Some(settings) = WebViewExt::settings(&webview) {
        settings.set_enable_webaudio(true);
        settings.set_enable_javascript(true);
    }
    if let Some(uri) = dashboard_uri(cfg.dash_url(), env!("CARGO_PKG_VERSION")) {
        webview.load_uri(&uri);
    } else {
        webview.load_html("<!doctype html><meta charset=utf-8><body></body>", None);
    }
    Ok(webview)
}

fn encode_query(raw: &str) -> String {
    let mut out = String::new();
    for byte in raw.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}
