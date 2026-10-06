//! Intenciones locales de resize; nunca reemplazan el documento autoritativo.
use serde_json::{Value, json};
use std::collections::BTreeMap;

type Key = (String, Vec<usize>);
#[derive(Clone)]
pub struct ResizeUpdate {
    pub group: String,
    pub path: Vec<usize>,
    pub shape: Value,
    pub ratio: f64,
}
#[derive(Clone)]
struct Intent {
    ratio: f64,
    shape: Value,
}
#[derive(Default)]
pub struct ResizeQueue {
    pending: BTreeMap<Key, Intent>,
    in_flight: Option<BTreeMap<Key, Intent>>,
    stopped: bool,
    waiting_authority: bool,
}
pub struct Publication {
    pub document: Value,
    pub rejected: usize,
    pub changed: bool,
}
fn node<'a>(doc: &'a Value, group: &str, path: &[usize]) -> Option<&'a Value> {
    let mut node = doc
        .get("groups")?
        .as_array()?
        .iter()
        .find(|g| g.get("id").and_then(Value::as_str) == Some(group))?
        .get("tree")?;
    for step in path {
        node = node.get(match step {
            0 => "first",
            1 => "second",
            _ => return None,
        })?;
    }
    (node.get("type").and_then(Value::as_str) == Some("split")).then_some(node)
}
impl ResizeQueue {
    /// Rechaza callbacks obsoletos antes de capturar identidad desde la autoridad nueva.
    pub fn record_visual(&mut self, doc: &Value, updates: &[ResizeUpdate]) -> usize {
        let mut rejected = 0;
        if self.stopped {
            return rejected;
        }
        for update in updates {
            if !update.ratio.is_finite()
                || node(doc, &update.group, &update.path)
                    .is_none_or(|node| crate::workspace_view::shape(node) != update.shape)
            {
                rejected += 1;
                continue;
            }
            self.pending.insert(
                (update.group.clone(), update.path.clone()),
                Intent {
                    ratio: update.ratio,
                    shape: update.shape.clone(),
                },
            );
        }
        rejected
    }
    pub fn record(&mut self, doc: &Value, updates: &[(String, Vec<usize>, f64)]) {
        if self.stopped {
            return;
        }
        for (group, path, ratio) in updates {
            if ratio.is_finite()
                && let Some(node) = node(doc, group, path)
            {
                self.pending.insert(
                    (group.clone(), path.clone()),
                    Intent {
                        ratio: *ratio,
                        shape: crate::workspace_view::shape(node),
                    },
                );
            }
        }
    }
    pub fn prepare(&mut self, doc: &Value, posting: bool) -> Option<Publication> {
        if self.stopped
            || self.waiting_authority
            || posting
            || self.in_flight.is_some()
            || self.pending.is_empty()
        {
            return None;
        }
        let mut out = doc.clone();
        let mut rejected = 0;
        let mut sent = BTreeMap::new();
        for ((group, path), intent) in std::mem::take(&mut self.pending) {
            if node(&out, &group, &path)
                .is_none_or(|n| crate::workspace_view::shape(n) != intent.shape)
            {
                rejected += 1;
                continue;
            }
            let wire = path
                .iter()
                .map(|step| if *step == 0 { "first" } else { "second" })
                .collect::<Vec<_>>();
            match comandos_core::workspace::layout::resize_split(
                &out,
                &group,
                &json!(wire),
                &json!(intent.ratio),
            ) {
                Ok(changed) => {
                    out = changed;
                    sent.insert((group, path), intent);
                }
                Err(_) => rejected += 1,
            }
        }
        let changed = !sent.is_empty();
        self.in_flight = changed.then_some(sent);
        Some(Publication {
            document: out,
            rejected,
            changed,
        })
    }
    /// Un resultado incierto espera un nuevo poll y conserva sólo la intención más reciente.
    pub fn complete(&mut self, accepted: bool) {
        self.waiting_authority = true;
        if let Some(sent) = self.in_flight.take()
            && !accepted
            && !self.stopped
        {
            for (key, intent) in sent {
                self.pending.entry(key).or_insert(intent);
            }
        }
    }
    /// Sólo después de aplicar una autoridad válida puede prepararse el siguiente POST.
    pub fn authority_received(&mut self) {
        self.waiting_authority = false;
    }
    pub fn cancel(&mut self) {
        self.stopped = true;
        self.pending.clear();
        self.in_flight = None;
    }
}
