//! Original public-key setup contract, using explicit SSH argument vectors.
use std::{
    env, fs,
    path::PathBuf,
    process::{Command, Stdio},
};

pub fn main(args: &[String]) -> i32 {
    let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let key = args
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".ssh/id_ed25519.pub"));
    if !key.is_file() {
        println!("No existe la llave {}", key.display());
        return 1;
    }
    // awk selects only the first alias in each Host line and skips patterns.
    let config = fs::read_to_string(home.join(".ssh/config")).unwrap_or_default();
    let hosts: Vec<&str> = config
        .lines()
        .filter_map(|line| {
            let mut words = line.split_ascii_whitespace();
            let field = words.next()?;
            let host = words.next()?;
            (field.eq_ignore_ascii_case("host") && !host.contains(['*', '?'])).then_some(host)
        })
        .collect();
    if hosts.is_empty() {
        println!("No hay servidores en ~/.ssh/config");
        return 1;
    }
    println!("Llave: {}\n", key.display());
    let mut pending = 0;
    for host in hosts {
        let present = Command::new("ssh")
            .args([
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=6",
                host,
                "true",
            ])
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if present {
            println!("  OK     {host} (ya entra con llave, sin password)");
            continue;
        }
        println!("\n>> {host} pide password. Tecleala UNA ultima vez:");
        let installed = Command::new("ssh-copy-id")
            .args(["-o", "ConnectTimeout=8", "-i"])
            .arg(&key)
            .arg(host)
            .status()
            .is_ok_and(|s| s.success());
        if installed {
            println!("  LISTO  {host} — no vuelve a pedir password");
        } else {
            println!(
                "  FALLO  {host} (password incorrecta o sin acceso); reintenta con: ssh-copy-id {host}"
            );
            pending += 1;
        }
    }
    println!();
    if pending == 0 {
        println!("Todos los servidores entran con llave. Se acabaron los passwords.");
    } else {
        println!("{pending} servidor(es) quedaron pendientes.");
    }
    // The maintained Bash command also exits successfully with pending hosts.
    0
}
