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
    for bad in [&b"[1,2]"[..], b"42", b"\"s\"", b"{}", b"{\"id\":1}"] {
        assert!(m.from_client(a, bad).is_empty());
        assert!(m.from_upstream(bad).is_empty());
    }
    assert!(m.from_upstream(b"not json").is_empty());
    let pe = m.from_client(a, b"not json");
    assert!(matches!(pe.as_slice(), [Outbound::ToClient(c, _)] if *c == a));
    let Outbound::ToClient(_, l) = &pe[0] else {
        panic!()
    };
    assert_eq!(parse(l)["error"]["code"], json!(-32700));
    assert_eq!(parse(l)["id"], Value::Null);
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
    // I1: la cancelación borra el pendiente; la respuesta tardía no va a nadie
    let late = m.from_upstream(&j(json!({"jsonrpc":"2.0","id":up_id,"result":{}})));
    assert!(late.is_empty());
}

fn ready(m: &mut Mux, caps: Value) -> u32 {
    let c = m.add_client();
    let _ = m.from_client(c, &init_with_caps(1, caps));
    if !m.upstream_initialized() {
        let _ = m.from_upstream(&init_result(json!(1)));
    }
    c
}

#[test]
fn progress_is_routed_only_to_the_owner_with_original_token() {
    let mut m = Mux::new();
    let a = ready(&mut m, json!({}));
    let b = ready(&mut m, json!({}));
    let req = |tok: &str| {
        j(
            json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"_meta":{"progressToken":tok}}}),
        )
    };
    let oa = m.from_client(a, &req("same"));
    let ob = m.from_client(b, &req("same"));
    let ta = up_line(&oa[0])["params"]["_meta"]["progressToken"].clone();
    let tb = up_line(&ob[0])["params"]["_meta"]["progressToken"].clone();
    assert_ne!(ta, tb);
    let prog = |t: &Value| {
        j(
            json!({"jsonrpc":"2.0","method":"notifications/progress","params":{"progressToken":t,"progress":1}}),
        )
    };
    let out = m.from_upstream(&prog(&ta));
    assert!(
        matches!(out.as_slice(), [Outbound::ToClient(c, l)] if *c == a && parse(l)["params"]["progressToken"] == json!("same"))
    );
    assert!(m.from_upstream(&prog(&json!(9999))).is_empty());
    // al responder, el token se olvida
    let ida = up_line(&oa[0])["id"].clone();
    let _ = m.from_upstream(&j(json!({"jsonrpc":"2.0","id":ida,"result":{}})));
    assert!(m.from_upstream(&prog(&ta)).is_empty());
}

#[test]
fn init_error_reaches_all_waiters_and_retry_is_possible() {
    let mut m = Mux::new();
    let a = m.add_client();
    let b = m.add_client();
    let oa = m.from_client(a, &init(1));
    let _ = m.from_client(b, &init(2));
    let up = up_line(&oa[0])["id"].clone();
    let out = m.from_upstream(&j(
        json!({"jsonrpc":"2.0","id":up,"error":{"code":-1,"message":"boom"}}),
    ));
    assert_eq!(out.len(), 2);
    assert!(
        out.iter().all(
            |o| matches!(o, Outbound::ToClient(_, l) if parse(l)["error"]["code"] == json!(-1))
        )
    );
    assert!(!m.upstream_initialized());
    let retry = m.from_client(a, &init(3));
    assert!(matches!(retry.as_slice(), [Outbound::ToUpstream(_)]));
    // respuesta sin result ni error -> -32603 sintetizado
    let up = up_line(&retry[0])["id"].clone();
    let out = m.from_upstream(&j(json!({"jsonrpc":"2.0","id":up})));
    assert!(
        matches!(out.as_slice(), [Outbound::ToClient(_, l)] if parse(l)["error"]["code"] == json!(-32603))
    );
}

#[test]
fn upstream_request_without_capable_client_gets_32601() {
    let mut m = Mux::new();
    let _ = ready(&mut m, json!({"sampling":{}}));
    let out = m.from_upstream(&j(json!({"jsonrpc":"2.0","id":3,"method":"roots/list"})));
    assert!(matches!(out.as_slice(), [Outbound::ToUpstream(_)]));
    assert_eq!(up_line(&out[0])["error"]["code"], json!(-32601));
}

#[test]
fn unknown_client_is_ignored_and_upstream_duplicate_id_is_rejected() {
    let mut m = Mux::new();
    assert!(m.from_client(99, &init(1)).is_empty());
    assert!(m.remove_client(99).is_empty());
    let _ = ready(&mut m, json!({"roots":{}}));
    let req = j(json!({"jsonrpc":"2.0","id":"d","method":"roots/list"}));
    assert!(matches!(
        m.from_upstream(&req).as_slice(),
        [Outbound::ToClient(..)]
    ));
    let dup = m.from_upstream(&req);
    assert_eq!(up_line(&dup[0])["error"]["code"], json!(-32600));
}

#[test]
fn remove_client_cancels_inflight_upstream_and_upstream_cancel_targets_owner() {
    let mut m = Mux::new();
    let a = ready(&mut m, json!({"roots":{}}));
    let o = m.from_client(a, &j(json!({"jsonrpc":"2.0","id":4,"method":"tools/call"})));
    let up = up_line(&o[0])["id"].clone();
    let down = m.from_upstream(&j(json!({"jsonrpc":"2.0","id":"u","method":"roots/list"})));
    let given = match &down[0] {
        Outbound::ToClient(_, l) => parse(l)["id"].clone(),
        _ => panic!(),
    };
    let c = m.from_upstream(&j(
        json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"u"}}),
    ));
    assert!(
        matches!(c.as_slice(), [Outbound::ToClient(cl, l)] if *cl == a && parse(l)["params"]["requestId"] == given)
    );
    let gone = m.remove_client(a);
    assert_eq!(gone.len(), 1);
    let v = up_line(&gone[0]);
    assert_eq!(v["method"], json!("notifications/cancelled"));
    assert_eq!(v["params"]["requestId"], up);
}

