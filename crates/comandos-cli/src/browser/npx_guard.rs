use std::path::{Path, PathBuf};

pub fn is_browser_package(args: &[String]) -> bool {
    for arg in args {
        match arg.as_str() {
            "-y" | "--yes" | "--no-install" => {}
            "chrome-devtools-mcp" => return true,
            value if value.starts_with("chrome-devtools-mcp@") => return true,
            _ => return false,
        }
    }
    false
}

pub fn real_npx(_home: &Path) -> Result<(String, Vec<String>), String> {
    let explicit = std::env::var_os("COMANDOS_REAL_NPX").map(PathBuf::from);
    let Some(path) = std::env::var_os("PATH") else {
        return Err("No encuentro el npx real; define COMANDOS_REAL_NPX".to_owned());
    };
    for dir in std::env::split_paths(&path) {
        let node = dir.join("node");
        use std::os::unix::fs::PermissionsExt;
        if !node.is_file()
            || node
                .metadata()
                .map(|m| m.permissions().mode() & 0o111 == 0)
                .unwrap_or(true)
            || same_executable(&node)
        {
            continue;
        }
        if let Some(npx) = explicit
            .as_ref()
            .filter(|path| path.is_file() && !same_executable(path))
        {
            return Ok((node.display().to_string(), vec![npx.display().to_string()]));
        }
        if explicit.is_some() {
            continue;
        }
        let npm_root = dir
            .parent()
            .map(|prefix| prefix.join("lib/node_modules/npm/bin/npx-cli.js"));
        if let Some(npx) = npm_root.filter(|p| p.is_file() && !same_executable(p)) {
            return Ok((node.display().to_string(), vec![npx.display().to_string()]));
        }
    }
    Err("No encuentro el npx real; define COMANDOS_REAL_NPX".to_owned())
}

pub fn run(args: &[String], home: impl AsRef<Path>) -> i32 {
    use std::os::unix::process::CommandExt;
    if is_browser_package(args) {
        let err =
            std::process::Command::new(home.as_ref().join(".local/bin/cc-browser-remote")).exec();
        eprintln!("cc-browser-npx-guard: no se pudo ejecutar cc-browser-remote: {err}");
        return 127;
    }
    let (program, mut rest) = match real_npx(home.as_ref()) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}");
            return 127;
        }
    };
    rest.extend(args.iter().cloned());
    let err = std::process::Command::new(program).args(rest).exec();
    eprintln!("cc-browser-npx-guard: no se pudo ejecutar npx real: {err}");
    127
}

fn same_executable(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (
        std::env::current_exe().and_then(std::fs::metadata),
        path.metadata(),
    ) {
        (Ok(current), Ok(candidate)) => {
            current.dev() == candidate.dev() && current.ino() == candidate.ino()
        }
        _ => true,
    }
}
