#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
use comandos_app::workspace_resize::ResizeQueue;
use serde_json::{Value, json};
fn document() -> Value {
    comandos_app::fixture::mixed(std::path::Path::new("/private-home"))["workspace"].clone()
}
struct DelayedBackend {
    queue: ResizeQueue,
    authority: Value,
    posts: Vec<(u64, Value)>,
    posting: bool,
    revision: u64,
}
impl DelayedBackend {
    fn new() -> Self {
        Self {
            queue: ResizeQueue::default(),
            authority: document(),
            posts: Vec::new(),
            posting: false,
            revision: 7,
        }
    }
    fn motion(&mut self, ratio: f64) {
        self.queue
            .record(&self.authority, &[("split-fixture".into(), vec![], ratio)]);
        self.publish();
    }
    fn publish(&mut self) {
        if let Some(publication) = self.queue.prepare(&self.authority, self.posting)
            && publication.changed
        {
            self.posts.push((self.revision, publication.document));
            self.posting = true;
        }
    }
    fn respond(&mut self, revision: u64, authority: Value, accepted: bool) {
        self.posting = false;
        self.queue.complete(accepted);
        self.authority = authority;
        self.revision = revision;
        self.queue.authority_received();
        self.publish();
    }
}
#[test]
fn last_resize_waits_for_post_and_rebases_on_authoritative_revision() {
    let mut backend = DelayedBackend::new();
    backend.motion(0.6);
    backend.motion(0.65);
    backend.motion(0.73);
    assert_eq!(backend.posts.len(), 1);
    let mut authority = backend.posts[0].1.clone();
    authority["tabs"]["local"]["deviceMetadata"] = json!("concurrent-authority");
    backend.respond(8, authority, true);
    assert_eq!(
        backend.posts.len(),
        2,
        "final coalesced motion must publish after the first response"
    );
    assert_eq!(backend.posts[1].0, 8);
    assert_eq!(
        backend.posts[1].1["groups"][1]["tree"]["ratio"],
        json!(0.73)
    );
    assert_eq!(
        backend.posts[1].1["tabs"]["local"]["deviceMetadata"],
        json!("concurrent-authority")
    );
    backend.respond(9, backend.posts[1].1.clone(), true);
    assert_eq!(backend.posts.len(), 2);
}
#[test]
fn conflict_rebases_latest_intent_and_cancel_emits_no_post() {
    let mut backend = DelayedBackend::new();
    backend.motion(0.6);
    backend.motion(0.8);
    backend.respond(10, document(), false);
    assert_eq!(backend.posts.len(), 2);
    assert_eq!(backend.posts[1].0, 10);
    assert_eq!(backend.posts[1].1["groups"][1]["tree"]["ratio"], json!(0.8));
    backend.motion(0.7);
    backend.queue.cancel();
    backend.respond(11, document(), true);
    backend.motion(0.9);
    assert_eq!(backend.posts.len(), 2);
}
#[test]
fn structural_conflict_rejects_path_instead_of_resizing_another_split() {
    let mut queue = ResizeQueue::default();
    let mut authority = document();
    queue.record(&authority, &[("split-fixture".into(), vec![1], 0.7)]);
    authority["groups"][1]["tree"]["second"]["axis"] = json!("x");
    let out = queue.prepare(&authority, false).unwrap();
    assert!(!out.changed);
    assert_eq!(out.rejected, 1);
    assert_eq!(out.document, authority);
    assert!(queue.prepare(&authority, false).is_none());
}
#[test]
fn nested_paths_use_core_wire_names_and_failed_attempt_preserves_newer_ratio() {
    let mut queue = ResizeQueue::default();
    let authority = document();
    queue.record(&authority, &[("split-fixture".into(), vec![1], 0.7)]);
    let out = queue.prepare(&authority, false).unwrap();
    assert!(out.changed);
    assert_eq!(
        out.document["groups"][1]["tree"]["second"]["ratio"],
        json!(0.7)
    );
    queue.record(&authority, &[("split-fixture".into(), vec![1], 0.8)]);
    queue.complete(false);
    queue.authority_received();
    let retry = queue.prepare(&authority, false).unwrap();
    assert_eq!(
        retry.document["groups"][1]["tree"]["second"]["ratio"],
        json!(0.8)
    );
}

#[test]
fn uncertain_transport_result_waits_for_new_authority_before_retry() {
    let mut queue = ResizeQueue::default();
    let authority = document();
    queue.record(&authority, &[("split-fixture".into(), vec![], 0.6)]);
    assert!(queue.prepare(&authority, false).unwrap().changed);
    queue.record(&authority, &[("split-fixture".into(), vec![], 0.75)]);
    queue.complete(false);
    assert!(queue.prepare(&authority, false).is_none());
    queue.authority_received();
    let retry = queue.prepare(&authority, false).unwrap();
    assert_eq!(retry.document["groups"][1]["tree"]["ratio"], json!(0.75));
}
