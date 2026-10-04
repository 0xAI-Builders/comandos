use comandos_extensions::broker::mux::{Mux, Outbound};
use serde_json::{Value, json};

fn j(v: Value) -> Vec<u8> {
    serde_json::to_vec(&v).unwrap()
}
fn parse(b: &[u8]) -> Value {
    serde_json::from_slice(b).unwrap()
}
fn init(id: u64) -> Vec<u8> {
    j(
        json!({"jsonrpc":"2.0","id":id,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"c","version":"1"}}}),
    )
}
fn init_with_caps(id: u64, caps: Value) -> Vec<u8> {
    j(
        json!({"jsonrpc":"2.0","id":id,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":caps,"clientInfo":{"name":"c","version":"1"}}}),
    )
}
fn init_result(id: Value) -> Vec<u8> {
    j(
        json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"up","version":"1"}}}),
    )
}
fn up_line(o: &Outbound) -> Value {
    match o {
        Outbound::ToUpstream(l) => parse(l),
        other => panic!("se esperaba ToUpstream, llegó {other:?}"),
    }
}

#[test]
fn concurrent_initialize_single_upstream() {
    let mut m = Mux::new();
    let a = m.add_client();
    let b = m.add_client();
    let out_a = m.from_client(a, &init(1));
    assert!(matches!(out_a.as_slice(), [Outbound::ToUpstream(_)]));
    let out_b = m.from_client(b, &init(7));
    assert!(out_b.is_empty(), "el segundo initialize espera al upstream");
    let up_id = up_line(&out_a[0])["id"].clone();
    let out = m.from_upstream(&init_result(up_id));
    let ids: Vec<(u32, Value)> = out
        .iter()
        .map(|o| match o {
            Outbound::ToClient(c, l) => (*c, parse(l)["id"].clone()),
            _ => panic!(),
        })
        .collect();
    assert_eq!(ids, vec![(a, json!(1)), (b, json!(7))]);
    assert!(m.upstream_initialized());
    let note = j(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    assert!(m.from_client(a, &note).len() == 1);
    assert!(m.from_client(b, &note).is_empty());
}

#[test]
fn responses_route_back_with_original_ids_including_strings() {
    let mut m = Mux::new();
    let a = m.add_client();
    let b = m.add_client();
    let _ = m.from_client(a, &init(1));
    let _ = m.from_upstream(&init_result(json!(1)));
    let _ = m.from_client(b, &init(1));
    let oa = m.from_client(
        a,
        &j(json!({"jsonrpc":"2.0","id":"x-1","method":"tools/list"})),
    );
    let ob = m.from_client(
        b,
        &j(json!({"jsonrpc":"2.0","id":"x-1","method":"tools/list"})),
    );
    let ida = up_line(&oa[0])["id"].clone();
    let idb = up_line(&ob[0])["id"].clone();
    assert_ne!(ida, idb);
    let r = m.from_upstream(&j(json!({"jsonrpc":"2.0","id":idb,"result":{"tools":[]}})));
    assert!(
        matches!(r.as_slice(), [Outbound::ToClient(c, l)] if *c == b && parse(l)["id"] == json!("x-1"))
    );
}

#[test]
fn late_response_after_client_gone() {
    let mut m = Mux::new();
    let a = m.add_client();
    let _ = m.from_client(a, &init(1));
    let _ = m.from_upstream(&init_result(json!(1)));
    let o = m.from_client(
        a,
        &j(
            json!({"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"x","arguments":{}}}),
        ),
    );
    let up_id = up_line(&o[0])["id"].clone();
    let _ = m.remove_client(a);
    let late = m.from_upstream(&j(
        json!({"jsonrpc":"2.0","id":up_id,"result":{"content":[]}}),
    ));
    assert!(late.is_empty(), "la respuesta tardía no debe ir a nadie");
}

#[test]
fn upstream_notifications_broadcast_only_to_initialized_clients() {
    let mut m = Mux::new();
    let a = m.add_client();
    let _b = m.add_client();
    let _ = m.from_client(a, &init(1));
    let _ = m.from_upstream(&init_result(json!(1)));
    let out = m.from_upstream(&j(
        json!({"jsonrpc":"2.0","method":"notifications/tools/list_changed"}),
    ));
    assert_eq!(out.len(), 1);
    assert!(matches!(&out[0], Outbound::ToClient(c, _) if *c == a));
}

#[test]
fn upstream_request_goes_to_capable_client_and_answer_returns_with_original_id() {
    let mut m = Mux::new();
    let a = m.add_client();
    let b = m.add_client();
    let _ = m.from_client(a, &init_with_caps(1, json!({"roots":{}})));
    let _ = m.from_upstream(&init_result(json!(1)));
    // b es más reciente pero no declara roots
    let _ = m.from_client(b, &init(1));
    let out = m.from_upstream(&j(
        json!({"jsonrpc":"2.0","id":"up-1","method":"roots/list"}),
    ));
    let (target, given) = match out.as_slice() {
        [Outbound::ToClient(c, l)] => (*c, parse(l)["id"].clone()),
        other => panic!("{other:?}"),
    };
    assert_eq!(target, a, "debe ir al cliente con capacidad roots");
    assert_ne!(given, json!("up-1"), "el id entregado al cliente es nuevo");
    // un tercero no puede contestar ese id
    assert!(
        m.from_client(
            b,
            &j(json!({"jsonrpc":"2.0","id":given.clone(),"result":{"roots":[]}}))
        )
        .is_empty()
    );
    let back = m.from_client(
        a,
        &j(json!({"jsonrpc":"2.0","id":given,"result":{"roots":[]}})),
    );
    assert!(matches!(back.as_slice(), [Outbound::ToUpstream(_)]));
    assert_eq!(up_line(&back[0])["id"], json!("up-1"));
}

#[test]
fn upstream_request_without_any_client_gets_error_and_departure_cancels_it() {
    let mut m = Mux::new();
    let out = m.from_upstream(&j(
        json!({"jsonrpc":"2.0","id":5,"method":"sampling/createMessage"}),
    ));
    assert!(matches!(out.as_slice(), [Outbound::ToUpstream(_)]));
    let v = up_line(&out[0]);
    assert_eq!(v["id"], json!(5));
    assert!(v["error"].is_object());

    let a = m.add_client();
    let _ = m.from_client(a, &init_with_caps(1, json!({"sampling":{}})));
    let _ = m.from_upstream(&init_result(json!(1)));
    let _ = m.from_upstream(&j(
        json!({"jsonrpc":"2.0","id":"s","method":"sampling/createMessage"}),
    ));
    let gone = m.remove_client(a);
    assert!(matches!(gone.as_slice(), [Outbound::ToUpstream(_)]));
    assert_eq!(up_line(&gone[0])["id"], json!("s"));
}

#[test]
fn malformed_input_never_panics_and_init_survives_initiator_leaving() {
    let mut m = Mux::new();
    let a = m.add_client();
    let b = m.add_client();
    for bad in [&b"not json"[..], b"[1,2]", b"42", b"{}", b"{\"id\":1}"] {
        assert!(m.from_client(a, bad).is_empty());
        assert!(m.from_upstream(bad).is_empty());
    }
    let oa = m.from_client(a, &init(1));
    let _ = m.from_client(b, &init(2));
    let _ = m.remove_client(a);
    let up_id = up_line(&oa[0])["id"].clone();
    let out = m.from_upstream(&init_result(up_id));
    assert!(matches!(out.as_slice(), [Outbound::ToClient(c, _)] if *c == b));
    assert!(m.upstream_initialized());
}

#[test]
fn cancelled_notification_is_translated_to_upstream_id() {
    let mut m = Mux::new();
    let a = m.add_client();
    let _ = m.from_client(a, &init(1));
    let _ = m.from_upstream(&init_result(json!(1)));
    let o = m.from_client(
        a,
        &j(json!({"jsonrpc":"2.0","id":"r","method":"tools/call"})),
    );
    let up_id = up_line(&o[0])["id"].clone();
    let c = m.from_client(
        a,
        &j(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"r"}})),
    );
    assert_eq!(up_line(&c[0])["params"]["requestId"], up_id);
}
