//! Differential contra el texto Python original; Python sólo corre como oráculo de prueba.
use std::os::unix::fs::{PermissionsExt, symlink};
use std::{fs, path::PathBuf, process::Command, time::SystemTime};

struct Fixture(PathBuf);

impl Fixture {
    fn new(html: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "comandos-css-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        for child in [
            "dash", "tools", "home", "config", "cache", "data", "state", "runtime", "tmp",
        ] {
            fs::create_dir(root.join(child)).unwrap();
        }
        fs::write(root.join("dash/index.html"), html).unwrap();
        fs::write(
            root.join("tools/css_orphans.py"),
            include_str!("../../tools/css_orphans.py"),
        )
        .unwrap();
        Self(root)
    }

    fn file(&self, name: &str, body: &str) {
        fs::write(self.0.join("dash").join(name), body).unwrap();
    }

    fn compare(&self, prefixes: &[&str]) -> String {
        let result = Command::new("/usr/bin/python3")
            .arg(self.0.join("tools/css_orphans.py"))
            .args(prefixes)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.0.join("home"))
            .env("XDG_CONFIG_HOME", self.0.join("config"))
            .env("XDG_CACHE_HOME", self.0.join("cache"))
            .env("XDG_DATA_HOME", self.0.join("data"))
            .env("XDG_STATE_HOME", self.0.join("state"))
            .env("XDG_RUNTIME_DIR", self.0.join("runtime"))
            .env("TMPDIR", self.0.join("tmp"))
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(result.stderr.is_empty());
        let wanted = prefixes.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let actual = xtask::css_orphans::scan(&self.0, &wanted).unwrap();
        let stdout = if actual.is_empty() {
            String::new()
        } else {
            format!("{}\n", actual.join("\n"))
        };
        assert_eq!(stdout.as_bytes(), result.stdout, "prefixes={prefixes:?}");
        stdout
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn python_parity_styles_prefixes_declarations_and_boundaries() {
    let fixture = Fixture::new(
        "<style>.gone,.used,#gone,.partial,.edge,.unicode,.combining,.connector { color:#inside;content:'.inside'; } @media x { .nested {x:1} } .outside {x:2}</style><style media='all'>.not-style{}</style><p class='used'>xpartial partial-more -edge unicode界 combininǵ connector‿</p>",
    );
    fixture.file("a.js", "const nested = true;");
    assert_eq!(
        fixture.compare(&[]),
        "#gone\n.edge\n.gone\n.outside\n.partial\n.unicode\n"
    );
    assert_eq!(fixture.compare(&[".g", "#"]), "#gone\n.gone\n");
    assert_eq!(fixture.compare(&["--help"]), "");
}

#[test]
fn python_parity_unicode_names_newlines_and_sorted_file_concatenation() {
    let fixture = Fixture::new(
        "<style>\r\n.aé,.a²,.a界,.á,.a‿,.joined,.hidden,.symlink,.newline {x:1}\r</style><p> a² a界́ newline\r</p>",
    );
    fixture.file("a.js", "join");
    fixture.file("z.js", "ed hidden");
    fixture.file(".hidden.js", "symlink");
    fixture.file("ignored.mjs", "aé");
    fixture.compare(&[]);
}

#[test]
fn follows_direct_js_symlinks_without_scanning_nested_files() {
    let fixture = Fixture::new("<style>.inlink,.nested,.gone {}</style>");
    fixture.file("target.txt", "inlink");
    symlink("target.txt", fixture.0.join("dash/link.js")).unwrap();
    fs::create_dir(fixture.0.join("dash/nested")).unwrap();
    fs::write(fixture.0.join("dash/nested/a.js"), "nested").unwrap();
    assert_eq!(fixture.compare(&[]), ".gone\n.nested\n");
}

#[test]
fn invalid_utf8_and_missing_input_fail_instead_of_reporting_clean() {
    let fixture = Fixture::new("<style>.gone {}</style>");
    fs::write(fixture.0.join("dash/index.html"), [0xff]).unwrap();
    assert!(xtask::css_orphans::scan(&fixture.0, &[]).is_err());
    fs::remove_file(fixture.0.join("dash/index.html")).unwrap();
    assert!(xtask::css_orphans::scan(&fixture.0, &[]).is_err());
}

#[test]
fn actual_binary_matches_original_on_checkout_from_arbitrary_directory() {
    let fixture = Fixture::new("");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    for args in [vec![], vec![".uw-", ".qh-", "#usage-"], vec!["--help"]] {
        let mut python = Command::new("/usr/bin/python3");
        python.arg(root.join("tools/css_orphans.py")).args(&args);
        let mut native = Command::new(env!("CARGO_BIN_EXE_xtask"));
        native.arg("css-orphans").args(&args);
        for command in [&mut python, &mut native] {
            command
                .current_dir(&fixture.0)
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("HOME", fixture.0.join("home"))
                .env("XDG_CONFIG_HOME", fixture.0.join("config"))
                .env("XDG_CACHE_HOME", fixture.0.join("cache"))
                .env("XDG_DATA_HOME", fixture.0.join("data"))
                .env("XDG_STATE_HOME", fixture.0.join("state"))
                .env("XDG_RUNTIME_DIR", fixture.0.join("runtime"))
                .env("TMPDIR", fixture.0.join("tmp"));
        }
        let python = python.output().unwrap();
        let native = native.output().unwrap();
        assert_eq!(native.status.code(), python.status.code());
        assert_eq!(native.stdout, python.stdout);
        assert_eq!(native.stderr, python.stderr);
    }
}
