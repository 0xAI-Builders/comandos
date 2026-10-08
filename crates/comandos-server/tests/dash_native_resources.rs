use comandos_server::dash::{
    native,
    router::{self, RouteClass},
};
use http::Method;

#[test]
fn resources_are_native_and_never_shadowed_by_a_static_file() {
    let route = native::route(&Method::GET, "/analytics/resources");
    assert!(route.is_some(), "resources must have a native handler");
    assert_eq!(
        native::route(&Method::GET, "/analytics/resources?x=1"),
        route
    );
    assert!(matches!(
        router::classify_with(&Method::GET, "/analytics/resources", &|_| true, true),
        RouteClass::Native(_)
    ));
    assert_ne!(
        native::route(&Method::GET, "/analytics/resources-other"),
        route
    );
    assert_ne!(native::route(&Method::POST, "/analytics/resources"), route);
}

#[test]
fn remote_resources_require_the_dashboard_token_even_when_an_asset_exists() {
    use comandos_core::dashboard_access::{Method as GateMethod, Request, request_admission};
    for path in ["/analytics/resources", "/analytics/resources?refresh=1"] {
        let headers = [("Host", "localhost:4777")];
        let request = Request {
            method: GateMethod::Get,
            path,
            peer_ip: Some("192.0.2.10"),
            headers: &headers,
        };
        assert_eq!(
            request_admission(&request, b"fixture-token", true)
                .unwrap_err()
                .status,
            401
        );
        let headers = [
            ("Host", "localhost:4777"),
            ("Authorization", "Bearer fixture-token"),
        ];
        let request = Request {
            headers: &headers,
            ..request
        };
        assert!(request_admission(&request, b"fixture-token", true).is_ok());
    }
}
