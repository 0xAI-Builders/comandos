//! Captured dialog results, materialized as asynchronous AppKit sheets on main.
use crate::dialogs::{DialogOutcome, TabScope};

pub(super) fn close_outcome(scope: &TabScope, accepted: bool) -> DialogOutcome {
    DialogOutcome::Close {
        scope: scope.clone(),
        confirmed: accepted,
    }
}

pub(super) fn rename_outcome(scope: &TabScope, accepted: bool, text: &str) -> DialogOutcome {
    let text = comandos_core::text::strip(text);
    DialogOutcome::Rename {
        scope: scope.clone(),
        label: (accepted && !text.is_empty()).then(|| text.to_owned()),
    }
}

#[cfg(target_os = "macos")]
use block2::RcBlock;
#[cfg(target_os = "macos")]
use comandos_desktop::Lang;
#[cfg(target_os = "macos")]
use objc2::{MainThreadOnly, rc::Retained};
#[cfg(target_os = "macos")]
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertSecondButtonReturn, NSTextField, NSWindow,
};
#[cfg(target_os = "macos")]
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString};

/// The caller retains this handle until completion and invalidates its ticket
/// before cancelling. The completion block never retains the alert itself.
#[cfg(target_os = "macos")]
pub(super) struct AlertHandle {
    alert: Retained<NSAlert>,
    _field: Option<Retained<NSTextField>>,
}

#[cfg(target_os = "macos")]
impl AlertHandle {
    pub fn cancel(&self, parent: &NSWindow, _mtm: MainThreadMarker) {
        let window = self.alert.window();
        if parent
            .attachedSheet()
            .is_some_and(|sheet| std::ptr::eq(&*sheet, &*window))
        {
            // Both retained windows belong to this main-thread handle. End only
            // our attached sheet, never a replacement sheet belonging to another owner.
            parent.endSheet_returnCode(&window, NSAlertSecondButtonReturn);
        }
    }
}

#[cfg(target_os = "macos")]
pub(super) fn prompt_close(
    parent: &NSWindow,
    scope: TabScope,
    label: &str,
    lang: Lang,
    mtm: MainThreadMarker,
    complete: impl Fn(DialogOutcome) + 'static,
) -> AlertHandle {
    let text = crate::dialogs::close_text(lang, label);
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(&text.message));
    alert.setInformativeText(&NSString::from_str(text.informative));
    alert.addButtonWithTitle(&NSString::from_str(text.accept));
    alert.addButtonWithTitle(&NSString::from_str(text.cancel));
    let callback = RcBlock::new(move |response| {
        if MainThreadMarker::new().is_some() {
            complete(close_outcome(&scope, response == NSAlertFirstButtonReturn));
        }
    });
    // AppKit copies the completion block and calls it on main after ending the
    // sheet. Returning now lets the caller release its mutable owner borrow.
    alert.beginSheetModalForWindow_completionHandler(parent, Some(&callback));
    AlertHandle {
        alert,
        _field: None,
    }
}

#[cfg(target_os = "macos")]
pub(super) fn prompt_rename(
    parent: &NSWindow,
    scope: TabScope,
    label: &str,
    lang: Lang,
    mtm: MainThreadMarker,
    complete: impl Fn(DialogOutcome) + 'static,
) -> AlertHandle {
    let (message, accept, cancel) = crate::dialogs::rename_text(lang);
    let alert = NSAlert::new(mtm);
    let field = NSTextField::initWithFrame(
        NSTextField::alloc(mtm),
        NSRect::new(NSPoint::new(0., 0.), NSSize::new(260., 24.)),
    );
    field.setStringValue(&NSString::from_str(label));
    alert.setMessageText(&NSString::from_str(message));
    alert.setAccessoryView(Some(&field));
    alert.addButtonWithTitle(&NSString::from_str(accept));
    alert.addButtonWithTitle(&NSString::from_str(cancel));
    let input = field.clone();
    let callback = RcBlock::new(move |response| {
        if MainThreadMarker::new().is_some() {
            let accepted = response == NSAlertFirstButtonReturn;
            let text = if accepted {
                input.stringValue().to_string()
            } else {
                String::new()
            };
            complete(rename_outcome(&scope, accepted, &text));
        }
    });
    // Only the field, captured scope, and caller's weak/ticket completion escape.
    // No Alert -> block -> Alert cycle, and no modal runloop under an owner borrow.
    alert.beginSheetModalForWindow_completionHandler(parent, Some(&callback));
    AlertHandle {
        alert,
        _field: Some(field),
    }
}
