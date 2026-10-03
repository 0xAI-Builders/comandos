//! Selected RustPython 0.6.0 State::search/_match/at operations.
//! See PROVENANCE.json and preserved MIT/Secret Labs/CNRI licenses.
//! Safe u32 positions replace the generic unsafe byte-pointer driver.
use super::compiler::{ANY, AT, BEGINNING, BEGINNING_STRING, END, END_STRING, LITERAL, SUCCESS};
use super::{RegexBudget, RegexFailure, RegexPhase, RegexText};

pub(super) fn search(
    words: &[u32],
    text: RegexText<'_>,
    budget: &mut RegexBudget,
) -> Result<bool, RegexFailure> {
    // General source search path: initial candidate then every position through
    // the end, retaining zero-width matches. No INFO/prefix/charset optimization.
    for candidate in 0..=text.0.len() {
        budget.charge(1, RegexPhase::Search)?;
        if matches_at(words, text, candidate, budget)? {
            return Ok(true);
        }
        if matches!(words.first(), Some(&AT))
            && matches!(words.get(1), Some(&BEGINNING) | Some(&BEGINNING_STRING))
        {
            return Ok(false);
        }
    }
    Ok(false)
}
fn matches_at(
    words: &[u32],
    text: RegexText<'_>,
    mut cursor: usize,
    budget: &mut RegexBudget,
) -> Result<bool, RegexFailure> {
    let mut position = 0usize;
    loop {
        budget.charge(1, RegexPhase::Search)?;
        let opcode = *words.get(position).ok_or(RegexFailure::InternalProgram)?;
        position = position
            .checked_add(1)
            .ok_or(RegexFailure::InternalProgram)?;
        match opcode {
            SUCCESS if position == words.len() => return Ok(true),
            ANY => {
                budget.charge(1, RegexPhase::Search)?;
                if text.0.get(cursor).is_none_or(|&point| point == 10) {
                    return Ok(false);
                }
                cursor = cursor.checked_add(1).ok_or(RegexFailure::InternalProgram)?;
            }
            LITERAL | AT => {
                budget.charge(1, RegexPhase::Search)?;
                let value = *words.get(position).ok_or(RegexFailure::InternalProgram)?;
                position = position
                    .checked_add(1)
                    .ok_or(RegexFailure::InternalProgram)?;
                let matched = if opcode == LITERAL {
                    budget.charge(1, RegexPhase::Search)?;
                    if value > 0x10ffff {
                        return Err(RegexFailure::InternalProgram);
                    }
                    text.0.get(cursor) == Some(&value)
                } else {
                    match value {
                        BEGINNING | BEGINNING_STRING => cursor == 0,
                        END => {
                            cursor == text.0.len()
                                || (text.0.len().checked_sub(cursor) == Some(1)
                                    && text.0.get(cursor) == Some(&10))
                        }
                        END_STRING => cursor == text.0.len(),
                        _ => return Err(RegexFailure::InternalProgram),
                    }
                };
                if !matched {
                    return Ok(false);
                }
                if opcode == LITERAL {
                    cursor = cursor.checked_add(1).ok_or(RegexFailure::InternalProgram)?;
                }
            }
            _ => return Err(RegexFailure::InternalProgram),
        }
    }
}
