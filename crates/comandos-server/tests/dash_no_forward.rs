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

const GET: &[&str] = &[
    "/operator",
    "/operator/x",
    "/session-config-history?session=s&pane=%250",
    "/pane-extensions?x=1",
    "/webterm-token",
    "/stateX",
    "/accounts",
    "/providers",
    "/optimization/plans",
    "/opencode/models",
    "/model-tiers",
    "/tab-models",
    "/active-tab",
    "/dedication",
    "/analytics/week",
    "/pomodoro/report",
    "/pomodoro",
    "/sovereignty",
    "/model/status",
    "/proxy",
    "/ui-log/summary",
    "/session-profiles",
    "/extension-usage",
    "/session-brain",
    "/usage/guard",
    "/usage/changes",
    "/notifs/count",
    "/news/latest",
    "/push/key",
    "/news/editions",
    "/news/media/0123456789abcdef0123456789abcdef.png",
    "/news/source",
    "/news/chat",
    "/news/notes",
    "/news/saved",
    "/news/edition?id=latest",
    "/models/latest",
    "/fs/dirs",
    "/usage/provider-compare",
    "/usage/experiments",
    "/usage/analytics",
    "/usage/interactions",
    "/usage/state",
    "/project-profiles",
    "/ssh",
    "/conf",
    "/prefs",
    "/tmux-mouse",
    "/workspace",
    "/notices/watch",
    "/notices",
    "/notices/prefs",
    "/workspace/close-group",
    "/workspace/client",
    "/tabs",
    "/tab-history",
    "/remote-state",
    "/remote-qr.png",
    "/work-marks",
    "/events/v2",
    "/events",
    "/commands/catalog",
    "/snippets",
    "/chains",
    "/no-existe.css",
    "/vendor/",
    "/assets",
];

const POST: &[&str] = &[
    "/push/subscription",
    "/operator",
    "/pane-extensions",
    "/pane-extensions/apply",
    "/workspace/sort",
    "/workspace",
    "/notify-popup",
    "/presence",
    "/workspace/close-group",
    "/work-marks",
    "/events/v2",
    "/workspace/client",
    "/session/recover",
    "/terminal-history",
    "/terminal-panes",
    "/open-path",
    "/proxy",
    "/ui-log",
    "/pomodoro",
    "/conf-set",
    "/session-profiles",
    "/session-profile-apply",
    "/test",
    "/remote-on",
    "/remote-off",
    "/remote-webterm-on",
    "/remote-webterm-off",
    "/ssh-add",
    "/ssh-del",
    "/ssh-update",
    "/prefs-set",
    "/push/test",
    "/news/saved",
    "/news/notes",
    "/news/chat",
    "/news/chat/note",
    "/news/translate",
    "/open-url",
    "/fs/mkdir",
    "/ssh-connect",
    "/ssh-new-tab",
    "/ssh-key-setup",
    "/chains",
    "/snippets",
    "/session-new",
    "/terminal/quick",
    "/account/add",
    "/tmux-mouse",
    "/tmux-scroll",
    "/tab-register",
    "/tab-metadata",
    "/tab-metadata-remove",
    "/tab-close",
    "/recover-tab",
    "/session/configure",
    "/account/switch",
    "/harness/switch",
    "/model/switch-cancel",
    "/model/switch",
    "/pane/type",
    "/send",
    "/paste",
    "/key",
    "/export",
    "/kill",
    "/focus",
    "/ensure",
    "/new",
    "/shell",
    "/up",
    "/ruta-que-no-existe",
];

#[test]
fn no_route_classifies_as_forward() {
    let exists = |_: &str| false;
    for path in GET {
        assert_ne!(
            router::classify_with(&Method::GET, path, &exists, true),
            RouteClass::Forward,
            "GET {path}"
        );
        assert_ne!(
            router::classify_with(&Method::HEAD, path, &exists, true),
            RouteClass::Forward,
            "HEAD {path}"
        );
    }
    for path in POST {
        assert_ne!(
            router::classify_with(&Method::POST, path, &exists, true),
            RouteClass::Forward,
            "POST {path}"
        );
    }
    assert_ne!(
        router::classify_with(&Method::DELETE, "/x", &exists, true),
        RouteClass::Forward,
        "DELETE"
    );
}
