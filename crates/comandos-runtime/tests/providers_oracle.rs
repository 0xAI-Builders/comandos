//! Registro real de config/providers.json contra lib/providers.py y bin/cc-dash.
#[path = "support/python.rs"]
mod python;

use comandos_core::json::response_dumps;
use comandos_runtime::model_catalog::catalog_paths;
use comandos_runtime::providers::{
    RegistryCache, agent_set, engine_for_model, harness_has_accounts, model_tier, process_aliases,
    proxy_port, py_regex, py_search, read_conf, runtime_facts, selectable_routes, tier_symbol,
    validate_registry, which, which_path,
};
use python::{repo, run_python};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, os::unix::fs::PermissionsExt, path::Path};

const ORACLE: &str = r#"
import importlib.machinery, importlib.util, json, os, sys
repo, case = sys.argv[1:3]
sys.path.insert(0, os.path.join(repo, "bin"))
loader = importlib.machinery.SourceFileLoader("cc_dash_oracle", os.path.join(repo, "bin/cc-dash"))
spec = importlib.util.spec_from_loader(loader.name, loader)
dash = importlib.util.module_from_spec(spec)
loader.exec_module(dash)
c = json.load(open(case))
reg = dash.provider_registry.load_registry(os.path.join(repo, "config/providers.json"))
dash.load_provider_registry = lambda: reg
dash.provider_registry.which = lambda b: "/bin/" + b if b in c["available"] else None
dash.account_registry.public_accounts = lambda r: c["discovered"]
dash.proxy_alive = lambda: c["alive"]
dash.shutil.which = lambda n: None
facts = dash.provider_runtime_facts()
routes = sorted(x["id"] for x in dash.provider_registry.evaluate_capability_matrix(reg, facts) if x.get("selectable"))
engines = [dash.provider_registry.engine_for_model(reg, m) for m in c["models"]]
tiers = json.load(open(os.path.join(repo, "config/model-tiers.json")))
dash.load_model_tiers = lambda: tiers
print(json.dumps({"facts": facts, "routes": routes, "engines": engines,
                  "tiers": [dash.model_tier(m) for m in c["models"]]}))
"#;

