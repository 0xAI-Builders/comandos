use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

pub const PROFILE: &[u8] = include_bytes!("../assets/comandos-browser-chrome");
const TARGET: &str = "/etc/apparmor.d/comandos-browser-chrome";

pub fn profile_text() -> &'static str {
    std::str::from_utf8(PROFILE).unwrap_or("")
}

pub fn install() -> Result<(), String> {
    if current_uid()? != 0 {
        return Err("Ejecuta: sudo comandos-broker-mac apparmor install".to_owned());
    }
    let chrome = profile_chrome_path().ok_or_else(|| {
        "perfil AppArmor inválido: no se encontró la ruta del ejecutable Chrome".to_owned()
    })?;
    if !chrome.exists() {
        return Err(format!(
            "no existe el ejecutable Chrome fijado: {}",
            chrome.display()
        ));
    }
    let target = Path::new(TARGET);
    if target.exists() && std::fs::read(target).map_err(|e| e.to_string())? != PROFILE {
        return Err(format!("Refusing to replace a different existing {TARGET}"));
    }
    let tmp = target.with_extension(format!("tmp.{}", std::process::id()));
    std::fs::write(&tmp, PROFILE).map_err(|e| e.to_string())?;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o644))
        .map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, target).map_err(|e| e.to_string())?;
    let status = Command::new("apparmor_parser")
        .args(["-r", TARGET])
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("apparmor_parser -r {TARGET} falló con {status}"));
    }
    Ok(())
}

fn current_uid() -> Result<u32, String> {
    let out = Command::new("id")
        .arg("-u")
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err("no se pudo consultar id -u".to_owned());
    }
    std::str::from_utf8(&out.stdout)
        .map_err(|e| e.to_string())?
        .trim()
        .parse::<u32>()
        .map_err(|e| e.to_string())
}

fn profile_chrome_path() -> Option<PathBuf> {
    profile_text().lines().find_map(|line| {
        let rest = line.strip_prefix("profile comandos-browser-chrome ")?;
        let path = rest.split(" flags=").next()?;
        Some(PathBuf::from(path))
    })
}
