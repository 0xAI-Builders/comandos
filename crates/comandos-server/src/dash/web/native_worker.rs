//! Explicit native worker entry point; the legacy route retains its own source.
use super::{WebState, native_page};
use crate::{HandlerError, Reply};
use http::{HeaderValue, StatusCode};
use std::fs;

fn render(web: &WebState) -> Result<Vec<u8>, String> {
    let manifest = web.manifest();
    manifest.check_files(&web.web_dir)?;
    native_page::bundle(&manifest, &web.web_dir)?;
    let metadata = manifest.path("comandos_web_sw_component.json");
    if metadata.is_empty()
        || fs::read_to_string(web.web_dir.join(metadata)).map_err(|e| e.to_string())?
            != include_str!("../../../../comandos-web-sw/components/sw.json")
    {
        return Err("native worker component provenance mismatch".into());
    }
    let js = manifest.path("comandos_web_sw.js");
    let wasm = manifest.path("comandos_web_sw_bg.wasm");
    let boot = manifest.path("comandos_web_sw_boot.js");
    if [js.as_str(), wasm.as_str(), boot.as_str()]
        .iter()
        .any(|p| p.is_empty())
    {
        return Err("native worker artifact missing".into());
    }
    let group = js.split('/').next();
    if wasm.split('/').next() != group || boot.split('/').next() != group {
        return Err("native worker artifact versions differ".into());
    }
    let path = web.web_dir.join(boot);
    if fs::metadata(&path).map_err(|e| e.to_string())?.len() > 1024 * 1024 {
        return Err("native worker loader exceeds maximum size".into());
    }
    let loader = fs::read_to_string(path).map_err(|e| e.to_string())?;
    if loader.matches("{{SW_MODULE}}").count() != 1
        || loader.matches("{{PRECACHE}}").count() != 1
        || !loader.contains("wasm_bindgen.initSync")
    {
        return Err("native worker loader format mismatch".into());
    }
    let mut precache = vec!["/?web=native".to_string()];
    for path in manifest.paths() {
        if [
            ".js",
            ".wasm",
            ".css",
            ".png",
            ".svg",
            ".ico",
            ".woff",
            ".woff2",
            ".ttf",
            ".webmanifest",
        ]
        .iter()
        .any(|ext| path.ends_with(ext))
        {
            precache.push(format!("/web/{path}"));
        }
    }
    precache.sort();
    precache.dedup();
    let js = serde_json::to_string(&format!("/web/{js}")).map_err(|e| e.to_string())?;
    let precache = serde_json::to_string(&precache).map_err(|e| e.to_string())?;
    Ok(loader
        .replace("{{SW_MODULE}}", &js)
        .replace("{{PRECACHE}}", &precache)
        .into_bytes())
}
pub fn serve(web: &WebState) -> Result<Reply, HandlerError> {
    match render(web) {
        Ok(body) => {
            let mut reply = Reply::bytes(StatusCode::OK, "text/javascript", body);
            reply
                .headers
                .insert("service-worker-allowed", HeaderValue::from_static("/"));
            Ok(reply)
        }
        Err(error) => Reply::json(
            StatusCode::SERVICE_UNAVAILABLE,
            &serde_json::json!({"error":error,"mode":"native-worker"}),
        ),
    }
}
