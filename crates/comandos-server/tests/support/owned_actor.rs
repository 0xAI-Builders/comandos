//! Real private Rust process for listener ownership and environment assertions.
#![allow(dead_code)]
use std::{
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
pub struct OwnedActor {
    child: Child,
    pub port: u16,
}
impl OwnedActor {
    pub fn start(home: &super::TestHome) -> Self {
        static LAUNCH: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _launch = LAUNCH.lock().unwrap();
        let port = (20_000..30_000)
            .find(|port| std::net::TcpListener::bind(("127.0.0.1", *port)).is_ok())
            .expect("private fixture port");
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "support::owned_actor::fixture", "--nocapture"])
            .env_clear()
            .envs(home.confined_env())
            .env_remove("PYTHONPATH")
            .env("COMANDOS_TEST_OWNED_ACTOR_PORT", port.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut actor = Self { child, port };
        let deadline = Instant::now() + Duration::from_secs(5);
        while !super::oracle::listens_on(actor.pid(), port) {
            assert!(
                actor.child.try_wait().unwrap().is_none(),
                "Rust fixture exited before owning its listener"
            );
            assert!(
                Instant::now() < deadline,
                "Rust fixture did not own its listener"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        actor
    }
    pub fn pid(&self) -> u32 {
        self.child.id()
    }
}
impl Drop for OwnedActor {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
#[test]
fn fixture() {
    let Ok(port) = std::env::var("COMANDOS_TEST_OWNED_ACTOR_PORT") else {
        return;
    };
    let port: u16 = port.parse().unwrap();
    let home = std::env::var("HOME").unwrap();
    assert!(std::path::Path::new(&home).join("fakebin").is_dir());
    assert!((20_000..30_000).contains(&port));
    let listener = std::net::TcpListener::bind(("127.0.0.1", port)).unwrap();
    for connection in listener.incoming() {
        use std::io::Write;
        let mut connection = connection.unwrap();
        let _ = connection
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}");
    }
}
