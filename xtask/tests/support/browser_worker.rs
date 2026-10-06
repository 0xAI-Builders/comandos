//! Stateful MCP double; no browser, network, subprocess, or user profile.
use base64::Engine;
use serde_json::{Value, json};
use std::io::{BufRead, Write};

fn main() {
    if std::env::args().next().is_some_and(|p| {
        std::path::Path::new(&p)
            .file_name()
            .is_some_and(|n| n == "ssh")
    }) {
        fake_ssh();
        return;
    }
    let mut pages = Vec::<String>::new();
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(id) = msg.get("id") else { continue };
        let result = if msg["method"] == "initialize" {
            json!({"protocolVersion":"2025-11-25"})
        } else {
            let args = &msg["params"]["arguments"];
            match msg["params"]["name"].as_str() {
                Some("new_page") => {
                    pages.push(args["url"].as_str().unwrap_or_default().to_owned());
                    json!({"content":[{"type":"text","text":format!("{}: {}", pages.len(), pages.last().unwrap())}]})
                }
                Some("list_pages") => {
                    json!({"content":[{"type":"text","text":if std::env::var_os("FIXTURE_LEAK_PAGES").is_some(){"1: COMANDOS_ISOLATION_ALPHA\n2: COMANDOS_ISOLATION_BETA".to_owned()}else{pages.iter().enumerate().map(|(i,p)|format!("{}: {p}", i+1)).collect::<Vec<_>>().join("\n")} }]})
                }
                Some("take_snapshot") => {
                    json!({"content":[{"type":"text","text":pages.get(args["pageId"].as_u64().unwrap_or(0).saturating_sub(1) as usize).cloned().unwrap_or_default()}]})
                }
                Some("take_screenshot") => {
                    json!({"content":[{"type":"image","mimeType":"image/png","data":fixture_png()}]})
                }
                _ => {
                    json!({"isError":true,"content":[{"type":"text","text":"unknown fixture tool"}]})
                }
            }
        };
        println!("{}", json!({"jsonrpc":"2.0","id":id,"result":result}));
        let _ = std::io::stdout().flush();
    }
}

fn fixture_png() -> String {
    match std::env::var("FIXTURE_BAD_PNG").as_deref() {
        Ok("signature") => "iVBORw0KGgo=".into(),
        Ok(_) => "bm90IGEgcG5n".into(),
        Err(_) => {
            let mut bytes = Vec::new();
            let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[0, 128, 255]).unwrap();
            writer.finish().unwrap();
            base64::engine::general_purpose::STANDARD.encode(bytes)
        }
    }
}

fn fake_ssh() {
    use std::{fs::OpenOptions, net::TcpStream};
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if let Some(path) = std::env::var_os("FIXTURE_SSH_TRACE") {
        writeln!(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .unwrap(),
            "{}",
            json!(args)
        )
        .unwrap();
    }
    assert_eq!(args.first().map(String::as_str), Some("-T"));
    assert!(args.windows(2).any(|a| a == ["-o", "BatchMode=yes"]));
    if let Some(index) = args.iter().position(|a| a == "-W") {
        assert_eq!(
            index + 3,
            args.len(),
            "forwarding options must precede destination"
        );
        assert_eq!(args.last().unwrap(), "fixture-host");
        let mut stream = TcpStream::connect(&args[index + 1]).unwrap();
        let mut writer = stream.try_clone().unwrap();
        std::thread::spawn(move || {
            let _ = std::io::copy(&mut std::io::stdin(), &mut writer);
            let _ = writer.shutdown(std::net::Shutdown::Write);
        });
        let _ = std::io::copy(&mut stream, &mut std::io::stdout());
    } else {
        assert_eq!(args[args.len() - 2], "fixture-host");
        assert!(args.last().unwrap().starts_with("cat -- "));
        let body = std::fs::read(std::env::var_os("FIXTURE_SSH_STATUS").unwrap()).unwrap();
        std::io::stdout().write_all(&body).unwrap();
    }
}
