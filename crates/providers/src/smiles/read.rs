//! The SMILES reader.
//!
//! Grammar decisions cite OpenSMILES 1.0 (https://opensmiles.github.io/opensmiles/opensmiles.html):
//! "Grammar" (the productions and the terminator), "Atoms" (bracket atoms: isotope, symbol,
//! hydrogens, charge, class), "Organic Subset" (bare atoms), "Bonds", "Branches", "Ring
//! Closures", "Disconnected Structures" and "Aromaticity". The reader is a single left-to-right
//! pass over bytes with an explicit stack: it never recurses, so nesting cannot overflow the
//! call stack, and every index is checked.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use chemifree_application::smiles_codec::{
    RadicalReason, ReadNote, SmilesAtom, SmilesBond, SmilesRead, SourceSpan,
};
use chemifree_domain::chemistry::{self, Finding};
use chemifree_domain::document::{
    Atom, AtomId, Bond, BondId, BondOrder, MAX_CHARGE, MAX_ISOTOPE_MASS_NUMBER, MIN_CHARGE,
};
use chemifree_domain::element::Element;
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::rings;
use chemifree_domain::states::{AtomKind, Hydrogens, Radical};

use super::error::{Code, Failure};
use super::resolve::{self, Resolution};
use super::tables::{self, AROMATIC, BARE_ORGANIC};
use super::{MAX_BRANCH_DEPTH, MAX_COMMANDS, MAX_INPUT_BYTES};

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

/// A bond symbol and the byte it stood at.
#[derive(Clone, Copy, Debug)]
struct Written {
    symbol: Symbol,
    at: usize,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum State {
    Start,
    AfterAtom,
    AfterOpen,
    AfterClose,
    AfterDot,
}

struct ParsedAtom {
    atom: Atom,
    aromatic: bool,
    bracket: bool,
    span: (usize, usize),
}

struct ParsedBond {
    begin: usize,
    end: usize,
    written: Option<Written>,
}

struct OpenRing {
    atom: usize,
    written: Option<Written>,
    /// The opening token, including a bond symbol before the digits.
    span: (usize, usize),
}

fn char_len(text: &str, at: usize) -> usize {
    text[at..].chars().next().map_or(1, char::len_utf8)
}

fn digit(bytes: &[u8], at: usize) -> Option<u8> {
    bytes
        .get(at)
        .filter(|byte| byte.is_ascii_digit())
        .map(|byte| byte - b'0')
}

struct Reader<'a> {
    text: &'a str,
    bytes: &'a [u8],
    atoms: Vec<ParsedAtom>,
    bonds: Vec<ParsedBond>,
    bonded: HashSet<(usize, usize)>,
    parts: usize,
}

