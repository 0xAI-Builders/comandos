//! Owned Py311SreV1 contract, independent of RustPython's different MAGIC.
use super::{RegexBudget, RegexFailure, RegexPhase, RegexText, parser};
// CPython 3.11.15 _constants.py opcode/anchor discriminants.
pub(super) const SUCCESS: u32 = 1;
pub(super) const ANY: u32 = 2;
pub(super) const AT: u32 = 6;
pub(super) const LITERAL: u32 = 16;
pub(super) const BEGINNING: u32 = 0;
pub(super) const BEGINNING_STRING: u32 = 2;
pub(super) const END: u32 = 5;
pub(super) const END_STRING: u32 = 7;
const _COMPATIBILITY: &str = "Py311SreV1";
const _CPYTHON_MAGIC: u32 = 20220615;
const _CPYTHON_MAXREPEAT: u32 = u32::MAX;
const _CPYTHON_MAXGROUPS: u32 = (i32::MAX as u32) / 2;

pub(super) fn emit(
    pattern: RegexText<'_>,
    budget: &mut RegexBudget,
) -> Result<Vec<u32>, RegexFailure> {
    let cap = pattern
        .0
        .len()
        .checked_mul(2)
        .and_then(|n| n.checked_add(1))
        .filter(|&n| n <= budget.words)
        .ok_or(RegexFailure::BudgetBoundary {
            phase: RegexPhase::Compile,
        })?;
    budget.allocation(cap, RegexPhase::Compile)?;
    budget.charge(cap, RegexPhase::Compile)?;
    let mut words = vec![0; cap];
    let mut used = 0;
    let mut position = 0;
    while position < pattern.0.len() {
        let (opcode, operand) = parser::token(pattern.0, &mut position, budget)?;
        words[used] = opcode;
        used += 1;
        if let Some(value) = operand {
            words[used] = value;
            used += 1;
        }
    }
    words[used] = SUCCESS;
    used += 1;
    words.truncate(used);
    Ok(words)
}

pub(super) fn validate(words: &[u32], budget: &mut RegexBudget) -> Result<(), RegexFailure> {
    if words.len() > budget.words {
        return Err(RegexFailure::BudgetBoundary {
            phase: RegexPhase::Compile,
        });
    }
    let mut position = 0usize;
    loop {
        budget.charge(1, RegexPhase::Compile)?;
        let opcode = *words.get(position).ok_or(RegexFailure::InternalProgram)?;
        position = position
            .checked_add(1)
            .ok_or(RegexFailure::InternalProgram)?;
        match opcode {
            SUCCESS if position == words.len() => return Ok(()),
            ANY => {}
            LITERAL | AT => {
                budget.charge(1, RegexPhase::Compile)?;
                let value = *words.get(position).ok_or(RegexFailure::InternalProgram)?;
                if (opcode == LITERAL && value > 0x10ffff)
                    || (opcode == AT
                        && ![BEGINNING, BEGINNING_STRING, END, END_STRING].contains(&value))
                {
                    return Err(RegexFailure::InternalProgram);
                }
                position = position
                    .checked_add(1)
                    .ok_or(RegexFailure::InternalProgram)?;
            }
            _ => return Err(RegexFailure::InternalProgram),
        }
    }
}
