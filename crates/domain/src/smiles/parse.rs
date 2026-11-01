//! The SMILES reader. Section numbers refer to the OpenSMILES specification.
//!
//! * 3.1 atoms: the organic subset (`B C N O P S F Cl Br I`, aromatic `b c n o p s`) may be bare;
//!   everything else is a bracket atom `[isotope? symbol chirality? hcount? charge? class?]`.
//! * 3.2 bonds: `-`, `=`, `#`, `$`, `:`, `/`, `\`; an unwritten bond between two aromatic atoms is
//!   aromatic, otherwise single. Here "aromatic" additionally requires a ring bond.
//! * 3.3 branches, 3.4 ring closures (`0`-`9`, `%nn`), 3.5 dot-disconnected components.
//! * 3.1.5 isotopes (at most three digits), 3.1.7 charge, 3.1.6 hydrogens.
//!
//! The reader works on bytes of an ASCII grammar; any other character is an error whose span is
//! that whole `char`, so every span lies on character boundaries.

use std::collections::{BTreeMap, HashSet};

use crate::aromaticity;
use crate::chemistry;
use crate::document::{
    Atom, AtomId, Bond, BondId, BondOrder, MAX_CHARGE, MAX_COMMANDS, MAX_ISOTOPE_MASS_NUMBER,
    MIN_CHARGE,
};
use crate::element::Element;
use crate::graph::MoleculeGraph;
use crate::states::{AtomKind, Hydrogens, Radical};

use super::error::{ErrorCode, SmilesError, Span};
use super::graph_build::bridges;
pub use super::radical::RadicalNote;
use super::{
    MAX_BRANCH_DEPTH, MAX_INPUT_BYTES, MAX_ISOTOPE_DIGITS, MAX_OPEN_RINGS, SMILES_MAX_CHARGE,
    radical,
};

