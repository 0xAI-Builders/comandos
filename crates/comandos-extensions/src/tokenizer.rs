//! Tokenizer allocation is confined to the transient `count` child.
use crate::metadata::{MAX_BYTES, lock_file, private_dir};
use base64::{Engine, engine::general_purpose::STANDARD};
use rustc_hash::FxHashMap;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
pub const ENCODING_FILE: &str = "9b5ad71b2ce5302211f9c61530b329a4922fc6a4";
const ENCODING_HASH: &str = "223921b76ee99bde995b7ff738513eef100fb51d18c93597a113bcffe865b2a7";
const PATTERN: &str = r"'(?i:[sdmt]|ll|ve|re)|[^\r\n\p{L}\p{N}]?+\p{L}++|\p{N}{1,3}+| ?[^\s\p{L}\p{N}]++[\r\n]*+|\s++$|\s*[\r\n]|\s+(?!\S)|\s";
// Eight million maximally escaped scalars plus framing for ten thousand texts.
const MAX_INPUT: usize = 48_100_001;
const MAX_OUTPUT: u64 = 200_001;
struct ChildGuard(Option<tokio::process::Child>);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            // Cancellation can run while the Tokio runtime itself is shutting down.
            // SIGKILL plus synchronous try_wait reaps without scheduling another task.
            let _ = child.start_kill();
            loop {
                match child.try_wait() {
                    Ok(Some(_)) | Err(_) => break,
                    Ok(None) => std::thread::sleep(Duration::from_millis(1)),
                }
            }
        }
    }
}
fn valid_texts(texts: &[String]) -> bool {
    texts.len() <= 10000
        && texts
            .iter()
            .try_fold(0usize, |n, t| n.checked_add(t.chars().count()))
            .is_some_and(|n| n <= 8_000_000)
}
pub async fn isolated_token_counts(home: &Path, texts: Vec<String>) -> Vec<Option<u64>> {
    let Ok(executable) = std::env::current_exe() else {
        return vec![None; texts.len()];
    };
    isolated_token_counts_with(&executable, home, texts, Duration::from_secs(8)).await
}
/// Explicit executable and deadline support isolated failure/lifecycle fixtures.
pub async fn isolated_token_counts_with(
    executable: &Path,
    home: &Path,
    texts: Vec<String>,
    timeout: Duration,
) -> Vec<Option<u64>> {
    let unknown = vec![None; texts.len()];
    if texts.is_empty() || !valid_texts(&texts) {
        return unknown;
    }
    let Ok(input) = serde_json::to_vec(&texts) else {
        return unknown;
    };
    if input.len() >= MAX_INPUT {
        return unknown;
    }
    let Ok(child) = tokio::process::Command::new(executable)
        .arg("--home")
        .arg(home)
        .arg("count")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
    else {
        return unknown;
    };
    let mut guard = ChildGuard(Some(child));
    let child = guard.0.as_mut().unwrap();
    let Some(mut stdin) = child.stdin.take() else {
        return unknown;
    };
    let Some(stdout) = child.stdout.take() else {
        return unknown;
    };
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
        let ((), output, status) = tokio::try_join!(write, read, child.wait())?;
        if !status.success() || output.len() as u64 >= MAX_OUTPUT {
            return Err(std::io::Error::other("Count unavailable"));
        }
        let counts: Vec<Option<u64>> =
            serde_json::from_slice(&output).map_err(|_| std::io::Error::other("Invalid count"))?;
        if counts.len() != texts.len() {
            return Err(std::io::Error::other("Invalid count"));
        }
        Ok(counts)
    };
    let result = tokio::time::timeout(timeout, operation).await;
    if matches!(result, Ok(Ok(_))) {
        guard.0.take();
    } else if let Some(child) = guard.0.as_mut() {
        let _ = child.kill().await;
        // Keep ownership in the guard across awaits: cancellation can happen
        // while cleanup itself is pending.
        if child.wait().await.is_ok() {
            guard.0.take();
        }
    }
    result.ok().and_then(Result::ok).unwrap_or(unknown)
}
fn slot(home: &Path, deadline: Instant) -> Option<fs::File> {
    let dir = home.join(".local/state/comandos/extensions/tokenizer-slots");
    private_dir(&dir).ok()?;
    let slots = [
        lock_file(&dir.join("0.lock")).ok()?,
        lock_file(&dir.join("1.lock")).ok()?,
    ];
    loop {
        for file in &slots {
            match file.try_lock() {
                Ok(()) => return file.try_clone().ok(),
                Err(fs::TryLockError::WouldBlock) => {}
                Err(_) => return None,
            }
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
}
fn offline_counts(home: &Path, texts: &[String]) -> Option<Vec<Option<u64>>> {
    let cache = std::env::var_os("TIKTOKEN_CACHE_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".cache/comandos/tiktoken"));
    offline_counts_at(&cache, texts)
}
/// Count using an explicit verified offline cache; no environment mutation or downloads.
pub fn offline_counts_at(cache: &Path, texts: &[String]) -> Option<Vec<Option<u64>>> {
    if !valid_texts(texts) {
        return None;
    }
    let mut bytes = Vec::new();
    fs::File::open(cache.join(ENCODING_FILE))
        .ok()?
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > MAX_BYTES || format!("{:x}", Sha256::digest(&bytes)) != ENCODING_HASH {
        return None;
    }
    let mut encoder = FxHashMap::default();
    for line in std::str::from_utf8(&bytes).ok()?.lines() {
        let (token, rank) = line.split_once(' ')?;
        encoder.insert(
            STANDARD.decode(token).ok()?,
            rank.parse::<tiktoken_rs::Rank>().ok()?,
        );
    }
    let special: FxHashMap<_, _> = [
        ("<|endoftext|>", 100257),
        ("<|fim_prefix|>", 100258),
        ("<|fim_middle|>", 100259),
        ("<|fim_suffix|>", 100260),
        ("<|endofprompt|>", 100276),
    ]
    .into_iter()
    .map(|(s, n)| (s.to_owned(), n))
    .collect();
    let encoding = tiktoken_rs::CoreBPE::new(encoder, special, PATTERN).ok()?;
    Some(
        texts
            .iter()
            .map(|text| Some(encoding.count_ordinary(text) as u64))
            .collect(),
    )
}
pub fn count_command(home: &Path) -> crate::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(MAX_INPUT as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "Invalid count input")?;
    let texts: Vec<String> = serde_json::from_slice(&bytes).map_err(|_| "Invalid count input")?;
    if bytes.len() >= MAX_INPUT || !valid_texts(&texts) {
        return Err("Count input limit reached".into());
    }
    let counts = if texts.is_empty() {
        Vec::new()
    } else if let Some(slot) = slot(home, deadline) {
        let counts = offline_counts(home, &texts).unwrap_or_else(|| vec![None; texts.len()]);
        // This command runs only in the transient counting process. Keep its
        // slot until OS process exit, including a blocked stdout write and any
        // allocator pages retained after CoreBPE is dropped. Exit/crash closes
        // this descriptor and releases flock atomically with process lifetime.
        use std::os::fd::IntoRawFd;
        let _ = slot.into_raw_fd();
        counts
    } else {
        vec![None; texts.len()]
    };
    println!(
        "{}",
        serde_json::to_string(&counts).map_err(|_| "Count unavailable")?
    );
    Ok(())
}
