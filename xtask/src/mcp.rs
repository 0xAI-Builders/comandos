//! Cliente MCP mínimo (JSON-RPC 2.0, una línea por mensaje) sobre
//! `cc-browser-remote`, que reenvía por SSH al broker de Chrome del Mac
//! (`chrome-bg`). Nunca lanza un navegador local.
//!
//! El broker actual exige `pageId` en casi todas las herramientas, crea las
//! páginas con `new_page` y emula con `viewport: "WxHxDPR[,mobile][,touch]"`
//! (preflight S14). El esquema se lee de `tools/list` y se comprueba antes de
//! usarlo: si no coincide, se falla con un mensaje claro en vez de adivinar.
use base64::Engine as _;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

/// Plazo por petición; las capturas (`fullPage` a DPR 2) tienen más margen.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
pub const SCREENSHOT_TIMEOUT: Duration = Duration::from_secs(120);

/// Opciones de ssh que detectan un enlace muerto sin RST en ~45 s.
pub const SSH_KEEPALIVE: [&str; 4] = [
    "-o",
    "ServerAliveInterval=15",
    "-o",
    "ServerAliveCountMax=3",
];

/// Herramienta anunciada por el broker.
#[derive(Debug, Clone)]
pub struct ToolInfo {
    pub name: String,
    pub schema: Value,
}

/// Lo que el arnés sabe hacer con el esquema validado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrowserSchema {
    /// `emulate` acepta `viewport` (DPR, móvil, táctil); si no, solo `resize_page`.
    pub viewport_emulation: bool,
}

/// Cómo se fijó el tamaño de la página.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmulationMode {
    /// `emulate` con `viewport`: ancho, alto, DPR y táctil reales.
    Viewport,
    /// Solo `resize_page`: DPR y táctil OMITIDOS (se anota en el informe).
    ResizeOnly,
}

pub struct Client {
    child: Option<Child>,
    writer: Box<dyn Write + Send>,
    /// Líneas leídas por el hilo lector; `Err` si la lectura falló.
    lines: Receiver<Result<String, String>>,
    next: u64,
    tools: Vec<ToolInfo>,
    schema: Option<BrowserSchema>,
    timeout: Duration,
    screenshot_timeout: Duration,
    /// Tras un plazo vencido o un flujo roto el cliente no se reutiliza: una
    /// respuesta tardía podría confundirse con la de la petición siguiente.
    poisoned: Option<String>,
}

/// Petición JSON-RPC en una sola línea terminada en `\n`.
pub fn request_line(id: u64, method: &str, params: Value) -> String {
    let mut s = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string();
    s.push('\n');
    s
}

/// Bytes de la primera imagen del resultado de `take_screenshot`.
pub fn image_from_result(result: &Value) -> Result<Vec<u8>, String> {
    let items = result
        .get("content")
        .and_then(Value::as_array)
        .ok_or("respuesta sin content")?;
    let data = items
        .iter()
        .find(|c| c.get("type").and_then(Value::as_str) == Some("image"))
        .and_then(|c| c.get("data"))
        .and_then(Value::as_str)
        .ok_or_else(|| format!("respuesta sin imagen: {}", result_text(result)))?;
    base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|e| format!("base64: {e}"))
}

/// Texto concatenado de los bloques `text` de un resultado.
pub fn result_text(result: &Value) -> String {
    result
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|c| c.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Valor devuelto por `evaluate_script`, que llega como bloque ```json … ```.
pub fn eval_value(result: &Value) -> Result<Value, String> {
    let text = result_text(result);
    let body = text
        .split("```json")
        .nth(1)
        .and_then(|s| s.split("```").next())
        .unwrap_or(&text);
    serde_json::from_str(body.trim()).map_err(|e| format!("eval: {e}: {text}"))
}

/// Id de la página marcada `[selected]` en el listado `## Pages` que devuelven
/// `new_page`, `list_pages` y `close_page`.
pub fn selected_page_id(text: &str) -> Option<u64> {
    text.lines()
        .filter(|l| l.trim_end().ends_with("[selected]"))
        .find_map(|l| l.split_once(':').and_then(|(n, _)| n.trim().parse().ok()))
}

fn find<'a>(tools: &'a [ToolInfo], name: &str) -> Option<&'a ToolInfo> {
    tools.iter().find(|t| t.name == name)
}

