//! TF: catch-all alone cannot prove a dynamic branch is native.
use comandos_server::dash::{
    native::{self, NativeRoute},
    router::{self, RouteClass},
};
use http::Method;
const PREFIXES: &[&str] = &[
    "/state",
    "/accounts",
    "/providers",
    "/optimization/plans",
    "/model-tiers",
    "/tab-models",
    "/active-tab",
    "/dedication",
    "/analytics/week",
    "/pomodoro/report",
    "/model/status",
    "/ui-log/summary",
    "/extension-usage",
    "/session-brain",
    "/usage/guard",
    "/usage/changes",
    "/notifs/count",
    "/usage/provider-compare",
    "/usage/experiments",
    "/usage/analytics",
    "/usage/interactions",
    "/usage/state",
    "/project-profiles",
    "/prefs",
    "/tabs",
    "/tab-history",
    "/events",
];
#[test]
fn prefixes_resolve_to_the_canonical_branch_instead_of_residual_decline() {
    for canonical in PREFIXES {
        let expected = native::route(&Method::GET, canonical);
        assert!(!matches!(expected, Some(NativeRoute::Residue(_)) | None));
        for suffix in ["X", "/x", "X?x=1", "/x?x=1"] {
            let target = format!("{canonical}{suffix}");
            assert_eq!(native::route(&Method::GET, &target), expected, "{target}");
        }
    }
}
#[test]
fn ordered_exact_and_retired_branches_keep_their_boundaries() {
    assert_eq!(
        native::route(&Method::GET, "/events/v2?x=1"),
        native::route(&Method::GET, "/events/v2")
    );
    assert_ne!(
        native::route(&Method::GET, "/events/v2"),
        Some(NativeRoute::Retired)
    );
    assert_eq!(
        native::route(&Method::GET, "/events/v2X"),
        Some(NativeRoute::Retired)
    );
    assert_eq!(
        native::route(&Method::GET, "/proxy?x=1"),
        Some(NativeRoute::Retired)
    );
    for target in [
        "/news/editionsX",
        "/pomodoro?x=1",
        "/sovereignty?x=1",
        "/tmux-mouse#x",
    ] {
        assert!(
            matches!(
                native::route(&Method::GET, target),
                Some(NativeRoute::Residue(_))
            ),
            "{target}"
        );
    }
    assert_eq!(
        router::classify_with(&Method::GET, "/stateX", &|_| false, false),
        RouteClass::Forward
    );
    assert!(matches!(
        native::route(&Method::HEAD, "/stateX"),
        Some(NativeRoute::Residue(_))
    ));
}
