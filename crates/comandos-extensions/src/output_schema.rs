//! Synchronous JSON Schema work stays inside a short-lived native child.
//! This helper is deliberately not connected to `check` until parity is settled.
use serde_json::Value;
use std::{
    io::{Read, Write},
    path::Path,
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    time::Instant,
};

const MAX_VALUE: usize = 8 * 1024 * 1024;
const MAX_INPUT: usize = 2 * MAX_VALUE + 34;
const MAX_OUTPUT: u64 = 32;
/// Linux/Unix worker address-space budget, not a host-wide RSS guarantee.
pub const ADDRESS_SPACE_BYTES: u64 = 384 * 1024 * 1024;
pub const CPU_SECONDS: (u64, u64) = (2, 3);

#[derive(Debug, PartialEq, Eq)]
pub enum Validation {
    Valid,
    InvalidSchema,
    InvalidContent,
}
#[derive(Debug, PartialEq, Eq)]
pub enum Failure {
    Deadline,
    InputLimit,
    Execution,
}

struct ChildGuard(Option<tokio::process::Child>);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.start_kill();
            // Reap without relying on another Tokio task. Drop also runs during
            // current-thread runtime shutdown and cancellation during cleanup.
            loop {
                match child.try_wait() {
                    Ok(Some(_)) | Err(_) => break,
                    Ok(None) => std::thread::sleep(Duration::from_millis(1)),
                }
            }
        }
    }
}

struct LimitedWriter {
    bytes: Vec<u8>,
    deadline: Instant,
    failure: Option<Failure>,
}
impl Write for LimitedWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let failure = if Instant::now() >= self.deadline {
            Some(Failure::Deadline)
        } else if bytes.len() > MAX_VALUE.saturating_sub(self.bytes.len()) {
            Some(Failure::InputLimit)
        } else {
            None
        };
        if let Some(failure) = failure {
            self.failure = Some(failure);
            return Err(std::io::Error::other("Schema input unavailable"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn serialize(value: &Value, deadline: Instant) -> Result<Vec<u8>, Failure> {
    let mut writer = LimitedWriter {
        bytes: Vec::new(),
        deadline,
        failure: None,
    };
    if serde_json::to_writer(&mut writer, value).is_err() {
        return Err(writer.failure.unwrap_or(Failure::Execution));
    }
    Ok(writer.bytes)
}

/// Explicit executable, objects, and absolute original-operation deadline.
/// Each encoded object is limited to the MCP transport's 8 MiB byte budget.
/// No home/config/tokenizer lookup, network retrieval, or Python fallback.
pub async fn validate_with(
    executable: &Path,
    schema: Value,
    content: Value,
    deadline: Instant,
) -> Result<Validation, Failure> {
    if Instant::now() >= deadline {
        return Err(Failure::Deadline);
    }
    if !schema.is_object() || !content.is_object() {
        return Err(Failure::Execution);
    }
    // Serialization checks the same absolute deadline on every writer call.
    // Validator compilation and instance evaluation happen only in the child.
    let schema = serialize(&schema, deadline)?;
    let content = serialize(&content, deadline)?;
    let mut input = Vec::with_capacity(schema.len() + content.len() + 34);
    input.extend_from_slice(b"{\"schema\":");
    input.extend_from_slice(&schema);
    input.extend_from_slice(b",\"structuredContent\":");
    input.extend_from_slice(&content);
    input.push(b'}');
    if Instant::now() >= deadline {
        return Err(Failure::Deadline);
    }
    let child = tokio::process::Command::new(executable)
        .arg("__schema_worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| Failure::Execution)?;
    let mut guard = ChildGuard(Some(child));
    let child = guard.0.as_mut().unwrap();
    let mut stdin = child.stdin.take().ok_or(Failure::Execution)?;
    let stdout = child.stdout.take().ok_or(Failure::Execution)?;
    let operation = async {
        let write = async {
            stdin.write_all(&input).await?;
            drop(stdin);
            Ok::<_, std::io::Error>(())
        };
        let read = async {
            let mut output = Vec::new();
            stdout.take(MAX_OUTPUT).read_to_end(&mut output).await?;
            Ok::<_, std::io::Error>(output)
        };
        let ((), output, status) =
            tokio::try_join!(write, read, child.wait()).map_err(|_| Failure::Execution)?;
        if !status.success() {
            return Err(Failure::Execution);
        }
        match output.as_slice() {
            b"valid\n" => Ok(Validation::Valid),
            b"invalid-schema\n" => Ok(Validation::InvalidSchema),
            b"invalid-content\n" => Ok(Validation::InvalidContent),
            _ => Err(Failure::Execution),
        }
    };
    let result = tokio::time::timeout_at(deadline, operation).await;
    if matches!(result, Ok(Ok(_))) {
        guard.0.take();
    } else if let Some(child) = guard.0.as_mut() {
        // Keep ownership even when cancellation interrupts kill/wait.
        let _ = child.kill().await;
        if child.wait().await.is_ok() {
            guard.0.take();
        }
    }
    result.map_err(|_| Failure::Deadline)?
}

fn resource_budget() -> Result<(), ()> {
    use nix::sys::resource::{Resource, setrlimit};
    setrlimit(
        Resource::RLIMIT_AS,
        ADDRESS_SPACE_BYTES,
        ADDRESS_SPACE_BYTES,
    )
    .map_err(|_| ())?;
    setrlimit(Resource::RLIMIT_CPU, CPU_SECONDS.0, CPU_SECONDS.1).map_err(|_| ())?;
    setrlimit(Resource::RLIMIT_CORE, 0, 0).map_err(|_| ())?;
    Ok(())
}
fn worker_status() -> &'static str {
    if resource_budget().is_err() {
        return "execution-failure\n";
    }
    let mut input = Vec::new();
    if std::io::stdin()
        .take(MAX_INPUT as u64 + 1)
        .read_to_end(&mut input)
        .is_err()
        || input.len() > MAX_INPUT
    {
        return "execution-failure\n";
    }
    let Ok(value) = serde_json::from_slice::<Value>(&input) else {
        return "execution-failure\n";
    };
    let Some(object) = value.as_object().filter(|o| o.len() == 2) else {
        return "execution-failure\n";
    };
    let (Some(schema), Some(content)) = (object.get("schema"), object.get("structuredContent"))
    else {
        return "execution-failure\n";
    };
    if !schema.is_object() || !content.is_object() {
        return "execution-failure\n";
    }
    let deadline = Instant::now() + Duration::from_secs(CPU_SECONDS.1);
    if serialize(schema, deadline).is_err() || serialize(content, deadline).is_err() {
        return "execution-failure\n";
    }
    // Schema checking remains enabled. Instance format assertions are disabled
    // for every draft, matching the SDK's default instance validator.
    let Ok(validator) = jsonschema::options()
        .offline()
        .should_validate_formats(false)
        .build(schema)
    else {
        return "invalid-schema\n";
    };
    match validator.validate(content) {
        Ok(()) => "valid\n",
        Err(_) => "invalid-content\n",
    }
}

/// Internal dispatch precedes all ordinary configuration and tokenizer loading.
/// Validator errors and untrusted input are never rendered to stdout/stderr.
pub fn worker_command() {
    let status = worker_status();
    let _ = std::io::stdout().write_all(status.as_bytes());
}