fn temp_home(tag: &str) -> std::path::PathBuf {
    let home = std::env::temp_dir().join(format!("cmd-prov-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(&home).unwrap();
    home
}

#[test]
fn providers_match_python_oracle() {
    let home = temp_home("main");
    let case = json!({
        "available": ["claude", "codex"],
        "discovered": {"claude": [{"alias": "main", "selectable": true}], "codex": [{"alias": "main", "selectable": false}], "grok": []},
        "alive": false,
        "models": ["claude-fable-5[1m]", "gpt-5.6-sol", "grok-4.5", "opencode/x", "gemini-3", "fable", "nada"]
    });
    let file = home.join("case.json");
    fs::write(&file, case.to_string()).unwrap();
    let Some(expected) = run_python(ORACLE, &[file.as_os_str()], &home) else {
        return;
    };
    let registry: Value =
        serde_json::from_str(&fs::read_to_string(repo().join("config/providers.json")).unwrap())
            .unwrap();
    assert_eq!(validate_registry(&registry), Ok(Ok(())));
    let tiers: Value =
        serde_json::from_str(&fs::read_to_string(repo().join("config/model-tiers.json")).unwrap())
            .unwrap();
    let available = |b: &str| matches!(b, "claude" | "codex");
    let facts = runtime_facts(
        &registry,
        &case["discovered"],
        &available,
        &home,
        false,
        false,
    )
    .unwrap();
    let models: Vec<&str> = case["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m.as_str().unwrap())
        .collect();
    let got = json!({
        "facts": facts,
        "routes": selectable_routes(&registry, &facts).unwrap(),
        "engines": models.iter().map(|m| engine_for_model(&registry, m).unwrap()).collect::<Vec<_>>(),
        "tiers": models.iter().map(|m| model_tier(&tiers, m).unwrap()).collect::<Vec<_>>(),
    });
    assert_eq!(response_dumps(&got).unwrap(), expected.trim_end());
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn python_only_regex_is_unsure() {
    assert!(py_regex(r"(?<=a)b", true).is_err());
    assert!(py_regex(r"(a)\1", false).is_err());
    let re = py_regex(r"^opus$", true).unwrap();
    assert_eq!(py_search(&re, "OPUS"), Ok(true));
    assert!(py_search(&re, "opus\n").is_err());
    assert!(py_search(&re, "ópus").is_err());
    assert_eq!(py_search(&py_regex(r"x\Z", false).unwrap(), "ax"), Ok(true));
}

#[test]
fn invalid_registry_reports_python_message() {
    assert_eq!(
        validate_registry(&json!({"version": 3})),
        Ok(Err("providers.json version must be 1 or 2".into()))
    );
}

/// Patrones y sujetos contra `re.search` del Python: donde la traducción no es
/// incierta, el resultado es el mismo.
#[test]
fn regex_translation_matches_python_re() {
    const RE_ORACLE: &str = r#"
import json, re, sys
cases = json.loads(sys.argv[2])
out = []
for p, flags, subjects in cases:
    try:
        rx = re.compile(p, re.I if flags else 0)
    except re.error:
        out.append(None)
        continue
    out.append([bool(rx.search(s)) for s in subjects])
print(json.dumps(out))
"#;
    let subjects = vec![
        "",
        "a",
        "ab",
        "AB",
        "gpt-5.4",
        "gpt-5.4-mini",
        "GPT-5.4-MINI-x",
        "gpt-5.4-nano",
        "x-pro",
        "x-pro2",
        "x-PROz",
        "claude-opus-4",
        "opus",
        "Opus 4",
        "{a}",
        "a{2}",
        "aa",
        "a b",
        "a\tb",
        "]",
        "-",
        "^",
        "a-b",
        "o3",
        "codex",
        "xai/grok",
        "\\",
        "ab_c",
        "Z",
        "z",
        "[",
    ];
    let patterns: Vec<(&str, bool)> = vec![
        (r"^(?:claude-|opus$|sonnet$|haiku$|fable$)", true),
        (r"^(?:gpt-|o[0-9]|codex)", true),
        (r"^(?:grok-|xai/)", true),
        (r"grok-4\.(6|5)", true),
        (r"gpt-5\.6|codex-max|-pro($|[^a-z])", true),
        (r"sonnet|gpt-5\.5|gpt-5\.4(?!-mini)|terra|sol|luna", true),
        (r"gpt-5\.4(?=-)", true),
        (r"(?!x)", false),
        (r"a{2}", false),
        (r"a{,1}b", false),
        (r"a{}", false),
        (r"{a}", false),
        (r"a{1,2", false),
        (r"[]]", false),
        (r"[^]]", false),
        (r"[a-]", false),
        (r"[-a]", false),
        (r"[\]]", false),
        (r"[\\]", false),
        (r"[[]", false),
        (r"[a-z]+", true),
        (r"[Z-a]", false),
        (r"\w+_\w", false),
        (r"\bab\b", true),
        (r"a\sb", false),
        (r"a\Sb", false),
        (r"\Aa", false),
        (r"b\Z", false),
        (r"a|", false),
        (r"|a", false),
        (r"a*?b", false),
        (r"a.b", false),
        (r"\-", false),
        (r"\^", false),
        (r"a\ b", false),
        (r"(a)(b)", true),
        (r"$^", true),
        (r"[^a-z]", true),
        (r"]", false),
        (r"}", false),
    ];
    let cases: Vec<Value> = patterns
        .iter()
        .map(|(p, f)| json!([p, f, subjects]))
        .collect();
    let home = temp_home("re");
    let arg = Value::Array(cases).to_string();
    let Some(out) = run_python(RE_ORACLE, &[std::ffi::OsStr::new(&arg)], &home) else {
        return;
    };
    let expected: Vec<Value> = serde_json::from_str(&out).unwrap();
    for ((pattern, flags), want) in patterns.iter().zip(expected) {
        // Todos estos patrones se pueden juzgar con certeza.
        let re = py_regex(pattern, *flags).unwrap();
        assert!(
            !want.is_null(),
            "{pattern:?}: el Python no compila y el port sí"
        );
        for (subject, want) in subjects.iter().zip(want.as_array().unwrap()) {
            if let Ok(got) = py_search(&re, subject) {
                assert_eq!(Value::Bool(got), *want, "{pattern:?} sobre {subject:?}");
            }
        }
    }
    // Sintaxis que el Python rechaza o lee distinto que `regex`: inciertas.
    for (pattern, flags) in [
        (r"a**", false),
        (r"^*", false),
        (r"a{2}{3}", false),
        (r"\p{L}", false),
        (r"(?P<x>a)", false),
        (r"(?i)a", false),
        (r"\x41", false),
        (r"\1", false),
        (r"é", true),
        (r"[Z-a]", true),
        (r"[a-\d]", false),
        (r"(a", false),
        (r"a)", false),
        (r"[a", false),
    ] {
        assert!(py_regex(pattern, flags).is_err(), "{pattern}");
    }
    let _ = fs::remove_dir_all(&home);
}

/// Mensajes de `ProviderRegistryError` sobre registros inválidos.
#[test]
fn validate_registry_matches_python_messages() {
    const VALIDATE_ORACLE: &str = r#"
import json, os, sys
repo, cases = sys.argv[1:3]
sys.path.insert(0, os.path.join(repo, "lib"))
import providers
out = []
for data in json.load(open(cases)):
    try:
        providers.validate_registry(data)
        out.append("OK")
    except providers.ProviderRegistryError as e:
        out.append(str(e))
    except Exception as e:
        out.append("EXC " + type(e).__name__)
print(json.dumps(out))
"#;
    let base: Value =
        serde_json::from_str(&fs::read_to_string(repo().join("config/providers.json")).unwrap())
            .unwrap();
    let edit = |f: &dyn Fn(&mut Value)| {
        let mut v = base.clone();
        f(&mut v);
        v
    };
    let cases = vec![
        json!({"version": 3}),
        json!({"version": true, "harnesses": {"a": {"label": "A"}}, "claudeEngines": {"b": {"label": "B"}}}),
        json!({"version": 1.0, "harnesses": {}, "claudeEngines": {"b": {"label": "B"}}}),
        json!({"version": 1, "harnesses": {"A": {"label": "x"}}, "claudeEngines": {}}),
        json!({"version": 1, "harnesses": {"a": []}, "claudeEngines": {}}),
        json!({"version": 1, "harnesses": {"a": {"label": "  "}}, "claudeEngines": {}}),
        json!({"version": 1, "harnesses": {"a": {"label": 5}}, "claudeEngines": {"b": {"label": "B", "modelMatch": "^x"}}}),
        json!({"version": 1, "harnesses": {"a": {"label": "A", "models": [{"id": ""}]}}, "claudeEngines": {"b": {"label": "B"}}}),
        json!({"version": 1, "harnesses": {"a": {"label": "A", "models": "xy"}}, "claudeEngines": {"b": {"label": "B"}}}),
        json!({"version": 1, "harnesses": {"a": {"label": "A", "models": [{"id": "m", "efforts": ["hi", "hi"]}]}}, "claudeEngines": {"b": {"label": "B"}}}),
        json!({"version": 1, "harnesses": {"a": {"label": "A", "models": [{"id": "m", "efforts": [1, true]}]}}, "claudeEngines": {"b": {"label": "B"}}}),
        json!({"version": 1, "harnesses": {"a": {"label": "A", "models": [{"id": "m", "efforts": "aa"}]}}, "claudeEngines": {"b": {"label": "B"}}}),
        json!({"version": 1, "harnesses": {"a": {"label": "A", "models": [{"id": 7, "efforts": ["lo"], "defaultEffort": "hi"}]}}, "claudeEngines": {"b": {"label": "B"}}}),
        json!({"version": 1, "harnesses": {"a": {"label": "A", "models": [{"id": "m", "efforts": "high", "defaultEffort": "ig"}]}}, "claudeEngines": {"b": {"label": "B"}}}),
        json!({"version": 1, "harnesses": {"a": {"label": "A"}}, "claudeEngines": {"b": {"label": "B"}}}),
        edit(&|v| v["motors"] = json!({})),
        edit(&|v| v["motors"]["nope"] = json!({"label": "x"})),
        edit(&|v| v["matrixHarnesses"] = json!(["claude", "codex"])),
        edit(&|v| v["matrixHarnesses"] = json!(["claude", "codex", "grok", "zzz"])),
        edit(&|v| v["acpAgents"]["claude"]["command"] = json!("")),
        edit(&|v| v["routes"][0]["id"] = json!("Bad Id")),
        edit(&|v| v["routes"][1]["id"] = json!("claude:claude")),
        edit(&|v| v["routes"][1]["harness"] = json!("claude")),
        edit(&|v| v["routes"][1]["motor"] = json!(5)),
        edit(&|v| v["routes"][1]["actionScopes"] = json!(["fly"])),
        edit(&|v| v["routes"][0]["driver"] = json!("acp")),
        edit(&|v| v["exclusions"][0]["motor"] = json!("codex")),
        edit(&|v| {
            v["exclusions"].as_array_mut().unwrap().remove(0);
        }),
        edit(&|v| v["exclusions"][0]["harness"] = json!("zz")),
        edit(&|v| v["coreHarnesses"] = json!(["claude", "codex"])),
        edit(&|v| v["acpAgents"] = json!({})),
        edit(&|v| v["acpAgents"] = json!([1])),
        edit(&|v| v["routes"] = json!([5])),
        edit(&|v| v["matrixHarnesses"] = json!(5)),
        edit(&|v| v["coreHarnesses"] = json!(7)),
        edit(&|v| v["routes"][0]["id"] = json!(["x:y"])),
        json!([]),
        json!({"version": 1, "harnesses": {"a": {"label": "A", "models": [{"id": "m", "efforts": [[1]]}]}}, "claudeEngines": {"b": {"label": "B"}}}),
        json!({"version": 1, "harnesses": {"a": {"label": "A", "models": 5}}, "claudeEngines": {"b": {"label": "B"}}}),
    ];
    let home = temp_home("validate");
    let file = home.join("cases.json");
    fs::write(&file, Value::Array(cases.clone()).to_string()).unwrap();
    let Some(out) = run_python(VALIDATE_ORACLE, &[file.as_os_str()], &home) else {
        return;
    };
    let expected: Vec<String> = serde_json::from_str(&out).unwrap();
    for (data, want) in cases.iter().zip(expected) {
        let got = match validate_registry(data) {
            Ok(Ok(())) => "OK".to_string(),
            Ok(Err(message)) => message,
            Err(_) => "UNSURE".to_string(),
        };
        // Otra excepción del Python (no `ProviderRegistryError`) → incierto.
        if want.starts_with("EXC ") {
            assert_eq!(got, "UNSURE", "{data}");
        } else {
            assert_eq!(got, want, "{data}");
        }
    }
    let _ = fs::remove_dir_all(&home);
}

/// `load_provider_registry`: registro hidratado con el catálogo de CLI del HOME.
#[test]
fn registry_cache_matches_python_hydration() {
    const LOAD_ORACLE: &str = r#"
import importlib.machinery, importlib.util, json, os, sys
repo, providers = sys.argv[1:3]
sys.path.insert(0, os.path.join(repo, "bin"))
loader = importlib.machinery.SourceFileLoader("cc_dash_oracle", os.path.join(repo, "bin/cc-dash"))
spec = importlib.util.spec_from_loader(loader.name, loader)
dash = importlib.util.module_from_spec(spec)
loader.exec_module(dash)
dash.PROVIDERS_FILE = providers
print(json.dumps(dash.load_provider_registry()))
"#;
    let home = temp_home("cache");
    let codex = home.join(".codex");
    fs::create_dir_all(&codex).unwrap();
    fs::write(
        codex.join("models_cache.json"),
        json!({"fetched_at": "2026-10-04T00:00:00Z", "models": [
            {"slug": "gpt-9.1-test", "visibility": "list", "display_name": "GPT 9.1",
             "supported_reasoning_levels": [{"effort": "low"}, {"effort": "high"}],
             "default_reasoning_level": "high", "context_window": 400000}
        ]})
        .to_string(),
    )
    .unwrap();
    let providers = home.join("providers.json");
    fs::copy(repo().join("config/providers.json"), &providers).unwrap();
    let Some(out) = run_python(LOAD_ORACLE, &[providers.as_os_str()], &home) else {
        return;
    };
    let paths = catalog_paths(&home, &repo(), None, None);
    let mut cache = RegistryCache::default();
    let got = cache.load(&providers, &paths).unwrap();
    assert_eq!(response_dumps(&got).unwrap(), out.trim_end());
    // Segunda lectura sin cambios: la misma copia.
    assert_eq!(cache.load(&providers, &paths).unwrap(), got);
    // Registro inválido o JSON roto: incierto, sin caché.
    fs::write(&providers, r#"{"version": 9}"#).unwrap();
    assert!(cache.load(&providers, &paths).is_err());
    fs::write(&providers, "{").unwrap();
    assert!(cache.load(&providers, &paths).is_err());
    assert!(cache.load(&home.join("absent.json"), &paths).is_err());
    let _ = fs::remove_dir_all(&home);
}

fn executable(path: &Path) {
    fs::write(path, "#!/bin/sh\n").unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// `provider_registry.which` y `shutil.which` con un PATH controlado.
#[test]
fn which_matches_python() {
    const WHICH_ORACLE: &str = r#"
import json, os, shutil, sys
repo, path, names = sys.argv[1], sys.argv[2], json.loads(sys.argv[3])
sys.path.insert(0, os.path.join(repo, "lib"))
import providers
if path == "<none>":
    os.environ.pop("PATH", None)
else:
    os.environ["PATH"] = path
print(json.dumps([[providers.which(n), shutil.which(n)] for n in names]))
"#;
    let home = temp_home("which");
    let first = home.join("first");
    let second = home.join("second");
    let local = home.join(".local/bin");
    let bun = home.join(".bun/bin");
    for dir in [&first, &second, &local, &bun] {
        fs::create_dir_all(dir).unwrap();
    }
    executable(&first.join("alpha"));
    executable(&second.join("alpha"));
    executable(&second.join("beta"));
    executable(&local.join("gamma"));
    executable(&bun.join("gamma"));
    executable(&bun.join("delta"));
    fs::write(first.join("plain"), "x").unwrap();
    executable(&local.join("plain"));
    fs::create_dir_all(first.join("dir")).unwrap();
    fs::create_dir_all(local.join("dir")).unwrap();
    std::os::unix::fs::symlink(first.join("missing"), first.join("broken")).unwrap();
    let absolute = second.join("beta").display().to_string();
    let names = json!([
        "alpha",
        "beta",
        "gamma",
        "delta",
        "plain",
        "dir",
        "broken",
        "nope",
        "",
        absolute,
        "second/beta"
    ]);
    let paths = vec![
        format!("{}:{}", first.display(), second.display()),
        format!("{}::{}", first.display(), second.display()),
        String::new(),
        "<none>".to_string(),
    ];
    for path in paths {
        let arg_names = names.to_string();
        let Some(out) = run_python(
            WHICH_ORACLE,
            &[
                std::ffi::OsStr::new(&path),
                std::ffi::OsStr::new(&arg_names),
            ],
            &home,
        ) else {
            return;
        };
        let expected: Vec<Value> = serde_json::from_str(&out).unwrap();
        let env_path = (path != "<none>").then(|| std::ffi::OsString::from(&path));
        for (name, want) in names.as_array().unwrap().iter().zip(expected) {
            let name = name.as_str().unwrap();
            let show = |p: Option<std::path::PathBuf>| {
                p.map_or(Value::Null, |p| Value::String(p.display().to_string()))
            };
            // El oráculo corre con el directorio del repositorio como actual.
            let got = json!([
                show(which(name, env_path.as_deref(), &home)),
                show(which_path(name, env_path.as_deref())),
            ]);
            if path.contains("::") && name == "second/beta" {
                // Relativa al directorio actual de cada proceso: no se compara.
                continue;
            }
            assert_eq!(got, want, "PATH={path:?} name={name:?}");
        }
    }
    let _ = fs::remove_dir_all(&home);
}

/// `read_conf`, `agent_set` y `agent_process_aliases` contra `bin/cc-dash`.
#[test]
fn conf_agents_and_aliases_match_python() {
    const CONF_ORACLE: &str = r#"
import importlib.machinery, importlib.util, json, os, sys
repo, reg = sys.argv[1:3]
sys.path.insert(0, os.path.join(repo, "bin"))
loader = importlib.machinery.SourceFileLoader("cc_dash_oracle", os.path.join(repo, "bin/cc-dash"))
spec = importlib.util.spec_from_loader(loader.name, loader)
dash = importlib.util.module_from_spec(spec)
loader.exec_module(dash)
registry = json.load(open(reg))
dash.load_provider_registry = lambda: registry
print(json.dumps({"conf": dash.read_conf(), "agents": sorted(dash.agent_set()),
                  "aliases": dict(sorted(dash.agent_process_aliases().items()))}))
"#;
    let home = temp_home("conf");
    let hooks = home.join(".claude/hooks");
    fs::create_dir_all(&hooks).unwrap();
    let conf = hooks.join("cc-notify.conf");
    let registry: Value =
        serde_json::from_str(&fs::read_to_string(repo().join("config/providers.json")).unwrap())
            .unwrap();
    let mut extra = registry.clone();
    extra["harnesses"]["claude"]["processNames"] = json!(["claude", "bin/claude-x", "node", 7]);
    extra["harnesses"]["zeta"] = json!({"label": "Z", "processNames": "zq"});
    extra["harnesses"]["codex"]["processNames"] = json!(["codex", "zeta-like"]);
    let reg_file = home.join("registry.json");
    let confs = [
        None,
        Some(
            "# c\nAGENTS=\"claude zeta\"\r\nX = 'y'\rK=v=w\n  =e\nQ=\"\nQ=\"two\"\n\u{1c}T=1\u{1d}\n",
        ),
        Some("AGENTS=\nOTHER=1\n"),
        Some("AGENTS=  codex\tgrok  \n"),
    ];
    for (index, text) in confs.iter().enumerate() {
        let _ = fs::remove_file(&conf);
        if let Some(text) = text {
            fs::write(&conf, text).unwrap();
        }
        let reg = if index == 1 { &extra } else { &registry };
        fs::write(&reg_file, reg.to_string()).unwrap();
        let Some(out) = run_python(CONF_ORACLE, &[reg_file.as_os_str()], &home) else {
            return;
        };
        let pairs = read_conf(&conf).unwrap();
        let agents = agent_set(
            pairs
                .iter()
                .find(|(k, _)| k == "AGENTS")
                .map(|(_, v)| v.as_str()),
            reg,
        );
        let aliases: BTreeMap<String, String> = process_aliases(&agents, reg).into_iter().collect();
        let mut conf_obj = serde_json::Map::new();
        for (k, v) in pairs {
            conf_obj.insert(k, Value::String(v));
        }
        let got = json!({"conf": conf_obj, "agents": agents, "aliases": aliases});
        assert_eq!(
            response_dumps(&got).unwrap(),
            out.trim_end(),
            "conf {index}"
        );
    }
    // Ilegible (un directorio): el Python lanzaría.
    let _ = fs::remove_file(&conf);
    fs::create_dir_all(&conf).unwrap();
    assert!(read_conf(&conf).is_err());
    fs::remove_dir(&conf).unwrap();
    fs::write(&conf, b"A=\xff\n").unwrap();
    assert!(read_conf(&conf).is_err());
    assert!(harness_has_accounts(&registry, "claude"));
    assert!(!harness_has_accounts(&registry, "acp"));
    assert!(!harness_has_accounts(&registry, "nope"));
    let _ = fs::remove_dir_all(&home);
}

/// `model_tier`, el símbolo de `write_app_tab_models` y `int(port or 18765)`.
#[test]
fn tiers_and_proxy_port_match_python() {
    const TIER_ORACLE: &str = r#"
import importlib.machinery, importlib.util, json, os, sys
repo, case = sys.argv[1:3]
sys.path.insert(0, os.path.join(repo, "bin"))
loader = importlib.machinery.SourceFileLoader("cc_dash_oracle", os.path.join(repo, "bin/cc-dash"))
spec = importlib.util.spec_from_loader(loader.name, loader)
dash = importlib.util.module_from_spec(spec)
loader.exec_module(dash)
c = json.load(open(case))
out = []
for tiers, models in c["tiers"]:
    dash.load_model_tiers = lambda: tiers
    row = []
    for m in models:
        try:
            t = dash.model_tier(m)
            row.append([t, (dash.tier_style(t).get("symbol") or "") if t else ""])
        except Exception as e:
            row.append("EXC")
    out.append(row)
ports = []
for root in c["roots"]:
    dash.PROXY_FILE = os.path.join(root, "config", "proxy.json")
    try:
        ports.append(int(dash.load_proxy_cfg().get("port") or 18765))
    except Exception:
        ports.append("EXC")
print(json.dumps({"tiers": out, "ports": ports}))
"#;
    let home = temp_home("tiers");
    let real: Value =
        serde_json::from_str(&fs::read_to_string(repo().join("config/model-tiers.json")).unwrap())
            .unwrap();
    let models = json!([
        "",
        "claude-fable-5[1m]",
        "GPT-5.4",
        "gpt-5.4-mini",
        "sonnet-4",
        "nada",
        "x-pro",
        "mini-z"
    ]);
    let tier_cases = json!([
        [real, models],
        [{"patterns": [{"match": "a", "tier": "t"}, {"tier": "u"}], "defaultTier": "d",
          "tiers": {"t": {"symbol": "$"}, "u": {"symbol": 0}, "d": []}}, ["b", "a", "zz"]],
        [{"patterns": [], "defaultTier": "", "tiers": []}, ["x"]],
        [{"patterns": null, "tiers": {"": {"symbol": "?"}}}, ["x"]],
        [{"patterns": [{"match": "x", "tier": "t"}], "tiers": {"t": "nope"}}, ["x", "y"]],
    ]);
    let roots: Vec<std::path::PathBuf> = (0..7).map(|i| home.join(format!("r{i}"))).collect();
    let proxies = [
        None,
        Some(r#"{"port": 4321}"#),
        Some(r#"{"port": 0}"#),
        Some(r#"{"port": "  7000 "}"#),
        Some(r#"{"port": 12.9}"#),
        Some("{"),
        Some(r#"{"other": 1}"#),
    ];
    for (root, proxy) in roots.iter().zip(proxies) {
        fs::create_dir_all(root.join("config")).unwrap();
        if let Some(text) = proxy {
            fs::write(root.join("config/proxy.json"), text).unwrap();
        }
    }
    let case = json!({"tiers": tier_cases, "roots": roots});
    let file = home.join("case.json");
    fs::write(&file, case.to_string()).unwrap();
    let Some(out) = run_python(TIER_ORACLE, &[file.as_os_str()], &home) else {
        return;
    };
    let mut tier_rows = Vec::new();
    for pair in tier_cases.as_array().unwrap() {
        let tiers = &pair[0];
        let mut row = Vec::new();
        for m in pair[1].as_array().unwrap() {
            let tier = model_tier(tiers, m.as_str().unwrap()).unwrap();
            let symbol = tier_symbol(tiers, &tier).unwrap();
            row.push(json!([tier, symbol]));
        }
        tier_rows.push(Value::Array(row));
    }
    let ports: Vec<Value> = roots
        .iter()
        .map(|r| json!(proxy_port(r).unwrap()))
        .collect();
    let got = json!({"tiers": tier_rows, "ports": ports});
    assert_eq!(response_dumps(&got).unwrap(), out.trim_end());
    // Entradas que el Python no resolvería con certeza.
    let bad = json!({"patterns": [{"match": "(?<=a)b", "tier": "x"}]});
    assert!(model_tier(&bad, "ab").is_err());
    assert!(model_tier(&json!({"patterns": ["x"]}), "a").is_err());
    assert!(model_tier(&json!({"patterns": [{"match": 5}]}), "a").is_err());
    assert_eq!(
        model_tier(&json!({"defaultTier": "d"}), "ñ"),
        Ok("d".into())
    );
    fs::write(roots[0].join("config/proxy.json"), r#"{"port": 99999}"#).unwrap();
    assert!(proxy_port(&roots[0]).is_err());
    fs::write(roots[0].join("config/proxy.json"), r#"[1]"#).unwrap();
    assert!(proxy_port(&roots[0]).is_err());
    let _ = fs::remove_dir_all(&home);
}
