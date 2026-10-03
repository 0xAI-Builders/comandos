//! Owned Py311SreV1 contract, independent of RustPython's different MAGIC.
use super::syntax::*;
use super::{RegexBudget, RegexFailure, RegexGap, RegexPhase};
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
    syntax: &ValidatedSyntax<'_>,
    budget: &mut RegexBudget,
) -> Result<Vec<u32>, RegexFailure> {
    let p = &syntax.syntax;
    let mut stack = Vec::new();
    push(&mut stack, p.root, 65_536, budget)?;
    let mut words = Vec::new();
    while let Some(id) = stack.pop() {
        budget.charge(1, RegexPhase::Compile)?;
        let node = p.nodes.get(id).ok_or(RegexFailure::InternalProgram)?;
        match node.kind {
            Kind::Empty => {}
            Kind::Concat { start, len } => {
                for &child in p
                    .edges
                    .get(start..start + len)
                    .ok_or(RegexFailure::InternalProgram)?
                    .iter()
                    .rev()
                {
                    push(&mut stack, child, 65_536, budget)?;
                }
            }
            Kind::Group {
                child,
                capture: None,
                ..
            } => push(&mut stack, child, 65_536, budget)?,
            Kind::Literal(_) | Kind::Any | Kind::At(0 | 2 | 5 | 7)
                if node.flags & (IGNORECASE | MULTILINE | DOTALL) == 0 =>
            {
                let (opcode, operand) = match node.kind {
                    Kind::Literal(c) => (LITERAL, Some(c)),
                    Kind::Any => (ANY, None),
                    Kind::At(a) => (AT, Some(a)),
                    _ => unreachable!(),
                };
                push(&mut words, opcode, budget.words, budget)?;
                if let Some(operand) = operand {
                    push(&mut words, operand, budget.words, budget)?;
                }
            }
            _ => {
                return Err(RegexFailure::ScopeGap {
                    kind: RegexGap::MatcherFeature,
                    position: Some(node.span.start),
                });
            }
        }
    }
    push(&mut words, SUCCESS, budget.words, budget)?;
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
