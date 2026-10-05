//! El registro de componentes portados (preflight R11: un archivo por
//! componente en `components/<id>.json`) contra `registry::COMPONENTS` y
//! contra el inventario de B3 (`xtask/web/{inventory,interop}.json`, que se
//! leen de su sitio: no hay copia en este crate).
use comandos_web::registry::{COMPONENTS, Report, meta_ids, mount_all};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const INVENTORY: &str = include_str!("../../../xtask/web/inventory.json");
const INTEROP: &str = include_str!("../../../xtask/web/interop.json");

fn components_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("components")
}

/// `(nombre de archivo, entrada)` de cada `components/*.json`, ordenados.
fn entries() -> Vec<(String, Map<String, Value>)> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(components_dir())
        .expect("components/ existe (lo lee `xtask web-port`)")
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|p| {
            let text = std::fs::read_to_string(&p).unwrap();
            let v: Value =
                serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            let m = v
                .as_object()
                .cloned()
                .unwrap_or_else(|| panic!("{}: no es un objeto", p.display()));
            (p.file_name().unwrap().to_string_lossy().into_owned(), m)
        })
        .collect()
}

fn strs(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Lo que una entrada debe declarar en `exports`: cada global que define una
/// de sus unidades y que consume algo de fuera de esas unidades (otra unidad,
/// `cc-app`, `cc-app-mac` o un iframe), o que otra unidad parchea o respalda.
fn required_exports(component: &str, inventory: &Value, interop: &Value) -> BTreeSet<String> {
    let units: BTreeSet<String> = inventory["units"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|u| u["component"] == component)
        .filter_map(|u| u["id"].as_str().map(str::to_string))
        .collect();
    let mut out = BTreeSet::new();
    for (name, g) in interop.as_object().into_iter().flatten() {
        if name.starts_with('@') || !strs(&g["defined_by"]).iter().any(|d| units.contains(d)) {
            continue;
        }
        let external = [
            "used_by",
            "mutated_by",
            "called_by",
            "patched_by",
            "fallback_by",
        ]
        .iter()
        .flat_map(|k| strs(&g[*k]))
        .any(|c| !units.contains(&c));
        if external {
            out.insert(name.clone());
        }
    }
    out
}

fn missing_exports(entry: &Map<String, Value>, inventory: &Value, interop: &Value) -> Vec<String> {
    let id = entry["id"].as_str().unwrap_or_default();
    let declared: BTreeSet<String> = strs(&entry["exports"]).into_iter().collect();
    required_exports(id, inventory, interop)
        .into_iter()
        .filter(|n| !declared.contains(n))
        .collect()
}

#[test]
fn components_dir_parses_and_ids_are_unique() {
    let list = entries();
    let mut ids: Vec<&str> = list.iter().filter_map(|(_, c)| c["id"].as_str()).collect();
    assert_eq!(ids.len(), list.len(), "toda entrada tiene id");
    let n = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(n, ids.len(), "ids repetidos");
}

#[test]
fn each_entry_is_well_formed_and_named_after_its_id() {
    let inventory: Value = serde_json::from_str(INVENTORY).unwrap();
    let known: BTreeSet<&str> = inventory["units"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|u| u["component"].as_str())
        .collect();
    for (file, e) in entries() {
        let id = e["id"].as_str().unwrap_or_default();
        assert_eq!(
            file,
            format!("{id}.json"),
            "una entrada por archivo, con su id"
        );
        assert!(
            known.contains(id),
            "{id}: no es un componente del inventario"
        );
        let kind = e["kind"].as_str().unwrap_or_default();
        assert!(
            ["script", "region", "page"].contains(&kind),
            "{id}: kind {kind:?}"
        );
        assert!(
            e["source"].as_str().is_some_and(|s| s.starts_with("dash/")),
            "{id}: source"
        );
        let sha = e["sha256"].as_str().unwrap_or_default();
        assert!(
            sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit()),
            "{id}: sha256 sin fijar (`xtask web-port pin {id}`)"
        );
        if kind == "region" {
            for k in ["marker_start", "marker_end"] {
                assert!(e[k].as_str().is_some_and(|s| !s.is_empty()), "{id}: {k}");
            }
        }
        for k in ["exports", "deps"] {
            assert!(
                e[k].as_array()
                    .is_some_and(|a| a.iter().all(Value::is_string)),
                "{id}: {k} debe ser una lista de cadenas"
            );
        }
        for d in strs(&e["deps"]) {
            assert!(
                known.contains(d.as_str()),
                "{id}: dependencia desconocida {d}"
            );
        }
    }
}

