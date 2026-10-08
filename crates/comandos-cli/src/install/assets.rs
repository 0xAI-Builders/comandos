//! Installer payloads are compiled into the executable; no checkout is needed.
pub struct Asset {
    pub path: &'static str,
    pub bytes: &'static [u8],
    pub mode: u32,
}
macro_rules! asset {
    ($dest:literal, $source:literal, $mode:literal) => {
        Asset {
            path: $dest,
            bytes: include_bytes!(concat!("../../../../", $source)),
            mode: $mode,
        }
    };
}
pub const CONFIG: &[Asset] = &[
    asset!(".config/kitty/kitty.conf", "config/kitty.conf", 0o600),
    asset!(
        ".claude/hooks/terminal-replies.conf",
        "config/terminal-replies.conf",
        0o600
    ),
    asset!(".tmux.conf", "config/tmux.conf", 0o600),
    asset!(
        ".claude/hooks/cc-notify.conf",
        "hooks/cc-notify.conf.example",
        0o600
    ),
    asset!(
        ".local/share/icons/hicolor/scalable/apps/centro-claude.svg",
        "dash/comandos.svg",
        0o644
    ),
    asset!(
        ".local/share/comandos/assets/sounds/pomodoro-complete.wav",
        "assets/sounds/pomodoro-complete.wav",
        0o644
    ),
];
pub const FONTS: &[Asset] = &[
    asset!(
        "JetBrainsMonoNerdFontMono-Regular.ttf",
        "assets/fonts/JetBrainsMono/JetBrainsMonoNerdFontMono-Regular.ttf",
        0o644
    ),
    asset!(
        "JetBrainsMonoNerdFontMono-Bold.ttf",
        "assets/fonts/JetBrainsMono/JetBrainsMonoNerdFontMono-Bold.ttf",
        0o644
    ),
    asset!(
        "JetBrainsMonoNerdFontMono-Italic.ttf",
        "assets/fonts/JetBrainsMono/JetBrainsMonoNerdFontMono-Italic.ttf",
        0o644
    ),
    asset!(
        "JetBrainsMonoNerdFontMono-BoldItalic.ttf",
        "assets/fonts/JetBrainsMono/JetBrainsMonoNerdFontMono-BoldItalic.ttf",
        0o644
    ),
    asset!("OFL.txt", "assets/fonts/JetBrainsMono/OFL.txt", 0o644),
];
pub const DESKTOP: &str = include_str!("../../../../dash/comandos.desktop.in");
pub const TMUX_UNIT: &[u8] = include_bytes!("../../../../systemd/tmux.service");
pub const DASH_UNIT: &[u8] = include_bytes!("../../../../systemd/cc-dash.service");
pub const NOTIFY_UNIT: &[u8] = include_bytes!("../../../../systemd/cc-notifyd.service");
pub const PROXY_UNIT: &[u8] = include_bytes!("../../../../systemd/cc-proxy.service");
pub const BROKER_UNIT: &[u8] = include_bytes!("../../../../systemd/comandos-broker.service");
