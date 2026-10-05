//! `cli_help`, `cli_catalog` y `command_chains` contra `lib/cli_help.py`,
//! `lib/cli_catalog.py` (D8), `lib/model_watch.py` y `lib/command_chains.py`.
//!
//! Ningún CLI real se ejecuta: el Python recibe `which`/`run` falsos o
//! funciones puras, y todos los archivos (binarios falsos, cadenas) viven en
//! un HOME temporal.
#[path = "support/python.rs"]
mod python;

use comandos_core::json::response_dumps;
use comandos_runtime::{
    cli_catalog::{
        ViewError, ViewInputs, catalog_view, latest_models, names_in_file, native_binary,
        slash_names, validate_catalog, version_of,
    },
    cli_help::parse_help,
    command_chains::{SaveError, list_chains, save_chain, slugify},
};
use python::{repo, run_python};
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

const LIB: &str = r#"
import json, os, sys
repo = sys.argv[1]
sys.path.insert(0, os.path.join(repo, "lib"))
import cli_help, cli_catalog, command_chains, model_watch
"#;

fn home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lane2f-cli-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn dumps(value: &Value) -> String {
    response_dumps(value).unwrap()
}

const CLAUDE_HELP: &str = "Usage: claude [options] [command] [prompt]

Claude Code - starts an interactive session by default, use -p/--print for
non-interactive output

Arguments:
  prompt                                            Your prompt

