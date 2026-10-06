#[allow(dead_code)]
#[path = "../src/components/servers.rs"]
mod servers;
#[test]
fn trim_ssh_fields_without_changing_edit_identity() {
    assert_eq!(
        servers::view::form_payload(
            &serde_json::json!({"host":" \u{feff}mac ","hostname":" example.org\n","port":" 2222 ","orig":" previous ","remember":true})
        ),
        serde_json::json!({"host":"mac","hostname":"example.org","user":"","port":"2222","identity":""})
    );
}
#[test]
fn targets_keep_optional_user_and_port() {
    assert_eq!(
        servers::view::target(&serde_json::json!({"hostname":"host","port":"0"})),
        "host:0"
    );
    assert_eq!(
        servers::view::target(&serde_json::json!({"user":"dev","hostname":"host","port":22})),
        "dev@host:22"
    );
    assert_eq!(
        servers::view::target(&serde_json::json!({"hostname":"host","port":0})),
        "host"
    );
}
#[test]
fn reduced_motion_and_hidden_zero_width_do_not_animate() {
    use servers::view::*;
    assert_eq!(
        panel_plan(true, true, true, true, true, 100.0),
        PanelPlan::Immediate
    );
    assert_eq!(
        panel_plan(true, true, true, true, false, 0.0),
        PanelPlan::Immediate
    );
    assert_eq!(
        panel_plan(false, true, true, true, false, 0.0),
        PanelPlan::Show
    );
    assert_eq!(menu_left(900.0, 1000.0, 200.0), 792.0);
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn mount_servers_fixture() -> Result<(), wasm_bindgen::JsValue> {
    servers::mount()
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn attach_servers_fixture() -> Result<(), wasm_bindgen::JsValue> {
    servers::attach()
}
