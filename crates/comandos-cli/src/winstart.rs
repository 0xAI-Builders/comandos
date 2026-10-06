//! Windows Start menu integration. External tools receive argument vectors.
use std::{
    env,
    ffi::OsString,
    fs,
    path::PathBuf,
    process::{Command, Output},
};

const ICON: &[u8] = include_bytes!("../../../dash/comandos.ico");

pub struct Config {
    pub kernel_release: String,
    pub distro: String,
    pub user: String,
    pub home: PathBuf,
    pub path: OsString,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Outcome {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Outcome {
    fn fail(message: impl Into<String>) -> Self {
        Self {
            code: 1,
            stdout: String::new(),
            stderr: message.into(),
        }
    }
}

fn tool(config: &Config, name: &str, args: &[&str]) -> Result<Output, Outcome> {
    let Some(program) = comandos_runtime::providers::which_path(name, Some(&config.path)) else {
        return Err(Outcome {
            code: 127,
            stdout: String::new(),
            stderr: format!("cc-winstart: no encuentro {name} en PATH.\n"),
        });
    };
    Command::new(program)
        .args(args)
        .env("HOME", &config.home)
        .env("PATH", &config.path)
        .output()
        .map_err(|error| Outcome::fail(format!("cc-winstart: {name}: {error}\n")))
}

fn captured(
    config: &Config,
    name: &str,
    args: &[&str],
    stderr: &mut String,
) -> Result<String, Outcome> {
    let output = tool(config, name, args)?;
    stderr.push_str(&String::from_utf8_lossy(&output.stderr));
    if !output.status.success() {
        return Err(Outcome {
            code: output.status.code().unwrap_or(1),
            stdout: String::new(),
            stderr: stderr.clone(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .trim_end_matches('\n')
        .to_owned())
}

fn ps_string(raw: &str) -> String {
    raw.replace('\'', "''")
}

pub fn run(config: &Config, args: &[String]) -> Outcome {
    if !config
        .kernel_release
        .to_ascii_lowercase()
        .contains("microsoft")
    {
        return Outcome::fail("cc-winstart: este script solo aplica en WSL2.\n");
    }
    if comandos_runtime::providers::which_path("powershell.exe", Some(&config.path)).is_none() {
        return Outcome::fail("cc-winstart: no encuentro powershell.exe en PATH.\n");
    }
    let distro = if config.distro.is_empty() {
        "Ubuntu"
    } else {
        &config.distro
    };
    if distro == "."
        || distro == ".."
        || distro
            .chars()
            .any(|c| c.is_control() || "/\\:<>\"|?*".contains(c))
    {
        return Outcome::fail("cc-winstart: nombre de distro inválido.\n");
    }
    let mut stderr = String::new();
    let appdata = match captured(
        config,
        "powershell.exe",
        &["-NoProfile", "-Command", "[Console]::Write($env:APPDATA)"],
        &mut stderr,
    ) {
        Ok(path) => path.replace('\r', ""),
        Err(error) => return error,
    };
    let localdata = match captured(
        config,
        "powershell.exe",
        &[
            "-NoProfile",
            "-Command",
            "[Console]::Write($env:LOCALAPPDATA)",
        ],
        &mut stderr,
    ) {
        Ok(path) => path.replace('\r', ""),
        Err(error) => return error,
    };
    if appdata.is_empty() || localdata.is_empty() {
        stderr.push_str("cc-winstart: no pude leer %APPDATA% / %LOCALAPPDATA%.\n");
        return Outcome {
            code: 1,
            stdout: String::new(),
            stderr,
        };
    }
    let appdir = match captured(config, "wslpath", &[&appdata], &mut stderr) {
        Ok(path) => PathBuf::from(path),
        Err(error) => return error,
    };
    let localdir = match captured(config, "wslpath", &[&localdata], &mut stderr) {
        Ok(path) => PathBuf::from(path),
        Err(error) => return error,
    };
    let sm_dir = appdir
        .join("Microsoft/Windows/Start Menu/Programs")
        .join(distro);
    let link_name = format!("ComandOS ({distro}).lnk");
    let lnk_wsl = sm_dir.join(&link_name);
    if args.first().is_some_and(|a| a == "--uninstall") {
        if let Err(error) = fs::remove_file(&lnk_wsl)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            return Outcome::fail(format!("cc-winstart: {}: {error}\n", lnk_wsl.display()));
        }
        return Outcome {
            code: 0,
            stdout: format!("cc-winstart: shortcut removido ({distro}).\n"),
            stderr,
        };
    }
    let icon_dir = localdir.join("ComandOS");
    if let Err(error) = fs::create_dir_all(&icon_dir)
        .and_then(|()| fs::create_dir_all(&sm_dir))
        .and_then(|()| fs::write(icon_dir.join("comandos.ico"), ICON))
    {
        return Outcome::fail(format!("cc-winstart: {error}\n"));
    }
    let lnk_win =
        format!("{appdata}\\Microsoft\\Windows\\Start Menu\\Programs\\{distro}\\{link_name}");
    let ico_win = format!("{localdata}\\ComandOS\\comandos.ico");
    let launch_args = format!(
        "-d {distro} -- env GDK_BACKEND=x11 /home/{}/.local/bin/cc-app",
        config.user
    );
    let script = format!(
        "\n$s = (New-Object -ComObject WScript.Shell).CreateShortcut('{}')\n$s.TargetPath  = [System.IO.Path]::Combine($env:SystemRoot, 'System32', 'wsl.exe')\n$s.Arguments   = '{}'\n$s.Description = 'ComandOS — comando central de sesiones Claude Code'\n$s.IconLocation = '{}'\n$s.WindowStyle  = 7\n$s.Save()\n",
        ps_string(&lnk_win),
        ps_string(&launch_args),
        ps_string(&ico_win)
    );
    let output = match tool(
        config,
        "powershell.exe",
        &["-NoProfile", "-Command", &script],
    ) {
        Ok(output) => output,
        Err(error) => return error,
    };
    stderr.push_str(&String::from_utf8_lossy(&output.stderr));
    let mut stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if !output.status.success() {
        stderr.push_str("cc-winstart: PowerShell falló al crear el shortcut.\n");
        return Outcome {
            code: 1,
            stdout,
            stderr,
        };
    }
    stdout.push_str(&format!("cc-winstart: instalado — busca 'ComandOS' en el menú Inicio.\n  shortcut : {}\n  ícono    : {ico_win}\n",lnk_wsl.display()));
    Outcome {
        code: 0,
        stdout,
        stderr,
    }
}

pub fn main(args: &[String]) -> i32 {
    let config = Config {
        kernel_release: fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default(),
        distro: env::var("WSL_DISTRO_NAME").unwrap_or_else(|_| "Ubuntu".into()),
        user: env::var("USER").unwrap_or_default(),
        home: env::var_os("HOME").map(PathBuf::from).unwrap_or_default(),
        path: env::var_os("PATH").unwrap_or_default(),
    };
    let result = run(&config, args);
    print!("{}", result.stdout);
    eprint!("{}", result.stderr);
    result.code
}
