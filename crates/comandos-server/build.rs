//! Bake component metadata into the server; installed binaries need no checkout.
use std::{env, fs, io, path::PathBuf};

fn main() -> io::Result<()> {
    let dir = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR")
            .ok_or_else(|| io::Error::other("missing manifest directory"))?,
    )
    .join("../comandos-web/components");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut files = fs::read_dir(&dir)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<io::Result<Vec<_>>>()?;
    files.retain(|path| {
        path.extension()
            .is_some_and(|extension| extension == "json")
    });
    files.sort();
    if files.is_empty() {
        return Err(io::Error::other("component metadata is empty"));
    }
    let mut generated = String::from("const EMBEDDED_COMPONENTS: &[&str] = &[\n");
    for path in files {
        println!("cargo:rerun-if-changed={}", path.display());
        generated.push_str(&format!("{:?},\n", fs::read_to_string(path)?));
    }
    generated.push_str("];\n");
    let output = PathBuf::from(
        env::var_os("OUT_DIR").ok_or_else(|| io::Error::other("missing build output directory"))?,
    );
    fs::write(output.join("web_components.rs"), generated)
}
