use crate::{
    Result,
    transport::{MAX_INFLIGHT, MAX_MESSAGE, Transport, line},
};
use serde_json::{Value, json};
use std::{collections::HashMap, path::Path, sync::Arc};
use tokio::{
    io::{AsyncWriteExt, BufReader},
    sync::{mpsc, watch},
    task::{AbortHandle, JoinSet},
};

type CompletedRequest = (String, Value, Option<(u64, Value)>);

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}
fn permitted(spec: &Value, name: &str) -> bool {
    !spec["disabled_tools"]
        .as_array()
        .is_some_and(|list| list.iter().any(|x| x == name))
        && spec
            .get("enabled_tools")
            .filter(|v| !v.is_null())
            .is_none_or(|v| {
                v.as_array()
                    .is_some_and(|list| list.iter().any(|x| x == name))
            })
}

pub async fn serve(home: &Path, name: &str, spec: &Value) -> Result<()> {
    let (input_tx, mut input_rx) = mpsc::channel(8);
    let (end_tx, mut end_rx) = watch::channel(false);
    let input = tokio::spawn(async move {
        let mut reader = BufReader::new(tokio::io::stdin());
        loop {
            match line(&mut reader).await {
                Ok(Some(bytes)) => {
                    if input_tx.send(bytes).await.is_err() {
                        break;
                    }
                }
                _ => {
                    let _ = end_tx.send(true);
                    break;
                }
            }
        }
    });
    let mut signals = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|_| "Signal setup failed")?;
    let mut upstream = tokio::select! {
        result=Transport::connect(home,name,spec)=>result?,
        _=end_rx.changed()=>{input.abort();return Ok(());},
        _=signals.recv()=>{input.abort();return Ok(());},
        _=tokio::signal::ctrl_c()=>{input.abort();return Ok(());},
    };
    let initial = tokio::select! {
        result=upstream.request(json!({"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"comandos","version":"1"}}}))=>result,
        _=end_rx.changed()=>{upstream.shutdown().await;input.abort();return Ok(());},
        _=signals.recv()=>{upstream.shutdown().await;input.abort();return Ok(());},
        _=tokio::signal::ctrl_c()=>{upstream.shutdown().await;input.abort();return Ok(());},
    }?;
    let result = initial
        .get("result")
        .filter(|v| v.is_object())
        .ok_or("Upstream initialization failed")?;
    let version = result["protocolVersion"]
        .as_str()
        .ok_or("Missing upstream protocol")?;
    upstream.set_protocol(version);
    tokio::select! {
        result=upstream.notify(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))=>result?,
        _=end_rx.changed()=>{upstream.shutdown().await;input.abort();return Ok(());},
        _=signals.recv()=>{upstream.shutdown().await;input.abort();return Ok(());},
        _=tokio::signal::ctrl_c()=>{upstream.shutdown().await;input.abort();return Ok(());},
    }
    let mut capabilities = serde_json::Map::new();
    for key in ["tools", "prompts", "resources", "completions"] {
        if let Some(cap) = result["capabilities"].get(key).filter(|v| !v.is_null()) {
            capabilities.insert(
                key.into(),
                if key == "completions" {
                    cap.clone()
                } else {
                    json!({})
                },
            );
        }
    }
    let mut initialize = json!({"protocolVersion":version,"capabilities":capabilities,"serverInfo":{"name":format!("comandos-{name}"),"version":"1"}});
    if let Some(instructions) = result.get("instructions") {
        initialize["instructions"] = instructions.clone();
    }
    let mut closed = upstream.closed.clone();
    let upstream = Arc::new(upstream);
    let spec = Arc::new(spec.clone());
    let (output_tx, mut output_rx) = mpsc::channel::<Value>(8);
    let (output_end, mut output_closed) = watch::channel(false);
    let writer = tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        while let Some(value) = output_rx.recv().await {
            let mut bytes = value.to_string().into_bytes();
            bytes.push(b'\n');
            if stdout.write_all(&bytes).await.is_err() || stdout.flush().await.is_err() {
                break;
            }
        }
        let _ = output_end.send(true);
    });
    let mut jobs: JoinSet<CompletedRequest> = JoinSet::new();
    let mut measurements = JoinSet::new();
    let mut capture = crate::metadata::ToolListCapture::default();
    let mut capture_generation = 0u64;
    let mut capture_waiting = false;
    let metadata_home = home.to_path_buf();
    let metadata_name = name.to_owned();
    let mut running: HashMap<String, AbortHandle> = HashMap::new();
    let outcome = loop {
        tokio::select! {
            biased;
            _=end_rx.changed()=>break Ok(()),
            _=signals.recv()=>break Ok(()),
            _=tokio::signal::ctrl_c()=>break Ok(()),
            _=closed.changed()=>break Err("Upstream closed".into()),
            _=output_closed.changed()=>break Ok(()),
            Some(completed)=jobs.join_next(),if !jobs.is_empty()=> {
                if let Ok((key,response,page))=completed {
                    running.remove(&key);
                    if let Some((generation,cursor))=page
                        && generation==capture_generation {
                            capture_waiting=false;
                            let result=&response["result"];
                            if response.get("error").is_none()
                                && (cursor.is_null()||cursor.is_string())
                                && (result["nextCursor"].is_null()||result["nextCursor"].is_string())
                                && let Some(tools)=result["tools"].as_array()
                            {
                                if let Some(tools)=capture.add(cursor.as_str(),result["nextCursor"].as_str(),tools)
                                    && measurements.is_empty()
                                {
                                    let home=metadata_home.clone();let name=metadata_name.clone();let spec=spec.clone();
                                    measurements.spawn(async move {
                                        crate::metadata::record_mcp_size_with_clock(&home,&name,&spec,&tools,crate::metadata::now,&|texts|crate::tokenizer::isolated_token_counts(&home,texts)).await;
                                    });
                                }
                            } else {capture.invalidate();}
                    }
                    if output_tx.try_send(response).is_err(){break Err("Downstream output limit reached".into());}
                }
            },
            _=measurements.join_next(),if !measurements.is_empty()=>{},
            bytes=input_rx.recv()=> {
                let Some(bytes)=bytes else {break Ok(())};
                let request:Value=match comandos_core::json::parse_slice(&bytes){Ok(v)=>v,Err(_)=>{if output_tx.try_send(error(Value::Null,-32700,"Parse error")).is_err(){break Err("Downstream output limit reached".into());}continue;}};
                let method=request["method"].as_str().unwrap_or("");
                if request.get("id").is_none() {
                    if method=="notifications/cancelled" {
                        let key=request["params"]["requestId"].to_string();
                        if let Some(handle)=running.remove(&key){handle.abort();}
                    }
                    continue;
                }
                let id=request["id"].clone();let key=id.to_string();
                let immediate=if request["jsonrpc"]!="2.0" || !matches!(id,Value::String(_)|Value::Number(_)) || method.is_empty() {Some(error(id.clone(),-32600,"Invalid request"))}
                else if method=="initialize" {Some(json!({"jsonrpc":"2.0","id":id,"result":initialize}))}
                else if method=="ping" {Some(json!({"jsonrpc":"2.0","id":id,"result":{}}))}
                else if !matches!(method,"tools/list"|"tools/call"|"resources/list"|"resources/templates/list"|"resources/read"|"prompts/list"|"prompts/get"|"completion/complete") {Some(error(id.clone(),-32601,"Method not found"))}
                else if method=="tools/call" && !permitted(&spec,request["params"]["name"].as_str().unwrap_or("")) {Some(error(id.clone(),-32601,"Tool disabled in shared catalog"))}
                else if running.contains_key(&key) || running.len()>=MAX_INFLIGHT {Some(error(id.clone(),-32000,"Request limit reached"))} else {None};
                if let Some(response)=immediate {if output_tx.try_send(response).is_err(){break Err("Downstream output limit reached".into());}continue;}
                let page=if method=="tools/list" {
                    let cursor=request["params"]["cursor"].clone();
                    if cursor.is_null() || capture_waiting {capture_generation=capture_generation.wrapping_add(1);capture.invalidate();}
                    capture_waiting=true;
                    Some((capture_generation,cursor))
                } else {None};
                let upstream=upstream.clone();let spec=spec.clone();let task_key=key.clone();let method=method.to_owned();
                let handle=jobs.spawn(async move {
                    let mut response=match upstream.request(request).await {Ok(v)=>v,Err(_)=>error(id.clone(),-32603,"Upstream request failed")};
                    response["id"]=id;
                    if method=="tools/list" && response.get("error").is_none()
                        && let Some(tools)=response.get_mut("result").and_then(|result|result.get_mut("tools")).and_then(Value::as_array_mut)
                    {tools.retain(|t|t["name"].as_str().is_some_and(|n|permitted(&spec,n)));}
                    if response.to_string().len()>MAX_MESSAGE {response=error(response["id"].clone(),-32603,"Upstream response too large");}
                    (task_key,response,page)
                });
                running.insert(key,handle);
            },
        }
    };
    jobs.abort_all();
    while jobs.join_next().await.is_some() {}
    measurements.abort_all();
    while measurements.join_next().await.is_some() {}
    input.abort();
    writer.abort();
    if let Ok(mut transport) = Arc::try_unwrap(upstream) {
        transport.shutdown().await;
    }
    outcome
}
