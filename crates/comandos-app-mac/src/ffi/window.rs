// D7 retains the shared pool contract on macOS 12; newer SDKs deprecate it.
#![allow(deprecated)]
use super::{delegate::Delegate, webview};
use crate::app::{App, AppConfig, WindowSpec};
use objc2::{MainThreadOnly, rc::Retained, runtime::ProtocolObject, sel};
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString};
use objc2_web_kit::{WKProcessPool, WKUserContentController, WKWebView};
pub(super) struct TabView {
    instance: u64,
    button: Retained<NSButton>,
    web: Option<Retained<WKWebView>>,
}
pub(super) struct Views {
    pub window: Retained<NSWindow>,
    pub dash: Retained<WKWebView>,
    pub ucc: Retained<WKUserContentController>,
    pool: Retained<WKProcessPool>,
    split: Retained<NSSplitView>,
    strip: Retained<NSView>,
    scroll: Retained<NSScrollView>,
    term: Retained<NSView>,
    plus: Retained<NSButton>,
    tabs: Vec<TabView>,
    theme: String,
    lang: comandos_desktop::Lang,
    pub retry: Option<Retained<objc2_foundation::NSTimer>>,
}
fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}
impl Views {
    pub fn owns(&self, web: &WKWebView) -> bool {
        std::ptr::eq(web, &*self.dash)
            || self.tabs.iter().any(|tab| {
                tab.web
                    .as_ref()
                    .is_some_and(|owned| std::ptr::eq(web, &**owned))
            })
    }
    pub fn new(
        cfg: &AppConfig,
        delegate: &Delegate,
        lang: comandos_desktop::Lang,
        mtm: MainThreadMarker,
    ) -> Result<Self, String> {
        let spec = WindowSpec::default();
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        // SEGURIDAD: AppKit globals and all retained views are accessed on mtm's thread.
        // The window has an owned Retained reference, so auto-release on close is disabled.
        let (window, pool, ucc, dash, strip, scroll, term, plus, split) = unsafe {
            app.setAppearance(NSAppearance::appearanceNamed(NSAppearanceNameDarkAqua).as_deref());
            let window = NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect(0., 0., spec.size.0, spec.size.1),
                NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Miniaturizable
                    | NSWindowStyleMask::Resizable,
                NSBackingStoreType::Buffered,
                false,
            );
            window.setReleasedWhenClosed(false);
            window.setTitle(&NSString::from_str(spec.title));
            window.setMinSize(NSSize::new(spec.minimum.0, spec.minimum.1));
            window.center();
            window.setDelegate(Some(ProtocolObject::from_ref(delegate)));
            let content = window.contentView().ok_or("ventana sin contentView")?;
            let size = content.frame().size;
            let mask = NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable;
            let split = NSSplitView::initWithFrame(
                NSSplitView::alloc(mtm),
                rect(0., 0., size.width, size.height),
            );
            split.setVertical(true);
            split.setDividerStyle(NSSplitViewDividerStyle::Thin);
            split.setAutoresizingMask(mask);
            let pool = WKProcessPool::new(mtm);
            let ucc = WKUserContentController::new(mtm);
            ucc.addScriptMessageHandler_name(
                ProtocolObject::from_ref(delegate),
                &NSString::from_str("centro"),
            );
            let dash = webview::new(
                rect(0., 0., size.width * spec.left_fraction, size.height),
                &pool,
                Some(&ucc),
                delegate,
                mtm,
            );
            dash.setNavigationDelegate(Some(ProtocolObject::from_ref(delegate)));
            webview::load(&dash, &cfg.dash_url)?;
            let rw = size.width * (1. - spec.left_fraction);
            let rh = size.height;
            let right = NSView::initWithFrame(NSView::alloc(mtm), rect(0., 0., rw, rh));
            right.setAutoresizingMask(mask);
            let scroll = NSScrollView::initWithFrame(
                NSScrollView::alloc(mtm),
                rect(0., rh - spec.strip_height, rw, spec.strip_height),
            );
            scroll.setAutoresizingMask(
                NSAutoresizingMaskOptions::ViewWidthSizable
                    | NSAutoresizingMaskOptions::ViewMinYMargin,
            );
            scroll.setDrawsBackground(false);
            scroll.setHasVerticalScroller(false);
            scroll.setHasHorizontalScroller(true);
            scroll.setAutohidesScrollers(true);
            let strip =
                NSView::initWithFrame(NSView::alloc(mtm), rect(0., 0., rw, spec.strip_height));
            scroll.setDocumentView(Some(&strip));
            right.addSubview(&scroll);
            let plus = button("＋", delegate, sel!(newLocalTab:), mtm);
            plus.setToolTip(Some(&NSString::from_str(
                if lang == comandos_desktop::Lang::Es {
                    "Nueva terminal local"
                } else {
                    "New local tab"
                },
            )));
            strip.addSubview(&plus);
            let term =
                NSView::initWithFrame(NSView::alloc(mtm), rect(0., 0., rw, rh - spec.strip_height));
            term.setAutoresizingMask(mask);
            right.addSubview(&term);
            split.addSubview(&dash);
            split.addSubview(&right);
            content.addSubview(&split);
            split.setPosition_ofDividerAtIndex((size.width * spec.left_fraction).floor(), 0);
            (window, pool, ucc, dash, strip, scroll, term, plus, split)
        };
        Ok(Self {
            window,
            pool,
            split,
            lang,
            ucc,
            dash,
            strip,
            scroll,
            term,
            plus,
            tabs: vec![],
            theme: String::new(),
            retry: None,
        })
    }
    pub fn show(&self, mtm: MainThreadMarker) {
        self.window.makeKeyAndOrderFront(None);
        #[allow(deprecated)]
        NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    }
    pub fn sync(
        &mut self,
        app: &App,
        focus: Option<u64>,
        cfg: &AppConfig,
        delegate: &Delegate,
        mtm: MainThreadMarker,
    ) -> Result<(), String> {
        // SEGURIDAD: Each button and web view is retained by this owner and kept in its
        // superview. Targets refer to the retained delegate; tab tags are instance ids.
        unsafe {
            self.tabs.retain(|view| {
                if app.tabs().iter().any(|tab| tab.instance == view.instance) {
                    return true;
                }
                if let Some(web) = &view.web {
                    web.stopLoading();
                    web.setUIDelegate(None);
                    web.removeFromSuperview();
                }
                view.button.setTarget(None);
                clear_context(&view.button);
                view.button.removeFromSuperview();
                false
            });
            let mut x = 6.;
            for tab in app.tabs() {
                if !self.tabs.iter().any(|view| view.instance == tab.instance) {
                    let b = button(&tab.label, delegate, sel!(tabClicked:), mtm);
                    b.setTag(isize::try_from(tab.instance).map_err(|e| e.to_string())?);
                    b.setMenu(Some(&super::menu::tab_context(
                        delegate,
                        &crate::dialogs::TabScope {
                            key: tab.key.clone(),
                            instance: tab.instance,
                        },
                        app.lang,
                        mtm,
                    )));
                    self.strip.addSubview(&b);
                    self.tabs.push(TabView {
                        instance: tab.instance,
                        button: b,
                        web: None,
                    });
                }
                let Some(view) = self
                    .tabs
                    .iter_mut()
                    .find(|view| view.instance == tab.instance)
                else {
                    continue;
                };
                if self.lang != app.lang {
                    clear_context(&view.button);
                    view.button.setMenu(Some(&super::menu::tab_context(
                        delegate,
                        &crate::dialogs::TabScope {
                            key: tab.key.clone(),
                            instance: tab.instance,
                        },
                        app.lang,
                        mtm,
                    )));
                }
                set_title(&view.button, &tab.label, tab.dot_color);
                view.button.sizeToFit();
                let width = (view.button.frame().size.width + 14.).max(64.);
                view.button.setFrame(rect(x, 3., width, 24.));
                x += width + 4.;
                let selected = Some(tab.key.as_str()) == app.active_key();
                view.button.setState(if selected {
                    NSControlStateValueOn
                } else {
                    NSControlStateValueOff
                });
                if tab.loaded && view.web.is_none() {
                    let web = webview::new(self.term.bounds(), &self.pool, None, delegate, mtm);
                    webview::load(
                        &web,
                        &comandos_desktop::term_url::TermUrl::build(
                            &cfg.dash_url,
                            Some(&tab.session),
                            app.theme(),
                            app.token(),
                        ),
                    )?;
                    self.term.addSubview(&web);
                    view.web = Some(web);
                }
                if let Some(web) = &view.web {
                    web.setHidden(!selected);
                    if self.theme != app.theme() {
                        web.evaluateJavaScript_completionHandler(
                            &NSString::from_str(&app.theme_script()),
                            None,
                        );
                    }
                    if selected && focus == Some(tab.instance) {
                        self.strip.scrollRectToVisible(view.button.frame());
                        self.window.makeFirstResponder(Some(web));
                    }
                }
            }
            self.plus.setFrame(rect(x, 3., 30., 24.));
            self.plus.setToolTip(Some(&NSString::from_str(
                if app.lang == comandos_desktop::Lang::Es {
                    "Nueva terminal local"
                } else {
                    "New local tab"
                },
            )));
            self.strip.setFrame(rect(
                0.,
                0.,
                self.scroll.contentSize().width.max(x + 36.),
                30.,
            ));
            self.theme = app.theme().into();
            self.lang = app.lang;
        }
        Ok(())
    }
    pub fn reload(&self) {
        // SEGURIDAD: Owned dashboard accessed on its owning main thread.
        unsafe {
            self.dash.reload();
        }
    }
    pub fn toggle(&self, visible: bool) {
        // SEGURIDAD: Split view has exactly two retained subviews; divider 0 is valid.
        let width = self.split.bounds().size.width;
        self.split.setPosition_ofDividerAtIndex(
            if visible {
                (width * 0.52).floor()
            } else {
                width
            },
            0,
        );
    }
    pub fn zoom(&self, active: Option<u64>, factor: f64) {
        let Some(web) = active
            .and_then(|id| self.tabs.iter().find(|t| t.instance == id))
            .and_then(|t| t.web.as_deref())
        else {
            return;
        };
        // SEGURIDAD: pageZoom uses the generated CGFloat signatures on a retained WKWebView.
        unsafe {
            web.setPageZoom(crate::strip::zoom(web.pageZoom(), factor));
        }
    }
    pub fn close(&mut self) {
        if let Some(timer) = self.retry.take() {
            timer.invalidate();
        }
        // SEGURIDAD: Clear WebKit's callbacks before dropping the retained delegates.
        // Removing centro breaks the UCC->handler reference; handler->owner is weak.
        unsafe {
            self.ucc
                .removeScriptMessageHandlerForName(&NSString::from_str("centro"));
            self.dash.stopLoading();
            self.dash.setNavigationDelegate(None);
            self.dash.setUIDelegate(None);
            for tab in &self.tabs {
                if let Some(web) = &tab.web {
                    web.stopLoading();
                    web.setUIDelegate(None);
                    web.removeFromSuperview();
                }
                clear_context(&tab.button);
                tab.button.setTarget(None);
                tab.button.removeFromSuperview();
            }
            self.plus.setTarget(None);
        }
        self.window.setDelegate(None);
        self.tabs.clear();
    }
}
unsafe fn button(
    title: &str,
    delegate: &Delegate,
    action: objc2::runtime::Sel,
    mtm: MainThreadMarker,
) -> Retained<NSButton> {
    let b = NSButton::initWithFrame(NSButton::alloc(mtm), rect(0., 3., 60., 24.));
    b.setBezelStyle(NSBezelStyle::Recessed);
    b.setButtonType(NSButtonType::PushOnPushOff);
    b.setBordered(true);
    b.setFont(Some(&NSFont::systemFontOfSize(11.)));
    b.setTitle(&NSString::from_str(title));
    // SEGURIDAD: The main-thread owner retains delegate beyond the lifetime of this
    // button. All selectors passed here are implemented with sender:&NSButton.
    unsafe {
        b.setTarget(Some(delegate));
        b.setAction(Some(action));
    }
    b
}

