//! The SMILES reader: one pass over the bytes, no recursion, never a panic.
//!
//! Grammar decisions cite the OpenSMILES specification
//! (<https://opensmiles.github.io/opensmiles/opensmiles.html>) by section: "Formal Grammar" for
//! the productions, "Rings" for ring labels, "Bonds" and "Branches" for chains, "Disconnected
//! Structures" for dots, "Charge", "Isotopes" and "Hydrogens" for bracket atoms, "Aromaticity"
//! for aromatic atoms and "Parsing Termination" for the end of the string.

use std::collections::{BTreeSet, HashSet};

use chemifree_application::smiles::{
    SmilesAtom, SmilesBond, SmilesCode, SmilesError, SmilesRead, Span,
};
use chemifree_application::{Loss, LossConsent, LossDisposition, SemanticImpact};
use chemifree_domain::document::{
    Atom, AtomId, Bond, BondId, BondOrder, MAX_CHARGE, MAX_COMMANDS, MAX_ISOTOPE_MASS_NUMBER,
    MIN_CHARGE,
};
use chemifree_domain::element::Element;
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::rings;
use chemifree_domain::states::{AtomKind, Hydrogens, Radical};

use super::analysis::{self, determine_radical};
use super::tables;
use super::{MAX_BRANCH_DEPTH, MAX_INPUT_BYTES, MAX_RING_LABEL};

type Fail = SmilesError;

/// What the reader resolved for one atom, which is what a writer must reproduce.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Resolved {
    /// The atom's hydrogens, stated or filled by the valence model, when it determines them.
    pub hydrogens: Option<u16>,
    pub radical: Radical,
}

/// A read string with everything a writer's read-back check compares.
pub(super) struct Parsed {
    pub read: SmilesRead,
    pub resolved: Vec<Resolved>,
    pub graph: MoleculeGraph,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Symbol {
    Single,
    Double,
    Triple,
    Aromatic,
}

impl Symbol {
    fn order(self) -> BondOrder {
        match self {
            Self::Single => BondOrder::Single,
            Self::Double => BondOrder::Double,
            Self::Triple => BondOrder::Triple,
            Self::Aromatic => BondOrder::Aromatic,
        }
    }
}

/// What the previous token allows next.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Prev {
    Start,
    /// An atom, possibly followed by ring labels: more ring labels may follow.
    Atom,
    /// A bond symbol directly after an atom or its ring labels: a ring label or an atom follows.
    Bond,
    /// A bond symbol after `(` or `)`: only an atom may follow.
    DetachedBond,
    Open,
    Close,
    Dot,
}

struct ReadAtom {
    atom: Atom,
    aromatic: bool,
    bracket: bool,
    span: Span,
}

struct ReadBond {
    begin: usize,
    end: usize,
    symbol: Option<Symbol>,
    /// The bond symbol when one was written, else the atom pair's later atom.
    span: Span,
}

struct OpenRing {
    atom: usize,
    symbol: Option<Symbol>,
    span: Span,
    sequence: usize,
}

/// A bracket atom as written, before its semantics are checked.
struct Bracket {
    element: Element,
    aromatic: bool,
    isotope: Option<(u32, Span)>,
    hydrogens: u8,
    charge: Option<(i16, Span)>,
    class: Option<Span>,
}

struct Reader<'a> {
    source: &'a str,
    bytes: &'a [u8],
    position: usize,
    atoms: Vec<ReadAtom>,
    bonds: Vec<ReadBond>,
    pairs: HashSet<(usize, usize)>,
    rings: Vec<Option<OpenRing>>,
    ring_sequence: usize,
    /// The parent atom and the offset of each open `(`.
    branches: Vec<(usize, usize)>,
    previous: Prev,
    /// The atom the next atom is bonded to; `None` at a fragment start.
    anchor: Option<usize>,
    /// The atom whose ring labels are being read.
    current: Option<usize>,
    pending: Option<(Symbol, Span)>,
    dot: Option<Span>,
    /// Bracket atoms that named an atom class, by atom index; the class is not kept.
    classes: Vec<(usize, Span)>,
}

fn char_span(source: &str, offset: usize) -> Span {
    let length = source
        .get(offset..)
        .and_then(|rest| rest.chars().next())
        .map_or(0, char::len_utf8);
    Span::new(offset, length)
}

