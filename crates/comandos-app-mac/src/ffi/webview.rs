// D7 retains the shared pool contract on macOS 12; newer SDKs deprecate it.
#![allow(deprecated)]
use super::delegate::Delegate;
use objc2::{MainThreadOnly, rc::Retained, runtime::ProtocolObject};
use objc2_app_kit::NSAutoresizingMaskOptions;
use objc2_foundation::{MainThreadMarker, NSRect, NSString, NSURL, NSURLRequest};
use objc2_web_kit::{WKProcessPool, WKUserContentController, WKWebView, WKWebViewConfiguration};
pub(super) fn new(
    frame: NSRect,
    pool: &WKProcessPool,
    ucc: Option<&WKUserContentController>,
    delegate: &Delegate,
    mtm: MainThreadMarker,
) -> Retained<WKWebView> {
    // SEGURIDAD: The configuration and common pool are retained by WebKit; this
    // function is called only with the owning main-thread marker. HTTP URLs need
    // no private file-access preferences or KVC selectors.
    unsafe {
        let cfg = WKWebViewConfiguration::new(mtm);
        cfg.setProcessPool(pool);
        if let Some(ucc) = ucc {
            cfg.setUserContentController(ucc);
        }
        let view = WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), frame, &cfg);
        view.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        view.setUIDelegate(Some(ProtocolObject::from_ref(delegate)));
        view
    }
}
pub(super) fn load(view: &WKWebView, url: &str) -> Result<(), String> {
    let url = NSURL::URLWithString(&NSString::from_str(url)).ok_or("URL inválida")?;
    let request = NSURLRequest::requestWithURL(&url);
    // SEGURIDAD: view is a retained view belonging to this main-thread owner. The
    // request owns its URL and loadRequest retains what it needs asynchronously.
    unsafe {
        view.loadRequest(&request);
    }
    Ok(())
}
