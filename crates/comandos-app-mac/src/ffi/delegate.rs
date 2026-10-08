use super::{Native, webview};
use crate::app::Action;
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send, rc::Retained};
use objc2_app_kit::{
    NSApplication, NSApplicationDelegate, NSButton, NSMenuItem, NSWindowDelegate, NSWorkspace,
};
use objc2_foundation::{
    MainThreadMarker, NSError, NSJSONSerialization, NSJSONWritingOptions, NSNotification, NSObject,
    NSObjectProtocol, NSString, NSTimer,
};
use objc2_web_kit::*;
use std::{cell::RefCell, rc::Weak};
pub(super) struct Ivars {
    owner: Weak<RefCell<Native>>,
}
define_class!(
 // SEGURIDAD: NSObject has no subclass invariants. Ivars hold only a weak
 // main-thread owner, and Delegate implements no custom Objective-C destructor.
 #[unsafe(super=NSObject)]
 #[thread_kind=MainThreadOnly]
 #[ivars=Ivars]
 pub(super) struct Delegate;
 // SEGURIDAD: NSObjectProtocol imposes no additional requirements.
 unsafe impl NSObjectProtocol for Delegate {}
 // SEGURIDAD: The required signatures follow objc2-app-kit's generated protocol.
 unsafe impl NSApplicationDelegate for Delegate {
  // SEGURIDAD: AppKit invokes this selector with a valid NSNotification on main.
  #[unsafe(method(applicationDidFinishLaunching:))]
  fn launch(&self,_notification:&NSNotification){self.with_owner(|owner|owner.launch());}
  // SEGURIDAD: This selector takes NSApplication and returns Objective-C BOOL.
  #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
  fn last_window_closed(&self,_app:&NSApplication)->bool{true}
  // SEGURIDAD: Notification lifetime is limited to this synchronous callback.
  #[unsafe(method(applicationWillTerminate:))]
  fn terminate(&self,_notification:&NSNotification){self.with_owner(|owner|owner.close());}
 }
 // SEGURIDAD: NSWindowDelegate's optional callback has the generated signature.
 unsafe impl NSWindowDelegate for Delegate {
  // SEGURIDAD: Window callback runs on the class's MainThreadOnly thread.
  #[unsafe(method(windowWillClose:))]
  fn window_close(&self,_notification:&NSNotification){self.with_owner(|owner|owner.close());
   // SEGURIDAD: Cleanup completed and its mutable borrow ended before terminate,
   // which can synchronously reenter applicationWillTerminate.
   NSApplication::sharedApplication(self.mtm()).terminate(None);
  }
 }
 // SEGURIDAD: WebKit delivers the required method on this MainThreadOnly object.
 unsafe impl WKScriptMessageHandler for Delegate {
  // SEGURIDAD: Parameters are valid WebKit-owned objects for this callback.
  #[unsafe(method(userContentController:didReceiveScriptMessage:))]
  fn bridge(&self,_ucc:&WKUserContentController,message:&WKScriptMessage){self.receive_script(message);}
 }
 // SEGURIDAD: All selectors match the generated navigation delegate signatures.
 unsafe impl WKNavigationDelegate for Delegate {
  // SEGURIDAD: Navigation may be nil per the generated protocol; NSError is valid.
  #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
  fn failed(&self,web:&WKWebView,_navigation:Option<&WKNavigation>,_error:&NSError){self.navigation_failed(web);}
  // SEGURIDAD: Both objects are valid during WebKit's synchronous callback.
  #[unsafe(method(webView:didFinishNavigation:))]
  fn finished(&self,web:&WKWebView,_navigation:Option<&WKNavigation>){self.navigation_finished(web);}
 }
 // SEGURIDAD: The return is an owned optional WKWebView, as the protocol requires.
 unsafe impl WKUIDelegate for Delegate {
  // SEGURIDAD: Signature exactly follows generated WKUIDelegate; valid arguments
  // are used synchronously on main. Returning nil cancels embedded popup creation.
  #[unsafe(method_id(webView:createWebViewWithConfiguration:forNavigationAction:windowFeatures:))]
  fn external(&self,_web:&WKWebView,_configuration:&WKWebViewConfiguration,action:&WKNavigationAction,_features:&WKWindowFeatures)->Option<Retained<WKWebView>>{self.external_action(_web,action)}
 }
 impl Delegate {
  // SEGURIDAD: Both button target/action methods use NSButton* sender signatures.
  #[unsafe(method(newLocalTab:))]
  fn new_local(&self,_sender:Option<&objc2::runtime::AnyObject>){self.with_owner(|owner|owner.action(Action::NewLocal));}
  // SEGURIDAD: An owned NSButton invokes this selector synchronously on main.
  #[unsafe(method(tabClicked:))]
  fn tab_clicked(&self,sender:&NSButton){if let Ok(instance)=u64::try_from(sender.tag()){self.with_owner(|owner|owner.select(instance));}}
  // SEGURIDAD: Main menu target/action accepts a nullable Objective-C sender on main.
  #[unsafe(method(reloadDashboard:))]
  fn reload_menu(&self,_sender:Option<&objc2::runtime::AnyObject>){self.with_owner(|owner|owner.menu_action(crate::strip::MenuAction::Reload));}
  // SEGURIDAD: Main menu target/action accepts a nullable Objective-C sender on main.
  #[unsafe(method(toggleTermPane:))]
  fn toggle_menu(&self,_sender:Option<&objc2::runtime::AnyObject>){self.with_owner(|owner|owner.menu_action(crate::strip::MenuAction::Toggle));}
  // SEGURIDAD: Main menu target/action accepts a nullable Objective-C sender on main.
  #[unsafe(method(zoomIn:))]
  fn zoom_in(&self,_sender:Option<&objc2::runtime::AnyObject>){self.with_owner(|owner|owner.menu_action(crate::strip::MenuAction::ZoomIn));}
  // SEGURIDAD: Main menu target/action accepts a nullable Objective-C sender on main.
  #[unsafe(method(zoomOut:))]
  fn zoom_out(&self,_sender:Option<&objc2::runtime::AnyObject>){self.with_owner(|owner|owner.menu_action(crate::strip::MenuAction::ZoomOut));}
  // SEGURIDAD: Main menu target/action accepts a nullable Objective-C sender on main.
  #[unsafe(method(zoomReset:))]
  fn zoom_reset(&self,_sender:Option<&objc2::runtime::AnyObject>){self.with_owner(|owner|owner.menu_action(crate::strip::MenuAction::ZoomReset));}
  // SEGURIDAD: Main menu target/action accepts a nullable Objective-C sender on main.
  #[unsafe(method(closeCurrentTab:))]
  fn close_current(&self,_sender:Option<&objc2::runtime::AnyObject>){self.with_owner(|owner|owner.menu_action(crate::strip::MenuAction::Close));}
  // SEGURIDAD: Main menu target/action accepts a nullable Objective-C sender on main.
  #[unsafe(method(nextTab:))]
  fn next_tab(&self,_sender:Option<&objc2::runtime::AnyObject>){self.with_owner(|owner|owner.menu_action(crate::strip::MenuAction::Next));}
  // SEGURIDAD: Main menu target/action accepts a nullable Objective-C sender on main.
  #[unsafe(method(prevTab:))]
  fn prev_tab(&self,_sender:Option<&objc2::runtime::AnyObject>){self.with_owner(|owner|owner.menu_action(crate::strip::MenuAction::Previous));}
  // SEGURIDAD: Owned contextual NSMenuItem carries an NSString key and instance tag.
  #[unsafe(method(renameTabFromMenu:))]
  fn rename_from_menu(&self,sender:Option<&NSMenuItem>){if let Some(scope)=sender.and_then(menu_scope){self.with_owner(|owner|owner.prompt(scope,true));}}
  // SEGURIDAD: Owned contextual NSMenuItem carries an NSString key and instance tag.
  #[unsafe(method(closeTabFromMenu:))]
  fn close_from_menu(&self,sender:Option<&NSMenuItem>){if let Some(scope)=sender.and_then(menu_scope){self.with_owner(|owner|owner.prompt(scope,false));}}
  // SEGURIDAD: The owned 500ms timer passes NSTimer on main; shutdown invalidates it.
  #[unsafe(method(checkIPC:))]
  fn check_ipc(&self,_timer:&NSTimer){self.with_owner(|owner|owner.check_ipc());}
  // SEGURIDAD: The registered target/action uses NSTimer* exactly.
  #[unsafe(method(retryDash:))]
  fn retry(&self,_timer:&NSTimer){self.retry_dashboard();}
 }
);
impl Delegate {
    pub fn new(mtm: MainThreadMarker, owner: Weak<RefCell<Native>>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars { owner });
        // SEGURIDAD: NSObject init has its standard retained init-family signature.
        unsafe { msg_send![super(this), init] }
    }
    fn with_owner(&self, callback: impl FnOnce(&mut Native)) {
        let Some(owner) = self.ivars().owner.upgrade() else {
            return;
        };
        if let Ok(mut owner) = owner.try_borrow_mut()
            && !owner.closed
        {
            callback(&mut owner);
        }
    }
}

