//! Isolated differential-test entry point; not an installed command.
use std::{io::Read, path::PathBuf};
fn main() {
    let result = (|| {
        let home = PathBuf::from(std::env::args().nth(1).ok_or("Missing fixture home")?);
        let mut bytes = Vec::new();
        std::io::stdin()
            .take(16 * 1024 * 1024)
            .read_to_end(&mut bytes)
            .map_err(|_| "Fixture read failed")?;
        let catalog = comandos_extensions::auth::parse_config_bytes(&bytes)?;
        comandos_extensions::auth::import_credentials(&home, &catalog)
    })();
    match result {
        Ok(names) => println!("{}", serde_json::to_string(&names).unwrap()),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