fn at(code: SmilesCode, span: Span) -> Fail {
    SmilesError::at(code, span)
}

/// The largest character boundary of `source` that is at most `limit`.
fn boundary_at_most(source: &str, limit: usize) -> usize {
    let mut offset = limit.min(source.len());
    while !source.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

/// Read `source` completely.
pub(super) fn parse(source: &str) -> Result<Parsed, Fail> {
    if source.is_empty() {
        return Err(at(SmilesCode::EmptyInput, Span::new(0, 0)));
    }
    if source.len() > MAX_INPUT_BYTES {
        let offset = boundary_at_most(source, MAX_INPUT_BYTES);
        return Err(at(SmilesCode::InputTooLong, char_span(source, offset)));
    }
    if source
        .as_bytes()
        .first()
        .is_some_and(|b| tables::is_whitespace(*b))
    {
        return Err(at(SmilesCode::LeadingWhitespace, Span::new(0, 1)));
    }
    let mut reader = Reader {
        source,
        bytes: source.as_bytes(),
        position: 0,
        atoms: Vec::new(),
        bonds: Vec::new(),
        pairs: HashSet::new(),
        rings: (0..=MAX_RING_LABEL).map(|_| None).collect(),
        ring_sequence: 0,
        branches: Vec::new(),
        previous: Prev::Start,
        anchor: None,
        current: None,
        pending: None,
        dot: None,
        classes: Vec::new(),
    };
    reader.chain()?;
    reader.finish()
}

impl Reader<'_> {
    fn peek(&self, ahead: usize) -> Option<u8> {
        self.bytes.get(self.position + ahead).copied()
    }

    fn here(&self) -> Span {
        char_span(self.source, self.position)
    }

    fn command_budget(&self, span: Span) -> Result<(), Fail> {
        if self.atoms.len() + self.bonds.len() >= MAX_COMMANDS {
            Err(at(SmilesCode::CommandLimit, span))
        } else {
            Ok(())
        }
    }

    /// Reads tokens until the terminator, checking the whole chain's closing conditions.
    fn chain(&mut self) -> Result<(), Fail> {
        while let Some(byte) = self.peek(0) {
            if tables::is_whitespace(byte) {
                break;
            }
            match byte {
                b'(' => self.open_branch()?,
                b')' => self.close_branch()?,
                b'.' => self.dot_token()?,
                b'-' | b'=' | b'#' | b':' => self.bond_token(byte)?,
                b'$' => {
                    return Err(at(SmilesCode::QuadrupleBondUnsupported, self.here()));
                }
                b'/' | b'\\' | b'@' => return Err(at(SmilesCode::StereoUnsupported, self.here())),
                b'0'..=b'9' | b'%' => self.ring_token()?,
                _ => self.atom_token()?,
            }
        }
        self.check_complete()
    }

    fn check_complete(&self) -> Result<(), Fail> {
        match self.previous {
            Prev::Bond | Prev::DetachedBond => {
                let (_, span) = self.pending.expect("a pending bond follows a bond symbol");
                return Err(at(SmilesCode::DanglingBond, span));
            }
            Prev::Dot => {
                let span = self.dot.expect("a dot token records its span");
                return Err(at(SmilesCode::DanglingDot, span));
            }
            _ => {}
        }
        if let Some((_, offset)) = self.branches.first() {
            return Err(at(SmilesCode::UnclosedBranch, Span::new(*offset, 1)));
        }
        if let Some(ring) = self.rings.iter().flatten().min_by_key(|ring| ring.sequence) {
            return Err(at(SmilesCode::UnclosedRing, ring.span));
        }
        Ok(())
    }

    fn open_branch(&mut self) -> Result<(), Fail> {
        let span = self.here();
        match self.previous {
            Prev::Bond | Prev::DetachedBond => {
                let (_, bond) = self.pending.expect("a pending bond follows a bond symbol");
                return Err(at(SmilesCode::DanglingBond, bond));
            }
            Prev::Start | Prev::Open | Prev::Dot => {
                return Err(at(SmilesCode::BranchWithoutAtom, span));
            }
            Prev::Atom | Prev::Close => {}
        }
        if self.branches.len() >= MAX_BRANCH_DEPTH {
            return Err(at(SmilesCode::BranchDepthLimit, span));
        }
        let parent = self.anchor.expect("an atom precedes a branch");
        self.branches.push((parent, self.position));
        self.previous = Prev::Open;
        self.position += 1;
        Ok(())
    }

    fn close_branch(&mut self) -> Result<(), Fail> {
        let span = self.here();
        let Some(&(parent, open)) = self.branches.last() else {
            return Err(at(SmilesCode::UnmatchedCloseBranch, span));
        };
        match self.previous {
            Prev::Bond | Prev::DetachedBond => {
                let (_, bond) = self.pending.expect("a pending bond follows a bond symbol");
                return Err(at(SmilesCode::DanglingBond, bond));
            }
            Prev::Dot => {
                let dot = self.dot.expect("a dot token records its span");
                return Err(at(SmilesCode::DanglingDot, dot));
            }
            Prev::Open => return Err(at(SmilesCode::EmptyBranch, Span::new(open, 2))),
            Prev::Start | Prev::Atom | Prev::Close => {}
        }
        self.branches.pop();
        self.anchor = Some(parent);
        self.current = None;
        self.previous = Prev::Close;
        self.position += 1;
        Ok(())
    }

    fn dot_token(&mut self) -> Result<(), Fail> {
        let span = self.here();
        match self.previous {
            Prev::Start => return Err(at(SmilesCode::DotWithoutAtom, span)),
            Prev::Bond | Prev::DetachedBond => {
                let (_, bond) = self.pending.expect("a pending bond follows a bond symbol");
                return Err(at(SmilesCode::DanglingBond, bond));
            }
            Prev::Dot => {
                let first = self.dot.expect("a dot token records its span");
                return Err(at(SmilesCode::DanglingDot, first));
            }
            Prev::Atom | Prev::Close | Prev::Open => {}
        }
        self.dot = Some(span);
        self.anchor = None;
        self.current = None;
        self.previous = Prev::Dot;
        self.position += 1;
        Ok(())
    }

    fn bond_token(&mut self, byte: u8) -> Result<(), Fail> {
        let span = self.here();
        let symbol = match byte {
            b'-' => Symbol::Single,
            b'=' => Symbol::Double,
            b'#' => Symbol::Triple,
            _ => Symbol::Aromatic,
        };
        let next = match self.previous {
            Prev::Start | Prev::Dot => return Err(at(SmilesCode::BondWithoutAtom, span)),
            Prev::Bond | Prev::DetachedBond => {
                return Err(at(SmilesCode::RepeatedBondSymbol, span));
            }
            Prev::Atom => Prev::Bond,
            Prev::Open | Prev::Close => Prev::DetachedBond,
        };
        self.pending = Some((symbol, span));
        self.previous = next;
        self.position += 1;
        Ok(())
    }

    fn ring_token(&mut self) -> Result<(), Fail> {
        let start = self.position;
        let symbol = match self.previous {
            Prev::Atom => None,
            Prev::Bond => self.pending.take().map(|(symbol, _)| symbol),
            Prev::Start => return Err(at(SmilesCode::RingDigitAtStart, self.here())),
            Prev::Open => return Err(at(SmilesCode::RingDigitAfterBranchOpen, self.here())),
            Prev::Close => return Err(at(SmilesCode::RingDigitAfterBranchClose, self.here())),
            Prev::Dot => return Err(at(SmilesCode::RingDigitAfterDot, self.here())),
            Prev::DetachedBond => {
                return Err(at(SmilesCode::RingDigitAfterDetachedBond, self.here()));
            }
        };
        let label = self.ring_label()?;
        let span = Span::new(start, self.position - start);
        let atom = self.current.expect("ring labels follow an atom");
        self.close_or_open_ring(label, atom, symbol, span)?;
        self.previous = Prev::Atom;
        Ok(())
    }

    /// `d` or `%dd` (Rings). `%(ddd)` is valid OpenSMILES that the drawing cannot hold.
    fn ring_label(&mut self) -> Result<usize, Fail> {
        let digit = |byte: Option<u8>| {
            byte.filter(u8::is_ascii_digit)
                .map(|b| usize::from(b - b'0'))
        };
        if self.peek(0) != Some(b'%') {
            let value = digit(self.peek(0)).expect("a ring label starts with a digit");
            self.position += 1;
            return Ok(value);
        }
        let start = self.position;
        if self.peek(1) == Some(b'(') {
            let mut end = self.position + 2;
            while self.bytes.get(end).is_some_and(u8::is_ascii_digit) {
                end += 1;
            }
            if end > self.position + 2 && self.bytes.get(end) == Some(&b')') {
                return Err(at(
                    SmilesCode::RingLabelExtendedUnsupported,
                    Span::new(start, end + 1 - start),
                ));
            }
            return Err(at(SmilesCode::InvalidRingLabel, Span::new(start, 2)));
        }
        match (digit(self.peek(1)), digit(self.peek(2))) {
            (Some(tens), Some(ones)) => {
                self.position += 3;
                Ok(tens * 10 + ones)
            }
            (Some(_), None) => Err(at(SmilesCode::InvalidRingLabel, Span::new(start, 2))),
            _ => Err(at(SmilesCode::InvalidRingLabel, Span::new(start, 1))),
        }
    }

    fn close_or_open_ring(
        &mut self,
        label: usize,
        atom: usize,
        symbol: Option<Symbol>,
        span: Span,
    ) -> Result<(), Fail> {
        let Some(open) = self.rings[label].take() else {
            self.ring_sequence += 1;
            self.rings[label] = Some(OpenRing {
                atom,
                symbol,
                span,
                sequence: self.ring_sequence,
            });
            return Ok(());
        };
        if open.atom == atom {
            return Err(at(SmilesCode::RingSelfBond, span));
        }
        let joined = match (open.symbol, symbol) {
            (Some(a), Some(b)) if a != b => return Err(at(SmilesCode::RingBondMismatch, span)),
            (Some(symbol), _) | (None, Some(symbol)) => Some(symbol),
            (None, None) => None,
        };
        self.add_bond(open.atom, atom, joined, span)
    }

    fn add_bond(
        &mut self,
        begin: usize,
        end: usize,
        symbol: Option<Symbol>,
        span: Span,
    ) -> Result<(), Fail> {
        self.command_budget(span)?;
        if !self.pairs.insert((begin.min(end), begin.max(end))) {
            return Err(at(SmilesCode::DuplicateBond, span));
        }
        self.bonds.push(ReadBond {
            begin,
            end,
            symbol,
            span,
        });
        Ok(())
    }

    fn atom_token(&mut self) -> Result<(), Fail> {
        let start = self.position;
        self.command_budget(self.here())?;
        let atom = if self.peek(0) == Some(b'[') {
            self.bracket_atom()?
        } else {
            self.bare_atom()?
        };
        let index = self.atoms.len();
        self.atoms.push(atom);
        if let Some(anchor) = self.anchor {
            let (symbol, span) = match self.pending.take() {
                Some((symbol, span)) => (Some(symbol), span),
                None => (None, Span::new(start, self.position - start)),
            };
            self.add_bond(anchor, index, symbol, span)?;
        }
        self.anchor = Some(index);
        self.current = Some(index);
        self.previous = Prev::Atom;
        Ok(())
    }

    fn bare_atom(&mut self) -> Result<ReadAtom, Fail> {
        let start = self.position;
        let rest = &self.source[start..];
        let (symbol, element, aromatic) = match self.bytes[start] {
            b'*' => return Err(at(SmilesCode::WildcardUnsupported, self.here())),
            b'A'..=b'Z' => match tables::bare_organic_prefix(rest) {
                Some(symbol) => (symbol, symbol, false),
                None => return Err(at(SmilesCode::BracketRequired, self.here())),
            },
            b'a'..=b'z' => match tables::bare_aromatic_prefix(rest) {
                Some((symbol, element)) => (symbol, element, true),
                None => return Err(at(SmilesCode::UnexpectedCharacter, self.here())),
            },
            _ => return Err(at(SmilesCode::UnexpectedCharacter, self.here())),
        };
        self.position += symbol.len();
        let element = Element::from_symbol(element).expect("a bare symbol names an element");
        Ok(ReadAtom {
            atom: Atom::new(AtomKind::Element(element)),
            aromatic,
            bracket: false,
            span: Span::new(start, symbol.len()),
        })
    }

    fn digits(&mut self) -> Option<(u32, Span)> {
        let start = self.position;
        let mut value: u32 = 0;
        while let Some(byte) = self.peek(0).filter(u8::is_ascii_digit) {
            value = value
                .saturating_mul(10)
                .saturating_add(u32::from(byte - b'0'));
            self.position += 1;
        }
        (self.position > start).then(|| (value, Span::new(start, self.position - start)))
    }

    /// `[` isotope? symbol chiral? hcount? charge? class? `]` (Formal Grammar).
    fn bracket_atom(&mut self) -> Result<ReadAtom, Fail> {
        let start = self.position;
        self.position += 1;
        let isotope = self.digits();
        let (element, aromatic) = self.bracket_symbol(start)?;
        if self.peek(0) == Some(b'@') {
            return Err(at(SmilesCode::StereoUnsupported, self.here()));
        }
        let mut bracket = Bracket {
            element,
            aromatic,
            isotope,
            hydrogens: 0,
            charge: None,
            class: None,
        };
        if self.peek(0) == Some(b'H') {
            if element.symbol() == "H" {
                return Err(at(SmilesCode::HydrogenCountOnHydrogen, self.here()));
            }
            self.position += 1;
            bracket.hydrogens = match self.peek(0).filter(u8::is_ascii_digit) {
                Some(digit) => {
                    self.position += 1;
                    digit - b'0'
                }
                None => 1,
            };
        }
        if let Some(sign) = self.peek(0).filter(|b| matches!(b, b'+' | b'-')) {
            bracket.charge = Some(self.charge(sign));
        }
        if self.peek(0) == Some(b':') {
            let colon = self.position;
            self.position += 1;
            if self.digits().is_none() {
                return Err(at(SmilesCode::InvalidBracketContent, self.here()));
            }
            bracket.class = Some(Span::new(colon, self.position - colon));
        }
        match self.peek(0) {
            Some(b']') => self.position += 1,
            Some(byte) if !tables::is_whitespace(byte) => {
                return Err(at(SmilesCode::InvalidBracketContent, self.here()));
            }
            _ => {
                return Err(at(
                    SmilesCode::UnclosedBracket,
                    Span::new(start, self.position - start),
                ));
            }
        }
        let span = Span::new(start, self.position - start);
        self.bracket_semantics(bracket, span)
    }

    fn bracket_symbol(&mut self, open: usize) -> Result<(Element, bool), Fail> {
        let start = self.position;
        let Some(first) = self.peek(0) else {
            return Err(at(
                SmilesCode::UnclosedBracket,
                Span::new(open, start - open),
            ));
        };
        let lookup = |text: &str| Element::from_symbol(text);
        match first {
            b'*' => Err(at(SmilesCode::WildcardUnsupported, self.here())),
            b'A'..=b'Z' => {
                let two = self.source.get(start..start + 2);
                if let Some(element) = two
                    .filter(|t| t.as_bytes()[1].is_ascii_lowercase())
                    .and_then(lookup)
                {
                    self.position += 2;
                    return Ok((element, false));
                }
                match lookup(&self.source[start..=start]) {
                    Some(element) => {
                        self.position += 1;
                        Ok((element, false))
                    }
                    None => Err(at(SmilesCode::UnknownElement, self.here())),
                }
            }
            b'a'..=b'z' => {
                let two = self
                    .source
                    .get(start..start + 2)
                    .filter(|t| tables::aromatic_element(t).is_some());
                let text = two.unwrap_or(&self.source[start..=start]);
                match tables::aromatic_element(text).and_then(Element::from_symbol) {
                    Some(element) => {
                        self.position += text.len();
                        Ok((element, true))
                    }
                    None => Err(at(SmilesCode::UnknownElement, self.here())),
                }
            }
            byte if tables::is_whitespace(byte) => Err(at(
                SmilesCode::UnclosedBracket,
                Span::new(open, start - open),
            )),
            _ => Err(at(SmilesCode::MissingElementSymbol, self.here())),
        }
    }

    /// `+`, `++`, `+n`, `+nn` and the same for `-` (Charge). The value may exceed what the
    /// drawing records; that is judged once the bracket is complete.
    fn charge(&mut self, sign: u8) -> (i16, Span) {
        let start = self.position;
        self.position += 1;
        let magnitude = if self.peek(0) == Some(sign) {
            self.position += 1;
            2
        } else {
            let first = self
                .peek(0)
                .filter(u8::is_ascii_digit)
                .map(|b| i16::from(b - b'0'));
            match first {
                Some(tens) => {
                    self.position += 1;
                    match self.peek(0).filter(u8::is_ascii_digit) {
                        Some(ones) => {
                            self.position += 1;
                            tens * 10 + i16::from(ones - b'0')
                        }
                        None => tens,
                    }
                }
                None => 1,
            }
        };
        let value = if sign == b'+' { magnitude } else { -magnitude };
        (value, Span::new(start, self.position - start))
    }

    fn bracket_semantics(&mut self, bracket: Bracket, span: Span) -> Result<ReadAtom, Fail> {
        let mut atom = Atom::new(AtomKind::Element(bracket.element));
        if let Some((value, digits)) = bracket.isotope {
            if value == 0 {
                return Err(at(SmilesCode::IsotopeZero, digits));
            }
            if value > u32::from(MAX_ISOTOPE_MASS_NUMBER) {
                return Err(at(SmilesCode::IsotopeOutOfRange, digits));
            }
            atom.isotope = Some(u16::try_from(value).expect("bounded by the isotope maximum"));
        }
        if let Some((value, text)) = bracket.charge {
            let range = i16::from(MIN_CHARGE)..=i16::from(MAX_CHARGE);
            if !range.contains(&value) {
                return Err(at(SmilesCode::ChargeOutOfRange, text));
            }
            atom.charge = i8::try_from(value).expect("bounded by the charge range");
        }
        atom.hydrogens = Hydrogens::Specified(bracket.hydrogens);
        if let Some(class) = bracket.class {
            // Recorded as a loss by `finish`, which sees every atom; keep the span there.
            self.classes.push((self.atoms.len(), class));
        }
        Ok(ReadAtom {
            atom,
            aromatic: bracket.aromatic,
            bracket: true,
            span,
        })
    }
}

