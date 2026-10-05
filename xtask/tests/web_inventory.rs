// xtask/tests/web_inventory.rs
//! Inventario léxico de la interfaz (B3) y vigilancia de deriva (`web-port`).
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use xtask::web_inventory::{Kind, Unit, interop, region_text, scan};
use xtask::web_port::{self, CheckState, PortError, PortOptions};

const FIX: &str = "tests/fixtures/web";

fn unit<'a>(units: &'a [Unit], id: &str) -> &'a Unit {
    units.iter().find(|u| u.id == id).unwrap_or_else(|| {
        panic!(
            "falta {id}: {:?}",
            units.iter().map(|u| &u.id).collect::<Vec<_>>()
        )
    })
}

fn has(v: &[String], s: &str) -> bool {
    v.iter().any(|x| x == s)
}

#[test]
fn finds_regions_globals_routes_and_host_calls() {
    let units = scan(Path::new(FIX));
    let red = units.iter().find(|u| u.id == "region:red").unwrap();
    assert!(red.defines.contains(&"api".to_string()));
    assert!(red.routes.contains(&"/webterm-token".to_string()));
    let a = units.iter().find(|u| u.id == "script:a.js").unwrap();
    assert!(
        a.uses_globals.contains(&"api".to_string()),
        "a.js llama a api() definido en la región red"
    );
    assert!(a.storage_keys.contains(&"cc-axo".to_string()));
    assert_eq!(a.intervals_ms, vec![2000]);
    let interop: Value = interop(&units, Path::new(FIX));
    assert_eq!(interop["pomoRender"]["called_by"][0], "bin/cc-app");
}

#[test]
fn regions_follow_markers_in_document_order() {
    let units = scan(Path::new(FIX));
    let ids: Vec<&str> = units.iter().map(|u| u.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "script:a.js",
            "region:prelude",
            "region:red",
            "region:helpers",
            "region:render",
            "script:b.js",
            "region:tail",
            "script:c.js",
            "term:main",
            "term:tail",
        ]
    );
    let red = unit(&units, "region:red");
    assert_eq!(red.kind, Kind::Region);
    assert_eq!(red.source, "index.html");
    assert_eq!(
        red.marker_start.as_deref(),
        Some("// ---------- red ----------")
    );
    assert_eq!(
        red.marker_end.as_deref(),
        Some("// ---------- helpers ----------")
    );
    // El hash es el del tramo marker_start..marker_end, como lo corta B2.
    let html = fs::read_to_string(Path::new(FIX).join("index.html")).unwrap();
    let text = region_text(
        &html,
        "// ---------- red ----------",
        "// ---------- helpers ----------",
        None,
    )
    .unwrap();
    assert_eq!(
        text,
        "// ---------- red ----------\nasync function api(path){ return fetch(\"/webterm-token\") }\n"
    );
    assert_eq!(
        red.sha256,
        xtask::web_inventory::sha256_hex(text.as_bytes())
    );
    assert_eq!(red.lines, 2);
    assert_eq!(red.line_start, 11);
    let render = unit(&units, "region:render");
    assert_eq!(render.marker_end.as_deref(), Some("</script>"));
    let prelude = unit(&units, "region:prelude");
    assert_eq!(prelude.marker_start.as_deref(), Some("\"use strict\";"));
    assert!(has(&prelude.defines, "S"));
    let tail = unit(&units, "region:tail");
    assert_eq!(tail.script_index, Some(2));
    assert!(has(&tail.uses_globals, "render"));
    let a = unit(&units, "script:a.js");
    assert_eq!(a.kind, Kind::Script);
    assert_eq!(a.marker_start, None);
    assert_eq!(
        a.sha256,
        xtask::web_inventory::sha256_hex(&fs::read(Path::new(FIX).join("a.js")).unwrap())
    );
    assert_eq!(a.component, "a");
    assert_eq!(red.component, "red");
    assert_eq!(unit(&units, "term:main").component, "term-main");
}

