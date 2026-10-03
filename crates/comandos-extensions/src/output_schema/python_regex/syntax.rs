//! Flat syntax arenas. No recursive ownership, cloning or dropping.
use super::{RegexBudget, RegexFailure, RegexPhase, RegexText};
pub(super) const TEMPLATE: u32 = 1;
pub(super) const IGNORECASE: u32 = 2;
pub(super) const MULTILINE: u32 = 8;
pub(super) const DOTALL: u32 = 16;
pub(super) const UNICODE: u32 = 32;
pub(super) const VERBOSE: u32 = 64;
pub(super) const ASCII: u32 = 256;
pub(super) const TYPES: u32 = ASCII | UNICODE | 4;
pub(super) const MAXWIDTH: u128 = 1u128 << 64;
pub(super) const MAXGROUPS: u32 = 1_073_741_823;
pub(super) type Id = usize;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Span {
    pub start: usize,
    pub end: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Width {
    pub lo: u128,
    pub hi: u128,
}
impl Width {
    pub const ZERO: Self = Self { lo: 0, hi: 0 };
    pub const UNIT: Self = Self { lo: 1, hi: 1 };
    pub fn plus(self, b: Self) -> Self {
        Self {
            lo: (self.lo + b.lo).min(MAXWIDTH),
            hi: (self.hi + b.hi).min(MAXWIDTH),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Empty,
    Literal(u32),
    NotLiteral(u32),
    Any,
    At(u32),
    Category(u32),
    Class {
        start: usize,
        len: usize,
        negate: bool,
    },
    Concat {
        start: usize,
        len: usize,
    },
    Alternate {
        start: usize,
        len: usize,
    },
    Group {
        child: Id,
        capture: Option<u32>,
        add: u32,
        delete: u32,
    },
    Repeat {
        child: Id,
        min: u32,
        max: u32,
        mode: u8,
    },
    Assert {
        child: Id,
        behind: bool,
        positive: bool,
    },
    Atomic(Id),
    Conditional {
        group: u32,
        yes: Id,
        no: Option<Id>,
    },
    Backreference(u32),
}
#[derive(Clone, Copy, Debug)]
pub(super) struct Node {
    pub kind: Kind,
    pub span: Span,
    pub flags: u32,
    pub width: Width,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ClassEntry {
    Literal(u32),
    Range(u32, u32),
    Category(u32),
}
#[derive(Clone, Copy, Debug)]
pub(super) struct Group {
    pub name: Option<Span>,
    pub width: Option<Width>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WarningCategory {
    FutureWarning,
    DeprecationWarning,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct Warning {
    pub category: WarningCategory,
    pub position: Option<usize>,
}
#[derive(Default)]
pub(super) struct WarningSink {
    pub observations: Vec<Warning>,
}
impl WarningSink {
    pub fn push(
        &mut self,
        category: WarningCategory,
        position: usize,
        b: &mut RegexBudget,
    ) -> Result<(), RegexFailure> {
        push(
            &mut self.observations,
            Warning {
                category,
                position: Some(position),
            },
            16_384,
            b,
        )
    }
}
pub(super) fn boundary() -> RegexFailure {
    RegexFailure::BudgetBoundary {
        phase: RegexPhase::Compile,
    }
}
pub(super) fn push<T>(
    v: &mut Vec<T>,
    item: T,
    ceiling: usize,
    b: &mut RegexBudget,
) -> Result<(), RegexFailure> {
    b.charge(1, RegexPhase::Compile)?;
    if v.len() >= ceiling {
        return Err(boundary());
    }
    if v.len() == v.capacity() {
        let cap = (v.capacity().max(1) * 2).min(ceiling);
        b.charge(v.len(), RegexPhase::Compile)?;
        b.bytes(
            cap.checked_mul(size_of::<T>()).ok_or_else(boundary)?,
            RegexPhase::Compile,
        )?;
        v.try_reserve_exact(cap - v.len()).map_err(|_| boundary())?;
        if v.capacity() > cap {
            return Err(RegexFailure::InternalProgram);
        }
    }
    v.push(item);
    Ok(())
}
pub(super) struct ParsedSyntax<'p> {
    pub pattern: RegexText<'p>,
    pub nodes: Vec<Node>,
    pub edges: Vec<Id>,
    pub classes: Vec<ClassEntry>,
    pub groups: Vec<Group>,
    pub root: Id,
    pub flags: u32,
    pub peak_frames: usize,
    pub frame_capacity_bytes: usize,
}
pub(super) struct ValidatedSyntax<'p> {
    pub syntax: ParsedSyntax<'p>,
}
impl<'p> ParsedSyntax<'p> {
    pub fn new(pattern: RegexText<'p>) -> Self {
        Self {
            pattern,
            nodes: Vec::new(),
            edges: Vec::new(),
            classes: Vec::new(),
            groups: Vec::new(),
            root: 0,
            flags: 0,
            peak_frames: 0,
            frame_capacity_bytes: 0,
        }
    }
    pub fn node(
        &mut self,
        kind: Kind,
        span: Span,
        flags: u32,
        width: Width,
        b: &mut RegexBudget,
    ) -> Result<Id, RegexFailure> {
        let id = self.nodes.len();
        push(
            &mut self.nodes,
            Node {
                kind,
                span,
                flags,
                width,
            },
            65_536,
            b,
        )?;
        Ok(id)
    }
    pub fn list(
        &mut self,
        ids: &[Id],
        alternate: bool,
        span: Span,
        flags: u32,
        b: &mut RegexBudget,
    ) -> Result<Id, RegexFailure> {
        if ids.is_empty() {
            return self.node(Kind::Empty, span, flags, Width::ZERO, b);
        }
        if ids.len() == 1 {
            return Ok(ids[0]);
        }
        let start = self.edges.len();
        let mut width = if alternate {
            Width {
                lo: MAXWIDTH,
                hi: 0,
            }
        } else {
            Width::ZERO
        };
        for &id in ids {
            b.charge(1, RegexPhase::Compile)?;
            let w = self
                .nodes
                .get(id)
                .ok_or(RegexFailure::InternalProgram)?
                .width;
            width = if alternate {
                Width {
                    lo: width.lo.min(w.lo),
                    hi: width.hi.max(w.hi),
                }
            } else {
                width.plus(w)
            };
            push(&mut self.edges, id, 65_536, b)?;
        }
        self.node(
            if alternate {
                Kind::Alternate {
                    start,
                    len: ids.len(),
                }
            } else {
                Kind::Concat {
                    start,
                    len: ids.len(),
                }
            },
            span,
            flags,
            width,
            b,
        )
    }
}
