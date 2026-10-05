//! `model_watch` (`installed_versions`, `watch_models`) y `news_watch`
//! (`watch_news`, `collect`) contra `lib/model_watch.py` y `lib/news_watch.py`.
//!
//! Confinamiento: los CLIs son guiones del `fakebin` de un HOME temporal (el
//! Python recibe ese `which`: `model_watch._which = shutil.which(…, path=fakebin)`),
//! el binario versionado de Claude y el de Codex son archivos de texto con
//! ids sintéticos, y el registro es un `providers.json` de la prueba. Las
//! noticias no tocan la red en ningún lado: el Python sustituye
//! `urllib.request.urlopen` y el Rust recibe un `Fetch` con las mismas
//! respuestas fijas (cuerpos, 308 con `Location`, errores).
#[path = "support/python.rs"]
mod python;

use base64::Engine as _;
use comandos_runtime::{
    model_watch::{self, Fault, Host, Paths, WatchPaths},
    news_watch::{Endpoints, Fetch, Fetched, Sources, watch_news},
};
use python::run_python;
use serde_json::{Value, json};
use std::{
    ffi::OsStr,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Mutex,
    time::Duration,
};

/// 2026-10-05 08:53:20 UTC.
const NOW: i64 = 1_791_190_400;

fn home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lane2f-mw-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join(".claude/hooks")).unwrap();
    dir
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn script(path: &Path, text: &str) {
    write(path, text);
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Un CLI falso que anota `nombre argv GROK_HOME` en `$HOME/cli.log`.
fn cli(version_line: &str, extra: &str) -> String {
    format!(
        "#!/bin/sh\nprintf '%s %s %s\\n' \"$(basename \"$0\")\" \"$*\" \"${{GROK_HOME:-}}\" >> \"$HOME/cli.log\"\ncase \"$1\" in\n--version) printf '{version_line}\\n';;\n{extra}esac\n"
    )
}

/// HOME gemelo: CLIs, binarios versionados, registro, skills y MCPs.
fn seed(home: &Path, codex_cache: bool) {
    let bin = home.join("fakebin");
    script(
        &bin.join("claude"),
        &cli("\\033[1m2.1.286\\033[0m (Claude Code)", ""),
    );
    script(
        &bin.join("grok"),
        &cli(
            "grok 1.0.44",
            "models) printf '  - grok-4.7\\n* grok-4.7-fast\\n- \\033[1mgrok-5.0\\033[0m\\n  - not-grok\\n- grok-4.6 (default)\\nAvailable:\\n';;\n",
        ),
    );
    script(&bin.join("agy"), &cli("sin versión", ""));
    // `codex` es un enlace al paquete de npm; el «ELF» vendor está al lado.
    let pkg = home.join("npm/lib/node_modules/@openai/codex/bin/codex.js");
    script(&pkg, &cli("codex-cli 0.159.2", ""));
    std::os::unix::fs::symlink(&pkg, bin.join("codex")).unwrap();
    write(
        &home.join("npm/lib/node_modules/@openai/codex-linux-x64/vendor/x86_64-unknown-linux-musl/bin/codex"),
        "\0gpt-5.6\0gpt-5.6-codex\0gpt-5.6-codex-sol\0gpt-6\0gpt-5.5\0gpt-5.4-mini\0gpt-5-xx-yyyy-zzzz-wwwwwwww\0gpt-x\0",
    );
    let versions = home.join(".local/share/claude/versions");
    write(&versions.join("2.1.9"), "claude-opus-9-9\0");
    write(
        &versions.join("2.1.10"),
        "x claude-opus-5-6\0claude-opus-5-5\0claude-fable-5-1\0claude-sonnet-4-5-20250929\0claude-haiku-4-5[1m]\0claude-opus-4-1-xyz\0claude-mythos-6\0claude-sonnet-5\0",
    );
    write(&versions.join("notas"), "claude-opus-1-0\0");
    if codex_cache {
        write(
            &home.join(".codex/models_cache.json"),
            r#"{"models": [{"slug": "gpt-5.6", "visibility": "list"}, {"slug": "gpt-5.6-codex-astra", "visibility": "list"},
              {"slug": "gpt-5.7", "visibility": "hide"}, {"slug": "gpt-6.0-sol", "visibility": "list"}, {"slug": 5}, "x"]}"#,
        );
    }
    write(
        &home.join("providers.json"),
        r#"{"motors": {"claude": {"models": [{"id": "claude-opus-5-5"}, {"id": "claude-sonnet-4-5", "soon": true}, {"id": "claude-haiku-4-5-20251001"}]},
                       "codex": {"models": [{"id": "gpt-5.5"}, {"id": "gpt-5.5-codex"}]},
                       "grok": {"models": [{"id": "grok-4.7"}]},
                       "opencode": {}}}"#,
    );
    for d in ["alfa", "beta", ".oculta"] {
        fs::create_dir_all(home.join(".claude/skills").join(d)).unwrap();
    }
    fs::create_dir_all(home.join(".claude-accounts/relotto/skills/rel")).unwrap();
    write(&home.join(".codex/prompts/revisar.md"), "x");
    write(&home.join(".codex/prompts/notas.txt"), "x");
    write(
        &home.join(".claude.json"),
        r#"{"mcpServers": {"zeta": {"command": "secreto"}, "alfa": {}}, "otro": 1}"#,
    );
    write(
        &home.join(".codex/config.toml"),
        "[mcp_servers.alpha]\r\ncommand = \"x\"\r[mcp_servers.beta-2]\n[mcp_servers.alpha]\n",
    );
    write(&home.join(".grok/config.toml"), "# nada\n");
}

