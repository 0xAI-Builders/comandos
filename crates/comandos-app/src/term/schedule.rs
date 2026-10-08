//! Main-loop timing model; it never starts a graphical runtime.
use std::collections::BTreeSet;
#[derive(Default)]
pub struct PaintSchedule {
    lines: BTreeSet<usize>,
    focused: bool,
    synchronized: bool,
    blink_at: u64,
    queued: bool,
}
impl PaintSchedule {
    pub fn damage(&mut self, _now_ms: u64, lines: &[usize]) {
        self.lines.extend(lines.iter().copied());
    }
    pub fn pending_lines(&self) -> Vec<usize> {
        self.lines.iter().copied().collect()
    }
    pub fn next_delay_ms(&self, _now_ms: u64) -> Option<u64> {
        // GTK coalesces queue_draw() against its frame clock. Waiting another
        // 16 ms after draw() would skip the next frame; `queued` alone keeps
        // one request in flight, including damage received before that frame.
        (!self.lines.is_empty() && !self.queued).then_some(0)
    }
    /// Unmapped widgets retain damage but cannot acknowledge a GTK draw.
    pub fn next_delay_ms_if_mapped(&self, now_ms: u64, mapped: bool) -> Option<u64> {
        if mapped {
            self.next_delay_ms(now_ms)
        } else {
            None
        }
    }
    /// Queue at most one GTK frame while awaiting its draw acknowledgement.
    pub fn queue_due_paint(&mut self, now_ms: u64, mapped: bool) -> bool {
        if self.next_delay_ms_if_mapped(now_ms, mapped) == Some(0) {
            self.queued = true;
            true
        } else {
            false
        }
    }
    pub fn reset_queued_paint(&mut self) {
        self.queued = false;
    }
    pub fn painted(&mut self, _now_ms: u64) {
        self.lines.clear();
        self.queued = false;
    }
    pub fn set_focused(&mut self, focused: bool, now_ms: u64) {
        self.focused = focused;
        self.blink_at = now_ms.saturating_add(600);
    }
    pub fn blink_due(&self, now_ms: u64) -> bool {
        self.focused && !self.synchronized && now_ms >= self.blink_at
    }
    pub fn set_synchronized(&mut self, active: bool) {
        self.synchronized = active;
    }
    pub fn blinked(&mut self, now_ms: u64) {
        self.blink_at = now_ms.saturating_add(600);
    }
}
