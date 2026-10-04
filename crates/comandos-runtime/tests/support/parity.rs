//! Utilidades de la prueba de paridad del hook: binarios falsos generados por la
//! prueba, volcado normalizado de las bases SQLite y normalización de marcas de
//! tiempo e identificadores aleatorios (lo único que puede diferir entre corridas).
use rusqlite::{Connection, types::ValueRef};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, Instant};

/// Rango de tiempo de la prueba: segundos y milisegundos que se normalizan.
#[derive(Clone, Copy)]
pub struct Window {
    pub start_ms: i64,
    pub end_ms: i64,
}

impl Window {
    fn is_time(&self, n: i64) -> bool {
        let (s0, s1) = (self.start_ms / 1000 - 5, self.end_ms / 1000 + 5);
        (s0..=s1).contains(&n) || (self.start_ms - 5000..=self.end_ms + 5000).contains(&n)
    }
}

const FAKES: &[(&str, &str)] = &[
    (
        "tmux",
        "printf '%s\\n' \"tmux $*\" >> \"$FAKE_LOG\"\ncase \"$*\" in *'#S'*) echo fake-sess ;; *pane_pid*) echo \"$FAKE_PANE_PID\" ;; esac\n",
    ),
    (
        "pw-play",
        "printf '%s\\n' \"pw-play $*\" >> \"$FAKE_LOG\"\n",
    ),
    ("paplay", "printf '%s\\n' \"paplay $*\" >> \"$FAKE_LOG\"\n"),
    (
        "piper",
        "printf '%s\\n' \"piper $* <$(cat)>\" >> \"$FAKE_LOG\"\n",
    ),
    (
        "spd-say",
        "printf '%s\\n' \"spd-say $*\" >> \"$FAKE_LOG\"\n",
    ),
    (
        "osascript",
        "printf '%s\\n' \"osascript $*\" >> \"$FAKE_LOG\"\n",
    ),
    // El curl del bash: guarda el cuerpo y la URL; el Rust usa el notifyd falso.
    (
        "curl",
        "while [ $# -gt 0 ]; do\n  case \"$1\" in\n    -d) printf '%s\\n' \"$2\" >> \"$FAKE_POSTS\"; shift 2 ;;\n    http*) printf '%s\\n' \"$1\" >> \"$FAKE_POSTS.url\"; shift ;;\n    *) shift ;;\n  esac\ndone\n[ \"${FAKE_CURL_FAIL:-}\" = 1 ] && exit 7\nexit 0\n",
    ),
];