fn has_props(tool: &ToolInfo, props: &[&str]) -> Result<(), String> {
    let declared = tool.schema.get("properties").and_then(Value::as_object);
    for p in props {
        if !declared.is_some_and(|d| d.contains_key(*p)) {
            return Err(format!(
                "chrome-bg: esquema inesperado: {} sin «{p}» (¿cambió la versión del MCP del broker?)",
                tool.name
            ));
        }
    }
    Ok(())
}

/// Comprueba que `tools/list` tenga la forma que usa el arnés (S14).
pub fn check_schema(tools: &[ToolInfo]) -> Result<BrowserSchema, String> {
    let needed: [(&str, &[&str]); 5] = [
        ("new_page", &["url"]),
        ("navigate_page", &["pageId", "type", "url"]),
        ("take_screenshot", &["pageId", "format", "fullPage"]),
        ("evaluate_script", &["pageId", "function"]),
        ("close_page", &["pageId"]),
    ];
    for (name, props) in needed {
        let tool = find(tools, name).ok_or_else(|| {
            format!("chrome-bg: el broker no ofrece «{name}» (¿cambió la versión del MCP?)")
        })?;
        has_props(tool, props)?;
    }
    let viewport_emulation = find(tools, "emulate").is_some_and(|t| {
        has_props(t, &["pageId", "viewport"]).is_ok()
            && t.schema
                .pointer("/properties/viewport/type")
                .and_then(Value::as_str)
                == Some("string")
    });
    if !viewport_emulation {
        let resize = find(tools, "resize_page").ok_or(
            "chrome-bg: sin «emulate» con viewport ni «resize_page»: no se puede fijar el ancho",
        )?;
        has_props(resize, &["pageId", "width", "height"])?;
    }
    Ok(BrowserSchema { viewport_emulation })
}

/// Ruta del puente al broker (`chrome-bg`).
pub fn bridge_script() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    format!("{home}/.local/bin/cc-browser-remote")
}

/// Si el script es solo `exec ssh <args>` con palabras simples (sin comillas,
/// variables ni redirecciones), devuelve ese argv para lanzar ssh directamente
/// y poder añadirle las opciones de keepalive. Si no, `None`.
pub fn broker_argv_from_script(script: &str) -> Option<Vec<String>> {
    let mut exec = None;
    for line in script.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if exec.is_some() {
            return None;
        }
        exec = Some(line.strip_prefix("exec ")?);
    }
    let words: Vec<String> = exec?.split_whitespace().map(str::to_string).collect();
    let simple = |w: &String| {
        w.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.:=/@,+".contains(c))
    };
    (words.first().map(String::as_str) == Some("ssh") && words.iter().all(simple)).then_some(words)
}

/// Inserta `SSH_KEEPALIVE` tras `ssh`; cualquier otro programa queda igual.
pub fn with_keepalive(argv: Vec<String>) -> Vec<String> {
    let is_ssh = argv
        .first()
        .is_some_and(|p| p.rsplit('/').next() == Some("ssh"));
    if !is_ssh {
        return argv;
    }
    let mut out = Vec::with_capacity(argv.len() + SSH_KEEPALIVE.len());
    let mut it = argv.into_iter();
    out.extend(it.next());
    out.extend(SSH_KEEPALIVE.iter().map(|s| (*s).to_string()));
    out.extend(it);
    out
}

/// Argv por omisión: el `exec ssh …` de `cc-browser-remote` con keepalive;
/// si el script no tiene esa forma, el script tal cual (sin keepalive, pero
/// con el plazo de lectura del cliente).
pub fn default_command() -> Vec<String> {
    let script = bridge_script();
    match std::fs::read_to_string(&script)
        .ok()
        .as_deref()
        .and_then(broker_argv_from_script)
    {
        Some(argv) => with_keepalive(argv),
        None => {
            eprintln!("aviso: {script} no es un «exec ssh» simple; se lanza sin keepalive");
            vec![script]
        }
    }
}