fn serial(index: usize) -> u32 {
    u32::try_from(index + 1).expect("an import holds at most MAX_COMMANDS atoms")
}

fn atom_id(index: usize) -> AtomId {
    AtomId::from_serial(serial(index)).expect("serials start at one")
}

fn bond_id(index: usize) -> BondId {
    BondId::from_serial(serial(index)).expect("serials start at one")
}

fn dropped(feature: &str, span: Span, reason: &str, impact: SemanticImpact) -> Loss {
    Loss {
        consent: LossConsent::Required,
        feature: feature.into(),
        source_path: format!("source[{}]", span.describe()),
        disposition: LossDisposition::Dropped,
        reason: reason.into(),
        semantic_impact: impact,
    }
}

impl Reader<'_> {
    /// Non-whitespace text after the terminator: the first to the last non-whitespace byte.
    fn ignored_text(&self) -> Option<Span> {
        let rest = self.source.get(self.position..)?;
        let blank = |c: char| u8::try_from(c).is_ok_and(tables::is_whitespace);
        let trimmed = rest.trim_start_matches(blank);
        let start = self.position + (rest.len() - trimmed.len());
        let text = trimmed.trim_end_matches(blank);
        (!text.is_empty()).then(|| Span::new(start, text.len()))
    }

    /// Decides every bond's kind: an unspecified bond joining two aromatic atoms is aromatic
    /// only on a ring (Aromaticity); anywhere else it is single.
    fn bond_orders(&self, topology: &MoleculeGraph) -> Result<Vec<BondOrder>, Fail> {
        let on_ring: HashSet<BondId> = rings::perceive(topology)
            .iter()
            .flat_map(|system| system.bonds.iter().copied())
            .collect();
        let mut orders = Vec::with_capacity(self.bonds.len());
        for (index, bond) in self.bonds.iter().enumerate() {
            let ring = on_ring.contains(&bond_id(index));
            let both_aromatic = self.atoms[bond.begin].aromatic && self.atoms[bond.end].aromatic;
            orders.push(match bond.symbol {
                Some(Symbol::Aromatic) if !ring => {
                    return Err(at(SmilesCode::AromaticBondOutsideRing, bond.span));
                }
                Some(symbol) => symbol.order(),
                None if both_aromatic && ring => BondOrder::Aromatic,
                None => BondOrder::Single,
            });
        }
        Ok(orders)
    }

    fn finish(self) -> Result<Parsed, Fail> {
        let ignored_text = self.ignored_text();
        // One graph is built for the whole read: first with every bond single, which is enough to
        // find the rings, then with the bond kinds decided and, last, the radicals found.
        let mut graph = MoleculeGraph::from_parts(
            self.atoms
                .iter()
                .enumerate()
                .map(|(i, a)| (atom_id(i), a.atom.clone()))
                .collect(),
            self.bonds
                .iter()
                .enumerate()
                .map(|(i, b)| {
                    (
                        bond_id(i),
                        Bond {
                            begin: atom_id(b.begin),
                            end: atom_id(b.end),
                            order: BondOrder::Single,
                        },
                    )
                })
                .collect(),
        );
        let orders = self.bond_orders(&graph)?;
        for ((_, bond), order) in graph.bonds_mut().zip(&orders) {
            bond.order = order.clone();
        }
        let mut aromatic_degree = vec![0_usize; self.atoms.len()];
        for (bond, order) in self.bonds.iter().zip(&orders) {
            if *order == BondOrder::Aromatic {
                aromatic_degree[bond.begin] += 1;
                aromatic_degree[bond.end] += 1;
            }
        }
        if let Some(index) =
            (0..self.atoms.len()).find(|&i| self.atoms[i].aromatic && aromatic_degree[i] == 0)
        {
            return Err(at(
                SmilesCode::AromaticAtomOutsideRing,
                self.atoms[index].span,
            ));
        }
        let aromatic: BTreeSet<BondId> = orders
            .iter()
            .enumerate()
            .filter(|(_, order)| **order == BondOrder::Aromatic)
            .map(|(i, _)| bond_id(i))
            .collect();
        let aromatic_span = |id: AtomId| self.atoms[id.serial() as usize - 1].span;
        let interpretation = if aromatic.is_empty() {
            None
        } else {
            let found = analysis::interpret(&graph, &aromatic).map_err(|failure| {
                at(
                    SmilesCode::AromaticSystemUnsupported,
                    aromatic_span(failure.atom),
                )
                .because(failure.reason)
            })?;
            Some(found)
        };
        let analysed = analysis::analyse(&graph, interpretation.as_ref()).map_err(|failure| {
            at(
                SmilesCode::AromaticSystemUnsupported,
                aromatic_span(failure.atom),
            )
            .because(failure.reason)
        })?;
        let mut atoms = self.atoms;
        let mut losses = Vec::new();
        let mut resolved = Vec::with_capacity(atoms.len());
        if let Some(span) = ignored_text {
            losses.push(dropped(
                "ignored_text",
                span,
                "Text after the first whitespace is not read; names and properties are not stored.",
                SemanticImpact::Document,
            ));
        }
        for (_, class) in &self.classes {
            losses.push(dropped(
                "atom_class",
                *class,
                "An atom class is a label the drawing has no place for.",
                SemanticImpact::Document,
            ));
        }
        for (index, entry) in atoms.iter_mut().enumerate() {
            let id = atom_id(index);
            let assessment = &analysed.assessments[index];
            let radical = if entry.bracket {
                match determine_radical(id, &entry.atom, &analysed.orders[&id]) {
                    Some(radical) => radical,
                    None => {
                        losses.push(dropped(
                            "radical_state",
                            entry.span,
                            "No single radical state makes this bracket atom's valence consistent under the valence model, so the radical is recorded as unknown.",
                            SemanticImpact::Chemistry,
                        ));
                        Radical::Unknown
                    }
                }
            } else {
                Radical::None
            };
            entry.atom.radical = radical;
            resolved.push(Resolved {
                hydrogens: match entry.atom.hydrogens {
                    Hydrogens::Specified(count) => Some(u16::from(count)),
                    _ => assessment.hydrogen_count,
                },
                radical,
            });
        }
        for ((_, atom), entry) in graph.atoms_mut().zip(&atoms) {
            atom.radical = entry.atom.radical;
        }
        let read = SmilesRead {
            atoms: atoms
                .into_iter()
                .map(|entry| SmilesAtom {
                    atom: entry.atom,
                    span: entry.span,
                })
                .collect(),
            bonds: self
                .bonds
                .iter()
                .zip(orders)
                .map(|(bond, order)| SmilesBond {
                    begin: bond.begin,
                    end: bond.end,
                    order,
                })
                .collect(),
            ignored_text,
            losses,
        };
        Ok(Parsed {
            read,
            resolved,
            graph,
        })
    }
}
