//! Shared Python byte-JSON decoding at native compatibility boundaries.
use super::workspace_loads;
use serde_json::Value;

/// Python json.loads(bytes) detects UTF-8 BOM and BOM-less UTF-16/32 via null
/// positions. String-based credentials use their separate strict UTF-8 reader.
pub fn workspace_loads_bytes(raw: &[u8]) -> Option<Value> {
    fn wide(bytes: &[u8], width: usize, little: bool) -> Option<String> {
        if !bytes.len().is_multiple_of(width) {
            return None;
        }
        if width == 2 {
            let units = bytes.chunks_exact(2).map(|b| {
                if little {
                    u16::from_le_bytes([b[0], b[1]])
                } else {
                    u16::from_be_bytes([b[0], b[1]])
                }
            });
            char::decode_utf16(units)
                .collect::<std::result::Result<String, _>>()
                .ok()
        } else {
            bytes
                .chunks_exact(4)
                .map(|b| {
                    char::from_u32(if little {
                        u32::from_le_bytes([b[0], b[1], b[2], b[3]])
                    } else {
                        u32::from_be_bytes([b[0], b[1], b[2], b[3]])
                    })
                })
                .collect()
        }
    }
    let text = if let Some(b) = raw.strip_prefix(&[0xff, 0xfe, 0, 0]) {
        wide(b, 4, true)?
    } else if let Some(b) = raw.strip_prefix(&[0, 0, 0xfe, 0xff]) {
        wide(b, 4, false)?
    } else if let Some(b) = raw.strip_prefix(&[0xff, 0xfe]) {
        wide(b, 2, true)?
    } else if let Some(b) = raw.strip_prefix(&[0xfe, 0xff]) {
        wide(b, 2, false)?
    } else if let Some(b) = raw.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        std::str::from_utf8(b).ok()?.into()
    } else if raw.len() >= 4 && raw[..3] == [0, 0, 0] {
        wide(raw, 4, false)?
    } else if raw.len() >= 4 && raw[1..4] == [0, 0, 0] {
        wide(raw, 4, true)?
    } else if raw.len() >= 2 && raw[0] == 0 {
        wide(raw, 2, false)?
    } else if raw.len() >= 2 && raw[1] == 0 {
        wide(raw, 2, true)?
    } else {
        std::str::from_utf8(raw).ok()?.into()
    };
    workspace_loads(&text).ok()
}
