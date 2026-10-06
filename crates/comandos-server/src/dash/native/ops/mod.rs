//! Corte ops: configuración del pane exacto y resultados durables.
pub mod configure;
pub mod extensions;
pub mod profiles;
pub mod results;

use super::{Answer, Cut, Entry, Fault, Key, Native, NativeRoute, Verb, light, py, reply, target};
use crate::Request;
use comandos_core::json::truthy;
use serde_json::{Map, Value};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpsRoute {
    SessionConfigure,
    AccountSwitch,
    ModelSwitch,
    ProfilesGet,
    ProfilesPost,
    ProfileApply,
    ExtensionsGet,
    ExtensionsWrite(extensions::Action),
}
impl OpsRoute {
    pub const fn path(self) -> &'static str {
        match self {
            Self::SessionConfigure => "/session/configure",
            Self::AccountSwitch => "/account/switch",
            Self::ModelSwitch => "/model/switch",
            Self::ProfilesGet | Self::ProfilesPost => "/session-profiles",
            Self::ProfileApply => "/session-profile-apply",
            Self::ExtensionsGet => "/pane-extensions",
            Self::ExtensionsWrite(action) => action.path(),
        }
    }
}
const fn entry(route: OpsRoute) -> Entry {
    Entry {
        verb: Verb::Post,
        key: Key::Raw(route.path()),
        route: NativeRoute::Ops(route),
    }
}
pub const ROUTES: &[Entry] = &[
    Entry {
        verb: Verb::Get,
        key: Key::Prefix("/pane-extensions"),
        route: NativeRoute::Ops(OpsRoute::ExtensionsGet),
    },
    entry(OpsRoute::ExtensionsWrite(extensions::Action::Save)),
    entry(OpsRoute::ExtensionsWrite(extensions::Action::Apply)),
    entry(OpsRoute::ExtensionsWrite(extensions::Action::Template)),
    entry(OpsRoute::ExtensionsWrite(extensions::Action::Cancel)),
    entry(OpsRoute::ExtensionsWrite(extensions::Action::Recover)),
    entry(OpsRoute::SessionConfigure),
    entry(OpsRoute::AccountSwitch),
    entry(OpsRoute::ModelSwitch),
    Entry {
        verb: Verb::Get,
        key: Key::Prefix("/session-profiles"),
        route: NativeRoute::Ops(OpsRoute::ProfilesGet),
    },
    Entry {
        verb: Verb::Post,
        key: Key::Raw("/session-profiles"),
        route: NativeRoute::Ops(OpsRoute::ProfilesPost),
    },
    Entry {
        verb: Verb::Post,
        key: Key::Raw("/session-profile-apply"),
        route: NativeRoute::Ops(OpsRoute::ProfileApply),
    },
];

/// `str(value or fallback)`: valores que no convertimos con certeza declinan
/// antes del claim y de cualquier orden de tmux que mute.
fn text_or(value: Option<&Value>, fallback: &str) -> Result<String, Fault> {
    match value.filter(|v| truthy(v)) {
        None => Ok(fallback.into()),
        Some(v) => py::str_scalar(v).ok_or(Fault::Decline),
    }
}
fn configuration(
    route: OpsRoute,
    mut data: Map<String, Value>,
    sess: &str,
    pane: &str,
) -> Result<Value, Fault> {
    let pane = if route == OpsRoute::ModelSwitch {
        pane.to_owned()
    } else {
        text_or(data.get("pane"), pane)?
    };
    if route == OpsRoute::AccountSwitch {
        let alias = text_or(data.get("alias"), "main")?;
        for key in ["model", "effort", "routeId", "motor", "toHarness"] {
            data.shift_remove(key);
        }
        data.insert("harnessAccount".into(), alias.clone().into());
        data.insert("motorAccount".into(), alias.into());
        data.insert("accountOnly".into(), true.into());
        let interrupt = data.get("interrupt") != Some(&Value::Bool(false));
        data.insert("interrupt".into(), interrupt.into());
    }
    data.insert("session".into(), sess.into());
    data.insert("pane".into(), pane.into());
    Ok(Value::Object(data))
}

pub async fn answer(native: &Arc<Native>, route: OpsRoute, request: &Request) -> Answer {
    if matches!(
        route,
        OpsRoute::ProfilesGet | OpsRoute::ProfilesPost | OpsRoute::ProfileApply
    ) {
        return profiles::answer(native, route, request).await;
    }
    if matches!(
        route,
        OpsRoute::ExtensionsGet | OpsRoute::ExtensionsWrite(_)
    ) {
        return extensions::answer(native, route, request).await;
    }
    let data = light::data(request)?.clone();
    // El task guard mantiene vivo el trabajo si el cliente cierra la conexión.
    let native_job = Arc::clone(native);
    let job = native
        .tasks()
        .spawn_handle(async move {
            let value = Value::Object(data.clone());
            let pt = match target::post_target(&native_job, route.path(), &value).await {
                Ok(pt) => pt,
                Err(answer) => return answer,
            };
            let value = configuration(route, data, &pt.sess, &pt.pane)?;
            let (code, body) = configure::session_configure(&native_job, value).await?;
            reply(code, &body)
        })
        .map_err(|_| Fault::Error(crate::HandlerError::Failure))?;
    job.await
        .map_err(|_| Fault::Error(crate::HandlerError::Failure))?
}

/// Exportado para el arranque del maestro: llamar una vez por Native.
/// Solo el frente dueño de los bucles retoma la cola vieja; nunca replay.
pub fn start(native: &Native) {
    if !native.enabled()
        || !native.options().background.model_watch
        || native.options().cuts_off.contains(&Cut::Ops)
    {
        return;
    }
    let opts = native.options().clone();
    let _ = native.tasks().spawn(async move {
        let _ = tokio::task::spawn_blocking(move || configure::motor_queue_resume(&opts)).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn account_button_strips_model_fields_and_only_false_disables_interrupt() {
        let data = json!({"pane":"%3","alias":"work","model":"x","effort":"x","routeId":"x","motor":"x","toHarness":"x","interrupt":0});
        let v = configuration(
            OpsRoute::AccountSwitch,
            data.as_object().unwrap().clone(),
            "requested",
            "%9",
        )
        .unwrap_or_else(|_| panic!("configuration declined"));
        assert_eq!(
            v,
            json!({"pane":"%3","session":"requested","alias":"work","harnessAccount":"work","motorAccount":"work","accountOnly":true,"interrupt":true})
        );
        let data = json!({"alias":"","interrupt":false});
        let v = configuration(
            OpsRoute::AccountSwitch,
            data.as_object().unwrap().clone(),
            "s",
            "%9",
        )
        .unwrap_or_else(|_| panic!("configuration declined"));
        assert_eq!(v["harnessAccount"], "main");
        assert_eq!(v["interrupt"], false);
    }

    #[test]
    fn model_switch_uses_resolved_pane_while_configure_keeps_explicit_pane() {
        let data = json!({"pane":"%3","model":"m"})
            .as_object()
            .unwrap()
            .clone();
        assert_eq!(
            configuration(OpsRoute::ModelSwitch, data.clone(), "s", "%9")
                .unwrap_or_else(|_| panic!("configuration declined"))["pane"],
            "%9"
        );
        assert_eq!(
            configuration(OpsRoute::SessionConfigure, data, "s", "%9")
                .unwrap_or_else(|_| panic!("configuration declined"))["pane"],
            "%3"
        );
    }
}
