//! Preserve the original provider precedence and own every installed provider.
use crate::theme::{ThemeTokens, button_style_css, header_css, theme_css};
use gtk::prelude::*;

pub(super) struct ThemeLayers {
    screen: Option<gdk::Screen>,
    header: gtk::CssProvider,
    app: gtk::CssProvider,
    buttons: gtk::CssProvider,
}

impl ThemeLayers {
    pub(super) fn new(theme: &ThemeTokens, style: &str) -> Result<Self, glib::Error> {
        let header = gtk::CssProvider::new();
        let app = gtk::CssProvider::new();
        let buttons = gtk::CssProvider::new();
        // Parse all layers before changing the installed theme.
        header.load_from_data(header_css(theme).as_bytes())?;
        app.load_from_data(theme_css(theme).as_bytes())?;
        buttons.load_from_data(button_style_css(style, theme).as_bytes())?;
        Ok(Self {
            screen: gdk::Screen::default(),
            header,
            app,
            buttons,
        })
    }

    pub(super) fn install(&self) {
        if let Some(screen) = &self.screen {
            // The original installs the app provider after the header provider.
            // Combining them changes specificity: generic modal labels then
            // override help keys, section headings and descriptions.
            gtk::StyleContext::add_provider_for_screen(
                screen,
                &self.header,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
            gtk::StyleContext::add_provider_for_screen(
                screen,
                &self.app,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
            gtk::StyleContext::add_provider_for_screen(
                screen,
                &self.buttons,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
            );
        }
    }
}

impl Drop for ThemeLayers {
    fn drop(&mut self) {
        if let Some(screen) = &self.screen {
            for provider in [&self.header, &self.app, &self.buttons] {
                gtk::StyleContext::remove_provider_for_screen(screen, provider);
            }
        }
    }
}
