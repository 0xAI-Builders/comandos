use super::{
    Manifest,
    registry::{Entry, Kind, Resolved},
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub on: BTreeSet<String>,
    pub shadow: BTreeSet<String>,
}

impl Selection {
    pub fn load(path: &Path) -> Selection {
        fs::read_to_string(path)
            .ok()
            .map_or_else(Self::empty, |text| Self::from_text(&text))
    }
    fn empty() -> Self {
        Self {
            on: BTreeSet::new(),
            shadow: BTreeSet::new(),
        }
    }
    fn from_text(text: &str) -> Self {
        serde_json::from_str::<Value>(text).map_or_else(
            |_| Self::empty(),
            |value| Self {
                on: strings(&value, "on"),
                shadow: strings(&value, "shadow"),
            },
        )
    }
    pub fn load_domain(home: &Path, path: &Path) -> Self {
        Self::load_domain_with(home, path, false).unwrap_or_else(|_| Self::empty())
    }
    pub fn load_domain_live(home: &Path, path: &Path) -> Self {
        Self::try_load_domain_live(home, path).unwrap_or_else(|_| Self::empty())
    }
    pub fn try_load_domain_live(home: &Path, path: &Path) -> comandos_store::Result<Self> {
        Self::load_domain_with(home, path, true)
    }
    fn load_domain_with(home: &Path, path: &Path, live: bool) -> comandos_store::Result<Self> {
        let doc = comandos_store::domains::DomainStore { home }.document(
            "hooks/comandos-web.json",
            "ui-docs",
            path.to_owned(),
        );
        Ok((if live {
            doc.read_live()
        } else {
            doc.read_readonly()
        })?
        .and_then(|body| String::from_utf8(body).ok())
        .map_or_else(Self::empty, |text| Self::from_text(&text)))
    }

    pub fn refresh(&mut self, path: &Path) {
        *self = Selection::load(path);
    }
}

fn strings(value: &Value, key: &str) -> BTreeSet<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComponentState {
    Off,
    On,
    Shadow,
    Drift { expected: String, found: String },
    MissingSource,
    MissingDep(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Composed {
    pub html: Vec<u8>,
    pub active: Vec<String>,
    pub states: BTreeMap<String, ComponentState>,
}

pub fn compose(
    page: &[u8],
    reg: &Resolved,
    sel: &Selection,
    shadow: bool,
    assets: &Manifest,
    nonce: &str,
) -> Composed {
    let page_text = String::from_utf8_lossy(page).into_owned();
    let mut states = BTreeMap::new();
    let wanted = |id: &str| sel.on.contains(id) || (shadow && sel.shadow.contains(id));
    for e in reg.entries() {
        let state = if !wanted(&e.id) {
            ComponentState::Off
        } else {
            let found = match &e.kind {
                Kind::Script => reg.source_hash(e),
                Kind::Region { .. } => e.hash_in_page(&page_text),
            };
            match found {
                None => ComponentState::MissingSource,
                Some(found) if found != e.sha256 => ComponentState::Drift {
                    expected: e.sha256.clone(),
                    found,
                },
                Some(_) if sel.on.contains(&e.id) => ComponentState::On,
                Some(_) => ComponentState::Shadow,
            }
        };
        states.insert(e.id.clone(), state);
    }
    if assets.path("comandos_web_boot.js").is_empty() {
        for state in states.values_mut() {
            if matches!(state, ComponentState::On | ComponentState::Shadow) {
                *state = ComponentState::MissingDep("comandos_web_boot.js".into());
            }
        }
    }
    loop {
        let mut changed = false;
        for e in reg.entries() {
            if !matches!(
                states.get(&e.id),
                Some(ComponentState::On | ComponentState::Shadow)
            ) {
                continue;
            }
            if let Some(dep) = e.deps.iter().find(|d| {
                !matches!(
                    states.get(*d),
                    Some(ComponentState::On | ComponentState::Shadow)
                )
            }) {
                states.insert(e.id.clone(), ComponentState::MissingDep(dep.clone()));
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let active: Vec<&Entry> = reg
        .entries()
        .iter()
        .filter(|e| {
            matches!(
                states.get(&e.id),
                Some(ComponentState::On | ComponentState::Shadow)
            )
        })
        .collect();
    if active.is_empty() {
        return Composed {
            html: page.to_vec(),
            active: Vec::new(),
            states,
        };
    }
    let mut text = page_text;
    for e in &active {
        text = e.cut(&text);
    }
    let ids: Vec<&str> = active.iter().map(|e| e.id.as_str()).collect();
    let boot = serde_json::Value::String(format!("/web/{}", assets.path("comandos_web_boot.js")))
        .to_string()
        .replace('<', "\\u003c");
    // WebKit can defer an async module element behind the classic gate script.
    // Dynamic import starts its evaluation while that readiness gate is waiting.
    let head = format!(
        "<meta name=\"comandos-web\" content=\"{}\">\n<script data-k=\"{nonce}\">import({boot});</script>\n<script src=\"/web/gate.js?k={nonce}\"></script>\n",
        ids.join(" ")
    );
    let html = match text.split_once("<meta charset=\"utf-8\">\n") {
        Some((before, after)) => format!("{before}<meta charset=\"utf-8\">\n{head}{after}"),
        None => format!("{head}{text}"),
    };
    Composed {
        html: html.into_bytes(),
        active: ids.iter().map(|s| (*s).to_string()).collect(),
        states,
    }
}