impl Delegate {
    fn receive_script(&self, message: &WKScriptMessage) {
        self.with_owner(|owner| {
            let Some(views) = &owner.views else {
                return;
            };
            // SEGURIDAD: WebKit objects are accessed synchronously on main, and retained
            // return values keep body/frame alive. Only the owned dashboard main frame
            // at the configured origin is allowed to issue terminal commands.
            let body = unsafe {
                if !message.frameInfo().isMainFrame() {
                    return;
                }
                let Some(web) = message.webView() else {
                    return;
                };
                if !std::ptr::eq(&*web, &*views.dash) {
                    return;
                }
                let Some(url) = message
                    .frameInfo()
                    .request()
                    .URL()
                    .and_then(|u| u.absoluteString())
                else {
                    return;
                };
                let url = url.to_string();
                if !(url == owner.cfg.dash_url
                    || url
                        .strip_prefix(&owner.cfg.dash_url)
                        .is_some_and(|tail| tail.starts_with('/')))
                {
                    return;
                }
                message.body()
            };
            let value = if let Some(text) = body.downcast_ref::<NSString>() {
                if text.len() > 1 << 20 {
                    return;
                }
                serde_json::Value::String(text.to_string())
            } else {
                // SEGURIDAD: NSJSONSerialization inspects the retained Foundation body.
                // Invalid Foundation JSON is refused before serialization.
                let data = unsafe {
                    if !NSJSONSerialization::isValidJSONObject(&body) {
                        return;
                    }
                    NSJSONSerialization::dataWithJSONObject_options_error(
                        &body,
                        NSJSONWritingOptions::empty(),
                    )
                };
                let Ok(data) = data else {
                    return;
                };
                if data.length() > 1 << 20 {
                    return;
                }
                let bytes = data.to_vec();
                let Ok(value) = serde_json::from_slice(&bytes) else {
                    return;
                };
                value
            };
            owner.receive(value);
        });
    }
    fn navigation_failed(&self, web: &WKWebView) {
        self.with_owner(|owner| {
            let Some(views) = &mut owner.views else {
                return;
            };
            if !std::ptr::eq(web, &*views.dash) {
                return;
            }
            if let Some(timer) = views.retry.take() {
                timer.invalidate();
            }
            // SEGURIDAD: The owner retains its timer and delegate; selector signature is
            // retryDash:(NSTimer*). Cleanup invalidates the timer before delegate release.
            let Some(delay) =
                comandos_desktop::RetrySchedule::after_failure(std::time::Duration::ZERO, true)
            else {
                return;
            };
            views.retry = Some(unsafe {
                NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                    delay.as_secs_f64(),
                    self,
                    objc2::sel!(retryDash:),
                    None,
                    false,
                )
            });
        });
    }
    fn navigation_finished(&self, web: &WKWebView) {
        self.with_owner(|owner| {
            let Some(views) = &mut owner.views else {
                return;
            };
            if std::ptr::eq(web, &*views.dash) {
                if let Some(timer) = views.retry.take() {
                    timer.invalidate();
                }
                owner.dump_dashboard();
            }
        });
    }
    fn external_action(
        &self,
        _web: &WKWebView,
        action: &WKNavigationAction,
    ) -> Option<Retained<WKWebView>> {
        // SEGURIDAD: request owns the URL. NSWorkspace is accessed on main. Only the
        // original three permitted schemes can leave the app, never file/javascript.
        self.with_owner(|owner| {
            // SEGURIDAD: Owner still exists and is open. The navigation must originate
            // from its dashboard or one of its retained terminal views.
            if !owner.views.as_ref().is_some_and(|views| views.owns(_web)) {
                return;
            }
            unsafe {
                if let Some(url) = action.request().URL()
                    && url.scheme().is_some_and(|s| {
                        ["http", "https", "mailto"].contains(&s.to_string().to_lowercase().as_str())
                    })
                {
                    NSWorkspace::sharedWorkspace().openURL(&url);
                }
            }
        });
        None
    }
    fn retry_dashboard(&self) {
        self.with_owner(|owner| {
            if let Some(views) = &mut owner.views {
                views.retry.take();
                if let Err(error) = webview::load(&views.dash, &owner.cfg.dash_url) {
                    eprintln!("ComandOS: {error}");
                }
            }
        });
    }
}

fn menu_scope(sender: &NSMenuItem) -> Option<crate::dialogs::TabScope> {
    // SEGURIDAD: Retained representedObject is read synchronously on main, then
    // copied into Rust. A stale menu's tag never resolves a replacement instance.
    let object = sender.representedObject()?;
    let key = object.downcast_ref::<NSString>()?.to_string();
    let instance = u64::try_from(sender.tag()).ok()?;
    Some(crate::dialogs::TabScope { key, instance })
}
