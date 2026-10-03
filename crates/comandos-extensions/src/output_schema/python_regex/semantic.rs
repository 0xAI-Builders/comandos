//! CPython parser decimal conversions and compiler semantic traversal.
use super::syntax::*;
use super::{CompileAbort, RegexBudget, RegexFailure, RegexPhase, unicode14};
pub(super) fn decimal(
    points: &[u32],
    conditional: bool,
    b: &mut RegexBudget,
) -> Result<(u32, bool), RegexFailure> {
    let invalid = || RegexFailure::CompileAbort(CompileAbort::ValueError);
    let mut i = 0;
    let whitespace = |c: u32| {
        matches!(c, 9..=13 | 32)
            || (c >= 128 && unicode14::is_space(unicode14::CodePoint::new(c).unwrap()))
    };
    if conditional {
        while i < points.len() && whitespace(points[i]) {
            b.charge(1, RegexPhase::Compile)?;
            i += 1;
        }
    }
    let mut negative = false;
    if conditional && points.get(i).is_some_and(|c| *c == 43 || *c == 45) {
        negative = points[i] == 45;
        i += 1;
    }
    let mut value = 0u32;
    let mut count = 0;
    let mut previous = false;
    let mut ascii_decimal = true;
    while let Some(&c) = points.get(i) {
        b.charge(1, RegexPhase::Compile)?;
        let digit = if conditional {
            unicode14::decimal(unicode14::CodePoint::new(c).unwrap())
        } else {
            if (48..=57).contains(&c) {
                Some((c - 48) as u8)
            } else {
                None
            }
        };
        if let Some(d) = digit {
            ascii_decimal &= c < 128;
            count += 1;
            previous = true;
            value = value.saturating_mul(10).saturating_add(u32::from(d));
            i += 1;
        } else if conditional && c == 95 && previous {
            previous = false;
            ascii_decimal = false;
            i += 1;
        } else {
            break;
        }
    }
    if count == 0 || !previous {
        return Err(invalid());
    }
    if conditional {
        while i < points.len() && whitespace(points[i]) {
            b.charge(1, RegexPhase::Compile)?;
            i += 1;
        }
    }
    if i != points.len() || count > 4300 || negative && value != 0 {
        return Err(invalid());
    }
    for &c in points {
        b.charge(1, RegexPhase::Compile)?;
        ascii_decimal &= (48..=57).contains(&c);
    }
    Ok((value, ascii_decimal))
}
pub(super) fn validate<'p>(
    parsed: ParsedSyntax<'p>,
    b: &mut RegexBudget,
) -> Result<ValidatedSyntax<'p>, RegexFailure> {
    let mut stack = Vec::new();
    push(&mut stack, parsed.root, 65_536, b)?;
    while let Some(id) = stack.pop() {
        b.charge(1, RegexPhase::Compile)?;
        let n = parsed.nodes.get(id).ok_or(RegexFailure::InternalProgram)?;
        match n.kind {
            Kind::Repeat { child, .. } => {
                if n.flags & TEMPLATE != 0 {
                    return Err(RegexFailure::ReError { position: None });
                }
                push(&mut stack, child, 65_536, b)?;
            }
            Kind::Assert { child, behind, .. } => {
                let w = parsed.nodes[child].width;
                if behind && (w.lo > u128::from(u32::MAX) || w.lo != w.hi) {
                    return Err(RegexFailure::ReError { position: None });
                }
                push(&mut stack, child, 65_536, b)?;
            }
            Kind::Group { child, .. } | Kind::Atomic(child) => push(&mut stack, child, 65_536, b)?,
            Kind::Conditional { yes, no, .. } => {
                if let Some(no) = no {
                    push(&mut stack, no, 65_536, b)?;
                }
                push(&mut stack, yes, 65_536, b)?;
            }
            Kind::Concat { start, len } | Kind::Alternate { start, len } => {
                let edges = parsed
                    .edges
                    .get(start..start + len)
                    .ok_or(RegexFailure::InternalProgram)?;
                for &child in edges.iter().rev() {
                    push(&mut stack, child, 65_536, b)?;
                }
            }
            _ => {}
        }
    }
    Ok(ValidatedSyntax { syntax: parsed })
}