fn set_title(button: &NSButton, label: &str, color: &str) {
    use objc2::{AnyThread, runtime::AnyObject};
    use objc2_foundation::{NSAttributedString, NSDictionary, NSMutableAttributedString};
    let rgb = u32::from_str_radix(color.trim_start_matches('#'), 16).unwrap_or(0);
    let dot_color = NSColor::colorWithSRGBRed_green_blue_alpha(
        f64::from((rgb >> 16) & 255) / 255.,
        f64::from((rgb >> 8) & 255) / 255.,
        f64::from(rgb & 255) / 255.,
        1.,
    );
    let dot_font = NSFont::systemFontOfSize(8.);
    let label_font = NSFont::systemFontOfSize(11.);
    let label_color = NSColor::labelColor();
    // SEGURIDAD: Attribute keys have Apple's documented NSColor/NSFont value
    // types. NSDictionary retains these values and NSAttributedString copies the
    // dictionary. All AppKit objects are created and consumed on main.
    unsafe {
        let dot_attributes =
            NSDictionary::<objc2_foundation::NSAttributedStringKey, AnyObject>::from_slices(
                &[NSForegroundColorAttributeName, NSFontAttributeName],
                &[&*dot_color, &*dot_font],
            );
        let label_attributes =
            NSDictionary::<objc2_foundation::NSAttributedStringKey, AnyObject>::from_slices(
                &[NSForegroundColorAttributeName, NSFontAttributeName],
                &[&*label_color, &*label_font],
            );
        let dot = NSAttributedString::initWithString_attributes(
            NSAttributedString::alloc(),
            &NSString::from_str("● "),
            Some(&dot_attributes),
        );
        let text = NSAttributedString::initWithString_attributes(
            NSAttributedString::alloc(),
            &NSString::from_str(label),
            Some(&label_attributes),
        );
        let title = NSMutableAttributedString::new();
        title.appendAttributedString(&dot);
        title.appendAttributedString(&text);
        button.setAttributedTitle(&title);
    }
}

fn clear_context(button: &NSButton) {
    if let Some(menu) = button.menu() {
        for item in menu.itemArray() {
            // SEGURIDAD: This is the owned button's menu, whose targets refer to
            // Delegate. Clear every target/action before releasing or replacing it,
            // including items AppKit may retain while a context menu is tracking.
            unsafe {
                item.setTarget(None);
                item.setAction(None);
            }
        }
    }
    // SEGURIDAD: Release this owned button's context menu only after clearing its targets.
    unsafe {
        button.setMenu(None);
    }
}
