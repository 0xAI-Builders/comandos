use super::*;
use crate::acp_client::{py_str, py_str_or_empty};
fn cp(v: &Value, n: usize) -> String {
    clip(&py_str_or_empty(v), n)
}
use std::collections::{BTreeSet, HashMap, HashSet};
pub fn category(kind: &str) -> &str {
    match kind {
        "noticia" | "ia" => "ia",
        "modelo" | "model" => "modelo",
        "mcp" => "mcp",
        "skill" => "skill",
        "bounty" => "bounty",
        "hackathon" => "hackathon",
        "oficial" => "oficial",
        "hot" => "hot",
        _ => "ia",
    }
}
fn to_ms(v: &Value) -> Value {
    if let Some(n) = v.as_f64() {
        return json!((if n > 1e11 { n } else { n * 1000.0 }) as i64);
    }
    if let Some(text) = v.as_str() {
        let text = text.trim();
        if (9..=13).contains(&text.len())
            && text.bytes().all(|b| b.is_ascii_digit())
            && let Ok(n) = text.parse::<i64>()
        {
            return to_ms(&n.into());
        }
        if let Ok(date) = chrono::DateTime::parse_from_rfc3339(text) {
            return date.timestamp_millis().into();
        }
        if let Ok(date) = chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f") {
            return date.and_utc().timestamp_millis().into();
        }
    }
    json!(crate::news_radar::timestamp(v).map(|n| n * 1000))
}

