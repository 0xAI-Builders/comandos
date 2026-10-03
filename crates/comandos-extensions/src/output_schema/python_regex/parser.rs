//! Conservative whole-pattern eligibility precedes eligible Python syntax errors.
use super::compiler::{ANY, AT, BEGINNING, BEGINNING_STRING, END, END_STRING, LITERAL};
use super::{RegexBudget, RegexFailure, RegexGap, RegexPhase, RegexText};

fn ascii_letter(c: u32) -> bool {
    (65..=90).contains(&c) || (97..=122).contains(&c)
}
fn reserved_escape(c: u32) -> bool {
    (48..=57).contains(&c) || b"dDsSwWbBxuUN".iter().any(|&x| u32::from(x) == c)
}
fn literal_escape(c: u32) -> Option<u32> {
    match c {
        97 => Some(7),
        102 => Some(12),
        110 => Some(10),
        114 => Some(13),
        116 => Some(9),
        118 => Some(11),
        _ if c < 128 && !ascii_letter(c) && !(48..=57).contains(&c) => Some(c),
        _ => None,
    }
}
pub(super) fn eligible(
    pattern: RegexText<'_>,
    budget: &mut RegexBudget,
) -> Result<(), RegexFailure> {
    let mut position = 0;
    let mut gap = None;
    while let Some(&c) = pattern.0.get(position) {
        budget.charge(1, RegexPhase::Compile)?;
        let start = position;
        position += 1;
        if c == 92 {
            if let Some(&escaped) = pattern.0.get(position) {
                budget.charge(1, RegexPhase::Compile)?;
                position += 1;
                if reserved_escape(escaped) || (escaped >= 128 && literal_escape(escaped).is_none())
                {
                    gap.get_or_insert(start);
                }
            }
        } else if b"[]{}*+?|()".iter().any(|&x| u32::from(x) == c) {
            gap.get_or_insert(start);
        }
    }
    match gap {
        Some(position) => Err(RegexFailure::ScopeGap {
            kind: RegexGap::Grammar,
            position: Some(position),
        }),
        None => Ok(()),
    }
}

// Installed _parser._parse/_escape, only the admitted flat token grammar.
pub(super) fn token(
    points: &[u32],
    position: &mut usize,
    budget: &mut RegexBudget,
) -> Result<(u32, Option<u32>), RegexFailure> {
    let start = *position;
    let c = points[*position];
    *position += 1;
    budget.charge(1, RegexPhase::Compile)?;
    Ok(match c {
        46 => (ANY, None),
        94 => (AT, Some(BEGINNING)),
        36 => (AT, Some(END)),
        92 => {
            let &escaped = points.get(*position).ok_or(RegexFailure::ReError {
                position: Some(start),
            })?;
            *position += 1;
            budget.charge(1, RegexPhase::Compile)?;
            match escaped {
                65 => (AT, Some(BEGINNING_STRING)),
                90 => (AT, Some(END_STRING)),
                _ => (
                    LITERAL,
                    Some(literal_escape(escaped).ok_or(RegexFailure::ReError {
                        position: Some(start),
                    })?),
                ),
            }
        }
        _ => (LITERAL, Some(c)),
    })
}
