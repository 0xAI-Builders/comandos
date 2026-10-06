//! Data-only menu policy, materialized on the owning AppKit thread.
use crate::strip::MenuAction;
use std::ffi::CStr;

pub(super) fn selector_name(action: MenuAction) -> &'static CStr {
    match action {
        MenuAction::Quit => c"terminate:",
        MenuAction::Reload => c"reloadDashboard:",
        MenuAction::Toggle => c"toggleTermPane:",
        MenuAction::ZoomIn => c"zoomIn:",
        MenuAction::ZoomOut => c"zoomOut:",
        MenuAction::ZoomReset => c"zoomReset:",
        MenuAction::NewLocal => c"newLocalTab:",
        MenuAction::Close => c"closeCurrentTab:",
        MenuAction::Next => c"nextTab:",
        MenuAction::Previous => c"prevTab:",
    }
}

pub(super) fn context_spec(lang: comandos_desktop::Lang) -> [(&'static str, &'static CStr); 2] {
    let es = lang == comandos_desktop::Lang::Es;
    [
        (
            if es {
                "Renombrar pestana…"
            } else {
                "Rename tab…"
            },
            c"renameTabFromMenu:",
        ),
        (
            if es { "Cerrar pestana" } else { "Close tab" },
            c"closeTabFromMenu:",
        ),
    ]
}

#[cfg(target_os = "macos")]
use super::delegate::Delegate;
#[cfg(target_os = "macos")]
use crate::dialogs::TabScope;
#[cfg(target_os = "macos")]
use comandos_desktop::Lang;
#[cfg(target_os = "macos")]
use objc2::{
    MainThreadOnly,
    rc::Retained,
    runtime::{AnyObject, Sel},
};
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSMenu, NSMenuItem};
#[cfg(target_os = "macos")]
use objc2_foundation::{MainThreadMarker, NSString};

/// AppKit retains the installed menu; this helper keeps its own identity and all
/// target-bearing items so teardown can detach the borrowed delegate safely.
#[cfg(target_os = "macos")]
pub(super) struct Menus {
    main: Retained<NSMenu>,
    items: Vec<Retained<NSMenuItem>>,
    lang: Lang,
}

#[cfg(target_os = "macos")]
impl Menus {
    pub fn install(target: &Delegate, lang: Lang, mtm: MainThreadMarker) -> Self {
        let app = NSApplication::sharedApplication(mtm);
        let main = NSMenu::new(mtm);
        let mut items = Vec::new();
        for group in crate::strip::menu_spec(lang) {
            let parent = NSMenuItem::new(mtm);
            let submenu =
                NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(group.title));
            for spec in group.items {
                let action = Sel::register(selector_name(spec.action));
                // SEGURIDAD: The fixed selector map matches Delegate's one-argument
                // target/action methods, or NSApplication's terminate:. The owner
                // retains Delegate until detach clears every item's weak target.
                let item = unsafe {
                    NSMenuItem::initWithTitle_action_keyEquivalent(
                        NSMenuItem::alloc(mtm),
                        &NSString::from_str(spec.title),
                        Some(action),
                        &NSString::from_str(spec.key),
                    )
                };
                let destination: &AnyObject = if spec.action == MenuAction::Quit {
                    &app
                } else {
                    target
                };
                // SEGURIDAD: destination implements this selector on main. NSApp
                // outlives the menu and the caller retains the Delegate owner.
                unsafe {
                    item.setTarget(Some(destination));
                }
                item.setTag(spec.action as isize);
                let mut mask = NSEventModifierFlags::empty();
                if spec.command {
                    mask |= NSEventModifierFlags::Command;
                }
                if spec.shift {
                    mask |= NSEventModifierFlags::Shift;
                }
                item.setKeyEquivalentModifierMask(mask);
                submenu.addItem(&item);
                items.push(item);
            }
            parent.setSubmenu(Some(&submenu));
            main.addItem(&parent);
        }
        app.setMainMenu(Some(&main));
        Self { main, items, lang }
    }

    pub fn refresh(&mut self, target: &Delegate, lang: Lang, mtm: MainThreadMarker) {
        if self.lang == lang {
            return;
        }
        self.detach(mtm);
        *self = Self::install(target, lang, mtm);
    }

    pub fn detach(&mut self, mtm: MainThreadMarker) {
        for item in &self.items {
            // SEGURIDAD: Clear weak targets and their selectors before releasing
            // Delegate. The retained item is used only on its AppKit main thread.
            unsafe {
                item.setTarget(None);
                item.setAction(None);
            }
        }
        let app = NSApplication::sharedApplication(mtm);
        if app
            .mainMenu()
            .is_some_and(|menu| std::ptr::eq(&*menu, &*self.main))
        {
            app.setMainMenu(None);
        }
    }
}

#[cfg(target_os = "macos")]
pub(super) fn tab_context(
    target: &Delegate,
    scope: &TabScope,
    lang: Lang,
    mtm: MainThreadMarker,
) -> Retained<NSMenu> {
    let menu = NSMenu::new(mtm);
    let Ok(instance) = isize::try_from(scope.instance) else {
        return menu;
    };
    for (title, name) in context_spec(lang) {
        let action = Sel::register(name);
        // SEGURIDAD: Both selectors are Delegate methods accepting a menu sender.
        // The caller retains Delegate longer than its tab's context menu and
        // clears that menu before releasing the delegate during teardown.
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(title),
                Some(action),
                &NSString::from_str(""),
            )
        };
        // SEGURIDAD: The retained owner implements both selectors on main.
        unsafe {
            item.setTarget(Some(target));
        }
        item.setTag(instance);
        // SEGURIDAD: Delegate reads this representedObject as NSString. NSMenuItem
        // retains it, preserving the captured key beyond this temporary string.
        unsafe {
            item.setRepresentedObject(Some(&NSString::from_str(&scope.key)));
        }
        menu.addItem(&item);
    }
    menu
}