#[test]
fn registry_matches_the_component_files() {
    let rust: Vec<&str> = COMPONENTS.iter().map(|c| c.id).collect();
    let unique: BTreeSet<&str> = rust.iter().copied().collect();
    assert_eq!(unique.len(), rust.len(), "COMPONENTS con ids repetidos");
    let files: BTreeSet<String> = entries()
        .iter()
        .filter_map(|(_, e)| e["id"].as_str().map(str::to_string))
        .collect();
    let rust: BTreeSet<String> = unique.iter().map(|s| s.to_string()).collect();
    assert_eq!(
        rust, files,
        "cada componente con mount tiene su components/<id>.json y viceversa"
    );
}

#[test]
fn interop_exports_are_declared() {
    let inventory: Value = serde_json::from_str(INVENTORY).unwrap();
    let interop: Value = serde_json::from_str(INTEROP).unwrap();
    for (_, e) in entries() {
        let missing = missing_exports(&e, &inventory, &interop);
        assert!(
            missing.is_empty(),
            "{}: faltan en exports {missing:?}",
            e["id"]
        );
    }
}

#[test]
fn the_exports_check_catches_a_missing_global() {
    // La regla misma, con un componente real del inventario: `work-marks`
    // define `WorkMarks`, que otras unidades leen.
    let inventory: Value = serde_json::from_str(INVENTORY).unwrap();
    let interop: Value = serde_json::from_str(INTEROP).unwrap();
    let req = required_exports("work-marks", &inventory, &interop);
    assert!(req.contains("WorkMarks"), "{req:?}");
    let entry = |exports: Vec<String>| {
        json!({"id": "work-marks", "exports": exports})
            .as_object()
            .cloned()
            .unwrap()
    };
    assert_eq!(
        missing_exports(&entry(vec![]), &inventory, &interop),
        req.iter().cloned().collect::<Vec<_>>()
    );
    assert!(
        missing_exports(&entry(req.iter().cloned().collect()), &inventory, &interop).is_empty()
    );
    // Lo que solo consume la propia unidad no cuenta.
    let helpers = required_exports("helpers", &inventory, &interop);
    assert!(!helpers.is_empty());
    assert!(helpers.iter().all(|n| {
        interop[n.as_str()]["defined_by"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d == "region:helpers")
    }));
}

#[test]
fn frame_contract_names_match_the_inventory() {
    let interop: Value = serde_json::from_str(INTEROP).unwrap();
    let inv: BTreeSet<&str> = interop["@frame_contract"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let ours: BTreeSet<&str> = comandos_web_dom::bridge::FRAME_CONTRACT
        .iter()
        .copied()
        .collect();
    assert_eq!(
        ours, inv,
        "actualiza bridge::FRAME_CONTRACT y su documentación"
    );
}

#[test]
fn meta_ids_split_on_whitespace_and_drop_repeats() {
    assert_eq!(
        meta_ids("  red\thelpers\n red  iconos "),
        ["red", "helpers", "iconos"]
    );
    assert!(meta_ids("").is_empty());
}

#[test]
fn mount_all_keeps_going_after_a_failure_and_keeps_order() {
    fn ok() -> Result<(), String> {
        Ok(())
    }
    fn bad() -> Result<(), String> {
        Err("se rompió".into())
    }
    let known: BTreeMap<&str, fn() -> Result<(), String>> = [
        ("a", ok as fn() -> Result<(), String>),
        ("b", bad),
        ("c", ok),
    ]
    .into_iter()
    .collect();
    let r: Report = mount_all(
        &["c", "x", "b", "a"],
        |id| known.get(id).copied(),
        |e| e.clone(),
    );
    assert_eq!(r.mounted, ["c", "a"]);
    assert_eq!(
        r.failed,
        [
            (
                "x".to_string(),
                "componente desconocido en este WASM".to_string()
            ),
            ("b".to_string(), "se rompió".to_string())
        ]
    );
}