impl Client {
    /// Lanza `command` (por omisión `default_command()`) y
    /// negocia la sesión. Cada `Client` es una sesión del broker (máximo dos).
    pub fn spawn(command: &[&str]) -> Result<Client, String> {
        let (program, args) = command.split_first().ok_or("comando vacío")?;
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("{program}: {e}"))?;
        let stdin = child.stdin.take().ok_or("sin stdin")?;
        let stdout = BufReader::new(child.stdout.take().ok_or("sin stdout")?);
        let mut c = Client::bare(Box::new(stdout), Box::new(stdin));
        c.child = Some(child);
        c.handshake()?;
        Ok(c)
    }

    /// Igual que `spawn` sobre flujos ya abiertos (pruebas con broker falso).
    pub fn connect(
        reader: Box<dyn BufRead + Send>,
        writer: Box<dyn Write + Send>,
    ) -> Result<Client, String> {
        let mut c = Client::bare(reader, writer);
        c.handshake()?;
        Ok(c)
    }

    fn bare(mut reader: Box<dyn BufRead + Send>, writer: Box<dyn Write + Send>) -> Client {
        // Hilo lector: así cada petición espera con plazo (`recv_timeout`).
        // Termina al cerrarse el flujo (EOF) o al soltarse el receptor.
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            loop {
                let mut line = String::new();
                let msg = match reader.read_line(&mut line) {
                    Ok(0) => return,
                    Ok(_) => Ok(line),
                    Err(e) => Err(format!("mcp: {e}")),
                };
                let failed = msg.is_err();
                if tx.send(msg).is_err() || failed {
                    return;
                }
            }
        });
        Client {
            child: None,
            writer,
            lines,
            next: 1,
            tools: Vec::new(),
            schema: None,
            timeout: DEFAULT_TIMEOUT,
            screenshot_timeout: SCREENSHOT_TIMEOUT,
            poisoned: None,
        }
    }

    /// Cambia los plazos por petición (general y de `take_screenshot`).
    pub fn set_timeouts(&mut self, general: Duration, screenshot: Duration) {
        self.timeout = general;
        self.screenshot_timeout = screenshot;
    }

    /// `true` si un plazo vencido o un flujo roto dejó el cliente inutilizable.
    pub fn is_poisoned(&self) -> bool {
        self.poisoned.is_some()
    }

    /// Marca el cliente como inutilizable y corta la sesión del broker.
    fn poison(&mut self, why: String) -> String {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.poisoned = Some(why.clone());
        why
    }

    fn handshake(&mut self) -> Result<(), String> {
        self.rpc(
            "initialize",
            json!({"protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": {"name": "comandos-xtask-shots", "version": "1"}}),
        )?;
        self.notify("notifications/initialized")?;
        let listed = self.rpc("tools/list", json!({}))?;
        self.tools = listed
            .get("tools")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|t| {
                Some(ToolInfo {
                    name: t.get("name")?.as_str()?.to_string(),
                    schema: t.get("inputSchema").cloned().unwrap_or(Value::Null),
                })
            })
            .collect();
        Ok(())
    }

    pub fn tools(&self) -> &[ToolInfo] {
        &self.tools
    }

    fn send_line(&mut self, line: &str) -> Result<(), String> {
        let sent = self
            .writer
            .write_all(line.as_bytes())
            .and_then(|()| self.writer.flush());
        match sent {
            Ok(()) => Ok(()),
            Err(e) => Err(self.poison(format!("mcp: {e}"))),
        }
    }

    fn notify(&mut self, method: &str) -> Result<(), String> {
        let line = json!({"jsonrpc": "2.0", "method": method}).to_string() + "\n";
        self.send_line(&line)
    }

    /// Contesta una petición del servidor: `ping` con `{}`, el resto con
    /// «método no encontrado» (el broker no espera nada más del arnés).
    fn answer_server(&mut self, msg: &Value) -> Result<(), String> {
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        let reply = if msg.get("method").and_then(Value::as_str) == Some("ping") {
            json!({"jsonrpc": "2.0", "id": id, "result": {}})
        } else {
            json!({"jsonrpc": "2.0", "id": id,
                "error": {"code": -32601, "message": "método no soportado por xtask"}})
        };
        self.send_line(&(reply.to_string() + "\n"))
    }

    fn rpc(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.rpc_within(method, params, self.timeout)
    }

    fn rpc_within(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        if let Some(why) = &self.poisoned {
            return Err(format!("mcp {method}: cliente inutilizable ({why})"));
        }
        let id = self.next;
        self.next += 1;
        self.send_line(&request_line(id, method, params))?;
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let line = match self.lines.recv_timeout(left) {
                Ok(Ok(line)) => line,
                Ok(Err(e)) => return Err(self.poison(e)),
                Err(RecvTimeoutError::Timeout) => {
                    return Err(self.poison(format!(
                        "mcp {method}: sin respuesta en {} s",
                        timeout.as_secs_f64()
                    )));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(self.poison(format!("mcp {method}: el broker cerró la conexión")));
                }
            };
            if line.trim().is_empty() {
                continue;
            }
            let msg: Value = match serde_json::from_str(line.trim_end()) {
                Ok(v) => v,
                Err(e) => return Err(self.poison(format!("mcp: {e}"))),
            };
            if msg.get("method").is_some() {
                // Petición del servidor (con id) o notificación (sin id).
                if msg.get("id").is_some() {
                    self.answer_server(&msg)?;
                }
                continue;
            }
            if msg.get("id").and_then(Value::as_u64) != Some(id) {
                continue; // respuestas ajenas
            }
            if let Some(err) = msg.get("error") {
                return Err(format!("mcp {method}: {err}"));
            }
            return msg
                .get("result")
                .cloned()
                .ok_or_else(|| format!("mcp {method}: respuesta sin result"));
        }
    }

    /// Llama a una herramienta anunciada; `isError` se convierte en `Err`.
    pub fn call(&mut self, tool: &str, args: Value) -> Result<Value, String> {
        if !self.tools.iter().any(|t| t.name == tool) {
            return Err(format!("el broker no ofrece la herramienta {tool}"));
        }
        let timeout = if tool == "take_screenshot" {
            self.screenshot_timeout
        } else {
            self.timeout
        };
        let result = self.rpc_within(
            "tools/call",
            json!({"name": tool, "arguments": args}),
            timeout,
        )?;
        if result.get("isError").and_then(Value::as_bool) == Some(true) {
            return Err(format!("{tool}: {}", result_text(&result)));
        }
        Ok(result)
    }

    /// Esquema validado (se calcula una vez).
    pub fn schema(&mut self) -> Result<BrowserSchema, String> {
        if let Some(s) = self.schema {
            return Ok(s);
        }
        let s = check_schema(&self.tools)?;
        self.schema = Some(s);
        Ok(s)
    }

    /// Abre una página propia con `new_page`; se cierra al soltarla.
    pub fn open_page(&mut self, url: &str) -> Result<Page<'_>, String> {
        let schema = self.schema()?;
        let r = self.call("new_page", json!({"url": url}))?;
        let text = result_text(&r);
        let id = selected_page_id(&text).ok_or_else(|| {
            format!("new_page: no se encontró la página seleccionada en {text:?}")
        })?;
        Ok(Page {
            client: self,
            id,
            schema,
            open: true,
        })
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Página del broker identificada por `pageId`.
pub struct Page<'a> {
    client: &'a mut Client,
    id: u64,
    schema: BrowserSchema,
    open: bool,
}