#[test]
fn comments_strings_templates_and_regex_do_not_leak_code() {
    let units = scan(Path::new(FIX));
    let render = unit(&units, "region:render");
    assert_eq!(
        render.routes,
        Vec::<String>::new(),
        "ni el comentario ni la cadena son rutas"
    );
    assert!(
        has(&render.uses_globals, "esc"),
        "esc dentro de una plantilla sí es código"
    );
    assert!(has(&render.uses_globals, "counter"));
    assert_eq!(render.dom_ids, ["board"], "$(\"#board\") es un selector");
    assert!(render.strings >= 3);
    let a = unit(&units, "script:a.js");
    assert_eq!(a.routes, ["/state"]);
    assert!(has(&a.uses_globals, "esc"), "esc dentro de ${{…}} de a.js");
    assert!(has(&a.defines, "tick") && has(&a.defines, "pomoRender") && has(&a.defines, "paint"));
    assert!(
        !has(&a.defines, "re") && !has(&a.defines, "total"),
        "declaraciones locales no son globales"
    );
    assert_eq!(a.mutates_globals, ["counter"]);
}

#[test]
fn parameters_and_iife_scope_are_not_globals() {
    let units = scan(Path::new(FIX));
    let b = unit(&units, "script:b.js");
    assert!(!has(&b.uses_globals, "api"), "api es parámetro de helper");
    assert_eq!(
        b.routes,
        ["/local"],
        "la ruta sí se registra aunque api sea local"
    );
    assert!(
        has(&b.defines, "Bee") && has(&b.defines, "openPane"),
        "root.X = y window.X = exportan"
    );
    assert!(
        !has(&b.defines, "helper") && !has(&b.defines, "KEY"),
        "dentro de la IIFE no hay globales"
    );
    assert_eq!(
        b.storage_keys,
        ["cc-b-key"],
        "la clave se resuelve por su const"
    );
    assert_eq!(b.intervals_ms, vec![60_000]);
    assert!(has(&b.uses_globals, "render"));
    assert_eq!(
        b.post_messages,
        ["webkit.extensions:'close'", "webkit.centro:?"]
    );
    assert_eq!(b.dom_ids, ["b-panel"]);
}

