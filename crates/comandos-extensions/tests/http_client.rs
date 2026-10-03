#[test]
fn system_trust_client_can_be_constructed_in_sandbox() {
    reqwest::Client::builder()
        .build()
        .expect("public TLS trust must be mounted into the sandbox");
}
