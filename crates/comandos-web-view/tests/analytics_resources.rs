use comandos_web_view::analytics_resources::html;
use serde_json::{Value, json};

fn snapshot() -> Value {
    json!({
        "sampledAt": 1_791_479_059_000_u64,
        "memory": {
            "totalBytes": 16_u64 << 30,
            "availableBytes": 8_u64 << 30,
            "swapUsedBytes": 0,
            "groups": [
                {"id":"app", "label":"Interfaz", "pssBytes": 256_u64 << 20, "processCount":2},
                {"id":"codex", "label":"Codex", "pssBytes": 2_u64 << 30, "processCount":8}
            ],
            "measuredProcesses":10,
            "unreadableProcesses":0
        },
        "disk": {
            "totalBytes": 2_u64 << 40,
            "availableBytes": 1_u64 << 40,
            "paths":[{"id":"build", "label":"Compilaciones", "path":"/home/jesus/project/.build", "bytes": 1024}]
        },
        "safeguards":{"buildCacheLimitBytes":16_u64 << 30, "minFreeDiskBytes":16_u64 << 30},
        "warnings":[]
    })
}

#[test]
fn resource_snapshot_sums_pss_once_and_preserves_absolute_paths() {
    let markup = html(&snapshot(), "", false);
    assert!(markup.contains("<strong>2.25 GiB</strong>"));
    assert!(markup.contains("<td>256.00 MiB</td>"));
    assert!(markup.contains("<strong>8.00 GiB</strong>"));
    assert!(markup.contains("<strong>1.00 TiB</strong>"));
    assert!(markup.contains("<strong>0 B</strong>"));
    assert!(markup.contains("<code>/home/jesus/project/.build</code>"));
    assert!(markup.contains("Cada proceso aparece en un solo grupo"));
    assert!(markup.contains("sus tamaños no se suman entre sí"));
    assert!(markup.contains("data-tab=\"recursos\""));
    assert!(
        !markup.contains("data-w="),
        "resources do not use a week selector"
    );
}

#[test]
fn resources_do_not_present_unknown_data_as_zero() {
    let markup = html(&json!({"memory":{},"disk":{},"safeguards":{}}), "", false);
    assert!(markup.contains("<strong>—</strong>"));
    assert!(!markup.contains("0 B"));
    assert!(markup.contains("No hay procesos medidos"));
    assert!(markup.contains("No hay tamaños de carpetas disponibles"));
}

#[test]
fn partial_measurements_are_labelled_as_lower_bounds() {
    let mut data = snapshot();
    data["memory"]["unreadableProcesses"] = json!(2);
    data["disk"]["paths"][0]["partial"] = json!(true);
    let markup = html(&data, "", false);
    assert!(markup.contains("≥ 2.25 GiB"));
    assert!(markup.contains("≥ 1.00 KiB"));
    assert!(markup.contains("2 no se pudieron leer"));
    assert!(markup.contains("Medición de RAM parcial"));
    assert!(markup.contains("hay archivos sin contar"));
}

#[test]
fn unreadable_group_keeps_its_observed_count_and_other_groups_stay_measured() {
    let mut data = snapshot();
    data["memory"]["groups"][0]["pssBytes"] = Value::Null;
    data["memory"]["groups"][0]["partial"] = json!(true);
    data["memory"]["measuredProcesses"] = json!(8);
    data["memory"]["unreadableProcesses"] = json!(2);
    let markup = html(&data, "", false);
    assert!(markup.contains("<strong>≥ 2.00 GiB</strong>"));
    assert!(markup.contains("Interfaz</th><td>2</td><td>—</td>"));
    data["memory"]["unreadableProcesses"] = json!(0);
    data["memory"]["partial"] = json!(true);
    assert!(html(&data, "", false).contains("Medición de RAM parcial"));
}

#[test]
fn resource_errors_are_retryable_and_keep_the_last_snapshot() {
    let initial = html(&Value::Null, "sin conexión", false);
    assert!(initial.contains("No pude actualizar los recursos: sin conexión"));
    assert!(initial.contains("Pulsa Actualizar"));
    assert!(!initial.contains("data-resource-refresh disabled"));
    let loading = html(&Value::Null, "", true);
    assert!(loading.contains("data-resource-refresh disabled"));
    assert!(loading.contains("Leyendo la memoria y el disco"));
    let stale = html(&snapshot(), "sin conexión", false);
    assert!(stale.contains("Se conserva la última medición"));
    assert!(stale.contains("2.25 GiB"));
}

#[test]
fn every_external_label_path_warning_and_error_is_escaped() {
    let mut data = snapshot();
    data["memory"]["groups"][0]["label"] = json!("<script>bad()</script>");
    data["disk"]["paths"][0]["label"] = json!("<img src=x onerror=bad()>");
    data["disk"]["paths"][0]["path"] = json!("/home/jesus/<b>&\"'");
    data["warnings"] = json!(["<svg onload=bad()>"]);
    let markup = html(&data, "<i>error</i>", false);
    for unsafe_text in ["<script>", "<img", "<svg", "<i>error", "/home/jesus/<b>"] {
        assert!(!markup.contains(unsafe_text));
    }
    assert!(markup.contains("&lt;script&gt;bad()&lt;/script&gt;"));
    assert!(markup.contains("/home/jesus/&lt;b&gt;&amp;&quot;&#39;"));
}