#[test]
fn iframe_page_is_its_own_realm_and_parent_calls_are_captured() {
    let units = scan(Path::new(FIX));
    let term = unit(&units, "term:main");
    assert!(
        has(&term.uses_globals, "esc"),
        "esc de c.js, cargado por term.html"
    );
    assert_eq!(term.parent_refs, ["S", "openPane"]);
    assert_eq!(
        term.post_messages,
        ["comandos-term/ready", "comandos-term/ping"]
    );
    assert_eq!(term.message_types, ["theme"]);
    let helpers = unit(&units, "region:helpers");
    // esc de helpers no lo usa term.html (otro reino): solo render y a.js.
    let ix = interop(&units, Path::new(FIX));
    assert_eq!(
        ix["esc"]["defined_by"],
        json!(["region:helpers", "script:c.js"])
    );
    let used: Vec<&str> = ix["esc"]["used_by"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(
        used.contains(&"region:render")
            && used.contains(&"script:a.js")
            && used.contains(&"term:main")
    );
    assert!(has(&helpers.defines, "esc"));
    assert_eq!(ix["openPane"]["called_by"], json!(["iframe:term.html"]));
    let risks = ix["S"]["risks"].to_string();
    assert!(
        risks.contains("let/const"),
        "parent.S es undefined: S es const: {risks}"
    );
}

#[test]
fn host_calls_resolve_wrappers_variables_and_dom_ids() {
    let units = scan(Path::new(FIX));
    let ix = interop(&units, Path::new(FIX));
    assert_eq!(
        ix["render"]["called_by"],
        json!(["bin/cc-app"]),
        "_dash_js(f\"render(…)\")"
    );
    assert_eq!(ix["Bee"]["called_by"], json!(["bin/cc-app"]));
    assert_eq!(ix["ghostFn"]["defined_by"], json!([]));
    assert!(
        ix["ghostFn"]["risks"]
            .to_string()
            .contains("nadie lo define")
    );
    assert_eq!(
        ix["@dom_ids"]["btn-notif"]["called_by"],
        json!(["bin/cc-app"])
    );
    assert_eq!(ix["@dom_ids"]["btn-notif"]["in_markup"], json!(true));
    let dynamic = ix["@host_dynamic"].as_array().unwrap();
    assert_eq!(dynamic.len(), 1, "{dynamic:?}");
    assert_eq!(dynamic[0]["from"], "bin/cc-app");
    let host_calls = ix["pomoRender"]["host_calls"].as_array().unwrap();
    assert_eq!(host_calls[0]["line"], 16);
    assert_eq!(host_calls[0]["via"], "run_javascript");
    let theme = &ix["@messages"]["comandos/theme"];
    assert_eq!(theme["sent_by"], json!(["bin/cc-app-mac"]));
    assert_eq!(theme["handled_by"], json!(["term:main"]));
    assert_eq!(
        ix["@messages"]["comandos-term/ready"]["sent_by"],
        json!(["term:main"])
    );
    assert_eq!(
        ix["@host_handlers"]["centro"]["registered_by"],
        json!(["bin/cc-app", "bin/cc-app-mac"])
    );
    assert_eq!(
        ix["@host_handlers"]["extensions"]["registered_by"],
        json!([])
    );
    assert_eq!(
        ix["@host_handlers"]["extensions"]["posted_by"],
        json!(["script:b.js"])
    );
    assert!(ix["counter"]["risks"].to_string().contains("mutado"));
}

#[test]
fn port_order_puts_leaves_first() {
    let units = scan(Path::new(FIX));
    let order = xtask::web_inventory::port_order(&units);
    let pos = |id: &str| {
        order
            .iter()
            .position(|g| g.iter().any(|x| x == id))
            .unwrap()
    };
    assert!(pos("region:red") < pos("script:a.js"));
    assert!(pos("region:helpers") < pos("region:render"));
    assert!(pos("script:c.js") < pos("term:main"));
}

#[test]
fn window_assignment_over_a_foreign_global_is_a_patch_not_a_definition() {
    // Review I1: `window.setSplitLeft = (orig => …)(setSplitLeft)` y `window.quickTerminal` en dos regiones.
    let units = scan(Path::new(FIX));
    let b = unit(&units, "script:b.js");
    assert!(
        !has(&b.defines, "render"),
        "b.js parchea render, no lo define"
    );
    assert_eq!(b.patches, ["render"]);
    assert!(has(&b.uses_globals, "render") && has(&b.mutates_globals, "render"));
    let helpers = unit(&units, "region:helpers");
    assert_eq!(
        helpers.patches,
        ["shared"],
        "a.js lo crea antes (orden de carga)"
    );
    let ix = interop(&units, Path::new(FIX));
    assert_eq!(ix["render"]["defined_by"], json!(["region:render"]));
    assert_eq!(ix["render"]["patched_by"], json!(["script:b.js"]));
    assert!(
        ix["render"]["risks"].to_string().contains("parcheado"),
        "{}",
        ix["render"]
    );
    assert_eq!(ix["shared"]["defined_by"], json!(["script:a.js"]));
    assert_eq!(ix["shared"]["patched_by"], json!(["region:helpers"]));
    assert!(ix["shared"]["risks"].to_string().contains("parcheado"));
    // Mismo nombre en dos páginas distintas no es colisión.
    assert!(
        !ix["esc"]["risks"].to_string().contains("definido en"),
        "{}",
        ix["esc"]["risks"]
    );
}

#[test]
fn message_handlers_with_optional_chaining_negation_and_switch() {
    // Review I2: `event.data?.type === …`, `d.type !== "user-interaction"`, `switch (d.type)`.
    let units = scan(Path::new(FIX));
    assert_eq!(unit(&units, "script:a.js").message_types, ["ping", "pong"]);
    assert_eq!(unit(&units, "term:tail").message_types, ["resize", "zoom"]);
    let ix = interop(&units, Path::new(FIX));
    assert_eq!(
        ix["@messages"]["comandos-term/ping"]["handled_by"],
        json!(["script:a.js"])
    );
}

#[test]
fn parent_to_iframe_access_is_an_explicit_contract() {
    // Review I3: `const win = frame.contentWindow; win.__comandosOwnsTouchGestures`, `win.X = true`.
    let units = scan(Path::new(FIX));
    let render = unit(&units, "region:render");
    assert_eq!(
        render.child_refs,
        ["__direct", "__owns"],
        "sin APIs estándar de window"
    );
    assert!(render.child_dom_access, "contentDocument");
    assert_eq!(render.child_writes, ["__wired"]);
    let ix = interop(&units, Path::new(FIX));
    let c = &ix["@frame_contract"];
    assert_eq!(c["__owns"]["read_by"], json!(["region:render"]));
    assert_eq!(c["__owns"]["defined_by"], json!(["term:main"]));
    assert_eq!(c["__wired"]["written_by"], json!(["region:render"]));
    assert_eq!(ix["__owns"]["called_by"], json!(["parent:index.html"]));
}

#[test]
fn local_root_is_not_window() {
    // Review minor 2: `const root = document.documentElement.style; root.background = …`.
    let units = scan(Path::new(FIX));
    let term = unit(&units, "term:main");
    assert!(!has(&term.defines, "background"), "{:?}", term.defines);
    assert!(has(&term.defines, "__owns"));
}

// ---------- web-port ----------

struct Tmp(PathBuf);
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn tmp(tag: &str) -> Tmp {
    let d = std::env::temp_dir().join(format!("xtask-web-port-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("repo")).unwrap();
    for f in ["index.html", "a.js"] {
        fs::copy(Path::new(FIX).join(f), d.join("repo").join(f)).unwrap();
    }
    Tmp(d)
}

fn opts(t: &Tmp) -> PortOptions {
    PortOptions {
        repo: t.0.join("repo"),
        registry: t.0.join("components.json"),
        ported: t.0.join("ported"),
        inventory: None,
    }
}

fn sha(s: &str) -> String {
    xtask::web_inventory::sha256_hex(s.as_bytes())
}

#[test]
fn check_without_registry_is_a_clear_error() {
    let t = tmp("sin-registro");
    let err = web_port::check(&opts(&t)).unwrap_err();
    assert!(matches!(err, PortError::NoRegistry(_)), "{err}");
    assert!(err.to_string().contains("components.json"));
    assert_eq!(err.exit_code(), 3);
}

#[test]
fn check_reports_ok_then_drift_with_diff_and_pin_clears_it() {
    let t = tmp("deriva");
    let o = opts(&t);
    let a = fs::read_to_string(o.repo.join("a.js")).unwrap();
    let html = fs::read_to_string(o.repo.join("index.html")).unwrap();
    let red = region_text(
        &html,
        "// ---------- red ----------",
        "// ---------- helpers ----------",
        None,
    )
    .unwrap();
    let reg = json!([
        {"id": "a", "kind": "script", "source": "a.js", "sha256": sha(&a), "exports": ["pomoRender"], "deps": []},
        {"id": "red", "kind": "region", "source": "index.html",
         "marker_start": "// ---------- red ----------", "marker_end": "// ---------- helpers ----------",
         "sha256": sha(red), "exports": ["api"], "deps": []}
    ]);
    fs::write(&o.registry, serde_json::to_string_pretty(&reg).unwrap()).unwrap();
    fs::create_dir_all(&o.ported).unwrap();
    fs::write(o.ported.join("a.txt"), &a).unwrap();

    let r = web_port::check(&o).unwrap();
    assert!(r.iter().all(|e| e.state == CheckState::Ok), "{r:?}");
    assert_eq!(web_port::exit_code(&r), 0);

    // Otra sesión edita a.js después del port.
    fs::write(o.repo.join("a.js"), a.replace("2000", "5000")).unwrap();
    let r = web_port::check(&o).unwrap();
    let ea = r.iter().find(|e| e.id == "a").unwrap();
    assert!(matches!(ea.state, CheckState::Drift { .. }), "{ea:?}");
    let diff = ea.diff.as_deref().unwrap();
    assert!(
        diff.contains("-setInterval(tick, 2000);") && diff.contains("+setInterval(tick, 5000);"),
        "{diff}"
    );
    assert!(
        diff.contains("--- portado/a\n") && diff.contains("+++ actual/a\n"),
        "{diff}"
    );
    assert_eq!(
        r.iter().find(|e| e.id == "red").unwrap().state,
        CheckState::Ok
    );
    assert_eq!(web_port::exit_code(&r), 1);

    let pinned = web_port::pin(&o, "a").unwrap();
    assert_eq!(
        pinned.sha256,
        sha(&fs::read_to_string(o.repo.join("a.js")).unwrap())
    );
    assert!(
        fs::read_to_string(o.ported.join("a.txt"))
            .unwrap()
            .contains("5000")
    );
    let reg: Value = serde_json::from_str(&fs::read_to_string(&o.registry).unwrap()).unwrap();
    assert_eq!(reg[0]["sha256"], json!(pinned.sha256));
    assert_eq!(
        reg[0]["exports"],
        json!(["pomoRender"]),
        "pin conserva el resto de la entrada"
    );
    assert_eq!(web_port::exit_code(&web_port::check(&o).unwrap()), 0);
}

#[test]
fn missing_marker_or_source_is_reported() {
    let t = tmp("falta");
    let o = opts(&t);
    let reg = json!([
        {"id": "x", "kind": "script", "source": "no-existe.js", "sha256": "00", "exports": [], "deps": []},
        {"id": "y", "kind": "region", "source": "index.html", "marker_start": "// ---------- nada ----------",
         "marker_end": "</script>", "sha256": "00", "exports": [], "deps": []}
    ]);
    fs::write(&o.registry, reg.to_string()).unwrap();
    let r = web_port::check(&o).unwrap();
    assert!(
        r.iter().all(|e| e.state == CheckState::MissingSource),
        "{r:?}"
    );
    assert_eq!(web_port::exit_code(&r), 1);
}

#[test]
fn per_component_files_are_read_and_pin_creates_entries_from_the_inventory() {
    // Preflight R11: una entrada por componente en components/<id>.json.
    let t = tmp("r11");
    let mut o = opts(&t);
    fs::create_dir_all(t.0.join("components")).unwrap();
    o.registry = t.0.join("components.json"); // no existe: solo el directorio hermano
    let units = scan(&o.repo);
    let inv = t.0.join("inventory.json");
    fs::write(
        &inv,
        xtask::web_inventory::inventory_json(&units, &o.repo).to_string(),
    )
    .unwrap();
    o.inventory = Some(inv);
    assert_eq!(web_port::check(&o).unwrap().len(), 0);
    let e = web_port::pin(&o, "red").unwrap();
    assert_eq!(e.kind, "region");
    let saved: Value =
        serde_json::from_str(&fs::read_to_string(t.0.join("components/red.json")).unwrap())
            .unwrap();
    assert_eq!(saved["marker_start"], "// ---------- red ----------");
    assert_eq!(saved["deps"], json!([]));
    assert!(
        fs::read_to_string(o.ported.join("red.txt"))
            .unwrap()
            .starts_with("// ---------- red")
    );
    let r = web_port::check(&o).unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].state, CheckState::Ok);
    assert!(matches!(
        web_port::pin(&o, "no-existe"),
        Err(PortError::UnknownId(_))
    ));
}

#[test]
fn pin_by_unit_id_is_idempotent_with_a_list_registry() {
    // Review I4: `pin region:red` dos veces dejaba dos entradas `red`.
    let t = tmp("idempotente");
    let o = opts(&t);
    let a = fs::read_to_string(o.repo.join("a.js")).unwrap();
    let reg = json!([{"id": "a", "kind": "script", "source": "a.js", "sha256": sha(&a), "exports": [], "deps": []}]);
    fs::write(&o.registry, reg.to_string()).unwrap();
    web_port::pin(&o, "region:red").unwrap();
    web_port::pin(&o, "region:red").unwrap();
    web_port::pin(&o, "red").unwrap();
    web_port::pin(&o, "script:a.js").unwrap();
    let reg: Value = serde_json::from_str(&fs::read_to_string(&o.registry).unwrap()).unwrap();
    let ids: Vec<&str> = reg
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["id"].as_str())
        .collect();
    assert_eq!(ids, ["a", "red"]);
    assert_eq!(web_port::check(&o).unwrap().len(), 2);
}

