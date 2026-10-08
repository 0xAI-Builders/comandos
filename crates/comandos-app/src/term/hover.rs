//! Visible underline spans for a hovered terminal link.
pub fn lines(
    start: (i32, u16),
    end: (i32, u16),
    cols: u16,
    rows: u16,
    offset: i32,
) -> Vec<(u16, u16, u16)> {
    let Some(last_col) = cols.checked_sub(1) else {
        return Vec::new();
    };
    (0..rows)
        .filter_map(|row| {
            let absolute = i32::from(row).saturating_sub(offset);
            if absolute < start.0 || absolute > end.0 {
                return None;
            }
            let left = if absolute == start.0 { start.1 } else { 0 };
            let right = if absolute == end.0 {
                end.1.min(last_col)
            } else {
                last_col
            };
            (left <= right).then_some((row, left, right.saturating_sub(left).saturating_add(1)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::lines;
    #[test]
    fn underline_tracks_wrapped_rows_and_scrollback_without_padding() {
        assert_eq!(lines((0, 4), (1, 8), 20, 4, 0), vec![(0, 4, 16), (1, 0, 9)]);
        assert_eq!(
            lines((-1, 4), (0, 8), 20, 4, 1),
            vec![(0, 4, 16), (1, 0, 9)]
        );
        assert_eq!(lines((-2, 4), (-1, 8), 20, 4, 0), vec![]);
    }
}
