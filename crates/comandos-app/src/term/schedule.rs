//! Main-loop timing model; it never starts a graphical runtime.
use std::collections::BTreeSet;
#[derive(Default)]
pub struct PaintSchedule {
    lines: BTreeSet<usize>,
    last_paint: Option<u64>,
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
    pub fn next_delay_ms(&self, now_ms: u64) -> Option<u64> {
        (!self.lines.is_empty() && !self.queued).then(|| {
            self.last_paint
                .map_or(0, |t| t.saturating_add(16).saturating_sub(now_ms))
        })
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
    pub fn painted(&mut self, now_ms: u64) {
        self.last_paint = Some(now_ms);
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
