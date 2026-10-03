//! Unicode14 development capture format v1.
//! Primitive records: cp/lower/upper LE u32, flags LE u16, decimal u8
//! (255 = absent), reserved zero u8. Flags: decimal, alnum, word, space,
//! linebreak, regex-iscased, XID-start, XID-continue, identifier-start,
//! identifier-continue in bits 0..9 respectively.
//! Canonical records: cp LE u32, name length LE u16 (65535 = missing),
//! followed by ASCII bytes. Both inventories cover every cp in order.
//! JSONL lookup rows preserve ASCII/scalar input strings or u32 vectors for
//! surrogate-containing inputs, and both raw lookup/re.compile outcomes.
pub const DOMAIN: u32 = 0x110000;
pub const WIDTH: u32 = 1024;
pub const FORMAT: u32 = 1;
pub fn rows_valid(rows: &[(u32, u32)]) -> Result<(), String> {
    if rows.len() > 8 {
        return Err("more than eight rows".into());
    }
    for (i, &(start, end)) in rows.iter().enumerate() {
        if start >= end || end > DOMAIN || end - start > WIDTH {
            return Err("invalid range".into());
        }
        if i > 0 && rows[i - 1].1 != start {
            return Err("duplicate/gap/reordered ranges".into());
        }
    }
    Ok(())
}
pub fn exact(want: &[u8], got: &[u8]) -> Result<(), String> {
    if want.len() != got.len() {
        return Err(format!(
            "length mismatch expected {} actual {}",
            want.len(),
            got.len()
        ));
    }
    if let Some(i) = want.iter().zip(got).position(|(a, b)| a != b) {
        return Err(format!(
            "first mismatch byte {i}: expected {} actual {}",
            want[i], got[i]
        ));
    }
    Ok(())
}
pub fn packed_rows(
    value: &serde_json::Value,
    rows: &[(u32, u32)],
    op: &str,
) -> Result<Vec<u8>, String> {
    use base64::Engine;
    rows_valid(rows)?;
    let items = value.as_array().ok_or("not a row array")?;
    if items.len() != rows.len() {
        return Err("row count".into());
    }
    let mut out = Vec::new();
    for (item, &(start, end)) in items.iter().zip(rows) {
        if item["v"] != FORMAT
            || item["op"] != op
            || item["start"] != start
            || item["end"] != end
            || item["count"] != end - start
        {
            return Err("row identity/count/order".into());
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(item["data"].as_str().ok_or("no packed bytes")?)
            .map_err(|e| e.to_string())?;
        if op == "primitive" {
            if bytes.len() != (end - start) as usize * 16 {
                return Err("primitive byte count".into());
            }
            for (i, r) in bytes.chunks_exact(16).enumerate() {
                let flags = u16::from_le_bytes([r[12], r[13]]);
                if u32::from_le_bytes(r[..4].try_into().unwrap()) != start + i as u32
                    || r[15] != 0
                    || flags > 1023
                    || (r[14] > 9 && r[14] != 255)
                    || (flags & 1 != 0) != (r[14] != 255)
                    || u32::from_le_bytes(r[4..8].try_into().unwrap()) >= DOMAIN
                    || u32::from_le_bytes(r[8..12].try_into().unwrap()) >= DOMAIN
                {
                    return Err("primitive record".into());
                }
            }
        } else if op == "canonical" {
            let mut p = 0;
            for cp in start..end {
                let head = bytes.get(p..p + 6).ok_or("truncated canonical")?;
                if u32::from_le_bytes(head[..4].try_into().unwrap()) != cp {
                    return Err("canonical cp order".into());
                }
                let n = u16::from_le_bytes([head[4], head[5]]);
                p += 6;
                if n != 65535 {
                    if n == 0
                        || n > 256
                        || !bytes
                            .get(p..p + n as usize)
                            .ok_or("name truncated")?
                            .is_ascii()
                    {
                        return Err("canonical name".into());
                    }
                    p += n as usize;
                }
            }
            if p != bytes.len() {
                return Err("canonical trailing bytes".into());
            }
        } else {
            return Err("unknown op".into());
        }
        out.extend(bytes);
    }
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_nine_rows() {
        assert!(rows_valid(&[(0, 1); 9]).is_err());
    }
    #[test]
    fn rejects_reversed_and_outside_ranges() {
        assert!(rows_valid(&[(2, 1)]).is_err());
        assert!(rows_valid(&[(DOMAIN, DOMAIN + 1)]).is_err());
    }
    #[test]
    fn forced_mismatch_is_an_error() {
        assert!(exact(&[0, 1, 2], &[0, 1, 3]).is_err());
        assert!(exact(&[0, 1, 2], &[0, 1]).is_err());
    }
    fn row(start: u32, end: u32) -> serde_json::Value {
        use base64::Engine;
        let mut data = Vec::new();
        for cp in start..end {
            let mut r = [0u8; 16];
            r[..4].copy_from_slice(&cp.to_le_bytes());
            r[14] = 255;
            data.extend(r);
        }
        serde_json::json!({"v":FORMAT,"op":"primitive","start":start,"end":end,"count":end-start,"data":base64::engine::general_purpose::STANDARD.encode(data)})
    }
    #[test]
    fn zero_one_eight_and_nine() {
        assert!(rows_valid(&[]).is_ok());
        assert!(
            packed_rows(&serde_json::json!([]), &[], "primitive")
                .unwrap()
                .is_empty()
        );
        assert!(packed_rows(&serde_json::json!([row(0, 1)]), &[(0, 1)], "primitive").is_ok());
        let rows: Vec<_> = (0..8).map(|i| (i, i + 1)).collect();
        let output: Vec<_> = rows.iter().map(|&(a, b)| row(a, b)).collect();
        assert_eq!(
            packed_rows(&serde_json::json!(output), &rows, "primitive")
                .unwrap()
                .len(),
            128
        );
        let rows: Vec<_> = (0..9).map(|i| (i, i + 1)).collect();
        assert!(rows_valid(&rows).is_err());
        assert!(rows_valid(&[(0, WIDTH + 1)]).is_err());
        assert!(rows_valid(&[(0, 1), (0, 1)]).is_err());
        assert!(rows_valid(&[(0, 1), (2, 3)]).is_err());
    }
    #[test]
    fn missing_duplicate_reordered_wrong_count_and_packed_faults() {
        let rows = [(0, 1), (1, 2)];
        let good = serde_json::json!([row(0, 1), row(1, 2)]);
        for bad in [
            serde_json::json!([row(0, 1)]),
            serde_json::json!([row(0, 1), row(0, 1)]),
            serde_json::json!([row(1, 2), row(0, 1)]),
        ] {
            assert!(packed_rows(&bad, &rows, "primitive").is_err());
        }
        for field in ["count", "start", "end", "v"] {
            let mut bad = good.clone();
            bad[0][field] = serde_json::json!(99);
            assert!(packed_rows(&bad, &rows, "primitive").is_err());
        }
        for data in ["?", "AA==", ""] {
            let mut bad = good.clone();
            bad[0]["data"] = serde_json::json!(data);
            assert!(packed_rows(&bad, &rows, "primitive").is_err());
        }
        let mut bad = good;
        bad[0]["op"] = serde_json::json!("canonical");
        assert!(packed_rows(&bad, &rows, "primitive").is_err());
    }
}
