//! Oracle de cable contra ttyd 1.6.3 en un puerto efímero y tmux privado.
#[path = "support/private_tmux.rs"]
mod private_tmux;
use comandos_term::proto;
use futures_util::{SinkExt, StreamExt};
use std::{process::Stdio, time::Duration};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};
#[tokio::test]
async fn real_ttyd_title_and_preferences_match_codec() {
    if !std::path::Path::new("/usr/bin/ttyd").exists() {
        eprintln!("sin ttyd, oracle omitido");
        return;
    }
    let Some(tmux) = private_tmux::PrivateTmux::start(&["t1"]) else {
        return;
    };
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut cmd = tokio::process::Command::new("/usr/bin/ttyd");
    cmd.env_clear()
        .env("HOME", tmux.home.root.join("home"))
        .env("PATH", "/usr/bin:/bin")
        .env("SHELL", "/bin/sh")
        .args(["-i", "127.0.0.1", "-p", &port.to_string(), "--url-arg"]);
    for setting in [
        r##"theme={"background":"#0A0D13","foreground":"#EAF0FB","cursor":"#FFAE1A","selectionBackground":"#2E3852"}"##,
        "fontSize=11",
        "fontFamily=Ubuntu Sans Mono, JetBrainsMono Nerd Font Mono, JetBrainsMono Nerd Font, JetBrains Mono, DejaVu Sans Mono, monospace",
        "rendererType=canvas",
        "scrollback=10000",
        "cursorBlink=true",
        "disableLeaveAlert=true",
        "disableResizeOverlay=true",
    ] {
        cmd.args(["-t", setting]);
    }
    // -f is before the first command even though the private server is already up.
    let socket = comandos_server::dash::native::tmux::private_socket(&tmux.home.root);
    let args = [
        "tmux".to_owned(),
        "-f".into(),
        "/dev/null".into(),
        "-S".into(),
        socket.to_string_lossy().into_owned(),
        "attach".into(),
        "-t".into(),
        "=t1".into(),
    ];
    cmd.args(&args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = cmd.spawn().unwrap();
    let mut client = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let mut request = format!("ws://127.0.0.1:{port}/ws")
                .into_client_request()
                .unwrap();
            request
                .headers_mut()
                .insert("Sec-WebSocket-Protocol", "tty".parse().unwrap());
            if let Ok((socket, _)) = tokio_tungstenite::connect_async(request).await {
                break socket;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    client
        .send(Message::text(r#"{"AuthToken":"","columns":80,"rows":24}"#))
        .await
        .unwrap();
    let title = tokio::time::timeout(Duration::from_secs(2), client.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .into_data();
    let prefs = tokio::time::timeout(Duration::from_secs(2), client.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .into_data();
    let _ = child.start_kill();
    let _ = child.wait().await;
    let host = std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap();
    assert_eq!(prefs.as_ref(), proto::prefs(proto::TTY_PREFS));
    assert_eq!(
        title.as_ref(),
        proto::title(&proto::tty_title(&args.join(" "), host.trim()))
    );
}
