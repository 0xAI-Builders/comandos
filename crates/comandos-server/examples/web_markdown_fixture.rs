//! Disposable loopback-only transport for original-CJS versus actual-WASM checks.
use comandos_server::{Config, Reply, dash, serve};
use std::{sync::Arc, time::Duration};
use tokio::{net::TcpListener, sync::watch};
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().any(|a| a == "--normalize") {
        use std::io::Read;
        let mut source = String::new();
        std::io::stdin().read_to_string(&mut source)?;
        let pairs: Vec<serde_json::Value> = serde_json::from_str(&source)?;
        let out=pairs.iter().map(|p| {
            let baseline=p["baseline"].as_str().unwrap_or("");let candidate=p["candidate"].as_str().unwrap_or("");
            serde_json::json!({"id":p["id"],"difference":comandos_domdiff::first_difference(baseline,candidate)})
        }).collect::<Vec<_>>();
        println!("{}", serde_json::to_string(&out)?);
        return Ok(());
    }
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    println!("http://{}", listener.local_addr()?);
    let config = Config {
        token: b"b8-disposable-fixture".to_vec(),
        token_file: None,
        asset_exists: Arc::new(|_| false),
        handler: Arc::new(|r| {
            Box::pin(async move {
                if r.method == http::Method::POST && r.target == "/web/markdown" {
                    dash::web::markdown::handle(&r)
                } else {
                    Ok(Reply::bytes(
                        http::StatusCode::NOT_FOUND,
                        "text/plain",
                        Vec::new(),
                    ))
                }
            })
        }),
        websocket: None,
        limits: dash::limits(),
    };
    let (stop, rx) = watch::channel(false);
    let eof = stop.clone();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
        let _ = eof.send(true);
    });
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(300)).await;
        let _ = stop.send(true);
    });
    serve(listener, config, rx).await?;
    Ok(())
}
