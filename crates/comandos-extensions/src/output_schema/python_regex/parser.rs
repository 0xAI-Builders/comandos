//! Bounded translation of CPython 3.11.15 re._parser string grammar.
//! Secret Labs AB 1997-2001 and 1998-2001. See retained notices/licenses.
use super::syntax::*;
use super::{CompileAbort, RegexBudget, RegexFailure, RegexPhase, RegexText, unicode14};
#[derive(Clone, Copy)]
struct Token {
    span: Span,
    c: u32,
    escape: bool,
}
struct Source<'p> {
    p: &'p [u32],
    next: Option<Token>,
    index: usize,
}
impl<'p> Source<'p> {
    fn new(p: &'p [u32], b: &mut RegexBudget) -> Result<Self, RegexFailure> {
        let mut s = Self {
            p,
            next: None,
            index: 0,
        };
        s.advance(b)?;
        Ok(s)
    }
    fn tell(&self) -> usize {
        self.next.map_or(self.index, |t| t.span.start)
    }
    fn error(&self, offset: usize) -> RegexFailure {
        RegexFailure::ReError {
            position: Some(self.tell().saturating_sub(offset)),
        }
    }
    fn advance(&mut self, b: &mut RegexBudget) -> Result<(), RegexFailure> {
        b.charge(1, RegexPhase::Compile)?;
        let start = self.index;
        self.next = match self.p.get(start) {
            None => None,
            Some(&c) => {
                self.index += 1;
                let escape = c == 92;
                let c = if escape {
                    let &c = self.p.get(self.index).ok_or(RegexFailure::ReError {
                        position: Some(start),
                    })?;
                    self.index += 1;
                    c
                } else {
                    c
                };
                Some(Token {
                    span: Span {
                        start,
                        end: self.index,
                    },
                    c,
                    escape,
                })
            }
        };
        Ok(())
    }
    fn get(&mut self, b: &mut RegexBudget) -> Result<Option<Token>, RegexFailure> {
        let t = self.next;
        self.advance(b)?;
        Ok(t)
    }
    fn is(&self, c: u32) -> bool {
        self.next.is_some_and(|t| !t.escape && t.c == c)
    }
    fn take(&mut self, c: u32, b: &mut RegexBudget) -> Result<bool, RegexFailure> {
        if self.is(c) {
            self.advance(b)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
    fn until(&mut self, end: u32, b: &mut RegexBudget) -> Result<Span, RegexFailure> {
        let start = self.tell();
        loop {
            let t = self.get(b)?;
            match t {
                None => return Err(self.error(self.tell() - start)),
                Some(t) if !t.escape && t.c == end => {
                    if t.span.start == start {
                        return Err(self.error(1));
                    }
                    return Ok(Span {
                        start,
                        end: t.span.start,
                    });
                }
                _ => {}
            }
        }
    }
    fn seek(&mut self, index: usize, b: &mut RegexBudget) -> Result<(), RegexFailure> {
        b.charge(self.tell().abs_diff(index), RegexPhase::Compile)?;
        self.index = index;
        self.advance(b)
    }
}
#[derive(Clone, Copy)]
enum Shell {
    Root,
    Group {
        capture: Option<u32>,
        add: u32,
        delete: u32,
    },
    Assert {
        behind: bool,
        positive: bool,
    },
    Atomic,
    Conditional(u32),
}
struct Frame {
    items: Vec<Id>,
    branches: Vec<Id>,
    shell: Shell,
    start: usize,
    flags: u32,
    old_behind: Option<usize>,
}
impl Frame {
    fn new(shell: Shell, start: usize, flags: u32, old_behind: Option<usize>) -> Self {
        Self {
            items: Vec::new(),
            branches: Vec::new(),
            shell,
            start,
            flags,
            old_behind,
        }
    }
}
struct Parser<'p, 'b, 'w> {
    source: Source<'p>,
    arena: ParsedSyntax<'p>,
    budget: &'b mut RegexBudget,
    warnings: &'w mut WarningSink,
    frames: Vec<Frame>,
    behind: Option<usize>,
    deferred: Vec<(u32, usize)>,
}
fn flag(c: u32) -> Option<u32> {
    match c {
        105 => Some(IGNORECASE),
        109 => Some(MULTILINE),
        115 => Some(DOTALL),
        120 => Some(VERBOSE),
        97 => Some(ASCII),
        117 => Some(UNICODE),
        76 => Some(4),
        116 => Some(TEMPLATE),
        _ => None,
    }
}
fn ascii_digit(t: Token) -> bool {
    !t.escape && (48..=57).contains(&t.c)
}
fn octal(t: Token) -> bool {
    !t.escape && (48..=55).contains(&t.c)
}
fn literal_escape(c: u32) -> Option<u32> {
    match c {
        97 => Some(7),
        98 => Some(8),
        102 => Some(12),
        110 => Some(10),
        114 => Some(13),
        116 => Some(9),
        118 => Some(11),
        _ => None,
    }
}
impl<'p, 'b, 'w> Parser<'p, 'b, 'w> {
    fn identifier(&mut self, span: Span) -> Result<bool, RegexFailure> {
        let points = &self.source.p[span.start..span.end];
        if points.is_empty() {
            return Ok(false);
        }
        for (i, &c) in points.iter().enumerate() {
            self.budget.charge(1, RegexPhase::Compile)?;
            let cp = unicode14::CodePoint::new(c).ok_or(RegexFailure::InternalProgram)?;
            if !(if i == 0 {
                unicode14::identifier_start(cp)
            } else {
                unicode14::identifier_continue(cp)
            }) {
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn name_group(&mut self, name: Span) -> Result<Option<u32>, RegexFailure> {
        for (id, g) in self.arena.groups.iter().enumerate().skip(1) {
            if let Some(old) = g.name {
                let a = &self.source.p[name.start..name.end];
                let z = &self.source.p[old.start..old.end];
                self.budget.charge(1, RegexPhase::Compile)?;
                if a.len() != z.len() {
                    continue;
                }
                let mut equal = true;
                for (x, y) in a.iter().zip(z) {
                    self.budget.charge(1, RegexPhase::Compile)?;
                    if x != y {
                        equal = false;
                        break;
                    }
                }
                if equal {
                    return Ok(Some(id as u32));
                }
            }
        }
        Ok(None)
    }
    fn check_behind(&self, id: u32) -> Result<(), RegexFailure> {
        if let Some(boundary) = self.behind {
            let g = self.arena.groups.get(id as usize);
            if g.is_none_or(|g| g.width.is_none()) || id as usize >= boundary {
                return Err(self.source.error(0));
            }
        }
        Ok(())
    }
    fn reference(
        &mut self,
        id: u32,
        ordinary_offset: usize,
        invalid_offset: usize,
    ) -> Result<Width, RegexFailure> {
        let group = self
            .arena
            .groups
            .get(id as usize)
            .ok_or_else(|| self.source.error(invalid_offset))?;
        let w = group
            .width
            .ok_or_else(|| self.source.error(ordinary_offset))?;
        self.check_behind(id)?;
        Ok(w)
    }
    fn node(&mut self, kind: Kind, span: Span, width: Width) -> Result<Id, RegexFailure> {
        let flags = self
            .frames
            .last()
            .ok_or(RegexFailure::InternalProgram)?
            .flags;
        self.arena.node(kind, span, flags, width, self.budget)
    }
    fn append(&mut self, id: Id) -> Result<(), RegexFailure> {
        push(
            &mut self
                .frames
                .last_mut()
                .ok_or(RegexFailure::InternalProgram)?
                .items,
            id,
            65_536,
            self.budget,
        )
    }
    fn branch(&mut self) -> Result<(), RegexFailure> {
        let f = self
            .frames
            .last_mut()
            .ok_or(RegexFailure::InternalProgram)?;
        let span = Span {
            start: f.start,
            end: self.source.tell(),
        };
        let id = self
            .arena
            .list(&f.items, false, span, f.flags, self.budget)?;
        f.items.clear();
        push(&mut f.branches, id, 65_536, self.budget)
    }
    fn close(&mut self) -> Result<Id, RegexFailure> {
        self.branch()?;
        let f = self.frames.pop().ok_or(RegexFailure::InternalProgram)?;
        let span = Span {
            start: f.start,
            end: self.source.tell(),
        };
        let id = if let Shell::Conditional(group) = f.shell {
            let yes = f.branches[0];
            let no = f.branches.get(1).copied();
            let y = self.arena.nodes[yes].width;
            let width = if let Some(no) = no {
                let n = self.arena.nodes[no].width;
                Width {
                    lo: y.lo.min(n.lo),
                    hi: y.hi.max(n.hi),
                }
            } else {
                Width { lo: 0, hi: y.hi }
            };
            self.arena.node(
                Kind::Conditional { group, yes, no },
                span,
                f.flags,
                width,
                self.budget,
            )?
        } else {
            let child = self
                .arena
                .list(&f.branches, true, span, f.flags, self.budget)?;
            let width = self.arena.nodes[child].width;
            match f.shell {
                Shell::Root => child,
                Shell::Group {
                    capture,
                    add,
                    delete,
                } => {
                    if let Some(c) = capture {
                        self.arena.groups[c as usize].width = Some(width);
                    }
                    self.arena.node(
                        Kind::Group {
                            child,
                            capture,
                            add,
                            delete,
                        },
                        span,
                        f.flags,
                        width,
                        self.budget,
                    )?
                }
                Shell::Assert { behind, positive } => self.arena.node(
                    Kind::Assert {
                        child,
                        behind,
                        positive,
                    },
                    span,
                    f.flags,
                    Width::ZERO,
                    self.budget,
                )?,
                Shell::Atomic => {
                    self.arena
                        .node(Kind::Atomic(child), span, f.flags, width, self.budget)?
                }
                Shell::Conditional(_) => unreachable!(),
            }
        };
        self.behind = f.old_behind;
        Ok(id)
    }
    fn escaped(&mut self, t: Token, class: bool) -> Result<(Kind, Width), RegexFailure> {
        let c = t.c;
        let start = t.span.start;
        if !class
            && let Some(at) = match c {
                65 => Some(2),
                90 => Some(7),
                98 => Some(10),
                66 => Some(11),
                _ => None,
            }
        {
            return Ok((Kind::At(at), Width::ZERO));
        }
        if b"dDsSwW".iter().any(|x| u32::from(*x) == c) {
            return Ok((Kind::Category(c), Width::UNIT));
        }
        if let Some(v) = literal_escape(c) {
            return Ok((Kind::Literal(v), Width::UNIT));
        }
        if matches!(c, 120 | 117 | 85) {
            let n = match c {
                120 => 2,
                117 => 4,
                _ => 8,
            };
            let mut value = 0u32;
            for _ in 0..n {
                let Some(t) = self.source.next else {
                    return Err(self.source.error(self.source.tell() - start));
                };
                let digit = if t.escape {
                    None
                } else {
                    match t.c {
                        48..=57 => Some(t.c - 48),
                        65..=70 => Some(t.c - 55),
                        97..=102 => Some(t.c - 87),
                        _ => None,
                    }
                };
                let d = digit.ok_or_else(|| self.source.error(self.source.tell() - start))?;
                self.source.get(self.budget)?;
                value = value * 16 + d;
            }
            if value > 0x10ffff {
                return Err(self.source.error(self.source.tell() - start));
            }
            return Ok((Kind::Literal(value), Width::UNIT));
        }
        if c == 78 {
            if !self.source.take(123, self.budget)? {
                return Err(self.source.error(0));
            }
            let span = self.source.until(125, self.budget)?;
            let raw = &self.source.p[span.start..span.end];
            let mut name = Vec::new();
            self.budget.bytes(
                raw.len()
                    .checked_mul(size_of::<unicode14::CodePoint>())
                    .ok_or_else(boundary)?,
                RegexPhase::Compile,
            )?;
            name.try_reserve_exact(raw.len()).map_err(|_| boundary())?;
            self.budget.bytes(
                (name.capacity() - raw.len()) * size_of::<unicode14::CodePoint>(),
                RegexPhase::Compile,
            )?;
            for &cp in raw {
                self.budget.charge(1, RegexPhase::Compile)?;
                name.push(unicode14::CodePoint::new(cp).ok_or(RegexFailure::InternalProgram)?);
            }
            let b = &mut *self.budget;
            let result = unicode14::lookup_name(&name, &mut |n| {
                b.charge(n, RegexPhase::Compile)
                    .map_err(|_| unicode14::NameFailure::BudgetBoundary)
            });
            return match result {
                Ok(unicode14::NameValue::Character(c)) => {
                    Ok((Kind::Literal(c.value()), Width::UNIT))
                }
                Ok(unicode14::NameValue::Sequence { .. })
                | Err(unicode14::NameFailure::Missing) => {
                    Err(self.source.error(self.source.tell() - start))
                }
                Err(unicode14::NameFailure::UnicodeEncodeError) => Err(self.source.error(2)),
                Err(unicode14::NameFailure::BudgetBoundary) => Err(boundary()),
                Err(unicode14::NameFailure::InternalTable) => Err(RegexFailure::InternalProgram),
            };
        }
        if (48..=57).contains(&c) {
            let mut value = c - 48;
            if class || c == 48 {
                if c > 55 {
                    return Err(self.source.error(2));
                }
                for _ in 0..2 {
                    if let Some(n) = self.source.next.filter(|t| octal(*t)) {
                        self.source.get(self.budget)?;
                        value = value * 8 + n.c - 48;
                    } else {
                        break;
                    }
                }
                if value > 255 {
                    return Err(self.source.error(self.source.tell() - start));
                }
                return Ok((Kind::Literal(value), Width::UNIT));
            }
            let mut length = 2;
            if let Some(n) = self.source.next.filter(|t| ascii_digit(*t)) {
                self.source.get(self.budget)?;
                length += 1;
                value = value * 10 + n.c - 48;
                if c <= 55 && n.c <= 55 && self.source.next.is_some_and(octal) {
                    let last = self.source.get(self.budget)?.unwrap();
                    value = (c - 48) * 64 + (n.c - 48) * 8 + last.c - 48;
                    if value > 255 {
                        return Err(self.source.error(4));
                    }
                    return Ok((Kind::Literal(value), Width::UNIT));
                }
            }
            let w = self.reference(value, length, length - 1)?;
            return Ok((Kind::Backreference(value), w));
        }
        if c < 128 && ((65..=90).contains(&c) || (97..=122).contains(&c)) {
            return Err(self.source.error(2));
        }
        Ok((Kind::Literal(c), Width::UNIT))
    }
    fn class(&mut self, start: usize) -> Result<Id, RegexFailure> {
        if self.source.is(91) {
            self.warnings.push(
                WarningCategory::FutureWarning,
                self.source.tell(),
                self.budget,
            )?;
        }
        let negate = self.source.take(94, self.budget)?;
        let mut entries = Vec::new();
        loop {
            let t = self
                .source
                .get(self.budget)?
                .ok_or_else(|| self.source.error(self.source.tell() - start))?;
            if !t.escape && t.c == 93 && !entries.is_empty() {
                break;
            }
            let k = if t.escape {
                self.escaped(t, true)?.0
            } else {
                if !entries.is_empty() && matches!(t.c, 45 | 38 | 126 | 124) && self.source.is(t.c)
                {
                    self.warnings.push(
                        WarningCategory::FutureWarning,
                        self.source.tell() - 1,
                        self.budget,
                    )?;
                }
                Kind::Literal(t.c)
            };
            let first = match k {
                Kind::Literal(c) => ClassEntry::Literal(c),
                Kind::Category(c) => ClassEntry::Category(c),
                _ => return Err(RegexFailure::InternalProgram),
            };
            let entry = if self.source.take(45, self.budget)? {
                let that = self
                    .source
                    .get(self.budget)?
                    .ok_or_else(|| self.source.error(self.source.tell() - start))?;
                if !that.escape && that.c == 93 {
                    self.class_push(&mut entries, first)?;
                    self.class_push(&mut entries, ClassEntry::Literal(45))?;
                    break;
                }
                let other = if that.escape {
                    self.escaped(that, true)?.0
                } else {
                    if that.c == 45 {
                        self.warnings.push(
                            WarningCategory::FutureWarning,
                            self.source.tell() - 2,
                            self.budget,
                        )?;
                    }
                    Kind::Literal(that.c)
                };
                match (first, other) {
                    (ClassEntry::Literal(a), Kind::Literal(z)) if a <= z => ClassEntry::Range(a, z),
                    _ => {
                        return Err(self.source.error(
                            (t.span.end - t.span.start) + 1 + (that.span.end - that.span.start),
                        ));
                    }
                }
            } else {
                first
            };
            self.class_push(&mut entries, entry)?;
        }
        let span = Span {
            start,
            end: self.source.tell(),
        };
        if entries.len() == 1
            && let ClassEntry::Literal(c) = entries[0]
        {
            return self.node(
                if negate {
                    Kind::NotLiteral(c)
                } else {
                    Kind::Literal(c)
                },
                span,
                Width::UNIT,
            );
        }
        let offset = self.arena.classes.len();
        for entry in &entries {
            push(&mut self.arena.classes, *entry, 16_384, self.budget)?;
        }
        self.node(
            Kind::Class {
                start: offset,
                len: entries.len(),
                negate,
            },
            span,
            Width::UNIT,
        )
    }
    fn class_push(&mut self, v: &mut Vec<ClassEntry>, e: ClassEntry) -> Result<(), RegexFailure> {
        for old in v.iter() {
            self.budget.charge(1, RegexPhase::Compile)?;
            if *old == e {
                return Ok(());
            }
        }
        push(v, e, 16_384, self.budget)
    }
    fn flags(&mut self, first: Token) -> Result<Option<(u32, u32)>, RegexFailure> {
        let mut c = Some(first);
        let mut add = 0;
        let mut delete = 0;
        if first.c != 45 {
            loop {
                let t = c.ok_or_else(|| self.source.error(0))?;
                let f = flag(t.c).ok_or_else(|| self.source.error(t.span.end - t.span.start))?;
                if t.escape {
                    return Err(self.source.error(t.span.end - t.span.start));
                }
                if f == 4 {
                    return Err(self.source.error(0));
                }
                add |= f;
                if f & TYPES != 0 && add & TYPES != f {
                    return Err(self.source.error(0));
                }
                c = self.source.get(self.budget)?;
                let t = c.ok_or_else(|| self.source.error(0))?;
                if !t.escape && matches!(t.c, 41 | 45 | 58) {
                    break;
                }
                if t.escape || flag(t.c).is_none() {
                    return Err(self.source.error(t.span.end - t.span.start));
                }
            }
        }
        let mut t = c.unwrap();
        if t.c == 41 {
            self.arena.flags |= add;
            return Ok(None);
        }
        if add & TEMPLATE != 0 {
            return Err(self.source.error(1));
        }
        if t.c == 45 {
            t = self
                .source
                .get(self.budget)?
                .ok_or_else(|| self.source.error(0))?;
            loop {
                let f = if !t.escape { flag(t.c) } else { None }
                    .ok_or_else(|| self.source.error(t.span.end - t.span.start))?;
                if f & TYPES != 0 {
                    return Err(self.source.error(0));
                }
                delete |= f;
                t = self
                    .source
                    .get(self.budget)?
                    .ok_or_else(|| self.source.error(0))?;
                if !t.escape && t.c == 58 {
                    break;
                }
                if t.escape || flag(t.c).is_none() {
                    return Err(self.source.error(t.span.end - t.span.start));
                }
            }
        }
        if delete & TEMPLATE != 0 || add & delete != 0 {
            return Err(self.source.error(1));
        }
        Ok(Some((add, delete)))
    }
    fn open(&mut self, start: usize) -> Result<(), RegexFailure> {
        let mut shell = Shell::Group {
            capture: None,
            add: 0,
            delete: 0,
        };
        let mut capture = true;
        let mut name = None;
        let mut add = 0;
        let mut delete = 0;
        let old_behind = self.behind;
        if self.source.take(63, self.budget)? {
            let t = self
                .source
                .get(self.budget)?
                .ok_or_else(|| self.source.error(0))?;
            match (t.escape, t.c) {
                (false, 80) => {
                    if self.source.take(60, self.budget)? {
                        let n = self.source.until(62, self.budget)?;
                        if !self.identifier(n)? {
                            return Err(self.source.error(n.end - n.start + 1));
                        }
                        name = Some(n);
                    } else if self.source.take(61, self.budget)? {
                        let n = self.source.until(41, self.budget)?;
                        if !self.identifier(n)? {
                            return Err(self.source.error(n.end - n.start + 1));
                        }
                        let id = self
                            .name_group(n)?
                            .ok_or_else(|| self.source.error(n.end - n.start + 1))?;
                        let width = self.reference(id, n.end - n.start + 1, n.end - n.start + 1)?;
                        let id = self.node(
                            Kind::Backreference(id),
                            Span {
                                start,
                                end: self.source.tell(),
                            },
                            width,
                        )?;
                        self.append(id)?;
                        return Ok(());
                    } else {
                        let t = self
                            .source
                            .get(self.budget)?
                            .ok_or_else(|| self.source.error(0))?;
                        return Err(self.source.error(t.span.end - t.span.start + 2));
                    }
                }
                (false, 58) => capture = false,
                (false, 35) => {
                    loop {
                        let t = self
                            .source
                            .get(self.budget)?
                            .ok_or_else(|| self.source.error(self.source.tell() - start))?;
                        if !t.escape && t.c == 41 {
                            break;
                        }
                    }
                    return Ok(());
                }
                (false, 61 | 33 | 60) => {
                    capture = false;
                    let mut c = t.c;
                    let behind = c == 60;
                    if behind {
                        let t = self
                            .source
                            .get(self.budget)?
                            .ok_or_else(|| self.source.error(0))?;
                        c = t.c;
                        if t.escape || !matches!(c, 61 | 33) {
                            return Err(self.source.error(t.span.end - t.span.start + 2));
                        }
                        if self.behind.is_none() {
                            self.behind = Some(self.arena.groups.len());
                        }
                    }
                    shell = Shell::Assert {
                        behind,
                        positive: c == 61,
                    };
                }
                (false, 40) => {
                    capture = false;
                    let n = self.source.until(41, self.budget)?;
                    let group = if self.identifier(n)? {
                        self.name_group(n)?
                            .ok_or_else(|| self.source.error(n.end - n.start + 1))?
                    } else {
                        let (v, ascii) = super::semantic::decimal(
                            &self.source.p[n.start..n.end],
                            true,
                            self.budget,
                        )
                        .map_err(|e| {
                            if matches!(e, RegexFailure::CompileAbort(CompileAbort::ValueError)) {
                                self.source.error(n.end - n.start + 1)
                            } else {
                                e
                            }
                        })?;
                        if v == 0 || v >= MAXGROUPS {
                            return Err(self.source.error(n.end - n.start + 1));
                        }
                        self.budget
                            .charge(self.deferred.len(), RegexPhase::Compile)?;
                        if !self.deferred.iter().any(|(g, _)| *g == v) {
                            push(&mut self.deferred, (v, n.start), 16_384, self.budget)?;
                        }
                        if !ascii {
                            self.warnings.push(
                                WarningCategory::DeprecationWarning,
                                n.start,
                                self.budget,
                            )?;
                        }
                        v
                    };
                    self.check_behind(group)?;
                    shell = Shell::Conditional(group);
                }
                (false, 62) => {
                    capture = false;
                    shell = Shell::Atomic;
                }
                (false, c) if flag(c).is_some() || c == 45 => {
                    let flags = self.flags(t)?;
                    if let Some((a, d)) = flags {
                        capture = false;
                        add = a;
                        delete = d;
                    } else {
                        let root = self.frames.len() == 1;
                        let f = self.frames.last_mut().unwrap();
                        if !root || !f.items.is_empty() || !f.branches.is_empty() {
                            return Err(self.source.error(self.source.tell() - start));
                        }
                        f.flags = self.arena.flags;
                        return Ok(());
                    }
                }
                _ => return Err(self.source.error(t.span.end - t.span.start + 1)),
            }
        }
        if capture {
            let id = self.arena.groups.len() as u32;
            push(
                &mut self.arena.groups,
                Group { name, width: None },
                16_384,
                self.budget,
            )?;
            if self.arena.groups.len() > MAXGROUPS as usize {
                return Err(RegexFailure::InternalProgram);
            }
            if let Some(n) = name
                && self.name_group(n)?.is_some_and(|g| g != id)
            {
                return Err(self.source.error(n.end - n.start + 1));
            }
            shell = Shell::Group {
                capture: Some(id),
                add: 0,
                delete: 0,
            };
        } else if matches!(shell, Shell::Group { .. }) {
            shell = Shell::Group {
                capture: None,
                add,
                delete,
            };
        }
        let parent = self.frames.last().unwrap().flags;
        let flags = (if add & TYPES != 0 {
            parent & !TYPES
        } else {
            parent
        } | add)
            & !delete;
        if self.frames.len() > 128 {
            return Err(boundary());
        }
        push(
            &mut self.frames,
            Frame::new(shell, start, flags, old_behind),
            129,
            self.budget,
        )?;
        self.arena.peak_frames = self.arena.peak_frames.max(self.frames.len() - 1);
        Ok(())
    }
    fn repeat(&mut self, t: Token) -> Result<(), RegexFailure> {
        let here = self.source.tell();
        let (min, max) = match t.c {
            63 => (0, 1),
            42 => (0, u32::MAX),
            43 => (1, u32::MAX),
            123 => {
                if self.source.is(125) {
                    let id = self.node(Kind::Literal(123), t.span, Width::UNIT)?;
                    return self.append(id);
                }
                let lo_start = self.source.tell();
                while self.source.next.is_some_and(ascii_digit) {
                    self.source.get(self.budget)?;
                }
                let lo_end = self.source.tell();
                let (hi_start, hi_end) = if self.source.take(44, self.budget)? {
                    let s = self.source.tell();
                    while self.source.next.is_some_and(ascii_digit) {
                        self.source.get(self.budget)?;
                    }
                    (s, self.source.tell())
                } else {
                    (lo_start, lo_end)
                };
                if !self.source.take(125, self.budget)? {
                    let id = self.node(Kind::Literal(123), t.span, Width::UNIT)?;
                    self.append(id)?;
                    self.source.seek(here, self.budget)?;
                    return Ok(());
                }
                let lo = if lo_start == lo_end {
                    0
                } else {
                    let v = super::semantic::decimal(
                        &self.source.p[lo_start..lo_end],
                        false,
                        self.budget,
                    )?
                    .0;
                    if v == u32::MAX {
                        return Err(RegexFailure::CompileAbort(CompileAbort::OverflowError));
                    }
                    v
                };
                let hi = if hi_start == hi_end {
                    u32::MAX
                } else {
                    let v = super::semantic::decimal(
                        &self.source.p[hi_start..hi_end],
                        false,
                        self.budget,
                    )?
                    .0;
                    if v == u32::MAX {
                        return Err(RegexFailure::CompileAbort(CompileAbort::OverflowError));
                    }
                    if v < lo {
                        return Err(self.source.error(self.source.tell() - here));
                    }
                    v
                };
                (lo, hi)
            }
            _ => return Err(RegexFailure::InternalProgram),
        };
        let child = *self
            .frames
            .last()
            .unwrap()
            .items
            .last()
            .ok_or_else(|| self.source.error(self.source.tell() - here + 1))?;
        if matches!(
            self.arena.nodes[child].kind,
            Kind::At(_) | Kind::Repeat { .. }
        ) {
            return Err(self.source.error(self.source.tell() - here + 1));
        }
        let mode = if self.source.take(63, self.budget)? {
            1
        } else if self.source.take(43, self.budget)? {
            2
        } else {
            0
        };
        let w = self.arena.nodes[child].width;
        let width = Width {
            lo: (w.lo * u128::from(min)).min(MAXWIDTH),
            hi: if max == u32::MAX && w.hi != 0 {
                MAXWIDTH
            } else {
                (w.hi * u128::from(max)).min(MAXWIDTH)
            },
        };
        let id = self.node(
            Kind::Repeat {
                child,
                min,
                max,
                mode,
            },
            Span {
                start: t.span.start,
                end: self.source.tell(),
            },
            width,
        )?;
        *self.frames.last_mut().unwrap().items.last_mut().unwrap() = id;
        Ok(())
    }
    fn run(mut self) -> Result<ParsedSyntax<'p>, RegexFailure> {
        push(
            &mut self.arena.groups,
            Group {
                name: None,
                width: None,
            },
            16_384,
            self.budget,
        )?;
        push(
            &mut self.frames,
            Frame::new(Shell::Root, 0, 0, None),
            129,
            self.budget,
        )?;
        loop {
            let t = match self.source.next {
                None => {
                    if self.frames.len() != 1 {
                        return Err(self
                            .source
                            .error(self.source.tell() - self.frames.last().unwrap().start));
                    }
                    break;
                }
                Some(t) => t,
            };
            let flags = self.frames.last().unwrap().flags;
            if !t.escape && flags & VERBOSE != 0 {
                if matches!(t.c, 9..=13 | 32) {
                    self.source.get(self.budget)?;
                    continue;
                }
                if t.c == 35 {
                    while let Some(t) = self.source.get(self.budget)? {
                        if !t.escape && t.c == 10 {
                            break;
                        }
                    }
                    continue;
                }
            }
            if !t.escape && t.c == 41 {
                if self.frames.len() == 1 {
                    break;
                }
                self.source.get(self.budget)?;
                let id = self.close()?;
                self.append(id)?;
                continue;
            }
            if !t.escape && t.c == 124 {
                if matches!(self.frames.last().unwrap().shell, Shell::Conditional(_))
                    && !self.frames.last().unwrap().branches.is_empty()
                {
                    return Err(self.source.error(0));
                }
                self.source.get(self.budget)?;
                self.branch()?;
                continue;
            }
            self.source.get(self.budget)?;
            if t.escape {
                let (kind, width) = self.escaped(t, false)?;
                let id = self.node(
                    kind,
                    Span {
                        start: t.span.start,
                        end: self.source.tell(),
                    },
                    width,
                )?;
                self.append(id)?;
            } else {
                match t.c {
                    40 => self.open(t.span.start)?,
                    91 => {
                        let id = self.class(t.span.start)?;
                        self.append(id)?;
                    }
                    42 | 43 | 63 | 123 => self.repeat(t)?,
                    _ => {
                        let (kind, width) = match t.c {
                            46 => (Kind::Any, Width::UNIT),
                            94 => (Kind::At(0), Width::ZERO),
                            36 => (Kind::At(5), Width::ZERO),
                            _ => (Kind::Literal(t.c), Width::UNIT),
                        };
                        let id = self.node(kind, t.span, width)?;
                        self.append(id)?;
                    }
                }
            }
        }
        if self.arena.flags & ASCII == 0 {
            self.arena.flags |= UNICODE;
        } else if self.arena.flags & UNICODE != 0 {
            return Err(RegexFailure::CompileAbort(CompileAbort::ValueError));
        }
        if self.source.next.is_some() {
            return Err(self.source.error(0));
        }
        for &(g, pos) in &self.deferred {
            self.budget.charge(1, RegexPhase::Compile)?;
            if g as usize >= self.arena.groups.len() {
                return Err(RegexFailure::ReError {
                    position: Some(pos),
                });
            }
        }
        self.arena.frame_capacity_bytes = self.frames.capacity() * size_of::<Frame>();
        self.arena.root = self.close()?;
        Ok(self.arena)
    }
}
pub(super) fn parse_validate<'p>(
    pattern: RegexText<'p>,
    budget: &mut RegexBudget,
    warnings: &mut WarningSink,
) -> Result<ValidatedSyntax<'p>, RegexFailure> {
    budget.input(pattern.0.len(), true)?;
    let source = Source::new(pattern.0, budget)?;
    let p = Parser {
        source,
        arena: ParsedSyntax::new(pattern),
        budget,
        warnings,
        frames: Vec::new(),
        behind: None,
        deferred: Vec::new(),
    };
    super::semantic::validate(p.run()?, budget)
}