#[test]
fn initialize_without_id_is_rejected_and_duplicate_initialize_is_not_queued_twice() {
    let mut m = Mux::new();
    let a = m.add_client();
    let out = m.from_client(
        a,
        &j(json!({"jsonrpc":"2.0","method":"initialize","params":{}})),
    );
    assert!(matches!(out.as_slice(), [Outbound::ToClient(..)]));
    let o = m.from_client(a, &init(1));
    assert!(m.from_client(a, &init(2)).is_empty());
    let up = up_line(&o[0])["id"].clone();
    assert_eq!(m.from_upstream(&init_result(up)).len(), 1);
}

// --- Versión de protocolo: cada cliente recibe la que obtendría hablando directo con el upstream.

/// `initialize` del cliente con `params.protocolVersion` = `version` (sin la clave si `None`).
fn init_asking(id: u64, version: Option<Value>) -> Vec<u8> {
    let mut params = json!({"capabilities":{},"clientInfo":{"name":"c","version":"1"}});
    if let Some(v) = version {
        params["protocolVersion"] = v;
    }
    j(json!({"jsonrpc":"2.0","id":id,"method":"initialize","params":params}))
}
/// Respuesta del upstream con su versión `upstream` y el resto fijo.
fn init_result_at(id: Value, upstream: &str) -> Vec<u8> {
    j(
        json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":upstream,"capabilities":{"tools":{"listChanged":true}},"serverInfo":{"name":"up","version":"9"},"instructions":"usa bien"}}),
    )
}
fn client_result(o: &Outbound, who: u32) -> Value {
    match o {
        Outbound::ToClient(c, l) if *c == who => parse(l)["result"].clone(),
        other => panic!("se esperaba ToClient({who}), llegó {other:?}"),
    }
}
/// Mux con el upstream ya inicializado a versión `upstream` (lo pidió un cliente interno).
fn warmed(upstream: &str) -> Mux {
    let mut m = Mux::new();
    let w = m.add_client();
    let out = m.from_client(w, &init_asking(0, Some(json!("2025-11-25"))));
    let _ = m.from_upstream(&init_result_at(up_line(&out[0])["id"].clone(), upstream));
    m
}
fn version_for(m: &mut Mux, asked: Option<Value>) -> Value {
    let c = m.add_client();
    let out = m.from_client(c, &init_asking(1, asked));
    assert_eq!(out.len(), 1, "respuesta desde la caché");
    client_result(&out[0], c)["protocolVersion"].clone()
}

#[test]
fn cached_initialize_negotiates_each_client_version_against_upstream_latest() {
    let mut m = warmed("2025-11-25");
    for (asked, got) in [
        (Some(json!("2025-06-18")), "2025-06-18"),
        (Some(json!("2025-11-25")), "2025-11-25"),
        (Some(json!("2024-11-05")), "2024-11-05"),
        (Some(json!("2025-03-26")), "2025-03-26"),
        (Some(json!("2099-01-01")), "2025-11-25"),
        (Some(json!("2025-07-01")), "2025-11-25"),
        (Some(json!(20250618)), "2025-11-25"),
        (None, "2025-11-25"),
    ] {
        assert_eq!(
            version_for(&mut m, asked.clone()),
            json!(got),
            "pidió {asked:?}"
        );
    }
}

#[test]
fn cached_initialize_never_offers_more_than_the_upstream_answered() {
    let mut m = warmed("2025-06-18");
    assert_eq!(
        version_for(&mut m, Some(json!("2025-11-25"))),
        json!("2025-06-18")
    );
    assert_eq!(
        version_for(&mut m, Some(json!("2024-11-05"))),
        json!("2024-11-05")
    );
    assert_eq!(version_for(&mut m, None), json!("2025-06-18"));
    // Versión del upstream fuera de la lista: se entrega tal cual si la pedida no cabe.
    let mut odd = warmed("2026-01-01");
    assert_eq!(
        version_for(&mut odd, Some(json!("2099-01-01"))),
        json!("2026-01-01")
    );
    assert_eq!(
        version_for(&mut odd, Some(json!("2025-11-25"))),
        json!("2025-11-25")
    );
}

#[test]
fn waiting_clients_each_get_their_own_version_and_the_same_rest() {
    let mut m = Mux::new();
    let a = m.add_client();
    let b = m.add_client();
    let out = m.from_client(a, &init_asking(1, Some(json!("2025-11-25"))));
    assert!(
        m.from_client(b, &init_asking(2, Some(json!("2025-06-18"))))
            .is_empty()
    );
    let out = m.from_upstream(&init_result_at(
        up_line(&out[0])["id"].clone(),
        "2025-11-25",
    ));
    assert_eq!(out.len(), 2);
    let (mut ra, mut rb) = (client_result(&out[0], a), client_result(&out[1], b));
    assert_eq!(ra["protocolVersion"], "2025-11-25");
    assert_eq!(rb["protocolVersion"], "2025-06-18");
    ra.as_object_mut().unwrap().remove("protocolVersion");
    rb.as_object_mut().unwrap().remove("protocolVersion");
    assert_eq!(ra, rb, "capabilities, serverInfo e instructions idénticos");
    assert_eq!(ra["instructions"], "usa bien");
    assert_eq!(ra["capabilities"]["tools"]["listChanged"], true);
    // La caché conserva la versión del upstream: un tercero que no pide nada recibe la suya.
    assert_eq!(version_for(&mut m, None), json!("2025-11-25"));
}
