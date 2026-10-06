use crate::config::{AppConfig, RunMode};
use gtk::prelude::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};
use webkit2gtk::{
    NavigationPolicyDecisionExt, PermissionRequestExt, PolicyDecisionExt, SettingsExt,
    URIRequestExt, UserContentManagerExt, WebContext, WebViewExt,
};

pub type WebView = webkit2gtk::WebView;

#[derive(Debug)]
pub enum WebError {
    Gtk(String),
}

#[derive(Default)]
pub struct LoadObservation {
    pub finished: bool,
    pub error: Option<String>,
}
impl LoadObservation {
    pub fn started(&mut self) {
        self.finished = false;
        self.error = None;
    }
    pub fn failed(&mut self, message: String) {
        self.error = Some(message);
    }
    pub fn finished(&mut self) {
        self.finished = true;
    }
    pub fn ready(&self) -> bool {
        self.finished && self.error.is_none()
    }
}

pub fn dashboard_uri(base: Option<&str>, version: &str) -> Option<String> {
    let base = base?.trim_end_matches('/');
    Some(format!(
        "{base}/?app=1&anwin=1&version={}",
        encode_query(version)
    ))
}

pub fn create(cfg: &AppConfig) -> Result<WebView, WebError> {
    create_observed(cfg, None)
}
pub fn diagnostic_software_rendering(mode: RunMode, value: Option<&str>) -> bool {
    mode != RunMode::Live && crate::layout_dump::enabled(value)
}
pub fn create_observed(
    cfg: &AppConfig,
    observation: Option<Rc<RefCell<LoadObservation>>>,
) -> Result<WebView, WebError> {
    create_for_page(
        cfg,
        "centro",
        dashboard_uri(cfg.dash_url(), env!("CARGO_PKG_VERSION")).as_deref(),
        observation,
    )
    .map(|(view, _)| view)
}
pub(crate) struct OwnedPage {
    pub view: WebView,
    retry: Rc<RefCell<Option<glib::SourceId>>>,
    closed: Rc<Cell<bool>>,
    handler: String,
}
impl OwnedPage {
    pub fn cancel(&self) {
        if self.closed.replace(true) {
            return;
        }
        if let Some(source) = self.retry.borrow_mut().take() {
            source.remove();
        }
        self.view.stop_loading();
        if let Some(manager) = self.view.user_content_manager() {
            manager.unregister_script_message_handler(&self.handler);
        }
    }
}
impl Drop for OwnedPage {
    fn drop(&mut self) {
        self.cancel();
    }
}
pub(crate) fn create_page(
    cfg: &AppConfig,
    handler: &str,
    uri: &str,
) -> Result<OwnedPage, WebError> {
    if !matches!(handler, "centro" | "extensions") {
        return Err(WebError::Gtk("unknown shelf bridge".into()));
    }
    let (view, (retry, closed)) = create_for_page(cfg, handler, Some(uri), None)?;
    Ok(OwnedPage {
        view,
        retry,
        closed,
        handler: handler.into(),
    })
}
type PageControl = (Rc<RefCell<Option<glib::SourceId>>>, Rc<Cell<bool>>);
fn create_for_page(
    cfg: &AppConfig,
    handler: &str,
    uri: Option<&str>,
    observation: Option<Rc<RefCell<LoadObservation>>>,
) -> Result<(WebView, PageControl), WebError> {
    let manager = match cfg.mode() {
        RunMode::Shadow => webkit2gtk::WebsiteDataManager::new_ephemeral(),
        RunMode::Sandbox => webkit2gtk::WebsiteDataManager::builder()
            .base_data_directory(cfg.web_data_dir().to_string_lossy().as_ref())
            .base_cache_directory(cfg.web_cache_dir().to_string_lossy().as_ref())
            .build(),
        RunMode::Live => webkit2gtk::WebsiteDataManager::builder()
            .base_data_directory(cfg.web_data_dir().to_string_lossy().as_ref())
            .base_cache_directory(cfg.web_cache_dir().to_string_lossy().as_ref())
            .build(),
    };
    let context = WebContext::with_website_data_manager(&manager);
    let content = webkit2gtk::UserContentManager::new();
    if !content.register_script_message_handler(handler) {
        return Err(WebError::Gtk("page bridge unavailable".into()));
    }
    let webview = WebView::builder()
        .web_context(&context)
        .user_content_manager(&content)
        .build();
    webview.set_hexpand(true);
    webview.set_vexpand(true);
    if let Some(settings) = WebViewExt::settings(&webview) {
        if diagnostic_software_rendering(
            cfg.mode(),
            std::env::var("COMANDOS_APP_DIAGNOSTIC_SOFTWARE_RENDERING")
                .ok()
                .as_deref(),
        ) {
            settings
                .set_hardware_acceleration_policy(webkit2gtk::HardwareAccelerationPolicy::Never);
        }
        settings.set_enable_webaudio(true);
        settings.set_enable_javascript(cfg.mode() != RunMode::Shadow);
        settings.set_enable_write_console_messages_to_stdout(true);
    }
    let shadow = cfg.mode() == RunMode::Shadow;
    webview.connect_permission_request(move |_, request| {
        if !shadow && request.is::<webkit2gtk::NotificationPermissionRequest>() {
            request.allow();
        } else {
            request.deny();
        }
        true
    });
    if shadow {
        webview.connect_decide_policy(|_, decision, _| {
            if let Some(navigation) =
                decision.downcast_ref::<webkit2gtk::NavigationPolicyDecision>()
                && let Some(request) = navigation
                    .navigation_action()
                    .and_then(|action| action.request())
                && request
                    .http_method()
                    .is_some_and(|method| method.as_str() != "GET")
            {
                decision.ignore();
                return true;
            }
            false
        });
    }
    let retry = Rc::new(RefCell::new(None::<glib::SourceId>));
    let closed = Rc::new(Cell::new(false));
    let closed_on_fail = closed.clone();
    let retry_on_fail = retry.clone();
    let failure_observation = observation.clone();
    webview.connect_load_failed(move |webview, _, _, error| {
        if closed_on_fail.get() {
            return true;
        }
        if let Some(observation) = &failure_observation {
            observation.borrow_mut().failed(error.to_string());
        }
        if let Some(source) = retry_on_fail.borrow_mut().take() {
            source.remove();
        }
        let weak = webview.downgrade();
        let source_slot = retry_on_fail.clone();
        *retry_on_fail.borrow_mut() =
            Some(glib::timeout_add_local(Duration::from_secs(2), move || {
                source_slot.borrow_mut().take();
                if let Some(webview) = weak.upgrade() {
                    webview.reload();
                }
                glib::ControlFlow::Break
            }));
        true
    });
    let retry_on_destroy = retry.clone();
    let closed_on_destroy = closed.clone();
    webview.connect_destroy(move |_| {
        closed_on_destroy.set(true);
        if let Some(source) = retry_on_destroy.borrow_mut().take() {
            source.remove();
        }
    });
    if let Some(observation) = observation {
        webview.connect_load_changed(move |_, event| match event {
            webkit2gtk::LoadEvent::Started => observation.borrow_mut().started(),
            webkit2gtk::LoadEvent::Finished => observation.borrow_mut().finished(),
            _ => {}
        });
    }
    if let Some(uri) = uri {
        webview.load_uri(uri);
    } else {
        webview.load_html("<!doctype html><meta charset=utf-8><body></body>", None);
    }
    Ok((webview, (retry, closed)))
}

pub(crate) fn encode_query(raw: &str) -> String {
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

pub fn terminal_uri(path: &std::path::Path, session: &str, token: &str, theme: &str) -> String {
    format!(
        "{}?auth={}&arg={}&theme={}",
        gio::File::for_path(path).uri(),
        encode_query(token),
        encode_query(session),
        encode_query(theme)
    )
}
pub fn terminal_view(cfg: &AppConfig, uri: &str) -> Result<WebView, WebError> {
    if cfg.mode() != RunMode::Live {
        return Err(WebError::Gtk(
            "web terminal backend refused in isolated mode".into(),
        ));
    }
    let view = WebView::new();
    if let Some(settings) = WebViewExt::settings(&view) {
        settings.set_allow_file_access_from_file_urls(true);
        settings.set_allow_universal_access_from_file_urls(true);
        settings.set_enable_write_console_messages_to_stdout(true);
    }
    view.load_uri(uri);
    Ok(view)
}
