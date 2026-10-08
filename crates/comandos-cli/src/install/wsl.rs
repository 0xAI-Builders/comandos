//! WSL setup is opt-in at the terminal and never shuts down running sessions.
use std::{
    fs,
    io::{self, IsTerminal, Write},
    path::Path,
    time::Duration,
};
pub fn systemd_config(before: &str) -> Result<String, String> {
    let mut section = "";
    let mut boot = None;
    let mut setting = None;
    for (offset, line) in before.split_inclusive('\n').scan(0, |offset, line| {
        let start = *offset;
        *offset += line.len();
        Some((start, line))
    }) {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            section = trimmed;
            if section == "[boot]" && boot.replace(offset + line.len()).is_some() {
                return Err("duplicate [boot] section; edit /etc/wsl.conf manually".into());
            }
        } else if section == "[boot]"
            && trimmed
                .split_once('=')
                .is_some_and(|(key, _)| key.trim() == "systemd")
            && setting.replace((offset, line)).is_some()
        {
            return Err("duplicate systemd setting; edit /etc/wsl.conf manually".into());
        }
    }
    if let Some((offset, line)) = setting {
        let equal = line.find('=').ok_or("invalid systemd setting")?;
        let raw = &line[equal + 1..];
        let start = raw.len() - raw.trim_start().len();
        let value_end = raw.find('#').unwrap_or(raw.len());
        let value = raw[..value_end].trim();
        if value == "true" {
            return Ok(before.into());
        }
        let finish = raw[..value_end].trim_end().len();
        return Ok(format!(
            "{}true{}{}",
            &before[..offset + equal + 1 + start],
            &raw[finish..],
            &before[offset + line.len()..]
        ));
    }
    if let Some(at) = boot {
        let separator = if at > 0 && !before[..at].ends_with('\n') {
            "\n"
        } else {
            ""
        };
        Ok(format!(
            "{}{separator}systemd=true\n{}",
            &before[..at],
            &before[at..]
        ))
    } else {
        Ok(format!(
            "{before}{}[boot]\nsystemd=true\n",
            if before.is_empty() || before.ends_with('\n') {
                ""
            } else {
                "\n"
            }
        ))
    }
}
pub fn packages(os_release: &str) -> Vec<&'static str> {
    let noble = os_release
        .lines()
        .any(|line| line == "VERSION_CODENAME=noble" || line == "VERSION_CODENAME=\"noble\"");
    // Native GTK/WebKit runtime, without the retired Python GI interpreter.
    vec![
        "tmux",
        "xclip",
        "wmctrl",
        "pulseaudio-utils",
        "wslu",
        if noble { "libgtk-3-0t64" } else { "libgtk-3-0" },
        "libvte-2.91-0",
        "libwebkit2gtk-4.1-0",
    ]
}
fn confirm(prompt: &str) -> Result<(), String> {
    if !io::stdin().is_terminal() {
        return Err(format!("{prompt}; run interactively to confirm"));
    }
    eprint!("{prompt} [y/N] ");
    io::stderr().flush().map_err(|e| e.to_string())?;
    let mut response = String::new();
    io::stdin()
        .read_line(&mut response)
        .map_err(|e| e.to_string())?;
    if matches!(
        response.trim().to_lowercase().as_str(),
        "y" | "yes" | "si" | "sí"
    ) {
        Ok(())
    } else {
        Err("cancelled; no privileged command executed".into())
    }
}
/// False means the operator must restart WSL themselves before installation.
pub fn prepare(home: &Path, dry: bool) -> Result<bool, String> {
    if !Path::new("/run/systemd/system").is_dir() {
        eprintln!(
            "systemd no está corriendo en tu WSL; cc-dash y cc-notifyd no pueden autoarrancar."
        );
        let path = Path::new("/etc/wsl.conf");
        let before = match fs::read_to_string(path) {
            Ok(value) => value,
            Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e.to_string()),
        };
        let after = systemd_config(&before)?;
        if after == before {
            return Err("/etc/wsl.conf ya tiene systemd=true; verifica con sudo systemctl is-system-running".into());
        }
        if dry {
            println!(
                "dry-run: proponer systemd=true dentro de [boot] en /etc/wsl.conf; no sudo, no shutdown"
            );
            return Ok(false);
        }
        confirm("¿Habilito systemd editando /etc/wsl.conf con respaldo?")?;
        let sudo = super::full::find("sudo").ok_or("sudo requerido para configurar WSL")?;
        super::full::command(
            home,
            &sudo,
            vec!["-n".into(), "true".into()],
            Duration::from_secs(10),
        )
        .map_err(|_| {
            "autentica sudo en esta terminal con sudo -v y vuelve a ejecutar comandos install"
                .to_owned()
        })?;
        let staging = home.join(".local/share/comandos/wsl.conf.proposed");
        super::release::check_app_parents(&staging)?;
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(staging.parent().ok_or("proposal without parent")?)
            .map_err(|e| e.to_string())?;
        comandos_store::files::write_atomic(&staging, after.as_bytes())
            .map_err(|e| e.to_string())?;
        if fs::read_to_string(path).unwrap_or_default() != before {
            return Err("/etc/wsl.conf cambió durante confirmación; no se sobrescribe".into());
        }
        if path.exists() {
            super::full::command(
                home,
                &sudo,
                vec![
                    "-n".into(),
                    "cp".into(),
                    "--preserve=mode,timestamps".into(),
                    "--no-clobber".into(),
                    "/etc/wsl.conf".into(),
                    "/etc/wsl.conf.pre-comandos".into(),
                ],
                Duration::from_secs(120),
            )?;
        }
        super::full::command(
            home,
            &sudo,
            vec![
                "-n".into(),
                "install".into(),
                "-m".into(),
                "0644".into(),
                staging.to_string_lossy().into_owned(),
                "/etc/wsl.conf".into(),
            ],
            Duration::from_secs(120),
        )?;
        println!(
            "Configurado. Desde PowerShell corre wsl --shutdown cuando tus sesiones estén listas; reabre y ejecuta comandos install."
        );
        return Ok(false);
    }
    let os = fs::read_to_string("/etc/os-release").map_err(|e| e.to_string())?;
    let packages = packages(&os);
    if dry {
        println!(
            "dry-run: comprobar dependencias WSL: {}",
            packages.join(" ")
        );
        return Ok(true);
    }
    let dpkg = super::full::find("dpkg-query")
        .ok_or("dpkg-query requerido para comprobar dependencias WSL")?;
    let mut missing = Vec::new();
    for package in packages {
        let output = super::full::capture(
            home,
            &dpkg,
            vec!["-W".into(), "-f=${Status}".into(), package.into()],
            Duration::from_secs(10),
        )?;
        if output.code != Some(0)
            || !String::from_utf8_lossy(&output.stdout).contains("install ok installed")
        {
            missing.push(package.to_owned());
        }
    }
    if !missing.is_empty() {
        eprintln!("Faltan paquetes: {}", missing.join(" "));
        confirm("¿Instalo ahora con sudo apt install?")?;
        let sudo = super::full::find("sudo").ok_or("sudo requerido para instalar dependencias")?;
        super::full::command(
            home,
            &sudo,
            vec!["-n".into(), "apt".into(), "update".into()],
            Duration::from_secs(600),
        )?;
        let mut args = vec!["-n".into(), "apt".into(), "install".into(), "-y".into()];
        args.extend(missing);
        super::full::command(home, &sudo, args, Duration::from_secs(1200))?;
    }
    Ok(true)
}
