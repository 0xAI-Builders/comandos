use comandos_browser::wire::{Line, MAX_MESSAGE, encode_message, encode_status, read_line};
use serde_json::json;
use tokio::io::BufReader;

#[tokio::test]
async fn exact_limit_oversize_and_following_line_are_distinct() {
    let mut data = vec![b'x'; MAX_MESSAGE - 1];
    data.push(b'\n');
    data.extend(std::iter::repeat_n(b'y', MAX_MESSAGE));
    data.extend_from_slice(b"\n{}\nlast");
    let mut reader = BufReader::with_capacity(3071, data.as_slice());
    let mut buffer = Vec::new();
    assert!(
        matches!(read_line(&mut reader, &mut buffer).await.unwrap(), Line::Message(v) if v.len() == MAX_MESSAGE)
    );
    assert!(matches!(
        read_line(&mut reader, &mut buffer).await.unwrap(),
        Line::TooLarge
    ));
    assert!(buffer.capacity() <= MAX_MESSAGE + 8192);
    assert!(
        matches!(read_line(&mut reader, &mut buffer).await.unwrap(), Line::Message(v) if v == b"{}\n")
    );
    assert!(
        matches!(read_line(&mut reader, &mut buffer).await.unwrap(), Line::Message(v) if v == b"last")
    );
    assert!(matches!(
        read_line(&mut reader, &mut buffer).await.unwrap(),
        Line::Eof
    ));
}

#[tokio::test]
async fn preserves_raw_bytes_crlf_and_rejects_unterminated_oversize() {
    let mut reader = BufReader::new(&b"\xff\r\n\n"[..]);
    let mut buffer = Vec::new();
    assert!(
        matches!(read_line(&mut reader, &mut buffer).await.unwrap(), Line::Message(v) if v == b"\xff\r\n")
    );
    assert!(
        matches!(read_line(&mut reader, &mut buffer).await.unwrap(), Line::Message(v) if v == b"\n")
    );
    let data = vec![b'x'; MAX_MESSAGE + 1];
    let mut reader = BufReader::new(data.as_slice());
    assert!(matches!(
        read_line(&mut reader, &mut buffer).await.unwrap(),
        Line::TooLarge
    ));
}

#[test]
fn encoding_matches_python_including_order_unicode_and_float_spelling() {
    let value = json!({"jsonrpc":"2.0","id":1,"result":{"t":"ñ🙂 : , \\\"","n":1.0,"e":1e-7}});
    let input = value.to_string();
    for compact in [true, false] {
        let script = if compact {
            "import json,sys;print(json.dumps(json.loads(sys.argv[1]),separators=(',',':')))"
        } else {
            "import json,sys;sys.stdout.write(json.dumps(json.loads(sys.argv[1])))"
        };
        let oracle = comandos_oracle::oracle_at(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"),
            "browser-wire",
            &json!({"source":"CPython stdlib json", "raw": input, "compact": compact, "script": script}),
            || {
                let output = std::process::Command::new(
                    std::env::var("COMANDOS_BROWSER_ORACLE_PYTHON")
                        .unwrap_or_else(|_| "python3".into()),
                )
                .env_clear()
                .env("PYTHONIOENCODING", "utf-8")
                .args(["-c", script, &input])
                .output()
                .map_err(|e| e.to_string())?;
                if output.status.success() {
                    Ok(output.stdout)
                } else {
                    Err(String::from_utf8_lossy(&output.stderr).into_owned())
                }
            },
        );
        let actual = if compact {
            encode_message(&value).unwrap()
        } else {
            encode_status(&value).unwrap().into_bytes()
        };
        assert_eq!(actual, oracle);
    }
}