/// Escribe los binarios falsos (scripts de `sh` generados aquí, no del repo).
pub fn fake_bin(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    for (name, body) in FAKES {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// Cambia marcas de tiempo del rango por `T` (con su parte decimal) e
/// identificadores hexadecimales largos por `H`.
pub fn normalize(text: &str, window: Window) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex_end = i + bytes[i..]
            .iter()
            .take_while(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
            .count();
        let starts_word = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
        if starts_word && hex_end - i >= 16 {
            out.push('H');
            i = hex_end;
            continue;
        }
        let digit_end = i + bytes[i..].iter().take_while(|b| b.is_ascii_digit()).count();
        if starts_word && digit_end > i {
            if text[i..digit_end]
                .parse::<i64>()
                .is_ok_and(|n| window.is_time(n))
            {
                out.push('T');
                i = digit_end;
                if bytes.get(i) == Some(&b'.') && bytes.get(i + 1).is_some_and(u8::is_ascii_digit) {
                    i += 1;
                    while bytes.get(i).is_some_and(u8::is_ascii_digit) {
                        i += 1;
                    }
                }
            } else {
                out.push_str(&text[i..digit_end]);
                i = digit_end;
            }
            continue;
        }
        let ch = text[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Volcado de una base: esquema, `user_version` y todas las filas, normalizado.
/// Vacío si el archivo no existe.
pub fn dump_db(path: &Path, window: Window) -> Vec<String> {
    if !path.exists() {
        return Vec::new();
    }
    let conn = Connection::open(path).unwrap();
    let mut out = vec![format!(
        "user_version={}",
        conn.query_row("pragma user_version", [], |r| r.get::<_, i64>(0))
            .unwrap()
    )];
    let mut schema = conn
        .prepare("select type, name, coalesce(sql,'') from sqlite_master order by type, name")
        .unwrap();
    let objects: Vec<(String, String, String)> = schema
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    for (kind, name, sql) in objects {
        out.push(format!("{kind} {name}: {sql}"));
        if kind != "table" || name.starts_with("sqlite_") {
            continue;
        }
        let mut rows = conn.prepare(&format!("select * from \"{name}\"")).unwrap();
        let columns = rows.column_count();
        let mut query = rows.query([]).unwrap();
        while let Some(row) = query.next().unwrap() {
            let cells: Vec<String> = (0..columns)
                .map(|c| match row.get_ref(c).unwrap() {
                    ValueRef::Null => "NULL".into(),
                    ValueRef::Integer(n) if window.is_time(n) => "T".into(),
                    ValueRef::Integer(n) => n.to_string(),
                    ValueRef::Real(f) if window.is_time(f as i64) => "T".into(),
                    ValueRef::Real(f) => format!("{f:?}"),
                    ValueRef::Text(t) => normalize(&String::from_utf8_lossy(t), window),
                    ValueRef::Blob(b) => format!("blob({})", b.len()),
                })
                .collect();
            out.push(format!("  {name}: {}", cells.join(" | ")));
        }
    }
    out
}

/// Normaliza el registro de binarios falsos: `HOME` y el `.wav` temporal de piper.
pub fn normalize_log(log: &str, home: &Path) -> Vec<String> {
    let home = home.to_string_lossy();
    let mut lines: Vec<String> = log
        .lines()
        .map(|line| {
            let line = line.replace(home.as_ref(), "HOME");
            let mut out = String::new();
            let mut rest = line.as_str();
            while let Some(start) = rest.find("/tmp.") {
                let tail = &rest[start + 5..];
                out.push_str(&rest[..start + 5]);
                if tail.len() >= 14 && tail.is_char_boundary(10) && &tail[10..14] == ".wav" {
                    out.push_str("XXXXXXXXXX");
                    rest = &tail[10..];
                } else {
                    rest = tail;
                }
            }
            out.push_str(rest);
            out
        })
        .collect();
    // La voz y el popup corren en paralelo en ambos lados: el orden entre ellos no cuenta.
    lines.sort();
    lines
}

/// Lo observable de una corrida.
#[derive(Debug, PartialEq)]
pub struct Effects {
    pub code: Option<i32>,
    /// (nombre, modo, contenido) de cada archivo de `state/`.
    pub state: Vec<(String, u32, String)>,
    pub events: String,
    pub events_mode: Option<u32>,
    pub log: Vec<String>,
    pub posts: Vec<String>,
    pub intake: Vec<String>,
    pub usage: Vec<String>,
}

/// Lectura con pérdida: un byte inválido no debe vaciar la comparación.
pub fn read_lossy(path: &Path) -> String {
    String::from_utf8_lossy(&fs::read(path).unwrap_or_default()).into_owned()
}

fn mode(path: &Path) -> Option<u32> {
    fs::metadata(path)
        .ok()
        .map(|m| m.permissions().mode() & 0o777)
}

pub fn collect(home: &Path, code: Option<i32>, posts: Vec<String>, window: Window) -> Effects {
    let mut state: Vec<(String, u32, String)> = fs::read_dir(home.join(".claude/hooks/state"))
        .unwrap()
        .map(|e| {
            let p = e.unwrap().path();
            let text = normalize(&read_lossy(&p), window);
            (
                p.file_name().unwrap().to_string_lossy().into_owned(),
                mode(&p).unwrap(),
                text,
            )
        })
        .collect();
    state.sort();
    let events_path = home.join(".claude/hooks/events.jsonl");
    Effects {
        code,
        state,
        events: normalize(&read_lossy(&events_path), window),
        events_mode: mode(&events_path),
        log: normalize_log(&read_lossy(&home.join("fake.log")), home),
        posts,
        intake: dump_db(
            &home.join(".local/state/comandos/app-state.sqlite3"),
            window,
        ),
        usage: dump_db(&home.join(".claude/hooks/comandos-usage.sqlite"), window),
    }
}

pub fn lines(path: &Path) -> Vec<String> {
    read_lossy(path).lines().map(String::from).collect()
}

/// Espera, acotado, a que termine el proceso de entrega desacoplado del Rust
/// (`hook __deliver` con este `HOME`): uso, POST y sonido ya están hechos.
pub fn wait_for_delivery(home: &Path) {
    let marker = format!("HOME={}\0", home.display()).into_bytes();
    let running = || {
        fs::read_dir("/proc").unwrap().flatten().any(|entry| {
            let dir = entry.path();
            let cmdline = fs::read(dir.join("cmdline")).unwrap_or_default();
            cmdline.windows(9).any(|w| w == b"__deliver")
                && fs::read(dir.join("environ"))
                    .unwrap_or_default()
                    .windows(marker.len())
                    .any(|w| w == marker.as_slice())
        })
    };
    let deadline = Instant::now() + Duration::from_secs(15);
    while running() {
        assert!(
            Instant::now() < deadline,
            "el proceso de entrega no terminó"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