impl Reader<'_> {
    fn add_bond(
        &mut self,
        begin: usize,
        end: usize,
        written: Option<Written>,
        span: (usize, usize),
    ) -> Result<(), Failure> {
        let key = (begin.min(end), begin.max(end));
        if !self.bonded.insert(key) {
            return Err(Failure::at(Code::DuplicateBond, span.0, span.1));
        }
        self.bonds.push(ParsedBond {
            begin,
            end,
            written,
        });
        if self.atoms.len() + self.bonds.len() > MAX_COMMANDS {
            return Err(Failure::at(Code::CommandLimit, span.0, span.1));
        }
        Ok(())
    }

    fn bare_atom(&self, at: usize) -> Option<ParsedAtom> {
        let rest = &self.bytes[at..];
        let element_atom = |symbol: &str, aromatic: bool| {
            let element = Element::from_symbol(symbol)?;
            Some(ParsedAtom {
                atom: Atom::new(AtomKind::Element(element)),
                aromatic,
                bracket: false,
                span: (at, at + symbol.len()),
            })
        };
        for symbol in BARE_ORGANIC {
            if rest.starts_with(symbol.as_bytes()) {
                return element_atom(symbol, false);
            }
        }
        AROMATIC
            .iter()
            .filter(|symbol| symbol.bare)
            .find(|symbol| rest.starts_with(symbol.lower.as_bytes()))
            .and_then(|symbol| element_atom(symbol.element, true))
    }

    /// A bracket atom starting at `[` (OpenSMILES, "Atoms").
    fn bracket_atom(&self, start: usize) -> Result<ParsedAtom, Failure> {
        let bytes = self.bytes;
        let unclosed = || Failure::at(Code::UnclosedBracket, start, start + 1);
        let mut at = start + 1;
        // Isotope: up to three digits, leading zeros allowed ("Isotopes").
        let digits_start = at;
        while digit(bytes, at).is_some() {
            at += 1;
        }
        let mut isotope = None;
        if at > digits_start {
            if at - digits_start > 3 {
                return Err(Failure::at(Code::IsotopeTooLong, digits_start, at));
            }
            let value = self.text[digits_start..at]
                .bytes()
                .fold(0_u16, |sum, byte| sum * 10 + u16::from(byte - b'0'));
            if value == 0 {
                return Err(Failure::at(Code::IsotopeZeroUnsupported, digits_start, at));
            }
            debug_assert!(value <= MAX_ISOTOPE_MASS_NUMBER, "three digits fit the range");
            isotope = Some(value);
        }
        // Symbol.
        let Some(&first) = bytes.get(at) else {
            return Err(unclosed());
        };
        if tables::is_whitespace(first) {
            return Err(unclosed());
        }
        let (element, aromatic) = match first {
            b']' => return Err(Failure::at(Code::EmptyBracket, start, at + 1)),
            b'*' => return Err(Failure::at(Code::WildcardUnsupported, at, at + 1)),
            b'A'..=b'Z' => {
                let two = bytes
                    .get(at + 1)
                    .filter(|byte| byte.is_ascii_lowercase())
                    .and_then(|second| {
                        Element::from_symbol(&format!("{}{}", char::from(first), char::from(*second)))
                    });
                if let Some(element) = two {
                    at += 2;
                    (element, false)
                } else if let Some(element) = Element::from_symbol(&char::from(first).to_string())
                {
                    at += 1;
                    (element, false)
                } else {
                    return Err(Failure::at(Code::UnknownElement, at, at + 1));
                }
            }
            b'a'..=b'z' => {
                let rest = &bytes[at..];
                let Some(symbol) = AROMATIC
                    .iter()
                    .find(|symbol| rest.starts_with(symbol.lower.as_bytes()))
                else {
                    return Err(Failure::at(Code::UnknownElement, at, at + 1));
                };
                let element = Element::from_symbol(symbol.element)
                    .ok_or_else(|| Failure::at(Code::UnknownElement, at, at + 1))?;
                at += symbol.lower.len();
                (element, true)
            }
            _ => {
                return Err(Failure::at(
                    Code::BracketUnexpectedCharacter,
                    at,
                    at + char_len(self.text, at),
                ));
            }
        };
        // Chirality (not supported).
        if bytes.get(at) == Some(&b'@') {
            return Err(Failure::at(Code::StereoUnsupported, at, at + 1));
        }
        // Hydrogens: `H` or `H` and one digit ("Hydrogens").
        let mut hydrogens = 0_u8;
        if bytes.get(at) == Some(&b'H') {
            at += 1;
            hydrogens = 1;
            if let Some(count) = digit(bytes, at) {
                hydrogens = count;
                at += 1;
            }
        }
        // Charge ("Charge"): `+`, `-`, a sign and one or two digits, or the deprecated `++`, `--`.
        let mut charge = 0_i8;
        if let Some(&sign) = bytes.get(at).filter(|byte| matches!(byte, b'+' | b'-')) {
            let charge_start = at;
            at += 1;
            let magnitude: u32 = if bytes.get(at) == Some(&sign) {
                at += 1;
                2
            } else if let Some(first) = digit(bytes, at) {
                at += 1;
                match digit(bytes, at) {
                    Some(second) => {
                        at += 1;
                        u32::from(first) * 10 + u32::from(second)
                    }
                    None => u32::from(first),
                }
            } else {
                1
            };
            if digit(bytes, at).is_some() || matches!(bytes.get(at), Some(b'+' | b'-')) {
                let mut end = at;
                while digit(bytes, end).is_some() || matches!(bytes.get(end), Some(b'+' | b'-')) {
                    end += 1;
                }
                return Err(Failure::at(Code::ChargeSyntax, charge_start, end));
            }
            let signed = if sign == b'-' {
                -i32::try_from(magnitude).unwrap_or(i32::MAX)
            } else {
                i32::try_from(magnitude).unwrap_or(i32::MAX)
            };
            charge = match i8::try_from(signed) {
                Ok(value) if (MIN_CHARGE..=MAX_CHARGE).contains(&value) => value,
                _ => return Err(Failure::at(Code::ChargeOutOfRange, charge_start, at)),
            };
        }
        // Atom class (not supported).
        if bytes.get(at) == Some(&b':') && digit(bytes, at + 1).is_some() {
            let mut end = at + 1;
            while digit(bytes, end).is_some() {
                end += 1;
            }
            return Err(Failure::at(Code::AtomClassUnsupported, at, end));
        }
        match bytes.get(at) {
            Some(b']') => {}
            Some(&byte) if !tables::is_whitespace(byte) => {
                return Err(Failure::at(
                    Code::BracketUnexpectedCharacter,
                    at,
                    at + char_len(self.text, at),
                ));
            }
            _ => return Err(unclosed()),
        }
        let mut atom = Atom::new(AtomKind::Element(element));
        atom.charge = charge;
        atom.isotope = isotope;
        atom.hydrogens = Hydrogens::Specified(hydrogens);
        Ok(ParsedAtom {
            atom,
            aromatic,
            bracket: true,
            span: (start, at + 1),
        })
    }
}

