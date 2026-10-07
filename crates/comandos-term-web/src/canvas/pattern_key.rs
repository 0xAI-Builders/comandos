//! wasm32 mask identity and RGB fit losslessly in one fixed hash word.
#[cfg(target_arch = "wasm32")]
pub(super) type Key = u64;
#[cfg(not(target_arch = "wasm32"))]
pub(super) type Key = (usize, [u8; 3]);
pub(super) fn key(mask: usize, color: [u8; 3]) -> Key {
    #[cfg(target_arch = "wasm32")]
    {
        ((mask as u64) << 24)
            | (u64::from(color[0]) << 16)
            | (u64::from(color[1]) << 8)
            | u64::from(color[2])
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        (mask, color)
    }
}
