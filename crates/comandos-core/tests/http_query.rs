use comandos_core::dashboard_access::{query_pairs, request_target_parts};

#[test]
fn request_target_split_preserves_path_query_and_python_url_boundaries() {
    for (raw, path, query) in [
        ("/events/v2?after=0#fragment", "/events/v2", "after=0"),
        (
            "http://localhost/events/v2?after=0",
            "/events/v2",
            "after=0",
        ),
        ("\t/events/v2\r\n?after=0", "/events/v2", "after=0"),
        ("/%65vents/v2", "/%65vents/v2", ""),
    ] {
        assert_eq!(request_target_parts(raw), Some((path.into(), query.into())));
    }
    assert_eq!(request_target_parts("http://[bad/events/v2"), None);
}

#[test]
fn query_pairs_preserve_decoded_duplicates_and_keep_blank_switch() {
    assert_eq!(
        query_pairs("after=&after=%31&turns=%31&x=%FF&x=a+b&flag", false),
        vec![
            ("after".into(), "1".into()),
            ("turns".into(), "1".into()),
            ("x".into(), "�".into()),
            ("x".into(), "a b".into())
        ]
    );
    assert_eq!(
        query_pairs("after=&flag", true),
        vec![("after".into(), "".into()), ("flag".into(), "".into())]
    );
}
