pub use comandos_desktop::bridge::*;
use std::rc::Rc;
pub fn install(app: &Rc<super::app::App>) -> glib::SourceId {
    app.install_bridge()
}
