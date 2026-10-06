//! Per-client relaunch policy; mosaic still receives its initial PTY once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RespawnPolicy {
    enabled: bool,
    exited: bool,
    closed: bool,
}
impl Default for RespawnPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            exited: false,
            closed: false,
        }
    }
}
impl RespawnPolicy {
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
    pub fn may_start(&self) -> bool {
        !self.closed && (self.enabled || !self.exited)
    }
    pub fn exited(&mut self) -> bool {
        self.exited = true;
        self.may_start()
    }
    pub fn shutdown(&mut self) {
        self.closed = true;
    }
}
