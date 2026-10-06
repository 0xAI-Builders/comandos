use crate::maud::{Markup, html};

pub fn panel() -> Markup {
    html! {
        section id="push-settings" {
            p id="push-status" {}
            button id="push-enable" type="button" { "Activar" }
            button id="push-disable" type="button" hidden { "Desactivar" }
            button id="push-test" type="button" hidden { "Enviar aviso de prueba" }
        }
    }
}
