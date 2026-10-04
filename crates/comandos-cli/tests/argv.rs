//! Un argumento que no es UTF-8 no hace entrar en pánico al despachador: sale con 2.
use std::{ffi::OsStr, os::unix::ffi::OsStrExt, process::Command};

#[test]
fn non_utf8_argument_exits_2_without_panicking() {
    for (bin, first) in [
        (env!("CARGO_BIN_EXE_comandos"), "ext"),
        (env!("CARGO_BIN_EXE_comandos"), "\u{e9}"),
    ] {
        let out = Command::new(bin)
            .arg(first)
            .arg(OsStr::from_bytes(b"\xff\xfe"))
            .env("XDG_RUNTIME_DIR", std::env::temp_dir().join("no-broker"))
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{bin}: {stderr}");
        assert!(stderr.contains("UTF-8"), "{bin}: {stderr}");
        assert!(!stderr.contains("panicked"), "{bin}: {stderr}");
    }
}