/// A successfully read structure, in input order.
#[derive(Clone, Debug, PartialEq)]
pub struct Read {
    pub atoms: Vec<ReadAtom>,
    pub bonds: Vec<ReadBond>,
    /// Number of dot-disconnected parts. Ring closures can join parts, so this counts
    /// connected components of the bonded graph.
    pub components: usize,
    /// Non-whitespace text after the terminating whitespace. SMILES strings may be followed by
    /// a name; it is reported, never stored.
    pub ignored_text: Option<Span>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReadAtom {
    pub atom: Atom,
    pub span: Span,
    /// Whether the atom was a bracket atom (its hydrogens are stated).
    pub bracket: bool,
    /// For a bracket atom, how its radical state was decided.
    pub radical_note: RadicalNote,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReadBond {
    pub begin: usize,
    pub end: usize,
    pub order: BondOrder,
}

impl Read {
    /// The read structure as a graph with document-style IDs `a1..` and `b1..` in input order.
    pub fn graph(&self) -> MoleculeGraph {
        let ids = |index: usize| {
            AtomId::from_serial(u32::try_from(index + 1).expect("at most MAX_COMMANDS atoms"))
                .expect("serials start at one")
        };
        MoleculeGraph::from_parts(
            self.atoms
                .iter()
                .enumerate()
                .map(|(index, atom)| (ids(index), atom.atom.clone()))
                .collect(),
            self.bonds
                .iter()
                .enumerate()
                .map(|(index, bond)| {
                    (
                        BondId::from_serial(
                            u32::try_from(index + 1).expect("at most MAX_COMMANDS bonds"),
                        )
                        .expect("serials start at one"),
                        Bond {
                            begin: ids(bond.begin),
                            end: ids(bond.end),
                            order: bond.order.clone(),
                        },
                    )
                })
                .collect(),
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Symbol {
    Single,
    Double,
    Triple,
    Aromatic,
}

#[derive(Clone, Copy, Debug)]
enum Zone {
    /// Start of input or just after a dot: only an atom may follow.
    Detached,
    /// Just after an atom or one of its ring digits: ring digits may follow.
    AfterAtom,
    AfterOpen,
    AfterClose,
}

struct RawAtom {
    atom: Atom,
    span: Span,
    aromatic: bool,
    bracket: bool,
}

struct RawBond {
    begin: usize,
    end: usize,
    symbol: Option<(Symbol, Span)>,
}

struct OpenRing {
    atom: usize,
    symbol: Option<(Symbol, Span)>,
    span: Span,
}

struct Parser<'a> {
    text: &'a str,
    bytes: &'a [u8],
    end: usize,
    pos: usize,
    atoms: Vec<RawAtom>,
    bonds: Vec<RawBond>,
    bonded: HashSet<(usize, usize)>,
    rings: Vec<Option<OpenRing>>,
    tail: Option<usize>,
    pending: Option<(Symbol, Span)>,
    dot: Option<Span>,
    zone: Zone,
    branches: Vec<(Option<usize>, Span)>,
}

fn is_blank(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r')
}

/// Reads one SMILES string. Whitespace ends it (section 2.1 of Daylight's theory manual: a name
/// may follow); text after the whitespace is reported in [`Read::ignored_text`].
///
/// Never panics, whatever the input.
pub fn read(text: &str) -> Result<Read, SmilesError> {
    if text.len() > MAX_INPUT_BYTES {
        let start = text.floor_char_boundary(MAX_INPUT_BYTES);
        return Err(SmilesError::at(ErrorCode::InputTooLong, start, text.len()));
    }
    let bytes = text.as_bytes();
    let Some(&first) = bytes.first() else {
        return Err(SmilesError::at(ErrorCode::EmptyInput, 0, 0));
    };
    if is_blank(first) {
        let run = bytes.iter().take_while(|&&b| is_blank(b)).count();
        return Err(SmilesError::at(ErrorCode::LeadingWhitespace, 0, run));
    }
    let end = bytes
        .iter()
        .position(|&b| is_blank(b))
        .unwrap_or(bytes.len());
    let ignored_text = {
        let rest = &text[end..];
        let trimmed = rest.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\r'));
        if trimmed.is_empty() {
            None
        } else {
            let start = end + (rest.len() - rest.trim_start_matches([' ', '\t', '\n', '\r']).len());
            Some(Span::new(start, start + trimmed.len()))
        }
    };
    let mut parser = Parser {
        text,
        bytes,
        end,
        pos: 0,
        atoms: Vec::new(),
        bonds: Vec::new(),
        bonded: HashSet::new(),
        rings: (0..MAX_OPEN_RINGS).map(|_| None).collect(),
        tail: None,
        pending: None,
        dot: None,
        zone: Zone::Detached,
        branches: Vec::new(),
    };
    parser.run()?;
    parser.finish(ignored_text)
}

impl Parser<'_> {
    fn char_end(&self, at: usize) -> usize {
        at + self.text[at..].chars().next().map_or(1, char::len_utf8)
    }

    /// The error for a character that cannot appear here: the whole `char` is the span.
    fn bad_character(&self, at: usize, ascii_code: ErrorCode) -> SmilesError {
        let code = if self.bytes[at].is_ascii() {
            ascii_code
        } else {
            ErrorCode::UnexpectedCharacter
        };
        SmilesError::at(code, at, self.char_end(at))
    }

    fn commands(&self) -> usize {
        self.atoms.len() + self.bonds.len()
    }

    fn check_room(&self, span: Span) -> Result<(), SmilesError> {
        if self.commands() >= MAX_COMMANDS {
            Err(SmilesError::at(
                ErrorCode::CommandLimit,
                span.start,
                span.end,
            ))
        } else {
            Ok(())
        }
    }

    fn dangling(&self) -> Result<(), SmilesError> {
        match self.pending {
            Some((_, span)) => Err(SmilesError::at(
                ErrorCode::DanglingBond,
                span.start,
                span.end,
            )),
            None => Ok(()),
        }
    }

    fn no_open_dot(&self) -> Result<(), SmilesError> {
        match self.dot {
            Some(span) => Err(SmilesError::at(
                ErrorCode::DotWithoutAtom,
                span.start,
                span.end,
            )),
            None => Ok(()),
        }
    }

    fn run(&mut self) -> Result<(), SmilesError> {
        while self.pos < self.end {
            let at = self.pos;
            match self.bytes[at] {
                b'[' => {
                    let raw = self.bracket()?;
                    self.add_atom(raw)?;
                }
                b'A'..=b'Z' | b'a'..=b'z' => {
                    let raw = self.organic()?;
                    self.add_atom(raw)?;
                }
                b'*' => {
                    return Err(SmilesError::at(ErrorCode::WildcardUnsupported, at, at + 1));
                }
                b'(' => self.open_branch()?,
                b')' => self.close_branch()?,
                b'.' => self.dot_token()?,
                b'-' => self.bond_token(Symbol::Single)?,
                b'=' => self.bond_token(Symbol::Double)?,
                b'#' => self.bond_token(Symbol::Triple)?,
                b':' => self.bond_token(Symbol::Aromatic)?,
                b'$' => {
                    return Err(SmilesError::at(
                        ErrorCode::QuadrupleBondUnsupported,
                        at,
                        at + 1,
                    ));
                }
                b'/' | b'\\' => {
                    return Err(SmilesError::at(
                        ErrorCode::DirectionalBondUnsupported,
                        at,
                        at + 1,
                    ));
                }
                b'@' => return Err(SmilesError::at(ErrorCode::ChiralityUnsupported, at, at + 1)),
                b'>' => return Err(SmilesError::at(ErrorCode::ReactionUnsupported, at, at + 1)),
                b'0'..=b'9' | b'%' => self.ring_token()?,
                _ => return Err(self.bad_character(at, ErrorCode::UnexpectedCharacter)),
            }
        }
        self.dangling()?;
        self.no_open_dot()?;
        if let Some(&(_, span)) = self.branches.first() {
            return Err(SmilesError::at(
                ErrorCode::UnclosedBranch,
                span.start,
                span.end,
            ));
        }
        // The earliest-opened unclosed ring, whatever its label.
        if let Some(open) = self
            .rings
            .iter()
            .flatten()
            .min_by_key(|open| open.span.start)
        {
            return Err(SmilesError::at(
                ErrorCode::UnclosedRing,
                open.span.start,
                open.span.end,
            ));
        }
        if self.atoms.is_empty() {
            return Err(SmilesError::at(ErrorCode::EmptyInput, 0, 0));
        }
        Ok(())
    }

    fn add_atom(&mut self, raw: RawAtom) -> Result<(), SmilesError> {
        self.check_room(raw.span)?;
        let index = self.atoms.len();
        let span = raw.span;
        self.atoms.push(raw);
        if let (Some(tail), None) = (self.tail, self.dot) {
            self.check_room(span)?;
            let symbol = self.pending.take();
            self.bonded.insert((tail, index));
            self.bonds.push(RawBond {
                begin: tail,
                end: index,
                symbol,
            });
        }
        self.pending = None;
        self.dot = None;
        self.tail = Some(index);
        self.zone = Zone::AfterAtom;
        Ok(())
    }

    fn organic(&mut self) -> Result<RawAtom, SmilesError> {
        let at = self.pos;
        let next = if at + 1 < self.end {
            self.bytes[at + 1]
        } else {
            0
        };
        let (symbol, length, aromatic) = match (self.bytes[at], next) {
            (b'C', b'l') => ("Cl", 2, false),
            (b'B', b'r') => ("Br", 2, false),
            (b'B', _) => ("B", 1, false),
            (b'C', _) => ("C", 1, false),
            (b'N', _) => ("N", 1, false),
            (b'O', _) => ("O", 1, false),
            (b'P', _) => ("P", 1, false),
            (b'S', _) => ("S", 1, false),
            (b'F', _) => ("F", 1, false),
            (b'I', _) => ("I", 1, false),
            (b'b', _) => ("B", 1, true),
            (b'c', _) => ("C", 1, true),
            (b'n', _) => ("N", 1, true),
            (b'o', _) => ("O", 1, true),
            (b'p', _) => ("P", 1, true),
            (b's', _) => ("S", 1, true),
            _ => return Err(SmilesError::at(ErrorCode::BareAtomNotOrganic, at, at + 1)),
        };
        self.pos += length;
        let element = Element::from_symbol(symbol).expect("organic subset symbols are elements");
        Ok(RawAtom {
            atom: Atom::new(AtomKind::Element(element)),
            span: Span::new(at, at + length),
            aromatic,
            bracket: false,
        })
    }

    fn bracket(&mut self) -> Result<RawAtom, SmilesError> {
        let open = self.pos;
        let unclosed = SmilesError::at(ErrorCode::UnclosedBracketAtom, open, open + 1);
        let mut i = open + 1;
        let digits = i;
        while i < self.end && self.bytes[i].is_ascii_digit() {
            i += 1;
        }
        let mut isotope = None;
        if i > digits {
            if i - digits > MAX_ISOTOPE_DIGITS {
                return Err(SmilesError::at(ErrorCode::IsotopeTooLong, digits, i));
            }
            let value: u16 = self.text[digits..i]
                .parse()
                .expect("at most three ASCII digits fit a u16");
            if value == 0 {
                return Err(SmilesError::at(
                    ErrorCode::IsotopeZeroUnsupported,
                    digits,
                    i,
                ));
            }
            debug_assert!(value <= MAX_ISOTOPE_MASS_NUMBER);
            isotope = Some(value);
        }
        if i >= self.end {
            return Err(unclosed);
        }
        let (element, aromatic, length) = self.symbol(i)?;
        i += length;
        let mut atom = Atom::new(AtomKind::Element(element));
        atom.isotope = isotope;
        // 3.1.4 chirality: recognised only to be refused.
        if i < self.end && self.bytes[i] == b'@' {
            return Err(SmilesError::at(ErrorCode::ChiralityUnsupported, i, i + 1));
        }
        let mut hydrogens = 0_u8;
        if i < self.end && self.bytes[i] == b'H' {
            i += 1;
            hydrogens = 1;
            if i < self.end && self.bytes[i].is_ascii_digit() {
                hydrogens = self.bytes[i] - b'0';
                i += 1;
            }
        }
        if i < self.end && matches!(self.bytes[i], b'+' | b'-') {
            let (charge, next) = self.charge(i)?;
            atom.charge = charge;
            i = next;
        }
        if i < self.end && self.bytes[i] == b':' {
            return Err(SmilesError::at(ErrorCode::AtomClassUnsupported, i, i + 1));
        }
        if i >= self.end {
            return Err(unclosed);
        }
        if self.bytes[i] != b']' {
            return Err(self.bad_character(i, ErrorCode::BracketUnexpectedCharacter));
        }
        atom.hydrogens = Hydrogens::Specified(hydrogens);
        self.pos = i + 1;
        Ok(RawAtom {
            atom,
            span: Span::new(open, i + 1),
            aromatic,
            bracket: true,
        })
    }

    /// The element at `i` in a bracket and whether it is written in lower case, with its length.
    fn symbol(&self, i: usize) -> Result<(Element, bool, usize), SmilesError> {
        let byte = self.bytes[i];
        let second = if i + 1 < self.end {
            self.bytes[i + 1]
        } else {
            0
        };
        match byte {
            b'*' => Err(SmilesError::at(ErrorCode::WildcardUnsupported, i, i + 1)),
            b'A'..=b'Z' => {
                if second.is_ascii_lowercase()
                    && let Some(element) = Element::from_symbol(&self.text[i..i + 2])
                {
                    return Ok((element, false, 2));
                }
                Element::from_symbol(&self.text[i..i + 1])
                    .map(|element| (element, false, 1))
                    .ok_or_else(|| {
                        let length = if second.is_ascii_lowercase() { 2 } else { 1 };
                        SmilesError::at(ErrorCode::UnknownElement, i, i + length)
                    })
            }
            b'a'..=b'z' => {
                // 3.1.3: aromatic symbols are b c n o p s, and se and as inside brackets.
                let pair = &self.bytes[i..(i + 2).min(self.end)];
                let (symbol, length) = if pair == b"se" {
                    ("Se", 2)
                } else if pair == b"as" {
                    ("As", 2)
                } else if matches!(byte, b'b' | b'c' | b'n' | b'o' | b'p' | b's') {
                    (
                        match byte {
                            b'b' => "B",
                            b'c' => "C",
                            b'n' => "N",
                            b'o' => "O",
                            b'p' => "P",
                            _ => "S",
                        },
                        1,
                    )
                } else {
                    return Err(SmilesError::at(ErrorCode::UnknownElement, i, i + 1));
                };
                let element = Element::from_symbol(symbol).expect("listed symbols are elements");
                Ok((element, true, length))
            }
            _ => Err(self.bad_character(i, ErrorCode::MissingElement)),
        }
    }

    /// 3.1.7: `+`, `-`, `+n`, `-n` (one or two digits) or a repeated sign (`++`); `+0` and `-0`
    /// are the grammar's zero charge. The limits are the drawing's own.
    fn charge(&self, start: usize) -> Result<(i8, usize), SmilesError> {
        let sign = self.bytes[start];
        let mut i = start + 1;
        let mut magnitude: i32 = 1;
        if i < self.end && self.bytes[i].is_ascii_digit() {
            let digits = i;
            while i < self.end && self.bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i - digits > 2 {
                return Err(SmilesError::at(ErrorCode::ChargeSyntax, start, i));
            }
            magnitude = self.text[digits..i]
                .parse()
                .expect("one or two ASCII digits");
        } else {
            while i < self.end && self.bytes[i] == sign {
                magnitude += 1;
                i += 1;
            }
        }
        if i < self.end && matches!(self.bytes[i], b'+' | b'-' | b'0'..=b'9') {
            return Err(SmilesError::at(ErrorCode::ChargeSyntax, start, i + 1));
        }
        let value = if sign == b'+' { magnitude } else { -magnitude };
        if value.abs() > SMILES_MAX_CHARGE {
            return Err(SmilesError::at(ErrorCode::ChargeOutOfRange, start, i));
        }
        match i8::try_from(value) {
            Ok(charge) if (MIN_CHARGE..=MAX_CHARGE).contains(&charge) => Ok((charge, i)),
            _ => Err(SmilesError::at(
                ErrorCode::ChargeBeyondDrawingUnsupported,
                start,
                i,
            )),
        }
    }

    fn bond_token(&mut self, symbol: Symbol) -> Result<(), SmilesError> {
        let at = self.pos;
        let span = Span::new(at, at + 1);
        if self.pending.is_some() {
            return Err(SmilesError::at(ErrorCode::RepeatedBond, at, at + 1));
        }
        if matches!(self.zone, Zone::Detached) {
            let code = if self.dot.is_some() {
                ErrorCode::BondAfterDot
            } else {
                ErrorCode::BondWithoutAtom
            };
            return Err(SmilesError::at(code, at, at + 1));
        }
        self.pending = Some((symbol, span));
        self.pos += 1;
        Ok(())
    }

    fn dot_token(&mut self) -> Result<(), SmilesError> {
        let at = self.pos;
        self.dangling()?;
        // A dot needs an atom, a closed branch or an opening parenthesis before it.
        if matches!(self.zone, Zone::Detached) {
            return Err(SmilesError::at(ErrorCode::DotWithoutAtom, at, at + 1));
        }
        self.dot = Some(Span::new(at, at + 1));
        self.zone = Zone::Detached;
        self.pos += 1;
        Ok(())
    }

    fn open_branch(&mut self) -> Result<(), SmilesError> {
        let at = self.pos;
        self.dangling()?;
        self.no_open_dot()?;
        if !matches!(self.zone, Zone::AfterAtom | Zone::AfterClose) {
            return Err(SmilesError::at(ErrorCode::BranchWithoutAtom, at, at + 1));
        }
        if self.branches.len() >= MAX_BRANCH_DEPTH {
            return Err(SmilesError::at(ErrorCode::BranchDepthLimit, at, at + 1));
        }
        self.branches.push((self.tail, Span::new(at, at + 1)));
        self.zone = Zone::AfterOpen;
        self.pos += 1;
        Ok(())
    }

    fn close_branch(&mut self) -> Result<(), SmilesError> {
        let at = self.pos;
        self.dangling()?;
        self.no_open_dot()?;
        let Some((parent, _)) = self.branches.pop() else {
            return Err(SmilesError::at(ErrorCode::UnmatchedCloseBranch, at, at + 1));
        };
        if matches!(self.zone, Zone::AfterOpen) {
            return Err(SmilesError::at(ErrorCode::EmptyBranch, at, at + 1));
        }
        self.tail = parent;
        self.zone = Zone::AfterClose;
        self.pos += 1;
        Ok(())
    }

    /// 3.4: a digit or `%nn` directly after its atom (or after that atom's bond symbol).
    fn ring_token(&mut self) -> Result<(), SmilesError> {
        let at = self.pos;
        if self.bytes[at] == b'%' && at + 1 < self.end && self.bytes[at + 1] == b'(' {
            // %(nnn): valid in some dialects, not implemented here. Anything else after %( is
            // malformed.
            let digits = self.bytes[at + 2..self.end]
                .iter()
                .take_while(|byte| byte.is_ascii_digit())
                .count();
            let close = at + 2 + digits;
            return Err(
                if digits > 0 && close < self.end && self.bytes[close] == b')' {
                    SmilesError::at(ErrorCode::ExtendedRingLabelUnsupported, at, close + 1)
                } else {
                    SmilesError::at(ErrorCode::RingLabelSyntax, at, at + 1)
                },
            );
        }
        let (label, length) = if self.bytes[at] == b'%' {
            let mut digits = 0;
            while digits < 2
                && at + 1 + digits < self.end
                && self.bytes[at + 1 + digits].is_ascii_digit()
            {
                digits += 1;
            }
            if digits != 2 {
                return Err(SmilesError::at(
                    ErrorCode::RingLabelSyntax,
                    at,
                    at + 1 + digits,
                ));
            }
            let label: usize = self.text[at + 1..at + 3].parse().expect("two ASCII digits");
            (label, 3)
        } else {
            (usize::from(self.bytes[at] - b'0'), 1)
        };
        let span = Span::new(at, at + length);
        let code = match self.zone {
            Zone::AfterAtom => None,
            Zone::AfterOpen => Some(ErrorCode::RingDigitAfterBranchOpen),
            Zone::AfterClose => Some(ErrorCode::RingDigitAfterBranchClose),
            Zone::Detached => Some(ErrorCode::RingDigitWithoutAtom),
        };
        if let Some(code) = code {
            return Err(SmilesError::at(code, span.start, span.end));
        }
        let atom = self.tail.expect("an atom precedes ring digits");
        let symbol = self.pending.take();
        match self.rings[label].take() {
            None => {
                self.rings[label] = Some(OpenRing {
                    atom,
                    symbol,
                    span: Span::new(symbol.map_or(span.start, |(_, s)| s.start), span.end),
                });
            }
            Some(open) => {
                if open.atom == atom {
                    return Err(SmilesError::at(
                        ErrorCode::RingClosureSelf,
                        span.start,
                        span.end,
                    ));
                }
                let merged = match (open.symbol, symbol) {
                    (Some((a, _)), Some((b, _))) if a != b => {
                        return Err(SmilesError::at(
                            ErrorCode::RingBondConflict,
                            span.start,
                            span.end,
                        ));
                    }
                    (Some(symbol), _) | (None, Some(symbol)) => Some(symbol),
                    (None, None) => None,
                };
                let key = (open.atom.min(atom), open.atom.max(atom));
                if !self.bonded.insert(key) {
                    return Err(SmilesError::at(
                        ErrorCode::RingClosureDuplicateBond,
                        span.start,
                        span.end,
                    ));
                }
                self.check_room(span)?;
                self.bonds.push(RawBond {
                    begin: open.atom,
                    end: atom,
                    symbol: merged,
                });
            }
        }
        self.pos += length;
        Ok(())
    }

    /// Decides bond orders, checks aromaticity against the shared model and derives bracket
    /// atoms' radical states from the shared valence model.
    fn finish(self, ignored_text: Option<Span>) -> Result<Read, SmilesError> {
        let count = self.atoms.len();
        let edges: Vec<(usize, usize)> = self.bonds.iter().map(|b| (b.begin, b.end)).collect();
        let bridge = bridges(count, &edges);
        let mut orders = Vec::with_capacity(self.bonds.len());
        let mut aromatic_bond = vec![false; count];
        let mut ring_bond = vec![false; count];
        for (index, bond) in self.bonds.iter().enumerate() {
            let order = match bond.symbol {
                Some((Symbol::Single, _)) => BondOrder::Single,
                Some((Symbol::Double, _)) => BondOrder::Double,
                Some((Symbol::Triple, _)) => BondOrder::Triple,
                Some((Symbol::Aromatic, span)) => {
                    if bridge[index] {
                        return Err(SmilesError::at(
                            ErrorCode::AromaticBondOutsideRing,
                            span.start,
                            span.end,
                        ));
                    }
                    BondOrder::Aromatic
                }
                // 3.2: between two aromatic atoms the unwritten bond is aromatic; here only
                // in a ring. A bridge between two rings (biphenyl) is single.
                None if self.atoms[bond.begin].aromatic
                    && self.atoms[bond.end].aromatic
                    && !bridge[index] =>
                {
                    BondOrder::Aromatic
                }
                None => BondOrder::Single,
            };
            if order == BondOrder::Aromatic {
                aromatic_bond[bond.begin] = true;
                aromatic_bond[bond.end] = true;
            }
            if !bridge[index] {
                ring_bond[bond.begin] = true;
                ring_bond[bond.end] = true;
            }
            orders.push(order);
        }
        for (index, atom) in self.atoms.iter().enumerate() {
            if atom.aromatic && !aromatic_bond[index] {
                let code = if ring_bond[index] {
                    ErrorCode::AromaticAtomNotInAromaticRing
                } else {
                    ErrorCode::AromaticAtomOutsideRing
                };
                return Err(SmilesError::at(code, atom.span.start, atom.span.end));
            }
        }

        let mut read = Read {
            atoms: self
                .atoms
                .iter()
                .map(|raw| ReadAtom {
                    atom: raw.atom.clone(),
                    span: raw.span,
                    bracket: raw.bracket,
                    radical_note: RadicalNote::Stated,
                })
                .collect(),
            bonds: self
                .bonds
                .iter()
                .zip(orders)
                .map(|(bond, order)| ReadBond {
                    begin: bond.begin,
                    end: bond.end,
                    order,
                })
                .collect(),
            components: components(count, &edges),
            ignored_text,
        };
        let graph = read.graph();
        let assignment: BTreeMap<_, _> =
            if read.bonds.iter().any(|b| b.order == BondOrder::Aromatic) {
                let interpretation = aromaticity::perceive(&graph);
                match interpretation.kekulize() {
                    Ok(assignment) => assignment.into_iter().collect(),
                    Err(reason) => {
                        let atom = failing_atom(&interpretation, &read);
                        let span = self.atoms[atom].span;
                        return Err(SmilesError::at(
                            ErrorCode::AromaticSystemUnsupported,
                            span.start,
                            span.end,
                        )
                        .because(reason));
                    }
                }
            } else {
                BTreeMap::new()
            };
        let incident = chemistry::incident_orders(
            graph.atoms().map(|(id, _)| id),
            graph.bonds(),
            |id, bond| aromaticity::kekule_order_value(&assignment, id, bond),
        );
        for (index, (id, _)) in graph.atoms().enumerate() {
            let entry = &mut read.atoms[index];
            if !entry.bracket {
                continue;
            }
            let Hydrogens::Specified(stated) = entry.atom.hydrogens else {
                continue;
            };
            let (state, note) = radical::infer(&entry.atom, stated, &incident[&id]);
            entry.atom.radical = state;
            entry.radical_note = note;
            debug_assert!(state != Radical::Triplet && state != Radical::Singlet);
        }
        Ok(read)
    }
}

/// Connected components by union-find with path halving.
fn components(count: usize, edges: &[(usize, usize)]) -> usize {
    let mut parent: Vec<usize> = (0..count).collect();
    fn root(parent: &mut [usize], mut v: usize) -> usize {
        while parent[v] != v {
            parent[v] = parent[parent[v]];
            v = parent[v];
        }
        v
    }
    let mut parts = count;
    for &(a, b) in edges {
        let (x, y) = (root(&mut parent, a), root(&mut parent, b));
        if x != y {
            parent[y] = x;
            parts -= 1;
        }
    }
    parts
}

/// The first atom (in input order) of an aromatic bond in a system the model could not
/// interpret, so the error points into the system that failed and not at the first aromatic
/// atom of the input.
fn failing_atom(interpretation: &aromaticity::Interpretation, read: &Read) -> usize {
    let aromatic_ends = |system: &aromaticity::System| {
        system
            .bonds
            .iter()
            .filter_map(|id| {
                let index = usize::try_from(id.serial()).ok()?.checked_sub(1)?;
                read.bonds.get(index)
            })
            .filter(|bond| bond.order == BondOrder::Aromatic)
            .map(|bond| bond.begin.min(bond.end))
            .min()
    };
    interpretation
        .systems
        .iter()
        .filter(|system| system.classification != aromaticity::Classification::Aromatic)
        .find_map(aromatic_ends)
        .or_else(|| {
            read.bonds
                .iter()
                .filter(|bond| bond.order == BondOrder::Aromatic)
                .map(|bond| bond.begin.min(bond.end))
                .min()
        })
        .unwrap_or(0)
}
