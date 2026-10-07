//! Direct finite custom-glyph slots; probes never allocate or scan.
#[derive(Default)]
pub(super) struct BoxIndex<T> {
    slots: Vec<Option<T>>,
    absent: T,
}
fn slot(c: char) -> Option<usize> {
    match u32::from(c) {
        0x2500..=0x259f => Some(c as usize - 0x2500),
        0xe0b0..=0xe0bf => Some(160 + c as usize - 0xe0b0),
        0x1fb70..=0x1fb97 => Some(176 + c as usize - 0x1fb70),
        _ => None,
    }
}
impl<T> BoxIndex<T> {
    pub(super) fn entry(&mut self, c: char, create: impl FnOnce() -> T) -> &T {
        let Some(index) = slot(c) else {
            return &self.absent;
        };
        if index >= self.slots.len() {
            self.slots.resize_with(index + 1, || None);
        }
        self.slots
            .get_mut(index)
            .expect("finite custom glyph slot")
            .get_or_insert_with(create)
    }
}
