//! Session profiles: persistence belongs to the usage lane; discovery to runtime.
use super::super::{
    Answer, Fault, Native, NativeOptions, light, query::Query, reply, states::gather, usage,
};
use super::OpsRoute;
use crate::{HandlerError, Request};
use comandos_core::json::truthy;
use comandos_runtime::{
    accounts,
    capabilities::{self, Paths},
    session_configuration::{Fail, resolve_profile_route},
    session_profiles as runtime,
};
use comandos_store::session_profiles as store;
use http::StatusCode;
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};
fn paths(opts: &NativeOptions) -> Paths {
    let mut p = Paths::new(&opts.home, &opts.cwd);
    p.env = (*opts.extension_env).clone();
    p
}
fn store_fault(e: store::Fault, status: StatusCode) -> Answer {
    match e {
        store::Fault::Invalid(s) => light::error(status, &s),
        store::Fault::Uncertain(_) => Err(Fault::Decline),
        store::Fault::Sql(_) | store::Fault::Random(_) => Err(Fault::Error(HandlerError::Failure)),
    }
}
fn runtime_fault(e: capabilities::Fault, status: StatusCode) -> Answer {
    match e {
        capabilities::Fault::Invalid(s) => light::error(status, &s),
        capabilities::Fault::Uncertain(_) | capabilities::Fault::Io(_) => Err(Fault::Decline),
    }
}
fn apply_error(s: &str) -> Answer {
    reply(
        StatusCode::CONFLICT,
        &json!({"error":s,"code":"profile_unavailable"}),
    )
}
pub async fn answer(native: &Arc<Native>, route: OpsRoute, request: &Request) -> Answer {
    if !native.usage.enabled() {
        return Err(Fault::Decline);
    }
    match route {
        OpsRoute::ProfilesGet => {
            let query = Query::parse(&request.target)?;
            let cwd = query
                .first("cwd")
                .map(PathBuf::from)
                .unwrap_or_else(|| native.options().home.clone());
            let harness = query.first("harness").unwrap_or("codex").to_owned();
            let alias = query.first("account").unwrap_or("main").to_owned();
            let opts = native.options().clone();
            let cache = native.registry.clone();
            let result = tokio::task::spawn_blocking(move || {
                let registry = gather::load_registry(&opts, &cache)
                    .map_err(|_| capabilities::Fault::Uncertain("registry unavailable".into()))?;
                runtime::inventory(&registry, &harness, &alias, &cwd, &paths(&opts))
            })
            .await
            .map_err(|_| Fault::Error(HandlerError::Failure))?;
            let inv = match result {
                Ok(v) => v,
                Err(e) => return runtime_fault(e, StatusCode::BAD_REQUEST),
            };
            let profiles = native.usage.with(|u| store::list_profiles(&u.conn)).await?;
            let profiles = match profiles {
                Ok(v) => v,
                Err(e) => return store_fault(e, StatusCode::BAD_REQUEST),
            };
            reply(
                StatusCode::OK,
                &json!({"profiles":profiles,"inventory":inv,"capabilities":inv["capabilities"],"scope":"profile","effectiveNow":null}),
            )
        }
        OpsRoute::ProfilesPost => {
            let data = Value::Object(light::data(request)?.clone());
            let delete = data["action"] == "delete";
            let now = (native.options().clock)() / 1000;
            let result = native.usage.with(move |u| {
                if delete {
                    store::delete_profile(&u.conn,&data["id"]).map(|()|json!({"ok":true}))
                } else {
                    store::save_profile(&u.conn,&data,now).map(|p|json!({"ok":true,"profile":p,"status":"saved","effectiveNow":false,"scope":"profile"}))
                }
            }).await?;
            match result {
                Ok(v) => reply(StatusCode::OK, &v),
                Err(e) => store_fault(e, StatusCode::BAD_REQUEST),
            }
        }
        OpsRoute::ProfileApply => {
            let data = Value::Object(light::data(request)?.clone());
            let id = data["profileId"].clone();
            let profile = match native
                .usage
                .with(move |u| store::get_profile(&u.conn, &id))
                .await?
            {
                Ok(v) => v,
                Err(store::Fault::Invalid(s)) => return apply_error(&s),
                Err(e) => return store_fault(e, StatusCode::CONFLICT),
            };
            let draft = runtime::launch_draft(&profile).map_err(|_| Fault::Decline)?;
            let (registry, matrix) = usage::providers::registry_and_matrix(native).await?;
            let opts = native.options().clone();
            let copy = profile.clone();
            let draft_copy = draft.clone();
            let result = tokio::task::spawn_blocking(move || -> Result<(), capabilities::Fault> {
                resolve_profile_route(
                    &registry,
                    &matrix,
                    &accounts::Paths::new(&opts.home, &opts.cwd),
                    draft_copy
                        .as_object()
                        .ok_or_else(|| capabilities::Fault::Uncertain("draft shape".into()))?,
                    "new_session",
                )
                .map_err(|e| match e {
                    Fail::Py(s) => capabilities::Fault::Invalid(s),
                    Fail::Unsure => capabilities::Fault::Uncertain("route uncertainty".into()),
                })?;
                if truthy(&copy["skills"]) || truthy(&copy["mcps"]) {
                    let cwd = super::super::py::str_scalar(
                        data.get("cwd").filter(|v| truthy(v)).unwrap_or(&json!("")),
                    )
                    .ok_or_else(|| capabilities::Fault::Uncertain("cwd value".into()))?;
                    runtime::launch_args(
                        &copy,
                        &registry,
                        &PathBuf::from(cwd),
                        &opts.hooks.join("profile-launches"),
                        true,
                        &paths(&opts),
                    )?;
                }
                Ok(())
            })
            .await
            .map_err(|_| Fault::Error(HandlerError::Failure))?;
            match result {
                Ok(()) => reply(
                    StatusCode::OK,
                    &json!({"ok":true,"profile":profile,"launchDraft":draft,"status":"next_launch","effectiveNow":false,"scope":"profile","note":"Preparado para una sesión nueva. El agente vivo conserva su configuración."}),
                ),
                Err(capabilities::Fault::Invalid(s)) => apply_error(&s),
                Err(e) => runtime_fault(e, StatusCode::CONFLICT),
            }
        }
        _ => Err(Fault::Error(HandlerError::Failure)),
    }
}
