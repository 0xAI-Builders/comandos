#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::ui::mosaic::{MosaicState, slots};
use serde_json::json;
#[test]
fn mosaic_keeps_local_first_label_order_cap_and_balanced_twelve_unit_rows() {
    let sessions = vec![
        json!({"session":"z","label":"Alpha"}),
        json!({"session":"local","label":"Zulu"}),
        json!({"session":"a","label":"Beta"}),
    ];
    let mut state = MosaicState::default();
    state.open(&sessions);
    assert_eq!(
        state
            .sessions
            .iter()
            .map(|s| s.session.as_str())
            .collect::<Vec<_>>(),
        vec!["local", "z", "a"]
    );
    state.zoom("z").unwrap();
    assert_eq!(state.zoomed.as_deref(), Some("z"));
    state.unzoom();
    assert!(state.zoomed.is_none());
    state.close();
    assert!(!state.on);
    assert!(state.sessions.is_empty());
    assert_eq!(
        slots(5),
        vec![(0, 0, 4), (0, 4, 4), (0, 8, 4), (1, 0, 6), (1, 6, 6)]
    );
    for n in 1..=12 {
        let points = slots(n);
        assert_eq!(points.len(), n);
        for row in 0..=points.last().unwrap().0 {
            assert_eq!(
                points
                    .iter()
                    .filter(|(r, _, _)| *r == row)
                    .map(|(_, _, w)| w)
                    .sum::<i32>(),
                12
            );
        }
    }
}
#[path = "support/t18_oracle.rs"]
mod oracle;
#[test]
fn actual_original_mosaic_session_order_and_grid_slots_match_native() {
    let names = json!(["z", "local", "a"]);
    let labels = json!({"z":"Alpha","local":"Zulu","a":"Beta"});
    let original = oracle::original(json!({"op":"mosaic","sessions":names,"labels":labels}));
    let mut state = MosaicState::default();
    state.open(&[
        json!({"session":"z","label":"Alpha"}),
        json!({"session":"local","label":"Zulu"}),
        json!({"session":"a","label":"Beta"}),
    ]);
    assert_eq!(
        json!(
            state
                .sessions
                .iter()
                .map(|s| (&s.label, &s.session))
                .collect::<Vec<_>>()
        ),
        original
    );
    for n in 1..=12 {
        assert_eq!(
            json!(slots(n)),
            oracle::original(json!({"op":"slots","n":n}))
        );
    }
}
#[test]
fn twenty_client_cycles_and_replacements_cancel_real_registry_tickets() {
    use comandos_app::ui::mosaic::{ManagedView, OwnedViews};
    use std::{cell::Cell, rc::Rc};
    struct Client {
        live: Rc<Cell<usize>>,
        closed: Cell<bool>,
    }
    impl ManagedView for Client {
        fn shutdown(&self) {
            if !self.closed.replace(true) {
                self.live.set(self.live.get() - 1);
            }
        }
    }
    let count = Rc::new(Cell::new(0));
    let mut views = OwnedViews::new(12);
    for cycle in 0..20 {
        let before = views.ticket();
        for i in 0..12 {
            count.set(count.get() + 1);
            views
                .insert(
                    format!("{i}"),
                    Client {
                        live: count.clone(),
                        closed: Cell::new(false),
                    },
                )
                .unwrap();
        }
        assert_eq!(count.get(), 12);
        let old = views.ticket();
        views.clear();
        assert_eq!(count.get(), 0);
        assert!(!old.current());
        assert!(!before.current(), "cycle {cycle}");
    }
    count.set(1);
    views
        .insert(
            "same".into(),
            Client {
                live: count.clone(),
                closed: Cell::new(false),
            },
        )
        .unwrap();
    count.set(2);
    views
        .insert(
            "same".into(),
            Client {
                live: count.clone(),
                closed: Cell::new(false),
            },
        )
        .unwrap();
    assert_eq!(count.get(), 1);
    drop(views);
    assert_eq!(count.get(), 0);
}
