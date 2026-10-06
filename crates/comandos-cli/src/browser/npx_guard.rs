use std::path::{Path, PathBuf};

pub fn is_browser_package(args: &[String]) -> bool {
    let mut seen = false;
    for arg in args {
        match arg.as_str() {
            "-y" | "--yes" | "--no-install" if !seen => {}
            "chrome-devtools-mcp" => return true,
            value if value.starts_with("chrome-devtools-mcp@") => return true,
            _ => return false,
        }
        seen = true;
    }
    false
}

pub fn real_npx(home: &Path) -> Result<(String, Vec<String>), String> {
    if let Some(path) = std::env::var_os("COMANDOS_REAL_NPX") {
        return Ok((
            "node".to_owned(),
            vec![PathBuf::from(path).display().to_string()],
        ));
    }
    let Some(path) = std::env::var_os("PATH") else {
        return Err("No encuentro el npx real; define COMANDOS_REAL_NPX".to_owned());
    };
    for dir in std::env::split_paths(&path) {
        let node = dir.join("node");
        if !node.exists() {
            continue;
        }
        let npm_root = dir
            .parent()
            .map(|prefix| prefix.join("lib/node_modules/npm/bin/npx-cli.js"));
        if let Some(npx) = npm_root.filter(|p| p.exists()) {
            return Ok((node.display().to_string(), vec![npx.display().to_string()]));
        }
    }
    let fallback = home.join(".nvm/versions/node/v22.19.0");
    let node = fallback.join("bin/node");
    let npx = fallback.join("lib/node_modules/npm/bin/npx-cli.js");
    if node.exists() && npx.exists() {
        return Ok((node.display().to_string(), vec![npx.display().to_string()]));
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