#[test]
fn doc_lists_patches_in_risks_and_the_frame_contract() {
    let repo = Path::new(FIX);
    let units = scan(repo);
    let inv = xtask::web_inventory::inventory_json(&units, repo);
    let ix = interop(&units, repo);
    let doc = xtask::web_inventory::doc::render(&units, &inv, &ix);
    let risks = doc
        .split("## Globales de riesgo")
        .nth(1)
        .unwrap()
        .split("\n## ")
        .next()
        .unwrap();
    assert!(
        risks.contains("`render`") && risks.contains("parcheado"),
        "{risks}"
    );
    assert!(risks.contains("`shared`"), "{risks}");
    let contract = doc
        .split("## Contrato padre → iframe")
        .nth(1)
        .unwrap()
        .split("\n## ")
        .next()
        .unwrap();
    assert!(
        contract.contains("| `__owns` | `region:render` | — | `term:main` |"),
        "{contract}"
    );
    assert!(doc.contains("`switch (x.type)`"), "límites al día");
}

#[test]
fn guarded_window_assignment_is_a_fallback_not_a_patch() {
    // Re-revisión r1, punto 2: `if(!window.quickTerminal…) window.quickTerminal = …`.
    let units = scan(Path::new(FIX));
    let b = unit(&units, "script:b.js");
    assert_eq!(b.fallbacks, ["lazy", "lazy2"]);
    assert_eq!(b.patches, ["render"]);
    assert!(!has(&b.defines, "lazy") && !has(&b.mutates_globals, "lazy"));
    let ix = interop(&units, Path::new(FIX));
    assert_eq!(ix["lazy"]["defined_by"], json!(["script:a.js"]));
    assert_eq!(ix["lazy"]["fallback_by"], json!(["script:b.js"]));
    assert_eq!(ix["lazy"]["patched_by"], json!([]));
    assert!(
        ix["lazy"]["risks"].to_string().contains("respaldo"),
        "{}",
        ix["lazy"]
    );
}