Options:
  -d, --debug [filter]                              Enable debug mode with optional category filtering
  --allow-dangerously-skip-permissions              Enable bypassing all permission checks as an option, without it being enabled by default.
  --dangerously-skip-permissions                    Bypass all permission checks. Recommended only for sandboxes with no internet access.
  --model <model>                                   Model for the current session. Provide an alias (e.g. 'sonnet' or 'opus')
  --permission-mode <mode>                          Permission mode to use for the session (choices: \"acceptEdits\", \"bypassPermissions\", \"default\", \"plan\")
  -c, --continue                                    Continue the most recent conversation
  -h, --help                                        Display help for command

Commands:
  config                                            Manage configuration (eg. claude config set -g theme dark)
  mcp                                               Configure and manage MCP servers
  update|upgrade                                    Check for updates and install if available
  help [command]                                    display help for command
";

const CODEX_HELP: &str = "Codex CLI

If no subcommand is specified, options will be forwarded to the interactive CLI.

Usage: codex [OPTIONS] [PROMPT]
       codex [OPTIONS] <COMMAND> [ARGS]

Commands:
  exec        Run Codex non-interactively [aliases: e]
  resume      Resume a previous interactive session (picker by default; use --last to continue
              the most recent)
  completion  Generate shell completion scripts
  help        Print this message or the help of the given subcommand(s)

Arguments:
  [PROMPT]
          Optional user prompt to start the session

Options:
  -m, --model <MODEL>
          Model the agent should use

  -s, --sandbox <SANDBOX_MODE>
          Select the sandbox policy to use when executing model-generated shell commands

          [possible values: read-only, workspace-write, danger-full-access]

  -a, --ask-for-approval <APPROVAL_POLICY>
          Configure when the model requires human approval before executing a command

          Possible values:
          - untrusted:  Only run \"trusted\" commands
          - on-failure: Run all commands without asking
          - on-request: The model decides when to ask
          - never:      Never ask

      --dangerously-bypass-approvals-and-sandbox
          Skip all confirmation prompts and execute commands without sandboxing. EXTREMELY
          DANGEROUS

  -h, --help
          Print help (see a summary with '-h')
";

const OPENCODE_HELP: &str = "\x1b[0m
\x1b[90m█▀▀█ █▀▀█ █▀▀ █▀▀▄\x1b[0m
\x1b[90m█░░█ █░░█ █▀▀ █░░█\x1b[0m

Commands:
  opencode [project]         start opencode tui                                [default]
  opencode run [message..]   run opencode with a message
  opencode models [provider] list all available models

Positionals:
  project  path to start opencode in                                               [string]

Options:
  -h, --help        show help                                                      [boolean]
  -v, --version     show version number                                            [boolean]
      --print-logs  print logs to stderr                                           [boolean]
      --log-level   log level           [string] [choices: \"DEBUG\", \"INFO\", \"WARN\", \"ERROR\"]
  -m, --model       model to use in the format of provider/model                    [string]
";

const AGY_HELP: &str = "Usage of agy:
  -effort string
    \tthinking effort (low|medium|high|max) (default \"medium\")
  -yolo
    \tAuto-approve all tool executions
  -v\tprint version
";

const ODD_HELP: &str = "Tool — a summary with é and ñ
second line of summary

Usage: tool

Options:
\t--tabbed  uses a tab
  --x=VAL, -x  eq flag [possible values: a, b c, \"d\", k:v]
  completions  shell stuff
  --  bare
Extra Section of things:
  thing  does things
       continues here
  - bullet: one
";

#[test]
fn parse_help_matches_python() {
    let dir = home("help");
    let cases: Vec<(&str, &str)> = vec![
        (CLAUDE_HELP, "claude"),
        (CODEX_HELP, "codex"),
        (OPENCODE_HELP, "opencode"),
        (AGY_HELP, "agy"),
        (ODD_HELP, "tool"),
        ("", "x"),
        ("only summary\n", "x"),
    ];
    let file = dir.join("cases.json");
    std::fs::write(
        &file,
        json!(cases.iter().map(|(t, b)| json!([t, b])).collect::<Vec<_>>()).to_string(),
    )
    .unwrap();
    let script = format!(
        "{LIB}\ncases = json.load(open(sys.argv[2]))\nprint(json.dumps([cli_help.parse_help(t, b) for t, b in cases]))\n"
    );
    let expected = run_python(&script, &[file.as_os_str()], &dir);
    let rust: Vec<Value> = cases
        .iter()
        .map(|(t, b)| Value::Object(parse_help(t, b).unwrap()))
        .collect();
    std::fs::remove_dir_all(&dir).unwrap();
    let Some(expected) = expected else {
        return;
    };
    assert_eq!(dumps(&Value::Array(rust)), expected.trim_end());
}

#[test]
fn parse_help_declines_ambiguous_text() {
    assert!(parse_help("Options:\n  --x\u{1c}  y\n", "x").is_err());
    assert!(parse_help("Options:\n  --x  y\u{301}\n", "x").is_err());
}

fn write_exec(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, bytes).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn native_binary_matches_python() {
    let dir = home("native");
    let elf = dir.join("real/codex-elf");
    write_exec(&elf, b"\x7fELF/model /status codex");
    let launcher = dir.join("bin/codex");
    write_exec(
        &launcher,
        format!(
            "#!/bin/sh\n# COMANDOS_CODEX_ORIGINAL={}\nexec x \"$@\"\n",
            json!(elf.display().to_string())
        )
        .as_bytes(),
    );
    // Lanzador que apunta a un envoltorio de node con su binario de vendor.
    let js = dir.join("node/lib/@openai/codex/bin/codex.js");
    write_exec(&js, b"#!/usr/bin/env node\nrequire('x')\n");
    let vendor = dir.join("node/lib/@openai/codex-linux-x64/vendor/x86_64/bin/codex");
    write_exec(&vendor, b"\x7fELF vendor");
    let other = dir.join("node/lib/@openai/codex-zz/vendor/a/bin/codex");
    write_exec(&other, b"not elf");
    let to_js = dir.join("bin/codex-js");
    write_exec(
        &to_js,
        format!(
            "#!/bin/sh\r\n# COMANDOS_CODEX_ORIGINAL={}\r\n",
            json!(js.display().to_string())
        )
        .as_bytes(),
    );
    let cycle_a = dir.join("bin/cycle-a");
    let cycle_b = dir.join("bin/cycle-b");
    write_exec(
        &cycle_a,
        format!(
            "# COMANDOS_CODEX_ORIGINAL={}\n",
            json!(cycle_b.display().to_string())
        )
        .as_bytes(),
    );
    write_exec(
        &cycle_b,
        format!(
            "# COMANDOS_CODEX_ORIGINAL={}\n",
            json!(cycle_a.display().to_string())
        )
        .as_bytes(),
    );
    let bad_json = dir.join("bin/bad-json");
    write_exec(&bad_json, b"# COMANDOS_CODEX_ORIGINAL={nope\n");
    let not_str = dir.join("bin/not-str");
    write_exec(&not_str, b"# COMANDOS_CODEX_ORIGINAL=42\n");
    let link = dir.join("bin/link");
    std::os::unix::fs::symlink(&launcher, &link).unwrap();
    let plain = dir.join("bin/plain");
    write_exec(&plain, b"#!/bin/sh\nexit 0\n");
    let paths: Vec<PathBuf> = vec![
        elf.clone(),
        launcher,
        to_js,
        js,
        cycle_a,
        bad_json,
        not_str,
        link,
        plain,
        dir.join("bin/no-existe"),
    ];
    let file = dir.join("paths.json");
    std::fs::write(
        &file,
        json!(
            paths
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
        )
        .to_string(),
    )
    .unwrap();
    let script = format!(
        "{LIB}\nprint(json.dumps([cli_catalog.native_binary(p) for p in json.load(open(sys.argv[2]))]))\n"
    );
    let expected = run_python(&script, &[file.as_os_str()], &dir);
    let rust: Vec<Value> = paths
        .iter()
        .map(|p| match native_binary(p).unwrap() {
            Some(found) => Value::from(found.display().to_string()),
            None => Value::Null,
        })
        .collect();
    std::fs::remove_dir_all(&dir).unwrap();
    let Some(expected) = expected else {
        return;
    };
    assert_eq!(dumps(&Value::Array(rust.clone())), expected.trim_end());
    // Sanidad: el lanzador sigue el marcador y el envoltorio llega al vendor.
    assert_eq!(rust.get(1), rust.first());
    assert!(
        rust.get(2)
            .and_then(Value::as_str)
            .unwrap()
            .ends_with("x86_64/bin/codex")
    );
}

fn real_catalog() -> Value {
    let raw = std::fs::read_to_string(repo().join("config/cli-commands.json")).unwrap();
    comandos_core::json::parse_value(&raw).unwrap()
}

#[test]
fn detected_commands_match_python() {
    let dir = home("detect");
    let catalog = real_catalog();
    // Un «binario» por CLI con parte de sus comandos (con y sin barra), en
    // trozos que cruzan el límite de lectura.
    let mut bins: Vec<(String, PathBuf)> = Vec::new();
    for (n, cli) in catalog["clis"].as_array().unwrap().iter().enumerate() {
        let id = cli["id"].as_str().unwrap().to_owned();
        let mut names: Vec<String> = slash_names(cli).unwrap().into_iter().collect();
        names.sort();
        let mut data = b"\x7fELF".to_vec();
        for (i, name) in names.iter().enumerate() {
            if (i + n) % 3 == 0 {
                continue;
            }
            data.extend(std::iter::repeat_n(b'.', (1 << 20) - 3));
            if i % 2 == 0 {
                data.extend(name.as_bytes());
            } else {
                data.extend(name.trim_start_matches('/').as_bytes());
            }
        }
        let path = dir.join(format!("bin/{id}"));
        write_exec(&path, &data);
        bins.push((id, path));
    }
    let script = format!(
        "{LIB}\nbins = dict(json.load(open(sys.argv[2])))\ncat = cli_catalog.load_catalog(os.path.join(repo, 'config/cli-commands.json'))\nfound = cli_catalog.detected_commands(cat, which=lambda b: bins.get(b))\nprint(json.dumps({{k: sorted(v) if v is not None else None for k, v in found.items()}}))\n"
    );
    let file = dir.join("bins.json");
    std::fs::write(
        &file,
        json!(
            bins.iter()
                .map(|(id, p)| json!([id, p.display().to_string()]))
                .collect::<Vec<_>>()
        )
        .to_string(),
    )
    .unwrap();
    let expected = run_python(&script, &[file.as_os_str()], &dir);
    let mut rust = Map::new();
    for cli in catalog["clis"].as_array().unwrap() {
        let id = cli["id"].as_str().unwrap();
        let path = &bins.iter().find(|(i, _)| i == id).unwrap().1;
        let mut found: Vec<String> = names_in_file(
            &native_binary(path).unwrap().unwrap(),
            &slash_names(cli).unwrap(),
        )
        .unwrap()
        .into_iter()
        .collect();
        found.sort();
        rust.insert(id.into(), json!(found));
    }
    std::fs::remove_dir_all(&dir).unwrap();
    let Some(expected) = expected else {
        return;
    };
    assert_eq!(dumps(&Value::Object(rust)), expected.trim_end());
}

#[test]
fn validate_catalog_matches_python() {
    let dir = home("validate");
    let cases = vec![
        real_catalog(),
        json!([]),
        json!({"version": 2, "clis": []}),
        json!({"version": true, "clis": []}),
        json!({"version": 1.0, "clis": []}),
        json!({"version": 1, "clis": {}}),
        json!({"version": 1, "clis": [{"id": "a", "label": "A", "binary": "a", "pinnedVersion": "1"}]}),
        json!({"version": 1, "clis": [
            {"id": "a", "label": "A", "binary": "a", "pinnedVersion": "1", "groups": []},
            {"id": "a", "label": "A", "binary": "a", "pinnedVersion": "1", "groups": []}]}),
        json!({"version": 1, "clis": [{"id": "a", "label": "A", "binary": "a", "pinnedVersion": "1",
            "groups": [{"commands": [{"text": "/x", "description": ""}]}]}]}),
        json!({"version": 1, "clis": [{"id": "a", "label": "A", "binary": "a", "pinnedVersion": "1",
            "groups": [{"commands": [{"text": "/x\ny", "description": "d"}]}]}]}),
        json!({"version": 1, "clis": [{"id": "a", "label": "A", "binary": "a", "pinnedVersion": "1",
            "groups": [{"title": "t"}, {"commands": [{"text": "/x", "description": "d"}]}]}]}),
    ];
    let file = dir.join("cases.json");
    std::fs::write(&file, Value::Array(cases.clone()).to_string()).unwrap();
    let script = format!(
        "{LIB}\nout = []\nfor i, c in enumerate(json.load(open(sys.argv[2]))):\n    p = os.path.join(os.path.dirname(sys.argv[2]), f'c{{i}}.json')\n    json.dump(c, open(p, 'w'))\n    try:\n        cli_catalog.load_catalog(p); out.append(True)\n    except Exception:\n        out.append(False)\nprint(json.dumps(out))\n"
    );
    let expected = run_python(&script, &[file.as_os_str()], &dir);
    let rust: Vec<Value> = cases
        .iter()
        .map(|c| match validate_catalog(c) {
            Ok(()) => json!(true),
            Err(ViewError::Raises) => json!(false),
            Err(ViewError::Unsure) => json!("unsure"),
        })
        .collect();
    std::fs::remove_dir_all(&dir).unwrap();
    let Some(expected) = expected else {
        return;
    };
    assert_eq!(dumps(&Value::Array(rust)), expected.trim_end());
}

#[test]
fn version_extraction_matches_python() {
    let dir = home("versions");
    let outputs = [
        "codex-cli 0.159.2\n",
        "2.1.286 (Claude Code)",
        "grok v1.0.44-beta.1+x",
        "sin versión",
        "",
        "1.2",
        "12",
    ];
    let file = dir.join("out.json");
    std::fs::write(&file, json!(outputs).to_string()).unwrap();
    let script = format!(
        "{LIB}\nouts = json.load(open(sys.argv[2]))\ncat = {{'clis': [{{'id': str(i), 'binary': str(i)}} for i in range(len(outs))]}}\nclass R:\n    def __init__(s, o): s.stdout, s.stderr = o, ''\nprint(json.dumps(cli_catalog.installed_versions(cat, which=lambda b: '/x/' + b, run=lambda a, **k: R(outs[int(a[0].split('/')[-1])]))))\n"
    );
    let expected = run_python(&script, &[file.as_os_str()], &dir);
    let mut rust = Map::new();
    for (i, out) in outputs.iter().enumerate() {
        rust.insert(i.to_string(), Value::from(version_of(out).unwrap()));
    }
    std::fs::remove_dir_all(&dir).unwrap();
    let Some(expected) = expected else {
        return;
    };
    assert_eq!(dumps(&Value::Object(rust)), expected.trim_end());
}

#[test]
fn latest_models_match_python() {
    let dir = home("latest");
    let cases = [
        (json!(null), json!({})),
        (
            json!({"claude": ["claude-opus-5-5", "claude-opus-5-4", "claude-sonnet-5-5", "claude-haiku-4-5-20251001",
                              "claude-opus-5-5[1m]", "claude-fable-5-1", 7],
                   "codex": ["gpt-5.6", "gpt-5.6-codex", "gpt-5.10", "o3"],
                   "grok": "grok-4.7"}),
            json!({"grok": ["grok-4.7", "grok-4.7-build-fast", "grok-4.6"], "claude": ["claude-opus-5-6"]}),
        ),
        (json!({}), json!({"codex": ["gpt-5", "gpt-5.0"]})),
    ];
    let file = dir.join("cases.json");
    std::fs::write(
        &file,
        json!(cases.iter().map(|(d, r)| json!([d, r])).collect::<Vec<_>>()).to_string(),
    )
    .unwrap();
    let script = format!(
        "{LIB}\nprint(json.dumps([model_watch.latest_models(d, {{k: set(v) for k, v in r.items()}}) for d, r in json.load(open(sys.argv[2]))]))\n"
    );
    let expected = run_python(&script, &[file.as_os_str()], &dir);
    let rust: Vec<Value> = cases
        .iter()
        .map(|(d, r)| {
            let ids: BTreeMap<String, Vec<String>> = r
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        v.as_array()
                            .unwrap()
                            .iter()
                            .map(|s| s.as_str().unwrap().to_owned())
                            .collect(),
                    )
                })
                .collect();
            Value::Object(latest_models(Some(d), &ids).unwrap())
        })
        .collect();
    std::fs::remove_dir_all(&dir).unwrap();
    let Some(expected) = expected else {
        return;
    };
    assert_eq!(dumps(&Value::Array(rust)), expected.trim_end());
    // Un `discovered` que no es objeto lanza (500).
    assert_eq!(
        latest_models(Some(&json!([1])), &BTreeMap::new()).unwrap_err(),
        ViewError::Raises
    );
}

#[test]
fn catalog_view_matches_python() {
    let dir = home("view");
    let catalog = real_catalog();
    let mut helps: HashMap<String, Map<String, Value>> = HashMap::new();
    for (id, text) in [
        ("claude", CLAUDE_HELP),
        ("codex", CODEX_HELP),
        ("opencode", OPENCODE_HELP),
    ] {
        let mut parsed = parse_help(text, id).unwrap();
        parsed.insert("command".into(), Value::from(format!("{id} --help")));
        helps.insert(id.into(), parsed);
    }
    let versions =
        json!({"claude": "2.1.286", "codex": "0.150.0", "grok": null, "opencode": "?", "agy": 1.2});
    let mut accounts: HashMap<String, Vec<Value>> = HashMap::new();
    accounts.insert(
        "claude".into(),
        vec![
            json!({"CLAUDE_CONFIG_DIR": "/h/.claude-accounts/b"}),
            json!({}),
        ],
    );
    let models = json!({"claude": ["claude-opus-5-6", "claude-sonnet-5-5"], "codex": [], "grok": ["grok-4.7"]});
    let new_models = json!({"claude": ["claude-opus-5-6"], "grok": []});
    let catalog_names: HashMap<String, HashSet<String>> = catalog["clis"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["id"].as_str().unwrap().to_owned(),
                slash_names(c).unwrap(),
            )
        })
        .collect();
    let mut detected: HashMap<String, Option<HashSet<String>>> = HashMap::new();
    let mut claude_found: HashSet<String> = catalog_names["claude"].clone();
    claude_found.retain(|n| n.len() % 2 == 0);
    detected.insert("claude".into(), Some(claude_found.clone()));
    detected.insert("codex".into(), None);
    detected.insert("grok".into(), Some(HashSet::new()));
    let inputs = |detected: Option<&HashMap<String, Option<HashSet<String>>>>| {
        catalog_view(
            &catalog,
            &ViewInputs {
                versions: versions.as_object().unwrap(),
                accounts: &accounts,
                models: models.as_object().unwrap(),
                new_models: new_models.as_object().unwrap(),
                detected,
                helps: &helps,
            },
        )
        .unwrap()
    };
    let rust = json!([inputs(Some(&detected)), inputs(None)]);
    let mut claude_sorted: Vec<&String> = claude_found.iter().collect();
    claude_sorted.sort();
    let payload = json!({
        "helps": helps,
        "versions": versions,
        "accounts": {"claude": [{"alias": "b", "env": {"CLAUDE_CONFIG_DIR": "/h/.claude-accounts/b"}}, {"alias": "c", "env": {}}]},
        "models": models,
        "new_models": new_models,
        "detected": {"claude": claude_sorted, "codex": null, "grok": []},
    });
    let file = dir.join("in.json");
    std::fs::write(&file, payload.to_string()).unwrap();
    let script = format!(
        "{LIB}\nd = json.load(open(sys.argv[2]))\ncat = cli_catalog.load_catalog(os.path.join(repo, 'config/cli-commands.json'))\ndet = {{k: (set(v) if v is not None else None) for k, v in d['detected'].items()}}\nkw = dict(versions=d['versions'], accounts=d['accounts'], models=d['models'], new_models=d['new_models'], helps=d['helps'])\nprint(json.dumps([cli_catalog.catalog_view(cat, detected=det, **kw), cli_catalog.catalog_view(cat, **kw)]))\n"
    );
    let expected = run_python(&script, &[file.as_os_str()], &dir);
    std::fs::remove_dir_all(&dir).unwrap();
    let Some(expected) = expected else {
        return;
    };
    assert_eq!(dumps(&rust), expected.trim_end());
    // Los tres atajos de Codex (D8): el `--help` de prueba los admite todos.
    let codex = &rust[0]["clis"][1];
    assert_eq!(codex["id"], "codex");
    assert_eq!(codex["start"]["shortcuts"].as_array().unwrap().len(), 3);
    // Sin ayuda, `start` no lleva `shortcuts` (rama temprana de `_start_view`).
    assert!(rust[0]["clis"][2]["start"].get("shortcuts").is_none());
}

#[test]
fn slugify_matches_python() {
    let dir = home("slug");
    let mut names: Vec<String> = [
        "Mi Cadena",
        "  ",
        "a--b__c",
        "UPPER case 123",
        "-x-",
        &"z".repeat(80),
        "a b c d",
        // M5: nombres en español y otros latinos (NFKD + `ascii` ignorado).
        "Revisión diaria",
        "Año nuevo: ¡Despliegue!",
        "Ærø Œuvre straße ĳssel ŀl ŉ ſ",
        "café — “citas” … ½ ²",
        "çà-et-là",
    ]
    .iter()
    .map(|n| (*n).to_owned())
    .collect();
    // Cada carácter de los tramos cubiertos, solo y entre letras.
    for c in (0x80u32..0x370)
        .chain(0x2000..0x2070)
        .filter_map(char::from_u32)
    {
        names.push(c.to_string());
        names.push(format!("a{c}b"));
    }
    let file = dir.join("n.json");
    std::fs::write(&file, json!(names).to_string()).unwrap();
    let script = format!(
        "{LIB}\nprint(json.dumps([command_chains.slugify(n) for n in json.load(open(sys.argv[2]))]))\n"
    );
    let expected = run_python(&script, &[file.as_os_str()], &dir);
    let rust: Vec<Value> = names
        .iter()
        .map(|n| Value::from(slugify(n).unwrap()))
        .collect();
    std::fs::remove_dir_all(&dir).unwrap();
    let Some(expected) = expected else {
        return;
    };
    assert_eq!(dumps(&Value::Array(rust)), expected.trim_end());
}

/// Fuera de los tramos latinos (emoji, CJK, compatibilidad nueva) la versión
/// de Unicode del Python puede no ser la de ICU: se declina.
#[test]
fn slugify_declines_outside_the_stable_ranges() {
    for name in ["deploy 🚀", "日本", "🄫", "\u{1F16A}", "x\u{0378}"] {
        assert!(slugify(name).is_err(), "{name}");
    }
}

/// El directorio de cadenas sembrado igual para los dos lados.
fn seed_chains(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    let files: [(&str, &[u8]); 8] = [
        (
            "b-uno.md",
            b"# Uno\n\n1. shell: echo hola\n2) pane:  ls -la  \n- shell: pwd\n",
        ),
        ("a-sin-nombre.md", b"\xef\xbb\xbf1. pane: git status\n"),
        ("Mayus.md", b"# x\n1. shell: y\n"),
        ("c-mala.md", b"# Mala\nno es un paso\n"),
        ("d-tipo.md", b"# T\n1. bash: ls\n"),
        (".oculto.md", b"# h\n1. shell: z\n"),
        ("e-vacia.md", b"# Solo nombre\n"),
        ("nota.txt", b"no cuenta"),
    ];
    for (name, body) in files {
        std::fs::write(dir.join(name), body).unwrap();
    }
    std::fs::create_dir_all(dir.join("f-dir.md")).unwrap();
    std::os::unix::fs::symlink(dir.join("nada"), dir.join("g-rota.md")).unwrap();
}

#[test]
fn list_chains_matches_python() {
    let dir = home("list");
    let chains = dir.join("cadenas");
    seed_chains(&chains);
    let script = format!(
        "{LIB}\nprint(json.dumps([command_chains.list_chains(sys.argv[2]), command_chains.list_chains(sys.argv[2] + '/no-existe')]))\n"
    );
    let expected = run_python(&script, &[chains.as_os_str()], &dir);
    let rust = json!([
        list_chains(&chains).unwrap(),
        list_chains(&chains.join("no-existe")).unwrap()
    ]);
    std::fs::remove_dir_all(&dir).unwrap();
    let Some(expected) = expected else {
        return;
    };
    assert_eq!(dumps(&rust), expected.trim_end());
}

/// Casos de `save_chain` (nombre, pasos, slug): el resultado y los archivos.
fn save_cases() -> Vec<Value> {
    vec![
        json!([{"name": "Mi Cadena", "steps": [{"kind": "shell", "text": "  echo hola "}, {"kind": "pane", "text": "ls"}]}]),
        // Segunda con el mismo nombre: `mi-cadena-2`.
        json!([{"name": "Mi Cadena", "steps": [{"kind": "shell", "text": "a"}]},
               {"name": "Mi Cadena", "steps": [{"kind": "shell", "text": "b"}]}]),
        json!([{"name": "Otra", "steps": [{"kind": "shell", "text": "x"}], "slug": "fija-1"}]),
        json!([{"name": "Otra", "steps": [{"kind": "shell", "text": "x"}], "slug": "Mal Slug"}]),
        json!([{"name": "Otra", "steps": [{"kind": "shell", "text": "x"}], "slug": 7}]),
        json!([{"name": "", "steps": [{"kind": "shell", "text": "x"}]}]),
        json!([{"name": "N", "steps": []}]),
        json!([{"name": "N"}]),
        json!([{"name": "N", "steps": [{"kind": "bash", "text": "x"}]}]),
        json!([{"name": "N", "steps": [{"kind": null, "text": "x"}]}]),
        json!([{"name": "N", "steps": ["no-dict"]}]),
        json!([{"name": "N", "steps": [{"kind": "shell", "text": "a\nb"}]}]),
        json!([{"name": "N", "steps": [{"kind": "shell", "text": "   "}]}]),
        json!([{"name": "N", "steps": [{"kind": "pane", "text": 42}]}]),
        json!([{"name": 12, "steps": [{"kind": "pane", "text": true}]}]),
        json!([{"name": "con\ttab", "steps": [{"kind": "pane", "text": "x"}]}]),
        json!([{"name": "!!!", "steps": [{"kind": "pane", "text": "x"}]}]),
    ]
}

#[test]
fn save_chain_matches_python() {
    let dir = home("save");
    let cases = save_cases();
    let file = dir.join("cases.json");
    std::fs::write(&file, Value::Array(cases.clone()).to_string()).unwrap();
    let script = format!(
        "{LIB}\nbase = os.path.join(os.path.dirname(sys.argv[2]), 'py')\nout = []\nfor i, calls in enumerate(json.load(open(sys.argv[2]))):\n    d = os.path.join(base, str(i), 'cadenas')\n    res = []\n    for c in calls:\n        try:\n            res.append({{'ok': command_chains.save_chain(d, c.get('name'), c.get('steps'), slug=c.get('slug') or None)}})\n        except command_chains.ChainError as e:\n            res.append({{'error': str(e)}})\n        except (OSError, UnicodeError) as e:\n            res.append({{'os': e.__class__.__name__}})\n    files = {{}}\n    if os.path.isdir(d):\n        for n in sorted(os.listdir(d)):\n            files[n] = open(os.path.join(d, n)).read()\n    out.append([res, files])\nprint(json.dumps(out))\n"
    );
    let expected = run_python(&script, &[file.as_os_str()], &dir);
    let mut rust = Vec::new();
    for (i, calls) in cases.iter().enumerate() {
        let d = dir.join("rs").join(i.to_string()).join("cadenas");
        let mut res = Vec::new();
        for c in calls.as_array().unwrap() {
            let outcome = save_chain(&d, c.get("name"), c.get("steps"), c.get("slug")).unwrap();
            res.push(match outcome {
                Ok(chain) => json!({"ok": chain}),
                Err(SaveError::Chain(m)) => json!({"error": m}),
                Err(SaveError::Os(class)) => json!({"os": class}),
            });
        }
        let mut files = Map::new();
        if d.is_dir() {
            let mut names: Vec<String> = std::fs::read_dir(&d)
                .unwrap()
                .map(|e| e.unwrap().file_name().into_string().unwrap())
                .collect();
            names.sort();
            for n in names {
                files.insert(
                    n.clone(),
                    Value::from(std::fs::read_to_string(d.join(&n)).unwrap()),
                );
            }
        }
        rust.push(json!([res, files]));
    }
    std::fs::remove_dir_all(&dir).unwrap();
    let Some(expected) = expected else {
        return;
    };
    assert_eq!(dumps(&Value::Array(rust)), expected.trim_end());
}

#[test]
fn save_chain_declines_before_effects() {
    let dir = home("unsure");
    let d = dir.join("cadenas");
    // Nombre fuera de los tramos de NFKD estable sin slug y slug que es una
    // lista: nada en disco.
    assert!(
        save_chain(
            &d,
            Some(&json!("Café ☕")),
            Some(&json!([{"kind": "shell", "text": "x"}])),
            None
        )
        .is_err()
    );
    assert!(
        save_chain(
            &d,
            Some(&json!("x")),
            Some(&json!([{"kind": "shell", "text": "x"}])),
            Some(&json!([1]))
        )
        .is_err()
    );
    assert!(!d.exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn opencode_models_match_python() {
    use comandos_runtime::cli_catalog::parse_opencode_models;
    let dir = home("opencode");
    let mut many = String::new();
    for i in 0..45 {
        let free = if i % 3 == 0 { ":free" } else { "" };
        many.push_str(&format!(
            "openrouter/m{i}{free}\n{{\"id\": \"m{i}{free}\", \"providerID\": \"openrouter\", \"name\": \"M {i}\"}}\n"
        ));
    }
    let outputs = vec![
        String::new(),
        "opencode/a\n{\"id\": \"a\", \"providerID\": \"opencode\", \"status\": \"active\", \"capabilities\": {\"toolcall\": true, \"input\": {\"text\": true}, \"output\": {\"text\": false}}}\n\
         b/x\n{\"id\": \"x\", \"providerID\": \"b\", \"name\": \"X FREE\", \"limit\": {\"context\": 12.9}}\n\
         c/y\n{\"id\": \"y\", \"providerID\": \"c\", \"limit\": {\"context\": \"abc\"}, \"status\": null}\n\
         c/z\n{\"id\": 5, \"providerID\": \"c\", \"name\": 7, \"limit\": {\"context\": true}}\n\
         roto {\"id\": \n{\"id\": \"~alias\", \"providerID\": \"c\"}\n[1, {\"id\": \"in-list\", \"providerID\": \"d\"}]\n"
            .to_owned(),
        many,
        // `capabilities` que no es objeto: excepción, lista vacía.
        "{\"id\": \"a\", \"providerID\": \"p\"}\n{\"id\": \"b\", \"providerID\": \"p\", \"capabilities\": [1]}\n".to_owned(),
        // `limit` que no es objeto pero el id es un alias: no se evalúa.
        "{\"id\": \"~a\", \"providerID\": \"p\", \"limit\": 3}\n{\"id\": \"b\", \"providerID\": \"p\", \"limit\": {}}\n".to_owned(),
    ];
    let file = dir.join("out.json");
    std::fs::write(&file, json!(outputs).to_string()).unwrap();
    let script = "import json, os, sys\nsys.path.insert(0, os.path.join(sys.argv[1], 'bin'))\nimport cc_usage\n\
        def run(t):\n    try:\n        return cc_usage.parse_opencode_models(t)\n    except Exception:\n        return []\n\
        print(json.dumps([run(t) for t in json.load(open(sys.argv[2]))]))\n";
    let expected = run_python(script, &[file.as_os_str()], &dir);
    let rust: Vec<Value> = outputs
        .iter()
        .map(|t| Value::Array(parse_opencode_models(t).unwrap()))
        .collect();
    std::fs::remove_dir_all(&dir).unwrap();
    let Some(expected) = expected else {
        return;
    };
    assert_eq!(dumps(&Value::Array(rust)), expected.trim_end());
}