/// Where the SMILES ends: the first whitespace byte, or the end of the text.
fn terminator(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .position(|&byte| tables::is_whitespace(byte))
        .unwrap_or(bytes.len())
}

pub(super) fn read(text: &str) -> Result<SmilesRead, Failure> {
    let bytes = text.as_bytes();
    if bytes.is_empty() {
        return Err(Failure::at(Code::EmptyInput, 0, 0));
    }
    if bytes.len() > MAX_INPUT_BYTES {
        let mut start = MAX_INPUT_BYTES;
        while !text.is_char_boundary(start) {
            start -= 1;
        }
        return Err(Failure::at(Code::InputTooLong, start, bytes.len()));
    }
    if tables::is_whitespace(bytes[0]) {
        return Err(Failure::at(Code::LeadingWhitespace, 0, 1));
    }
    let end = terminator(bytes);
    let mut reader = Reader {
        text,
        bytes,
        atoms: Vec::new(),
        bonds: Vec::new(),
        bonded: HashSet::new(),
        parts: 0,
    };
    let mut state = State::Start;
    let mut previous: Option<usize> = None;
    let mut pending: Option<Written> = None;
    let mut ring_bond: Option<Written> = None;
    // Open parentheses: the atom the branch hangs from and the byte of the `(`.
    let mut branches: Vec<(Option<usize>, usize)> = Vec::new();
    let mut open_rings: BTreeMap<u8, OpenRing> = BTreeMap::new();
    let mut last_dot = 0;
    let mut at = 0;
    while at < end {
        let byte = bytes[at];
        match byte {
            b'(' => {
                if let Some(bond) = pending {
                    return Err(Failure::at(Code::DanglingBond, bond.at, bond.at + 1));
                }
                if !matches!(state, State::AfterAtom | State::AfterClose) {
                    return Err(Failure::at(Code::BranchWithoutAtom, at, at + 1));
                }
                if branches.len() >= MAX_BRANCH_DEPTH {
                    return Err(Failure::at(Code::BranchDepthLimit, at, at + 1));
                }
                branches.push((previous, at));
                state = State::AfterOpen;
                at += 1;
            }
            b')' => {
                if let Some(bond) = pending {
                    return Err(Failure::at(Code::DanglingBond, bond.at, bond.at + 1));
                }
                let Some((parent, open)) = branches.pop() else {
                    return Err(Failure::at(Code::UnmatchedCloseParen, at, at + 1));
                };
                match state {
                    State::AfterOpen => {
                        return Err(Failure::at(Code::EmptyBranch, open, at + 1));
                    }
                    State::AfterDot => {
                        return Err(Failure::at(Code::DanglingDot, last_dot, last_dot + 1));
                    }
                    _ => {}
                }
                previous = parent;
                state = State::AfterClose;
                at += 1;
            }
            b'.' => {
                if let Some(bond) = pending {
                    return Err(Failure::at(Code::DanglingBond, bond.at, bond.at + 1));
                }
                if matches!(state, State::Start | State::AfterDot) {
                    return Err(Failure::at(Code::MisplacedDot, at, at + 1));
                }
                previous = None;
                last_dot = at;
                state = State::AfterDot;
                at += 1;
            }
            b'/' | b'\\' | b'@' => {
                return Err(Failure::at(Code::StereoUnsupported, at, at + 1));
            }
            b'$' => return Err(Failure::at(Code::QuadrupleBondUnsupported, at, at + 1)),
            b'>' => return Err(Failure::at(Code::ReactionUnsupported, at, at + 1)),
            b'*' => return Err(Failure::at(Code::WildcardUnsupported, at, at + 1)),
            b'-' | b'=' | b'#' | b':' => {
                if pending.is_some() {
                    return Err(Failure::at(Code::ConsecutiveBonds, at, at + 1));
                }
                if matches!(state, State::Start | State::AfterDot) {
                    return Err(Failure::at(Code::BondWithoutAtom, at, at + 1));
                }
                let symbol = match byte {
                    b'-' => Symbol::Single,
                    b'=' => Symbol::Double,
                    b'#' => Symbol::Triple,
                    _ => Symbol::Aromatic,
                };
                let written = Written { symbol, at };
                let ring_digit_follows = digit(bytes, at + 1).is_some()
                    || (bytes.get(at + 1) == Some(&b'%') && at + 1 < end);
                if state == State::AfterAtom && ring_digit_follows {
                    ring_bond = Some(written);
                } else {
                    // Where no atom can take a following digit, the digit is the error.
                    pending = Some(written);
                }
                at += 1;
            }
            b'0'..=b'9' | b'%' => {
                match state {
                    State::AfterAtom => {}
                    State::AfterClose => {
                        return Err(Failure::at(Code::RingDigitAfterBranch, at, at + 1));
                    }
                    _ => return Err(Failure::at(Code::RingDigitWithoutAtom, at, at + 1)),
                }
                let Some(atom) = previous else {
                    return Err(Failure::at(Code::RingDigitWithoutAtom, at, at + 1));
                };
                let token_start = ring_bond.map_or(at, |bond| bond.at);
                let (label, next) = if byte == b'%' {
                    if bytes.get(at + 1) == Some(&b'(') {
                        let mut close = at + 2;
                        while digit(bytes, close).is_some() {
                            close += 1;
                        }
                        return Err(if close > at + 2 && bytes.get(close) == Some(&b')') {
                            Failure::at(Code::ExtendedRingLabelUnsupported, at, close + 1)
                        } else {
                            Failure::at(Code::RingLabelSyntax, at, (close).min(end).max(at + 1))
                        });
                    }
                    match (digit(bytes, at + 1), digit(bytes, at + 2)) {
                        (Some(tens), Some(ones)) => (tens * 10 + ones, at + 3),
                        (Some(_), None) => {
                            return Err(Failure::at(Code::RingLabelSyntax, at, at + 2));
                        }
                        _ => return Err(Failure::at(Code::RingLabelSyntax, at, at + 1)),
                    }
                } else {
                    (byte - b'0', at + 1)
                };
                let span = (token_start, next);
                match open_rings.remove(&label) {
                    Some(open) => {
                        if open.atom == atom {
                            return Err(Failure::at(Code::RingSelfBond, span.0, span.1));
                        }
                        let written = match (open.written, ring_bond) {
                            (Some(first), Some(second)) if first.symbol != second.symbol => {
                                return Err(Failure::at(
                                    Code::RingBondConflict,
                                    second.at,
                                    second.at + 1,
                                ));
                            }
                            (first, second) => first.or(second),
                        };
                        reader.add_bond(open.atom, atom, written, span)?;
                    }
                    None => {
                        open_rings.insert(
                            label,
                            OpenRing {
                                atom,
                                written: ring_bond,
                                span,
                            },
                        );
                    }
                }
                ring_bond = None;
                at = next;
            }
            _ => {
                let parsed = match byte {
                    b'[' => reader.bracket_atom(at)?,
                    b'A'..=b'Z' | b'a'..=b'z' => match reader.bare_atom(at) {
                        Some(parsed) => parsed,
                        None => {
                            return Err(Failure::at(Code::UnexpectedCharacter, at, at + 1));
                        }
                    },
                    _ => {
                        return Err(Failure::at(
                            Code::UnexpectedCharacter,
                            at,
                            at + char_len(text, at),
                        ));
                    }
                };
                let span = parsed.span;
                let index = reader.atoms.len();
                reader.atoms.push(parsed);
                if previous.is_none() {
                    reader.parts += 1;
                }
                match previous {
                    Some(parent) => {
                        let written = pending.take();
                        let bond_span = (written.map_or(span.0, |bond| bond.at), span.1);
                        reader.add_bond(parent, index, written, bond_span)?;
                    }
                    None => {
                        if reader.atoms.len() + reader.bonds.len() > MAX_COMMANDS {
                            return Err(Failure::at(Code::CommandLimit, span.0, span.1));
                        }
                    }
                }
                previous = Some(index);
                state = State::AfterAtom;
                at = span.1;
            }
        }
    }
    if let Some(bond) = pending.or(ring_bond) {
        return Err(Failure::at(Code::DanglingBond, bond.at, bond.at + 1));
    }
    if state == State::AfterDot {
        return Err(Failure::at(Code::DanglingDot, last_dot, last_dot + 1));
    }
    if let Some(&(_, open)) = branches.first() {
        return Err(Failure::at(Code::UnclosedBranch, open, open + 1));
    }
    if let Some(open) = open_rings.values().min_by_key(|open| open.span.0) {
        return Err(Failure::at(Code::UnclosedRing, open.span.0, open.span.1));
    }
    let mut notes = Vec::new();
    let rest = bytes[end..]
        .iter()
        .position(|&byte| !tables::is_whitespace(byte))
        .map(|offset| end + offset);
    if let Some(start) = rest {
        notes.push(ReadNote::IgnoredText(SourceSpan {
            start,
            end: bytes.len(),
        }));
    }
    finish(reader, notes)
}

