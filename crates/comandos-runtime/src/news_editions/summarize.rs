use super::*;
use crate::news_agents::{self, Opener, SUMMARY_INSTRUCTIONS};
use std::{
    collections::BTreeMap,
    io::Read,
    sync::{Arc, Mutex},
    time::Duration,
};
#[derive(Debug, Clone)]
pub enum SummaryError {
    Budget(String),
    Failure(String),
}
impl From<String> for SummaryError {
    fn from(s: String) -> Self {
        Self::Failure(s)
    }
}
impl std::fmt::Display for SummaryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Budget(s) | Self::Failure(s) => s.fmt(f),
        }
    }
}
pub trait Post: Send + Sync {
    fn post(
        &self,
        url: &str,
        headers: &BTreeMap<String, String>,
        body: &Value,
    ) -> Result<(u16, Value)>;
}
/// A provider response is bounded to 2 MiB and one total 60-second request.
/// Redirects are disabled so credentials never travel to a different origin.
pub struct HttpPost;
impl Post for HttpPost {
    fn post(
        &self,
        url: &str,
        headers: &BTreeMap<String, String>,
        body: &Value,
    ) -> Result<(u16, Value)> {
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| e.to_string())?;
        let mut req = client.post(url).json(body);
        for (k, v) in headers {
            req = req.header(k, v)
        }
        let resp = req.send().map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        let mut bytes = vec![];
        resp.take(2 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 2 * 1024 * 1024 {
            return Err("respuesta del proveedor demasiado grande".into());
        }
        Ok((
            status,
            serde_json::from_slice(&bytes).map_err(|e| e.to_string())?,
        ))
    }
}
fn acp(step: &Value, opener: &Opener, request: &Value) -> std::result::Result<Value, SummaryError> {
    let prompt = format!(
        "{SUMMARY_INSTRUCTIONS}\n\n{}",
        news_agents::summary_prompt(request)?
    );
    let timeout = step["timeout"].as_u64().unwrap_or(600);
    let reply = news_agents::acp_text(
        opener,
        s(&step["agent"]),
        s(&step["model"]),
        Duration::from_secs(timeout),
        &prompt,
    )?;
    Ok(json!({"costUsd":0.0,"model":step["model"],"story":news_agents::extract_json(&reply)?}))
}
pub fn make_summarizer(
    config: &Value,
    env: &dyn Fn(&str) -> Option<String>,
    post: Arc<dyn Post>,
    opener: Opener,
) -> Result<Arc<Summarizer>> {
    let summary = config
        .get("summarizer")
        .filter(|s| s.is_object())
        .ok_or("sin resumidor")?
        .clone();
    match s(&summary["kind"]) {
        "acp" => Ok(Arc::new(move |r| acp(&summary, &opener, r))),
        "chain" => {
            let steps = summary["steps"].as_array().ok_or("sin pasos")?.clone();
            if steps.iter().any(|s| !s.is_object()) {
                return Err("paso ACP no válido".into());
            }
            let start = Mutex::new(0usize);
            Ok(Arc::new(move |request| {
                let mut start = start.lock().unwrap_or_else(|e| e.into_inner());
                let mut notes = vec![];
                for (index, step) in steps.iter().enumerate().skip(*start) {
                    let model = s(&step["model"]);
                    let label = format!(
                        "{}:{}",
                        s(&step["agent"]),
                        if model.is_empty() {
                            "predeterminado"
                        } else {
                            model
                        }
                    );
                    let mut step = step.clone();
                    step["timeout"] = step["timeoutSeconds"].as_u64().unwrap_or(180).into();
                    let result = acp(&step, &opener, request).and_then(|r| {
                        if !comandos_core::json::truthy(&r["story"]["title"])
                            || !comandos_core::json::truthy(&r["story"]["sourceIds"])
                        {
                            Err(SummaryError::Failure("respuesta sin resumen válido".into()))
                        } else {
                            Ok(r)
                        }
                    });
                    match result {
                        Ok(mut r) => {
                            r["model"] = label.into();
                            r["notes"] = json!(notes);
                            return Ok(r);
                        }
                        Err(e) => {
                            notes.push(format!(
                                "{label} falló ({}); se usó el siguiente de la cadena.",
                                clip(&e.to_string(), 100)
                            ));
                            *start = index + 1
                        }
                    }
                }
                Err(SummaryError::Failure(format!(
                    "ningún agente de la cadena pudo resumir: {}",
                    notes.join(" ")
                )))
            }))
        }
        "anthropic-messages" | "openai-chat" => {
            let price_in = summary["inputUsdPerMTok"]
                .as_f64()
                .ok_or("precio de entrada inválido")?;
            let price_out = summary["outputUsdPerMTok"]
                .as_f64()
                .ok_or("precio de salida inválido")?;
            if !price_in.is_finite() || !price_out.is_finite() || price_in < 0.0 || price_out < 0.0
            {
                return Err("precio inválido".into());
            }
            let key = env(s(&summary["apiKeyEnv"])).unwrap_or_default();
            Ok(Arc::new(move |request| {
                let prompt = news_agents::summary_prompt(request)?;
                let est = (SUMMARY_INSTRUCTIONS.chars().count() + prompt.chars().count()) as f64
                    / 3.0
                    + 50.0;
                let room = request["maxCostUsd"].as_f64().unwrap_or(0.0) - est * price_in / 1e6;
                let max_out = if price_out != 0.0 {
                    (room * 1e6 / price_out) as i64
                } else {
                    3000
                };
                if room <= 0.0 || max_out < 200 {
                    return Err(SummaryError::Budget(
                        "sin presupuesto para otro resumen".into(),
                    ));
                }
                let max_out = max_out.min(3000);
                let anthropic = summary["kind"] == "anthropic-messages";
                let mut headers = BTreeMap::new();
                let (url, body) = if anthropic {
                    headers.insert("x-api-key".into(), key.clone());
                    headers.insert("anthropic-version".into(), "2023-06-01".into());
                    headers.insert("content-type".into(), "application/json".into());
                    (
                        "https://api.anthropic.com/v1/messages".into(),
                        json!({"model":summary["model"],"max_tokens":max_out,"system":SUMMARY_INSTRUCTIONS,"messages":[{"role":"user","content":prompt}]}),
                    )
                } else {
                    headers.insert("Authorization".into(), format!("Bearer {key}"));
                    headers.insert("Content-Type".into(), "application/json".into());
                    (
                        format!(
                            "{}/chat/completions",
                            s(&summary["baseUrl"]).trim_end_matches('/')
                        ),
                        json!({"model":summary["model"],"max_tokens":max_out,"messages":[{"role":"system","content":SUMMARY_INSTRUCTIONS},{"role":"user","content":prompt}]}),
                    )
                };
                let (status, data) = post.post(&url, &headers, &body)?;
                if status >= 300 {
                    return Err(SummaryError::Failure(format!(
                        "proveedor respondió {status}"
                    )));
                }
                let text = if anthropic {
                    data["content"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|b| b["type"] == "text")
                        .map(|b| s(&b["text"]))
                        .collect::<String>()
                } else {
                    s(&data["choices"][0]["message"]["content"]).into()
                };
                let usage = &data["usage"];
                let input = usage[if anthropic {
                    "input_tokens"
                } else {
                    "prompt_tokens"
                }]
                .as_f64()
                .unwrap_or(est);
                let output = usage[if anthropic {
                    "output_tokens"
                } else {
                    "completion_tokens"
                }]
                .as_f64()
                .unwrap_or(max_out as f64);
                let cost = news::py_round(input * price_in / 1e6 + output * price_out / 1e6, 6)
                    .map_err(|e| e.to_string())?;
                Ok(
                    json!({"costUsd":cost,"model":summary["model"],"story":news_agents::extract_json(&text)?}),
                )
            }))
        }
        _ => Err("proveedor no soportado".into()),
    }
}
