//! Test-only JSONL serialization oracle; never installed with the application.
use comandos_core::json::parse_value;
use comandos_extensions::python_json::{default_len, digest, dumps};
use serde_json::json;
use std::io::{self, BufRead, Write};
fn main() -> io::Result<()> {
    let mut output = io::BufWriter::new(io::stdout().lock());
    for line in io::stdin().lock().lines() {
        let line = line?;
        let result=(||{
            let value=parse_value(&line).map_err(|_|"Invalid JSON".to_string())?;
            Ok::<_,String>(json!({"ascii":dumps(&value,true,true)?,"unicode":dumps(&value,false,true)?,"default_len":default_len(&value)?,"digest":digest(&value)?}))
        })().unwrap_or_else(|error|json!({"error":error}));
        serde_json::to_writer(&mut output, &result)?;
        writeln!(output)?;
    }
    output.flush()
}