/// `which` y `_run` del lado Rust: solo el `fakebin`.
struct TestHost {
    fakebin: PathBuf,
}

impl Host for TestHost {
    fn which(&self, name: &str) -> Option<PathBuf> {
        let path = self.fakebin.join(name);
        fs::metadata(&path)
            .ok()
            .filter(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .map(|_| path)
    }

    fn run(
        &self,
        exe: &Path,
        args: &[&str],
        _timeout: Duration,
        env: &[(&str, &OsStr)],
    ) -> Option<String> {
        let home = self.fakebin.parent()?;
        let out = Command::new(exe)
            .args(args)
            .env("HOME", home)
            .env_remove("GROK_HOME")
            .envs(env.iter().map(|(k, v)| (*k, *v)))
            .stdin(Stdio::null())
            .output()
            .ok()?;
        let text = format!(
            "{}{}",
            String::from_utf8(out.stdout).ok()?,
            String::from_utf8(out.stderr).ok()?
        );
        Some(text.replace("\r\n", "\n").replace('\r', "\n"))
    }
}

const MODEL_ORACLE: &str = r#"
import json, os, shutil, sys
repo, home, mode = sys.argv[1], sys.argv[2], sys.argv[3]
sys.path.insert(0, os.path.join(repo, "lib"))
import model_watch
fakebin = os.path.join(home, "fakebin")
model_watch._which = lambda n: shutil.which(n, path=fakebin)
os.environ.pop("GROK_HOME", None)
if mode == "versions":
    print(json.dumps(model_watch.installed_versions()))
else:
    try:
        r = model_watch.watch_models(os.path.join(home, ".claude/hooks"), os.path.join(home, "providers.json"),
                                     grok_home=os.path.join(home, ".grok"), now=int(mode))
        print(json.dumps({"news": r["news"]}))
    except Exception as e:
        print(json.dumps({"raises": type(e).__name__}))
"#;

fn py(home: &Path, mode: &str) -> Option<Value> {
    let out = run_python(MODEL_ORACLE, &[home.as_os_str(), OsStr::new(mode)], home)?;
    Some(serde_json::from_str(out.trim()).unwrap())
}

fn rust_watch(home: &Path, now: i64) -> Result<Value, Fault> {
    let host = TestHost {
        fakebin: home.join("fakebin"),
    };
    let paths = Paths {
        home: home.to_path_buf(),
        codex_home: None,
        cwd: home.to_path_buf(),
    };
    let hooks = home.join(".claude/hooks");
    let registry = home.join("providers.json");
    let grok = home.join(".grok");
    let at = WatchPaths {
        paths: &paths,
        hooks: &hooks,
        registry: &registry,
        grok_home: Some(&grok),
    };
    let watch = model_watch::watch_models(&host, &at, now)?;
    let news: serde_json::Map<String, Value> =
        watch.news.into_iter().map(|(k, v)| (k, json!(v))).collect();
    Ok(json!({"news": news}))
}

fn snapshot(home: &Path) -> String {
    fs::read_to_string(home.join(".claude/hooks/model-watch.json")).unwrap()
}

fn log(home: &Path) -> String {
    fs::read_to_string(home.join("cli.log"))
        .unwrap_or_default()
        .replace(&home.display().to_string(), "<HOME>")
}

#[test]
fn installed_versions_match_python() {
    let (a, b) = (home("ver-a"), home("ver-b"));
    seed(&a, true);
    seed(&b, true);
    let Some(expected) = py(&b, "versions") else {
        return;
    };
    let host = TestHost {
        fakebin: a.join("fakebin"),
    };
    let got = model_watch::installed_versions(&host).unwrap();
    assert_eq!(Value::Object(got), expected);
    assert_eq!(
        expected,
        json!({"claude": "2.1.286", "codex": "0.159.2", "grok": "1.0.44", "agy": "?"})
    );
    assert_eq!(log(&a), log(&b));
}

/// Con el catálogo de Codex y sin él (binario vendor), primera vuelta y
/// segunda con snapshot previo (novedades ya vistas, altas de skills).
#[test]
fn watch_models_matches_python() {
    for cache in [true, false] {
        let tag = if cache { "cat" } else { "bin" };
        let (a, b) = (home(&format!("wm-{tag}-a")), home(&format!("wm-{tag}-b")));
        seed(&a, cache);
        seed(&b, cache);
        let Some(expected) = py(&b, &NOW.to_string()) else {
            return;
        };
        let got = rust_watch(&a, NOW).unwrap();
        assert_eq!(got, expected, "{tag}");
        assert_eq!(snapshot(&a), snapshot(&b), "{tag}");
        assert!(
            !expected["news"].as_object().unwrap().is_empty(),
            "{expected}"
        );
        // Segunda vuelta: lo pendiente ya no es noticia; una skill nueva sí.
        for h in [&a, &b] {
            fs::create_dir_all(h.join(".claude/skills/gamma")).unwrap();
            fs::create_dir_all(h.join(".grok/skills/g1")).unwrap();
        }
        let expected = py(&b, &(NOW + 60).to_string()).unwrap();
        let got = rust_watch(&a, NOW + 60).unwrap();
        assert_eq!(got, expected, "{tag}");
        assert_eq!(snapshot(&a), snapshot(&b), "{tag}");
        assert!(
            snapshot(&a).contains("\"addonNews\": {\n  \"skills\""),
            "{}",
            snapshot(&a)
        );
        assert_eq!(log(&a), log(&b), "{tag}");
        assert!(log(&a).contains("grok models <HOME>/.grok"), "{}", log(&a));
        let _ = fs::remove_dir_all(&a);
        let _ = fs::remove_dir_all(&b);
    }
}

/// Snapshots previos con formas raras: lo que lanza en el Python no escribe
/// nada en el Rust; lo que el Python tolera, igual.
#[test]
fn watch_models_odd_previous_snapshots_match_python() {
    let cases = [
        ("lista", "[1]", true),
        ("newsince-texto", r#"{"newSince": "abc"}"#, true),
        (
            "models-numero",
            r#"{"newSince": {"claude": {"models": 5}}}"#,
            true,
        ),
        (
            "models-texto",
            r#"{"newSince": {"claude": {"models": "claude-opus-5-6"}}}"#,
            false,
        ),
        ("addons-lista", r#"{"addons": [1]}"#, true),
        (
            "addons-falsos",
            r#"{"addons": [], "addonNews": {"viejo": 1}}"#,
            false,
        ),
        ("roto", "{", false),
    ];
    for (tag, prev, raises) in cases {
        let (a, b) = (home(&format!("odd-{tag}-a")), home(&format!("odd-{tag}-b")));
        for h in [&a, &b] {
            seed(h, true);
            write(&h.join(".claude/hooks/model-watch.json"), prev);
        }
        let Some(expected) = py(&b, &NOW.to_string()) else {
            return;
        };
        let got = rust_watch(&a, NOW);
        if raises {
            assert!(expected.get("raises").is_some(), "{tag}: {expected}");
            assert_eq!(got, Err(Fault::Raises), "{tag}");
            assert_eq!(snapshot(&a), prev, "{tag}");
        } else {
            assert_eq!(got.unwrap(), expected, "{tag}");
        }
        assert_eq!(snapshot(&a), snapshot(&b), "{tag}");
        let _ = fs::remove_dir_all(&a);
        let _ = fs::remove_dir_all(&b);
    }
}

/// Un id no textual en el registro: `.lower()` lanza en los dos lados.
#[test]
fn watch_models_bad_registry_raises_like_python() {
    let (a, b) = (home("reg-a"), home("reg-b"));
    for h in [&a, &b] {
        seed(h, true);
        write(
            &h.join("providers.json"),
            r#"{"motors": {"claude": {"models": [{"id": 7}]}}}"#,
        );
    }
    let Some(expected) = py(&b, &NOW.to_string()) else {
        return;
    };
    assert!(expected.get("raises").is_some(), "{expected}");
    assert_eq!(rust_watch(&a, NOW), Err(Fault::Raises));
    assert!(!a.join(".claude/hooks/model-watch.json").exists());
    assert!(!b.join(".claude/hooks/model-watch.json").exists());
}

// ---------------------------------------------------------------------------
// Noticias
// ---------------------------------------------------------------------------

const SEARX: &str = "http://searx.test:27950";

fn b64(text: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(text)
}

/// Respuestas fijas por URL: `{"body": base64}`, `{"status": n, "location"?}` o
/// `{"error": texto}`.
fn feeds(second: bool) -> Value {
    let recent = NOW - 3_600;
    let old = NOW - 10 * 86_400;
    let mut hits = vec![
        json!({"title": "Claude Code skill pack", "url": "https://ex.test/skill", "created_at_i": recent}),
        json!({"title": "Show HN: MCP server for X", "objectID": 4242, "created_at_i": recent - 10}),
        json!({"title": "Unrelated", "url": "https://ex.test/no", "created_at_i": recent}),
        json!({"title": "Claude old news", "url": "https://ex.test/old", "created_at_i": old}),
    ];
    if second {
        hits.push(json!({"title": "Anthropic ships skills 2", "url": "https://ex.test/new", "created_at_i": recent + 5}));
    }
    let body = |v: Value| json!({"body": b64(v.to_string().as_bytes())});
    let mut map = serde_json::Map::new();
    let mut put = |url: String, v: Value| {
        map.insert(url, v);
    };
    put(
        format!("{SEARX}/search?q=%22claude+code%22+skill&format=json&time_range=week&pageno=1"),
        body(json!({"results": [
            {"title": "  Una skill  ", "url": "https://s.test/1"},
            {"title": null, "url": "https://s.test/2"},
            {"title": "Sin url", "url": null},
            {"title": "Model Context Protocol en español: ñandú", "url": "https://s.test/3"}
        ]})),
    );
    put(
        format!("{SEARX}/search?q=%22claude%22+mcp+server&format=json&time_range=week&pageno=1"),
        json!({"status": 500}),
    );
    put(
        format!(
            "{SEARX}/search?q=model+context+protocol+server+nuevo&format=json&time_range=week&pageno=1"
        ),
        json!({"body": b64(b"no json \xff")}),
    );
    put(
        "https://hn.algolia.com/api/v1/search_by_date?query=%22claude+code%22+skill&tags=story&hitsPerPage=8".into(),
        body(json!({"hits": hits})),
    );
    put(
        "https://hn.algolia.com/api/v1/search_by_date?query=%22claude%22+skills&tags=story&hitsPerPage=8".into(),
        json!({"status": 308, "location": "/api/v1/redir?n=1"}),
    );
    put(
        "https://hn.algolia.com/api/v1/redir?n=1".into(),
        body(
            json!({"hits": [{"title": "Claude via 308", "url": "https://ex.test/308", "created_at_i": "1791180000"}]}),
        ),
    );
    put(
        "https://hn.algolia.com/api/v1/search_by_date?query=mcp+server&tags=story&hitsPerPage=8"
            .into(),
        body(json!({"hits": "abc"})),
    );
    put(
        "https://www.reddit.com/r/ClaudeAI/new.json?limit=15".into(),
        body(json!({"data": {"children": [
            {"data": {"title": "New MCP server released", "permalink": "/r/ClaudeAI/1", "created_utc": 1_791_185_000.75}},
            {"data": {"title": "Nada que ver", "permalink": "/r/ClaudeAI/2", "created_utc": 1_791_185_000}}
        ]}})),
    );
    put(
        "https://www.reddit.com/r/ClaudeCode/new.json?limit=15".into(),
        body(json!({"data": [1]})),
    );
    put(
        "https://www.reddit.com/r/mcp/new.json?limit=15".into(),
        body(json!({"data": {"children": [
            {"data": {"title": "My skills list", "permalink": "/r/mcp/3", "created_utc": 1_791_186_000}},
            {"data": {"title": 5}}
        ]}})),
    );
    put(
        "https://api.github.com/repos/anthropics/skills/commits?per_page=5".into(),
        body(json!([
            {"commit": {"message": "Add pdf skill\n\nbody", "author": {"date": "2026-10-05T07:00:00Z"}}, "html_url": "https://gh.test/a"},
            {"commit": {"message": "Fix\u{2028}otra", "author": {"date": "ayer"}}, "html_url": "https://gh.test/b"}
        ])),
    );
    put(
        "https://api.github.com/repos/modelcontextprotocol/servers/commits?per_page=5".into(),
        body(json!([
            {"commit": {"message": "Server x", "author": {"date": "2026-10-04T23:59:59.250+02:00"}}, "html_url": "https://gh.test/c"},
            {"commit": {"message": ""}, "html_url": "https://gh.test/d"}
        ])),
    );
    put(
        "https://devpost.com/api/hackathons?page=1".into(),
        body(json!({"hackathons": [
            {"title": "Online Hack", "url": "https://dp.test/1", "open_state": "open", "displayed_location": {"location": "Online"},
             "prize_amount": "<b>$1,000</b> in prizes ", "registrations_count": 120, "submission_period_dates": "Oct 1 - 20"},
            {"title": "In person", "url": "https://dp.test/2", "displayed_location": {"location": "Madrid"}},
            {"title": "Invite", "url": "https://dp.test/3", "invite_only": true, "displayed_location": {"location": "online"}},
            {"title": "Ended", "url": "https://dp.test/4", "open_state": "ended", "displayed_location": {"location": "online"}}
        ]})),
    );
    put(
        "https://earn.superteam.fun/api/listings".into(),
        body(json!([
            {"title": "Range bounty", "slug": "rb", "compensationType": "range", "minRewardAsk": 100, "maxRewardAsk": 2.5, "deadline": "2026-11-01"},
            {"title": "Fixed", "slug": "fx", "rewardAmount": 0, "token": "SOL"},
            {"title": "Sin slug"}
        ])),
    );
    put(
        "https://dorahacks.io/api/hackathon/?page=1".into(),
        body(json!({"results": [
            {"name": "Pasado", "uname": "p", "end_time": NOW - 1},
            {"title": "Futuro", "slug": "f", "end_time": (NOW + 86_400).to_string(), "prize_pool": 5000},
            {"name": "Solo url", "url": "https://dh.test/u", "bounty_prize": "$1"}
        ]})),
    );
    Value::Object(map)
}

const NEWS_ORACLE: &str = r#"
import base64, email.message, io, json, os, sys, time, urllib.error, urllib.request
repo, home, fixtures, now, mode = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4]), sys.argv[5]
sys.path.insert(0, os.path.join(repo, "lib"))
import news_watch
news_watch.SEARX = "http://searx.test:27950"
time.time = lambda: float(now) + 0.5
fx = json.load(open(fixtures))
calls = []
class Resp(io.BytesIO):
    def __enter__(self): return self
    def __exit__(self, *a): return False
def fake(req, timeout=None):
    url = req.full_url
    calls.append([url, req.get_header("User-agent"), req.get_header("Accept"), timeout])
    r = fx.get(url)
    if r is None:
        raise urllib.error.URLError("sin fixture")
    if "error" in r:
        raise urllib.error.URLError(r["error"])
    if "status" in r:
        h = email.message.Message()
        if r.get("location"):
            h["Location"] = r["location"]
        raise urllib.error.HTTPError(url, r["status"], "x", h, io.BytesIO(b""))
    return Resp(base64.b64decode(r["body"]))
urllib.request.urlopen = fake
if mode == "collect":
    c = news_watch.collect(now)
    print(json.dumps({"items": c["items"], "failed": sorted(f["source"] for f in c["failures"]), "calls": calls}))
else:
    try:
        r = news_watch.watch_news(os.path.join(home, ".claude/hooks"), now=now)
        print(json.dumps({"news": r["news"], "calls": calls}))
    except Exception as e:
        print(json.dumps({"raises": type(e).__name__, "calls": calls}))
"#;

/// El `Fetch` del lado Rust sobre las mismas respuestas.
struct FakeFetch {
    fixtures: Value,
    calls: Mutex<Vec<Value>>,
}

impl Fetch for FakeFetch {
    fn get(&self, url: &str, headers: &[(&str, &str)], timeout: Duration) -> Fetched {
        let header = |name: &str| {
            headers
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| Value::from(*v))
                .unwrap_or(Value::Null)
        };
        self.calls.lock().unwrap().push(json!([
            url,
            header("User-Agent"),
            header("Accept"),
            timeout.as_secs()
        ]));
        let Some(r) = self.fixtures.get(url) else {
            return Fetched::Failed("sin fixture".into());
        };
        if let Some(e) = r.get("error") {
            return Fetched::Failed(e.as_str().unwrap_or_default().into());
        }
        if let Some(code) = r.get("status") {
            return Fetched::Status {
                code: code.as_u64().unwrap() as u16,
                location: r.get("location").and_then(Value::as_str).map(str::to_owned),
            };
        }
        Fetched::Body(
            base64::engine::general_purpose::STANDARD
                .decode(r["body"].as_str().unwrap())
                .unwrap(),
        )
    }
}

