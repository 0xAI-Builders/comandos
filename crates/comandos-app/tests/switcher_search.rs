use comandos_app::ui::switcher::{Candidate, search};

fn sessions() -> Vec<Candidate> {
    (0..25)
        .map(|index| Candidate {
            label: format!("Session {index:02}"),
            key: format!("term-{index:02}"),
            state: "working".into(),
            open: true,
        })
        .collect()
}

#[test]
fn empty_query_keeps_all_twenty_five_sessions() {
    let sessions = sessions();
    for query in ["", "   "] {
        assert_eq!(search(query, &sessions), sessions);
    }
}

#[test]
fn typed_query_keeps_twelve_ranked_matches() {
    let sessions = sessions();
    assert_eq!(search("session", &sessions), sessions[..12]);
    assert!(search("missing", &sessions).is_empty());
}