#[test]
fn frame_contract_skips_window_apis_and_flags_dom_access() {
    // Re-revisión r1, punto 1.
    let units = scan(Path::new(FIX));
    let ix = interop(&units, Path::new(FIX));
    let c = ix["@frame_contract"].as_object().unwrap();
    let keys: Vec<&str> = c.keys().map(String::as_str).collect();
    assert_eq!(keys, ["__direct", "__owns", "__wired"]);
    assert_eq!(ix["@frame_dom_access"], json!(["region:render"]));
}

#[test]
fn messages_through_an_aliased_native_bridge_go_to_host_handlers() {
    // Re-revisión r1, punto 3: `const bridge = …messageHandlers.centro; bridge.postMessage(…)`.
    let units = scan(Path::new(FIX));
    let ix = interop(&units, Path::new(FIX));
    assert_eq!(
        ix["@host_handlers"]["centro"]["posted_by"],
        json!(["script:b.js"])
    );
    assert!(ix["@messages"].get("?").is_none(), "{}", ix["@messages"]);
}

#[test]
fn component_ids_are_unique_even_when_a_script_and_a_region_share_a_name() {
    // Revisión B1, I7: `region:analytics` y `script:analytics.js` daban los dos
    // `analytics`. Con un archivo por componente, el segundo `pin` pisaba al primero.
    let t = tmp("colision");
    fs::write(t.0.join("repo/red.js"), "function redScript(){}\n").unwrap();
    let units = scan(&t.0.join("repo"));
    let mut comps: Vec<&str> = units.iter().map(|u| u.component.as_str()).collect();
    let n = comps.len();
    comps.sort();
    comps.dedup();
    assert_eq!(comps.len(), n, "componentes repetidos: {units:#?}");
    assert_eq!(unit(&units, "script:red.js").component, "red");
    assert_eq!(unit(&units, "region:red").component, "red-inline");
    // Sin choque, el nombre no cambia.
    assert_eq!(unit(&units, "region:helpers").component, "helpers");
}

#[test]
fn pin_refuses_to_overwrite_an_entry_of_another_source() {
    let t = tmp("otra-fuente");
    let mut o = opts(&t);
    fs::create_dir_all(t.0.join("components")).unwrap();
    // Una entrada `red` hecha a mano que apunta a otro origen.
    let entry = json!({"id": "red", "kind": "script", "source": "a.js", "sha256": "00", "exports": [], "deps": []});
    fs::write(t.0.join("components/red.json"), entry.to_string()).unwrap();
    o.registry = t.0.join("components.json");
    let err = web_port::pin(&o, "region:red").unwrap_err();
    assert!(matches!(err, PortError::Invalid(_)), "{err}");
    assert!(err.to_string().contains("no se sobrescribe"), "{err}");
    let kept: Value =
        serde_json::from_str(&fs::read_to_string(t.0.join("components/red.json")).unwrap())
            .unwrap();
    assert_eq!(kept, entry, "la entrada queda intacta");
}
