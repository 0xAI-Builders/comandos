//! Platform-independent desktop policies and injected file boundaries.
#![forbid(unsafe_code)]
pub mod bridge;
pub mod command;
pub mod dash_client;
pub mod ipc;
pub mod lang;
pub mod mac_tabs;
pub mod mode;
pub mod restore;
pub mod state_files;
pub mod tabs;
pub mod term_url;
pub mod validation;
pub use bridge::{BridgeMessage, parse_mac_bridge as parse_bridge};
pub use dash_client::RetrySchedule;
pub use lang::{DEFAULT_THEME, DOT_COLORS, DOT_IDLE, Lang, VALID_THEMES, ui_lang};
pub use mac_tabs::{
    RestoreSpec, TabKind, TabMeta, agent_from_command, archive_into, cancel_restore_snapshot,
    find_project_dir, history_item, load_saved_tabs, load_tab_metadata, merge_tab_labels,
    restore_tab_spec, ssh_host_from_session,
};
mod python_digit;