fn atom_id(index: usize) -> AtomId {
    AtomId::from_serial(u32::try_from(index + 1).expect("at most a few hundred atoms"))
        .expect("serials start at one")
}

fn bond_id(index: usize) -> BondId {
    BondId::from_serial(u32::try_from(index + 1).expect("at most a few hundred bonds"))
        .expect("serials start at one")
}

/// Radical state of a bracket atom, from the shared valence model: the first state in which
/// the stated hydrogens and the bonds fit the atom's valence.
fn determine_radical(
    id: AtomId,
    atom: &Atom,
    orders: &[Option<u16>],
) -> (Radical, Option<RadicalReason>) {
    let fits = |radical: Radical| {
        let mut probe = atom.clone();
        probe.radical = radical;
        chemistry::assess(id, &probe, orders).finding == Finding::Valid
    };
    for radical in [Radical::None, Radical::Doublet] {
        if fits(radical) {
            return (radical, None);
        }
    }
    let plain = chemistry::assess(id, atom, orders);
    if plain.reason == Some("element_or_charge") {
        return (Radical::Unknown, Some(RadicalReason::OutsideModel));
    }
    if fits(Radical::Singlet) && fits(Radical::Triplet) {
        (Radical::Unknown, Some(RadicalReason::Ambiguous))
    } else {
        (Radical::Unknown, Some(RadicalReason::NoState))
    }
}