impl Page<'_> {
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Fija el viewport en píxeles CSS. Sin `emulate` con viewport se cae a
    /// `resize_page` y se devuelve `ResizeOnly`: DPR y táctil quedan omitidos
    /// y el llamador debe anotarlo (nunca en silencio).
    pub fn emulate(
        &mut self,
        width: u32,
        height: u32,
        dpr: u32,
        touch: bool,
    ) -> Result<EmulationMode, String> {
        if self.schema.viewport_emulation {
            let mut viewport = format!("{width}x{height}x{dpr}");
            if touch {
                viewport.push_str(",mobile,touch");
            }
            self.client
                .call("emulate", json!({"pageId": self.id, "viewport": viewport}))?;
            Ok(EmulationMode::Viewport)
        } else {
            self.client.call(
                "resize_page",
                json!({"pageId": self.id, "width": width, "height": height}),
            )?;
            Ok(EmulationMode::ResizeOnly)
        }
    }

    pub fn navigate(&mut self, url: &str) -> Result<(), String> {
        self.client.call(
            "navigate_page",
            json!({"pageId": self.id, "type": "url", "url": url}),
        )?;
        Ok(())
    }

    /// `function` es una función JS, p. ej. `() => document.title`.
    pub fn eval(&mut self, function: &str) -> Result<Value, String> {
        let r = self.client.call(
            "evaluate_script",
            json!({"pageId": self.id, "function": function}),
        )?;
        eval_value(&r)
    }

    pub fn screenshot_png(&mut self, full_page: bool) -> Result<Vec<u8>, String> {
        let r = self.client.call(
            "take_screenshot",
            json!({"pageId": self.id, "format": "png", "fullPage": full_page}),
        )?;
        image_from_result(&r)
    }

    /// Cierra la página en el broker.
    pub fn close(mut self) -> Result<(), String> {
        self.open = false;
        self.client
            .call("close_page", json!({"pageId": self.id}))
            .map(|_| ())
    }
}

impl Drop for Page<'_> {
    fn drop(&mut self) {
        // Con el cliente envenenado no se envía nada: el broker no contesta.
        if self.open && !self.client.is_poisoned() {
            let _ = self.client.call("close_page", json!({"pageId": self.id}));
        }
    }
}
