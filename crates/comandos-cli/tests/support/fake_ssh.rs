use std::{env, fs, io::Write, time::Duration};
fn main() {
    if let Some(path) = env::var_os("FAKE_SSH_PID_FILE") { fs::write(path, std::process::id().to_string()).unwrap(); }
    let args = env::args().skip(1).collect::<Vec<_>>();
    let mut log = fs::OpenOptions::new().create(true).append(true).open(env::var_os("FAKE_SSH_LOG").unwrap()).unwrap();
    writeln!(log, "{args:?}").unwrap();
    if env::var_os("FAKE_SSH_FLOOD").is_some() {
        let bytes = vec![b'x'; 512 * 1024];
        std::io::stdout().write_all(&bytes).unwrap();
        std::io::stderr().write_all(&bytes).unwrap();
    }
    if env::var_os("FAKE_SSH_STALL").is_some() { std::thread::sleep(Duration::from_secs(20)); }
    let socket = args.windows(2).find(|pair| pair[0] == "-S").map(|pair| &pair[1]).unwrap();
    let operation = args.windows(2).find(|pair| pair[0] == "-O").map(|pair| pair[1].as_str());
    let success = match operation {
        Some("check") => fs::metadata(socket).is_ok(),
        Some("exit") => fs::remove_file(socket).is_ok(),
        _ => fs::write(socket, b"").is_ok(),
    };
    std::process::exit(if success { 0 } else { 1 });
}