/// Bond orders, aromatic systems, hydrogens and radicals: the semantic pass of OpenSMILES,
/// "Aromaticity" and "Organic Subset".
fn finish(reader: Reader<'_>, mut notes: Vec<ReadNote>) -> Result<SmilesRead, Failure> {
    let Reader {
        atoms, bonds, parts, ..
    } = reader;
    let ids: Vec<AtomId> = (0..atoms.len()).map(atom_id).collect();
    // Which bonds lie on a cycle: the topology alone decides, before any order is chosen.
    let topology = MoleculeGraph::from_parts(
        atoms
            .iter()
            .enumerate()
            .map(|(i, parsed)| (ids[i], parsed.atom.clone()))
            .collect(),
        bonds
            .iter()
            .enumerate()
            .map(|(i, bond)| {
                (
                    bond_id(i),
                    Bond {
                        begin: ids[bond.begin],
                        end: ids[bond.end],
                        order: BondOrder::Single,
                    },
                )
            })
            .collect(),
    );
    let in_cycle: BTreeSet<BondId> = rings::perceive(&topology)
        .into_iter()
        .flat_map(|system| system.bonds)
        .collect();
    let mut orders_chosen = Vec::with_capacity(bonds.len());
    for (i, bond) in bonds.iter().enumerate() {
        let ring = in_cycle.contains(&bond_id(i));
        let order = match bond.written {
            Some(written) if written.symbol == Symbol::Aromatic && !ring => {
                return Err(Failure::at(
                    Code::AromaticBridgeUnsupported,
                    written.at,
                    written.at + 1,
                ));
            }
            Some(written) => written.symbol.order(),
            None if ring && atoms[bond.begin].aromatic && atoms[bond.end].aromatic => {
                BondOrder::Aromatic
            }
            None => BondOrder::Single,
        };
        orders_chosen.push(order);
    }
    for (i, parsed) in atoms.iter().enumerate() {
        if parsed.aromatic
            && !bonds.iter().zip(&orders_chosen).any(|(bond, order)| {
                *order == BondOrder::Aromatic && (bond.begin == i || bond.end == i)
            })
        {
            return Err(Failure::at(
                Code::AromaticAtomOutsideRing,
                parsed.span.0,
                parsed.span.1,
            ));
        }
    }
    let graph = MoleculeGraph::from_parts(
        atoms
            .iter()
            .enumerate()
            .map(|(i, parsed)| (ids[i], parsed.atom.clone()))
            .collect(),
        bonds
            .iter()
            .zip(&orders_chosen)
            .enumerate()
            .map(|(i, (bond, order))| {
                (
                    bond_id(i),
                    Bond {
                        begin: ids[bond.begin],
                        end: ids[bond.end],
                        order: order.clone(),
                    },
                )
            })
            .collect(),
    );
    let resolution: Resolution = resolve::resolve(&graph).map_err(|failure| {
        // Point inside the failing system: its first atom in text order.
        let index = failure
            .atoms
            .iter()
            .map(|id| usize::try_from(id.serial()).unwrap_or(1) - 1)
            .min()
            .or_else(|| atoms.iter().position(|parsed| parsed.aromatic))
            .unwrap_or(0);
        let span = atoms.get(index).map_or((0, 0), |parsed| parsed.span);
        Failure::at(Code::AromaticSystemUnsupported, span.0, span.1).because(failure.reason)
    })?;
    let incident = resolution.incident_orders(&graph);
    let aromatic_atoms: BTreeSet<usize> = bonds
        .iter()
        .zip(&orders_chosen)
        .filter(|(_, order)| **order == BondOrder::Aromatic)
        .flat_map(|(bond, _)| [bond.begin, bond.end])
        .collect();
    let mut out_atoms = Vec::with_capacity(atoms.len());
    for (i, parsed) in atoms.into_iter().enumerate() {
        let mut atom = parsed.atom;
        if parsed.bracket {
            let (radical, reason) = determine_radical(ids[i], &atom, &incident[&ids[i]]);
            if radical != Radical::None && aromatic_atoms.contains(&i) {
                return Err(Failure::at(
                    Code::AromaticRadicalUnsupported,
                    parsed.span.0,
                    parsed.span.1,
                ));
            }
            atom.radical = radical;
            if let Some(reason) = reason {
                notes.push(ReadNote::RadicalUndetermined {
                    atom: i,
                    span: SourceSpan {
                        start: parsed.span.0,
                        end: parsed.span.1,
                    },
                    reason,
                });
            }
        }
        let assessed = chemistry::assess(ids[i], &atom, &incident[&ids[i]]);
        let hydrogens = match atom.hydrogens {
            Hydrogens::Specified(count) => Some(u16::from(count)),
            _ if assessed.finding == Finding::Unsupported => None,
            _ => assessed.hydrogen_count,
        };
        out_atoms.push(SmilesAtom {
            atom,
            span: SourceSpan {
                start: parsed.span.0,
                end: parsed.span.1,
            },
            hydrogens,
        });
    }
    Ok(SmilesRead {
        atoms: out_atoms,
        bonds: bonds
            .iter()
            .zip(orders_chosen)
            .map(|(bond, order)| SmilesBond {
                begin: bond.begin,
                end: bond.end,
                order,
            })
            .collect(),
        notes,
        parts,
    })
}