fn news_py(home: &Path, fixtures: &Value, mode: &str) -> Option<Value> {
    let path = home.join("fixtures.json");
    fs::write(&path, fixtures.to_string()).unwrap();
    let now = NOW.to_string();
    let out = run_python(
        NEWS_ORACLE,
        &[
            home.as_os_str(),
            path.as_os_str(),
            OsStr::new(&now),
            OsStr::new(mode),
        ],
        home,
    )?;
    Some(serde_json::from_str(out.trim()).unwrap())
}

fn news_rust(home: &Path, fixtures: &Value) -> Value {
    let fetch = FakeFetch {
        fixtures: fixtures.clone(),
        calls: Mutex::new(Vec::new()),
    };
    let endpoints = Endpoints::production(Some(SEARX));
    let clock = || NOW as f64 + 0.5;
    let sources = Sources {
        fetch: &fetch,
        endpoints: &endpoints,
        clock: &clock,
    };
    let out = match watch_news(&sources, &home.join(".claude/hooks"), NOW) {
        Ok(w) => json!({"news": w.news}),
        Err(e) => json!({"raises": format!("{e:?}")}),
    };
    let mut out = out;
    out["calls"] = Value::Array(fetch.calls.into_inner().unwrap());
    out
}

fn history(home: &Path) -> Vec<Vec<String>> {
    let con = rusqlite::Connection::open(home.join(".claude/hooks/news-history.sqlite")).unwrap();
    let mut stmt = con
        .prepare("select url, source, kind, title, at, first_seen, last_seen, meta from events order by url")
        .unwrap();
    stmt.query_map([], |row| {
        (0..8)
            .map(|i| {
                row.get::<_, rusqlite::types::Value>(i)
                    .map(|v| format!("{v:?}"))
            })
            .collect::<Result<Vec<_>, _>>()
    })
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

fn news_snapshot(home: &Path) -> String {
    fs::read_to_string(home.join(".claude/hooks/news-watch.json")).unwrap()
}

#[test]
fn watch_news_matches_python() {
    let (a, b) = (home("news-a"), home("news-b"));
    let first = feeds(false);
    let Some(expected) = news_py(&b, &first, "watch") else {
        return;
    };
    let got = news_rust(&a, &first);
    assert_eq!(got, expected);
    // Primer arranque: siembra el panel sin noticias.
    assert_eq!(got["news"], json!([]));
    assert_eq!(news_snapshot(&a), news_snapshot(&b));
    assert_eq!(history(&a), history(&b));
    assert!(news_snapshot(&a).contains("patr\\u00f3n Radar"));
    let sources: std::collections::BTreeSet<String> =
        history(&a).into_iter().map(|row| row[1].clone()).collect();
    for source in [
        "searxng",
        "hackernews",
        "r/ClaudeAI",
        "r/mcp",
        "github/anthropics",
        "github/modelcontextprotocol",
        "devpost",
        "superteam",
        "dorahacks",
    ] {
        assert!(
            sources.contains(&format!("Text(\"{source}\")")),
            "{source}: {sources:?}"
        );
    }
    assert_eq!(history(&a).len(), 16, "{:#?}", history(&a));
    // Segunda vuelta: solo lo nuevo es noticia.
    let second = feeds(true);
    let expected = news_py(&b, &second, "watch").unwrap();
    let got = news_rust(&a, &second);
    assert_eq!(got, expected);
    assert_eq!(got["news"].as_array().unwrap().len(), 1, "{got}");
    assert_eq!(news_snapshot(&a), news_snapshot(&b));
    assert_eq!(history(&a), history(&b));
    let _ = fs::remove_dir_all(&a);
    let _ = fs::remove_dir_all(&b);
}

#[test]
fn collect_matches_python() {
    let (a, b) = (home("collect-a"), home("collect-b"));
    let fixtures = feeds(false);
    let Some(expected) = news_py(&b, &fixtures, "collect") else {
        return;
    };
    let fetch = FakeFetch {
        fixtures: fixtures.clone(),
        calls: Mutex::new(Vec::new()),
    };
    let endpoints = Endpoints::production(Some(SEARX));
    let clock = || NOW as f64 + 0.5;
    let sources = Sources {
        fetch: &fetch,
        endpoints: &endpoints,
        clock: &clock,
    };
    let (items, failures) = sources.collect(NOW);
    let mut failed: Vec<String> = failures.into_iter().map(|f| f.source).collect();
    failed.sort();
    assert_eq!(Value::Array(items), expected["items"]);
    assert_eq!(json!(failed), expected["failed"]);
    assert_eq!(
        Value::Array(fetch.calls.into_inner().unwrap()),
        expected["calls"]
    );
    let _ = fs::remove_dir_all(&a);
    let _ = fs::remove_dir_all(&b);
}

/// Snapshots previos con formas raras.
#[test]
fn watch_news_odd_previous_snapshots_match_python() {
    let cases = [
        ("lista", "[1]", true),
        ("seen-numero", r#"{"seen": 5}"#, true),
        ("recent-texto", r#"{"recent": "x"}"#, true),
        ("recent-sin-at", r#"{"recent": [{"title": "x"}]}"#, true),
        (
            "seen-texto",
            r#"{"seen": "https://s.test/1", "recent": [{"at": 1.5}]}"#,
            false,
        ),
        ("roto", "{", false),
    ];
    let fixtures = feeds(false);
    for (tag, prev, raises) in cases {
        let (a, b) = (
            home(&format!("nodd-{tag}-a")),
            home(&format!("nodd-{tag}-b")),
        );
        for h in [&a, &b] {
            write(&h.join(".claude/hooks/news-watch.json"), prev);
        }
        let Some(expected) = news_py(&b, &fixtures, "watch") else {
            return;
        };
        let got = news_rust(&a, &fixtures);
        if raises {
            assert!(expected.get("raises").is_some(), "{tag}: {expected}");
            assert!(got.get("raises").is_some(), "{tag}: {got}");
            assert_eq!(got["calls"], expected["calls"], "{tag}");
        } else {
            assert_eq!(got, expected, "{tag}");
        }
        assert_eq!(news_snapshot(&a), news_snapshot(&b), "{tag}");
        let _ = fs::remove_dir_all(&a);
        let _ = fs::remove_dir_all(&b);
    }
}
