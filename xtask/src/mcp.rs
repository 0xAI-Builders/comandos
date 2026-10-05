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
    reader: Box<dyn BufRead + Send>,
    next: u64,
    tools: Vec<ToolInfo>,
    schema: Option<BrowserSchema>,
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

/// Ruta por omisión del puente al broker.
pub fn default_command() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    format!("{home}/.local/bin/cc-browser-remote")
}

impl Client {
    /// Lanza `command` (por omisión `[~/.local/bin/cc-browser-remote]`) y
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

    fn bare(reader: Box<dyn BufRead + Send>, writer: Box<dyn Write + Send>) -> Client {
        Client {
            child: None,
            writer,
            reader,
            next: 1,
            tools: Vec::new(),
            schema: None,
        }
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

    fn notify(&mut self, method: &str) -> Result<(), String> {
        let line = json!({"jsonrpc": "2.0", "method": method}).to_string() + "\n";
        self.writer
            .write_all(line.as_bytes())
            .and_then(|()| self.writer.flush())
            .map_err(|e| format!("mcp: {e}"))
    }

    fn rpc(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next;
        self.next += 1;
        self.writer
            .write_all(request_line(id, method, params).as_bytes())
            .and_then(|()| self.writer.flush())
            .map_err(|e| format!("mcp: {e}"))?;
        let mut line = String::new();
        loop {
            line.clear();
            if self
                .reader
                .read_line(&mut line)
                .map_err(|e| format!("mcp: {e}"))?
                == 0
            {
                return Err(format!("mcp {method}: el broker cerró la conexión"));
            }
            if line.trim().is_empty() {
                continue;
            }
            let msg: Value =
                serde_json::from_str(line.trim_end()).map_err(|e| format!("mcp: {e}"))?;
            if msg.get("id").and_then(Value::as_u64) != Some(id) {
                continue; // notificaciones y respuestas ajenas
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
        let result = self.rpc("tools/call", json!({"name": tool, "arguments": args}))?;
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
        if self.open {
            let _ = self.client.call("close_page", json!({"pageId": self.id}));
        }
    }
}