pub fn prepare(items: &[Value], p: &Policy, now: i64, notes: &mut Vec<String>) -> Vec<Value> {
    let mut sources = vec![];
    let mut seen = HashSet::new();
    let mut expired = 0;
    let mut invalid = 0;
    for raw in items {
        if !raw.is_object() {
            continue;
        }
        let url = normalize_url(s(&raw["url"]));
        let title = cp(&raw["title"], 240);
        let Some(url) = url.filter(|_| !title.is_empty()) else {
            invalid += 1;
            continue;
        };
        if seen.contains(&url) {
            continue;
        }
        let kind = category(&s(&raw["kind"]).to_lowercase()).to_owned();
        if let Some(scope) = p.get("sourceScope").and_then(Value::as_array)
            && !scope.is_empty()
            && !scope.iter().any(|v| v == &kind)
        {
            continue;
        }
        let meta = raw["meta"].as_object().cloned().unwrap_or_default();
        if matches!(kind.as_str(), "bounty" | "hackathon")
            && to_ms(meta.get("deadline").unwrap_or(&Value::Null))
                .as_i64()
                .is_some_and(|d| d < now)
        {
            expired += 1;
            continue;
        }
        seen.insert(url.clone());
        let status = match s(&raw["fetchStatus"]) {
            "ok" => "ok",
            "failed" => "failed",
            _ => "not_fetched",
        };
        let error = cp(&raw["fetchError"], 300);
        let key = s(&raw["announcementKey"]);
        sources.push(json!({"url":url,"originalUrl":raw["url"],"title":title,"origin":clip(raw["source"].as_str().filter(|s|!s.is_empty()).unwrap_or("desconocida"),80),"category":kind,"publishedAt":to_ms(&raw["publishedAt"]),"discoveredAt":to_ms(&raw["discoveredAt"]).as_i64().filter(|n|*n!=0).unwrap_or(now),"fetchStatus":status,"fetchError":if error.is_empty(){Value::Null}else{error.into()},"text":if status=="ok"{cp(&raw["text"],8000)}else{String::new()},"meta":meta,"announcementKey":if key.is_empty(){Value::Null}else{key.into()},"capture":if status=="ok"&&raw["capture"].is_object(){raw["capture"].clone()}else{Value::Null}}));
    }
    if expired > 0 {
        notes.push(format!("{expired} oportunidad(es) vencida(s) omitida(s)."))
    }
    if invalid > 0 {
        notes.push(format!("{invalid} enlace(s) no válido(s) descartado(s)."))
    }
    let limit = n(p, "maxSources", 25).max(0) as usize;
    if sources.len() > limit {
        notes.push(format!(
            "Se leyeron {limit} de {} fuentes por el límite de la edición.",
            sources.len()
        ));
        sources.truncate(limit)
    }
    sources
}
pub fn groups(sources: &[Value]) -> Vec<Value> {
    let mut out: Vec<Value> = vec![];
    for src in sources {
        let key = src["announcementKey"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(s(&src["url"]));
        if let Some(g) = out.iter_mut().find(|g| g["key"] == key) {
            if let Some(sources) = g["sources"].as_array_mut() {
                sources.push(src.clone());
            }
        } else {
            out.push(json!({"key":key,"category":src["category"],"sources":[src]}))
        }
    }
    out
}
pub fn opportunity(story: &Value, group: &Value) -> Value {
    let mut meta = Map::new();
    for source in group["sources"].as_array().into_iter().flatten() {
        if let Some(m) = source["meta"].as_object() {
            for (k, v) in m {
                if !v.is_null() && v != "" {
                    meta.insert(k.clone(), v.clone());
                }
            }
        }
    }
    let mut out = Map::new();
    for (k, other) in [
        ("reward", "prize"),
        ("deadline", "deadline"),
        ("timezone", "timezone"),
        ("eligibility", "eligibility"),
        ("submission", "submission"),
    ] {
        let value = story["opportunity"]
            .get(k)
            .filter(|v| comandos_core::json::truthy(v))
            .or_else(|| meta.get(other));
        let val = match value.filter(|v| !v.is_null() && *v != "") {
            Some(v) => cp(v, 200),
            None => "desconocido".into(),
        };
        out.insert(k.into(), val.into());
    }
    Value::Object(out)
}
#[allow(clippy::too_many_arguments)]
pub fn build_edition(
    conn: &Connection,
    job: &Value,
    fetch: &Fetcher,
    summarize: &Summarizer,
    p: &Policy,
    clock: &dyn Fn() -> i64,
    lead: Option<&LeadWriter>,
) -> Result<Value> {
    let id = s(&job["editionId"]);
    match build(conn, id, fetch, summarize, p, clock, lead) {
        Ok(e) => Ok(e),
        Err(error) => {
            let now = clock();
            let tx = sql(rusqlite::Transaction::new_unchecked(
                conn,
                TransactionBehavior::Immediate,
            ))?;
            comandos_store::migrate::move_db::admit_write(conn).map_err(|e| e.to_string())?;
            sql(tx.execute(
                "UPDATE news_jobs SET state='failed',finished_at_ms=?,error=? WHERE edition_id=?",
                params![now, clip(&error, 300), id],
            ))?;
            sql(tx.execute(
                "UPDATE news_editions SET status='failed',notes=? WHERE id=?",
                params![
                    json!([format!("Error interno al generar: {}", clip(&error, 200))]).to_string(),
                    id
                ],
            ))?;
            sql(tx.commit())?;
            edition(conn, id)
        }
    }
}
#[allow(clippy::too_many_arguments)]
fn build(
    conn: &Connection,
    id: &str,
    fetch: &Fetcher,
    summarize: &Summarizer,
    p: &Policy,
    clock: &dyn Fn() -> i64,
    write_lead: Option<&LeadWriter>,
) -> Result<Value> {
    let start = clock();
    let deadline = start + n(p, "maxSeconds", 600) * 1000;
    let budget = p.get("budgetUsd").and_then(Value::as_f64).unwrap_or(0.25);
    let reserve = p
        .get("reserveUsdPerCall")
        .and_then(Value::as_f64)
        .unwrap_or(0.01);
    let fetched = fetch(p, n(p, "maxSources", 25).max(0) as usize).unwrap_or_else(
        |e| json!({"items":[],"failures":[{"source":"recolector","error":clip(&e,200)}]}),
    );
    let mut notes = vec![];
    let failures: Vec<_> = fetched["failures"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|f| f.is_object())
        .collect();
    if !failures.is_empty() {
        let names: BTreeSet<_> = failures
            .iter()
            .map(|f| {
                clip(
                    f["source"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .unwrap_or("?"),
                    40,
                )
            })
            .collect();
        notes.push(format!(
            "{} fuente(s) no respondieron: {}.",
            failures.len(),
            names.into_iter().collect::<Vec<_>>().join(", ")
        ))
    }
    let sources = prepare(
        fetched["items"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[]),
        p,
        start,
        &mut notes,
    );
    let failed: Vec<_> = sources
        .iter()
        .filter(|s| s["fetchStatus"] != "ok")
        .collect();
    for src in failed.iter().take(10) {
        notes.push(format!(
            "No se pudo leer «{}»: {}.",
            cp(&src["title"], 80),
            src["fetchError"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or("sin lectura")
        ))
    }
    let mut incomplete = !failures.is_empty() || !failed.is_empty();
    let mut stories = vec![];
    let mut cost = 0.0;
    let mut model = Value::Null;
    for group in groups(&sources) {
        let readable: Vec<_> = group["sources"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|s| s["fetchStatus"] == "ok")
            .collect();
        if readable.is_empty() {
            continue;
        }
        if clock() >= deadline {
            notes.push("Se alcanzó el límite de tiempo; la edición quedó parcial.".into());
            incomplete = true;
            break;
        }
        let remaining = budget - cost;
        if remaining < reserve {
            notes.push("Se agotó el presupuesto de la edición; quedó parcial.".into());
            incomplete = true;
            break;
        }
        let mut req_sources = vec![];
        for (i, src) in readable.iter().enumerate() {
            let mut source = Map::new();
            for k in [
                "url",
                "title",
                "origin",
                "text",
                "publishedAt",
                "discoveredAt",
                "meta",
            ] {
                source.insert(k.into(), src[k].clone());
            }
            source.insert("id".into(), format!("s{}", i + 1).into());
            req_sources.push(Value::Object(source))
        }
        let request = json!({"editionId":id,"language":"es","maxCostUsd":news::py_round(remaining,6).map_err(|e|e.to_string())?,"group":{"key":group["key"],"category":group["category"],"sources":req_sources}});
        let result = match summarize(&request) {
            Ok(r) => r,
            Err(SummaryError::Budget(e)) => {
                notes.push(format!("Presupuesto: {e}."));
                incomplete = true;
                break;
            }
            Err(SummaryError::Failure(e)) => {
                notes.push(format!(
                    "No se pudo resumir «{}»: {}.",
                    cp(&group["sources"][0]["title"], 80),
                    clip(&e, 120)
                ));
                incomplete = true;
                continue;
            }
        };
        let raw_cost = &result["costUsd"];
        let spent = if !comandos_core::json::truthy(raw_cost) {
            0.0
        } else if let Some(n) = raw_cost.as_f64() {
            n
        } else if let Some(b) = raw_cost.as_bool() {
            f64::from(b)
        } else {
            py_str(raw_cost)
                .trim()
                .parse::<f64>()
                .map_err(|_| "costo inválido")?
        };
        if spent < 0.0 || !spent.is_finite() {
            return Err("costo negativo".into());
        }
        cost += spent;
        if spent > request["maxCostUsd"].as_f64().unwrap_or(remaining) + 1e-9 {
            notes.push(format!("El proveedor superó el presupuesto: el resumen costó {spent:.4} USD sobre el tope {:.4}.",request["maxCostUsd"].as_f64().unwrap_or(remaining)));
            incomplete = true;
            break;
        }
        if comandos_core::json::truthy(&result["model"]) {
            model = result["model"].clone()
        }
        for note in result["notes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if !notes.iter().any(|n| n == note) {
                notes.push(clip(note, 240))
            }
        }
        let story = &result["story"];
        let cited: Vec<_> = story["sourceIds"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|id| {
                s(id)
                    .strip_prefix('s')
                    .and_then(|n| n.parse::<usize>().ok())
                    .and_then(|n| n.checked_sub(1))
                    .and_then(|n| {
                        if s(id) == format!("s{}", n + 1) {
                            readable.get(n)
                        } else {
                            None
                        }
                    })
            })
            .map(|v| (*v).clone())
            .collect();
        if cited.is_empty() || !comandos_core::json::truthy(&story["title"]) {
            notes.push(format!(
                "Resumen descartado por no citar la fuente leída: «{}» (sin fuente).",
                cp(&group["sources"][0]["title"], 80)
            ));
            incomplete = true;
            continue;
        }
        let meta = &group["sources"][0]["meta"];
        let radar_kind = s(&meta["groupKind"]);
        let requested = [
            radar_kind,
            s(&story["kind"]),
            s(&story["category"]),
            s(&group["category"]),
        ]
        .into_iter()
        .find(|s| !s.is_empty())
        .unwrap_or("ia")
        .to_lowercase();
        let cat = if matches!(
            requested.as_str(),
            "noticia"
                | "ia"
                | "modelo"
                | "model"
                | "mcp"
                | "skill"
                | "bounty"
                | "hackathon"
                | "oficial"
                | "hot"
        ) {
            category(&requested)
        } else {
            s(&group["category"])
        }
        .to_owned();
        let lab = [s(&meta["groupLab"]), s(&story["lab"])]
            .into_iter()
            .find(|s| !s.is_empty())
            .unwrap_or("");
        stories.push(json!({"key":group["key"],"category":cat,"meta":{"lab":if lab.is_empty(){Value::Null}else{clip(lab,60).into()},"rank":meta["groupRank"],"score":meta["groupScore"]},"title":cp(&story["title"],200),"summary":cp(&story["summary"],1200),"body":cp(&story["body"],12000),"sources":cited,"model":if s(&result["model"]).is_empty(){Value::Null}else{cp(&result["model"],120).into()},"opportunity":if matches!(cat.as_str(),"bounty"|"hackathon"){opportunity(story,&group)}else{Value::Null}}));
    }
    let mut lead = Value::Null;
    if !stories.is_empty()
        && clock() < deadline
        && let Some(writer) = write_lead
    {
        let payload: Vec<_> = stories
            .iter()
            .map(|st| json!({"title":st["title"],"summary":st["summary"],"lab":st["meta"]["lab"]}))
            .collect();
        match writer(&payload) {
            Ok(s) => {
                let s = clip(&s, 400);
                if !s.is_empty() {
                    lead = s.into()
                }
            }
            Err(e) => notes.push(format!(
                "No se pudo escribir la entrada de la edición: {}.",
                clip(&e, 120)
            )),
        }
    }
    let status = if sources.is_empty() {
        if failures.is_empty() {
            notes.push("Las fuentes respondieron sin novedades.".into());
            "empty"
        } else {
            "failed"
        }
    } else if stories.is_empty() {
        "failed"
    } else if incomplete {
        "partial"
    } else {
        "published"
    };
    store(
        conn,
        id,
        status,
        &stories,
        &sources,
        failed.len(),
        cost,
        &model,
        &notes,
        clock(),
        &lead,
    )?;
    edition(conn, id)
}
#[allow(clippy::too_many_arguments)]
fn store(
    conn: &Connection,
    id: &str,
    status: &str,
    stories: &[Value],
    sources: &[Value],
    failed: usize,
    cost: f64,
    model: &Value,
    notes: &[String],
    now: i64,
    lead: &Value,
) -> Result<()> {
    let tx = sql(rusqlite::Transaction::new_unchecked(
        conn,
        TransactionBehavior::Immediate,
    ))?;
    comandos_store::migrate::move_db::admit_write(conn).map_err(|e| e.to_string())?;
    let mut ids = HashMap::new();
    for src in sources {
        sql(tx.execute("INSERT INTO news_sources (url,original_url,title,origin,category,published_at_ms,discovered_at_ms,verified_at_ms,fetch_status,fetch_error,meta) VALUES (?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(url) DO UPDATE SET title=excluded.title,fetch_status=excluded.fetch_status,fetch_error=excluded.fetch_error,meta=excluded.meta,published_at_ms=COALESCE(excluded.published_at_ms,news_sources.published_at_ms),verified_at_ms=CASE WHEN excluded.fetch_status='ok' THEN excluded.verified_at_ms ELSE news_sources.verified_at_ms END",params![s(&src["url"]),s(&src["originalUrl"]),s(&src["title"]),s(&src["origin"]),s(&src["category"]),src["publishedAt"].as_i64(),src["discoveredAt"].as_i64(),(src["fetchStatus"]=="ok").then_some(now),s(&src["fetchStatus"]),src["fetchError"].as_str(),src["meta"].to_string()]))?;
        let sid: i64 = sql(tx.query_row(
            "SELECT id FROM news_sources WHERE url=?",
            [s(&src["url"])],
            |r| r.get(0),
        ))?;
        ids.insert(s(&src["url"]), sid);
        let cap = &src["capture"];
        if cap.is_object() {
            sql(tx.execute("INSERT INTO news_captures (source_id,captured_at_ms,final_url,title,byline,lang,blocks,partial) VALUES (?,?,?,?,?,?,?,?) ON CONFLICT(source_id) DO UPDATE SET captured_at_ms=excluded.captured_at_ms,final_url=excluded.final_url,title=excluded.title,byline=excluded.byline,lang=excluded.lang,blocks=excluded.blocks,partial=excluded.partial",params![sid,now,clip(cap["finalUrl"].as_str().filter(|s|!s.is_empty()).unwrap_or(s(&src["url"])),2000),cap["title"].as_str().map(|s|clip(s,300)).filter(|s|!s.is_empty()),cap["byline"].as_str().map(|s|clip(s,160)).filter(|s|!s.is_empty()),cap["lang"].as_str().map(|s|clip(s,12)).filter(|s|!s.is_empty()),cap["blocks"].as_array().cloned().unwrap_or_default().pipe_json(),cap["partial"]==true]))?;
        }
    }
    sql(tx.execute("DELETE FROM news_stories WHERE edition_id=?", [id]))?;
    for (position, story) in stories.iter().enumerate() {
        sql(tx.execute("INSERT INTO news_stories (edition_id,position,story_key,category,title,summary_md,body_md,opportunity,model,meta) VALUES (?,?,?,?,?,?,?,?,?,?)",params![id,position as i64,s(&story["key"]),s(&story["category"]),s(&story["title"]),s(&story["summary"]),s(&story["body"]),(!story["opportunity"].is_null()).then(||story["opportunity"].to_string()),story["model"].as_str(),story["meta"].to_string()]))?;
        let story_id = tx.last_insert_rowid();
        for src in story["sources"].as_array().into_iter().flatten() {
            sql(tx.execute(
                "INSERT OR IGNORE INTO news_story_sources VALUES (?,?)",
                params![story_id, ids[s(&src["url"])]],
            ))?;
        }
    }
    let published = matches!(status, "published" | "partial");
    sql(tx.execute("UPDATE news_editions SET status=?,published_at_ms=?,title=?,story_count=?,source_count=?,failed_source_count=?,cost_usd=?,model=?,notes=?,lead=? WHERE id=?",params![status,published.then_some(now),"Tu edición de IA",stories.len()as i64,sources.len()as i64,failed as i64,news::py_round(cost,6).map_err(|e|e.to_string())?,model.as_str(),json!(notes.iter().take(40).collect::<Vec<_>>()).to_string(),lead.as_str(),id]))?;
    sql(tx.execute(
        "UPDATE news_jobs SET state=?,finished_at_ms=?,error=NULL WHERE edition_id=?",
        params![
            if published || status == "empty" {
                "done"
            } else {
                "failed"
            },
            now,
            id
        ],
    ))?;
    sql(tx.commit())
}
trait JsonVec {
    fn pipe_json(self) -> String;
}
impl JsonVec for Vec<Value> {
    fn pipe_json(self) -> String {
        json!(self).to_string()
    }
}
