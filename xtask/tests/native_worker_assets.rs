#[test]
fn worker_loader_registers_synchronously_with_separate_module_and_embedded_wasm() {
    let loader = xtask::web_build::render_worker_boot(b"\0asm\x01\0\0\0");
    assert!(loader.contains("importScripts({{SW_MODULE}})"));
    assert!(loader.contains("wasm_bindgen.initSync"));
    assert!(loader.contains("AGFzbQEAAAA="));
    assert!(loader.contains("wasm_bindgen.boot(true, {{PRECACHE}})"));
    assert!(!loader.contains("await"));
    assert!(!loader.contains("fetch("));
}
