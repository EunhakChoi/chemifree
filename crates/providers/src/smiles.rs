//! A bounded, independently implemented SMILES reader and writer for connectivity.
//!
//! Supported: the organic subset, bracket atoms with isotope, hydrogen count and charge,
//! branches, ring closures (`0`-`9`, `%nn`), disconnected components, and aromatic atoms.
//! Refused with a positioned error: stereo marks, wildcards, atom maps, reactions and
//! unusual bond kinds. Whitespace ends the SMILES; later text is reported as ignored.
//! Both directions use explicit stacks, so input depth cannot exhaust the call stack.

use std::collections::HashSet;

use chemifree_application::smiles::{
    MAX_BRANCH_DEPTH, MAX_SMILES_BYTES, MAX_SMILES_ITEMS, MAX_WRITTEN_RING_LABELS, ORGANIC_SUBSET,
    SmilesAtom, SmilesBond, SmilesBondKind, SmilesCodec, SmilesError, SmilesErrorCode,
    SmilesFormatError, SmilesMolecule, SmilesRead, SmilesText,
};
use chemifree_domain::document::{MAX_CHARGE, MAX_ISOTOPE_MASS_NUMBER, MIN_CHARGE};
use chemifree_domain::element::Element;

#[derive(Clone, Copy, Debug, Default)]
pub struct SmilesTextCodec;

impl SmilesCodec for SmilesTextCodec {
    fn read(&self, text: &str) -> Result<SmilesRead, SmilesError> {
        Reader::new(text).run()
    }

    fn write(&self, molecule: &SmilesMolecule) -> Result<SmilesText, SmilesFormatError> {
        Writer::new(molecule)?.run()
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Token {
    Start,
    Atom,
    Ring,
    Open,
    Close,
    Dot,
    Bond,
}

struct OpenRing {
    atom: usize,
    bond: Option<SmilesBondKind>,
    start: usize,
}

struct PendingBond {
    /// `None` when no bond symbol was written; the kind is decided from the finished graph.
    explicit: Option<SmilesBondKind>,
    begin: usize,
    end: usize,
    /// Byte span of the bond symbol, or of the text that created the bond.
    span: (usize, usize),
}

struct Reader<'a> {
    text: &'a str,
    position: usize,
    atoms: Vec<SmilesAtom>,
    bonds: Vec<PendingBond>,
    pairs: HashSet<(usize, usize)>,
    rings: Vec<Option<OpenRing>>,
    /// Parent atom and byte offset of each open parenthesis.
    stack: Vec<(usize, usize)>,
    current: Option<usize>,
    last: Token,
    /// The atom a ring digit may attach to, if the previous token allows one.
    site: Option<usize>,
    site_error: SmilesErrorCode,
    /// A bond symbol waiting for its atom or ring closure, and its offset.
    bond: Option<(SmilesBondKind, usize)>,
    dot: usize,
}

/// The characters that end a SMILES (OpenSMILES): space, tab and line ends. Other Unicode
/// whitespace is not a terminator and is refused where it appears.
fn is_blank(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r')
}

fn fail<T>(code: SmilesErrorCode, start: usize, end: usize) -> Result<T, SmilesError> {
    Err(SmilesError { code, start, end })
}

fn aromatic_element(symbol: &str) -> Option<Element> {
    // Lower-case organic atoms are the subset's upper-case ones; Se and As only in brackets.
    let upper = match symbol {
        "se" => "Se".to_owned(),
        "as" => "As".to_owned(),
        _ if AROMATIC_ORGANIC.contains(&symbol) => symbol.to_ascii_uppercase(),
        _ => return None,
    };
    Element::from_symbol(&upper)
}

/// Aromatic atoms that need no brackets, the lower-case counterparts of the organic subset.
const AROMATIC_ORGANIC: [&str; 6] = ["b", "c", "n", "o", "p", "s"];

/// Which bonds lie on no cycle. Iterative low-link search, linear in atoms plus bonds.
fn bridge_flags(atoms: usize, ends: &[(usize, usize)]) -> Vec<bool> {
    let mut adjacent = vec![Vec::new(); atoms];
    for (edge, &(a, b)) in ends.iter().enumerate() {
        adjacent[a].push((b, edge));
        adjacent[b].push((a, edge));
    }
    let mut discovery = vec![0_usize; atoms];
    let mut low = vec![0_usize; atoms];
    let mut bridge = vec![false; ends.len()];
    let mut time = 0;
    for root in 0..atoms {
        if discovery[root] != 0 {
            continue;
        }
        time += 1;
        discovery[root] = time;
        low[root] = time;
        // (vertex, edge to parent, next neighbour)
        let mut stack = vec![(root, usize::MAX, 0_usize)];
        while let Some(&(v, parent_edge, next)) = stack.last() {
            if let Some(&(w, edge)) = adjacent[v].get(next) {
                if let Some(top) = stack.last_mut() {
                    top.2 += 1;
                }
                if edge == parent_edge {
                    continue;
                }
                if discovery[w] == 0 {
                    time += 1;
                    discovery[w] = time;
                    low[w] = time;
                    stack.push((w, edge, 0));
                } else {
                    low[v] = low[v].min(discovery[w]);
                }
            } else {
                stack.pop();
                if let Some(&(parent, _, _)) = stack.last() {
                    low[parent] = low[parent].min(low[v]);
                    if low[v] > discovery[parent] {
                        bridge[parent_edge] = true;
                    }
                }
            }
        }
    }
    bridge
}

impl<'a> Reader<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            position: 0,
            atoms: Vec::new(),
            bonds: Vec::new(),
            pairs: HashSet::new(),
            rings: (0..100).map(|_| None).collect(),
            stack: Vec::new(),
            current: None,
            last: Token::Start,
            site: None,
            site_error: SmilesErrorCode::RingClosureWithoutAtom,
            bond: None,
            dot: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.text[self.position..].chars().next()
    }

    fn char_span(&self) -> (usize, usize) {
        (
            self.position,
            self.position + self.peek().map_or(0, char::len_utf8),
        )
    }

    fn run(mut self) -> Result<SmilesRead, SmilesError> {
        if self.text.len() > MAX_SMILES_BYTES {
            // Offsets stay on character boundaries: the span starts at the last boundary that
            // fits in the limit.
            let mut start = MAX_SMILES_BYTES;
            while !self.text.is_char_boundary(start) {
                start -= 1;
            }
            return fail(SmilesErrorCode::InputTooLong, start, self.text.len());
        }
        if self.text.is_empty() {
            return fail(SmilesErrorCode::EmptyInput, 0, 0);
        }
        if self.text.starts_with(is_blank) {
            let end = self
                .text
                .char_indices()
                .find(|(_, c)| !is_blank(*c))
                .map_or(self.text.len(), |(i, _)| i);
            return fail(SmilesErrorCode::LeadingWhitespace, 0, end);
        }
        while let Some(c) = self.peek() {
            if is_blank(c) {
                break;
            }
            self.step(c)?;
        }
        self.finish()
    }

    fn step(&mut self, c: char) -> Result<(), SmilesError> {
        let (start, end) = self.char_span();
        match c {
            '(' => self.open_branch(start, end),
            ')' => self.close_branch(start, end),
            '.' => self.dot_token(start, end),
            '0'..='9' => {
                self.position = end;
                self.ring_closure(c as usize - '0' as usize, start, end)
            }
            '%' => self.ring_percent(start),
            '-' | '=' | '#' | ':' | '$' | '/' | '\\' => self.bond_symbol(c, start, end),
            '[' => self.bracket_atom(),
            '*' => fail(SmilesErrorCode::QueryUnsupported, start, end),
            '>' => fail(SmilesErrorCode::ReactionUnsupported, start, end),
            'A'..='Z' | 'a'..='z' => self.organic_atom(c, start),
            _ => fail(SmilesErrorCode::UnexpectedCharacter, start, end),
        }
    }

    fn dangling(&self) -> SmilesError {
        let at = self.bond.map_or(self.position, |(_, at)| at);
        SmilesError {
            code: SmilesErrorCode::DanglingBond,
            start: at,
            end: at + 1,
        }
    }

    fn open_branch(&mut self, start: usize, end: usize) -> Result<(), SmilesError> {
        let attachable = matches!(self.last, Token::Atom | Token::Ring | Token::Close);
        let Some(parent) = self.current.filter(|_| attachable) else {
            return fail(SmilesErrorCode::BranchWithoutAtom, start, end);
        };
        if self.stack.len() >= MAX_BRANCH_DEPTH {
            return fail(SmilesErrorCode::BranchDepthLimit, start, end);
        }
        self.stack.push((parent, start));
        self.position = end;
        self.last = Token::Open;
        self.site = None;
        self.site_error = SmilesErrorCode::RingClosureWithoutAtom;
        Ok(())
    }

    fn close_branch(&mut self, start: usize, end: usize) -> Result<(), SmilesError> {
        match self.last {
            Token::Open => {
                let open = self.stack.last().map_or(start, |&(_, at)| at);
                return fail(SmilesErrorCode::EmptyBranch, open, end);
            }
            Token::Bond => return Err(self.dangling()),
            Token::Dot => return fail(SmilesErrorCode::EmptyComponent, self.dot, self.dot + 1),
            _ => {}
        }
        let Some((parent, _)) = self.stack.pop() else {
            return fail(SmilesErrorCode::UnmatchedBranchClose, start, end);
        };
        self.current = Some(parent);
        self.position = end;
        self.last = Token::Close;
        self.site = None;
        self.site_error = SmilesErrorCode::RingClosureAfterBranch;
        Ok(())
    }

    fn dot_token(&mut self, start: usize, end: usize) -> Result<(), SmilesError> {
        match self.last {
            Token::Atom | Token::Ring | Token::Close | Token::Open => {}
            Token::Bond => return Err(self.dangling()),
            Token::Dot => return fail(SmilesErrorCode::EmptyComponent, self.dot, self.dot + 1),
            Token::Start => return fail(SmilesErrorCode::EmptyComponent, start, end),
        }
        self.dot = start;
        self.current = None;
        self.position = end;
        self.last = Token::Dot;
        self.site = None;
        self.site_error = SmilesErrorCode::RingClosureWithoutAtom;
        Ok(())
    }

    fn bond_symbol(&mut self, c: char, start: usize, end: usize) -> Result<(), SmilesError> {
        let kind = match c {
            '-' => SmilesBondKind::Single,
            '=' => SmilesBondKind::Double,
            '#' => SmilesBondKind::Triple,
            ':' => SmilesBondKind::Aromatic,
            '$' => return fail(SmilesErrorCode::BondKindUnsupported, start, end),
            _ => return fail(SmilesErrorCode::StereoUnsupported, start, end),
        };
        if self.last == Token::Bond {
            return fail(SmilesErrorCode::RepeatedBond, start, end);
        }
        if self.current.is_none() {
            return fail(SmilesErrorCode::BondWithoutSource, start, end);
        }
        self.bond = Some((kind, start));
        self.position = end;
        // The ring site is kept: a ring digit may follow a bond symbol (`C=1`).
        self.last = Token::Bond;
        Ok(())
    }

    fn ring_percent(&mut self, start: usize) -> Result<(), SmilesError> {
        let digits: Vec<u32> = self.text[start + 1..]
            .chars()
            .take(2)
            .map_while(|c| c.to_digit(10))
            .collect();
        if digits.len() < 2 {
            return fail(
                SmilesErrorCode::InvalidRingNumber,
                start,
                start + 1 + digits.len(),
            );
        }
        self.position = start + 3;
        self.ring_closure((digits[0] * 10 + digits[1]) as usize, start, start + 3)
    }

    fn ring_closure(&mut self, number: usize, start: usize, end: usize) -> Result<(), SmilesError> {
        let Some(atom) = self.site else {
            return fail(self.site_error, start, end);
        };
        let symbol = self.bond.take().map(|(kind, _)| kind);
        match self.rings[number].take() {
            None => {
                self.rings[number] = Some(OpenRing {
                    atom,
                    bond: symbol,
                    start,
                });
            }
            Some(open) => {
                if open.atom == atom {
                    return fail(SmilesErrorCode::RingClosureSelf, start, end);
                }
                let explicit = match (open.bond, symbol) {
                    (Some(a), Some(b)) if a != b => {
                        return fail(SmilesErrorCode::RingBondMismatch, start, end);
                    }
                    (Some(kind), _) | (None, Some(kind)) => Some(kind),
                    (None, None) => None,
                };
                self.add_bond(open.atom, atom, explicit, start, end)?;
            }
        }
        self.last = Token::Ring;
        Ok(())
    }

    fn add_bond(
        &mut self,
        begin: usize,
        end: usize,
        explicit: Option<SmilesBondKind>,
        start: usize,
        stop: usize,
    ) -> Result<(), SmilesError> {
        if !self.pairs.insert((begin.min(end), begin.max(end))) {
            return fail(SmilesErrorCode::DuplicateBond, start, stop);
        }
        if self.atoms.len() + self.bonds.len() + 1 > MAX_SMILES_ITEMS {
            return fail(SmilesErrorCode::SizeLimit, start, stop);
        }
        self.bonds.push(PendingBond {
            span: (start, stop),
            explicit,
            begin,
            end,
        });
        Ok(())
    }

    fn push_atom(&mut self, atom: SmilesAtom) -> Result<(), SmilesError> {
        let (start, end) = atom.span;
        if self.atoms.len() + self.bonds.len() + 1 > MAX_SMILES_ITEMS {
            return fail(SmilesErrorCode::SizeLimit, start, end);
        }
        let index = self.atoms.len();
        self.atoms.push(atom);
        if let Some(parent) = self.current {
            let symbol = self.bond.take();
            let explicit = symbol.map(|(kind, _)| kind);
            let (from, to) = symbol.map_or((start, end), |(_, at)| (at, at + 1));
            self.add_bond(parent, index, explicit, from, to)?;
        }
        self.current = Some(index);
        self.site = Some(index);
        self.last = Token::Atom;
        self.position = end;
        Ok(())
    }

    fn organic_atom(&mut self, c: char, start: usize) -> Result<(), SmilesError> {
        let one = &self.text[start..start + 1];
        let two = self.text.get(start..start + 2);
        let symbol = match (c, two) {
            ('C', Some("Cl")) | ('B', Some("Br")) => two.unwrap_or(one),
            _ => one,
        };
        let end = start + symbol.len();
        let (element, aromatic) = if c.is_ascii_uppercase() {
            if ORGANIC_SUBSET.contains(&symbol) {
                (Element::from_symbol(symbol), false)
            } else if Element::from_symbol(symbol).is_some() {
                return fail(SmilesErrorCode::BracketRequired, start, end);
            } else {
                return fail(SmilesErrorCode::UnknownElement, start, end);
            }
        } else if AROMATIC_ORGANIC.contains(&symbol) {
            (aromatic_element(symbol), true)
        } else {
            return fail(SmilesErrorCode::UnexpectedCharacter, start, end);
        };
        let Some(element) = element else {
            return fail(SmilesErrorCode::UnknownElement, start, end);
        };
        self.push_atom(SmilesAtom {
            element,
            aromatic,
            isotope: None,
            charge: 0,
            hydrogens: None,
            span: (start, end),
        })
    }

    /// An error for the character at the cursor inside a bracket that began at `open`.
    fn bracket_fault(&self, open: usize) -> SmilesError {
        let unterminated = SmilesError {
            code: SmilesErrorCode::UnterminatedBracket,
            start: open,
            end: self.position,
        };
        match self.peek() {
            None => unterminated,
            Some(c) if is_blank(c) => unterminated,
            Some(c) => SmilesError {
                code: SmilesErrorCode::InvalidBracketContent,
                start: self.position,
                end: self.position + c.len_utf8(),
            },
        }
    }

    /// The run of ASCII digits that starts `skip` bytes after the cursor.
    fn digits_after(&self, skip: usize) -> usize {
        self.text[self.position + skip..]
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count()
    }

    fn bracket_atom(&mut self) -> Result<(), SmilesError> {
        let open = self.position;
        self.position += 1;
        let mut isotope = None;
        let width = self.digits_after(0);
        if width > 0 {
            let digits = &self.text[self.position..self.position + width];
            let value: u16 = digits.parse().unwrap_or(u16::MAX);
            if width > 3 || !(1..=MAX_ISOTOPE_MASS_NUMBER).contains(&value) {
                return fail(
                    SmilesErrorCode::InvalidIsotope,
                    self.position,
                    self.position + width,
                );
            }
            isotope = Some(value);
            self.position += width;
        }
        let (element, aromatic) = self.bracket_symbol(open)?;
        let mut hydrogens = 0_u8;
        match self.peek() {
            Some('@') => {
                let run = self.text[self.position..]
                    .bytes()
                    .take_while(|b| *b == b'@')
                    .count();
                return fail(
                    SmilesErrorCode::StereoUnsupported,
                    self.position,
                    self.position + run,
                );
            }
            Some('H') => {
                self.position += 1;
                let width = self.digits_after(0);
                if width > 1 {
                    return fail(
                        SmilesErrorCode::InvalidHydrogenCount,
                        self.position - 1,
                        self.position + width,
                    );
                }
                hydrogens = if width == 1 {
                    let digit = self.text.as_bytes()[self.position] - b'0';
                    self.position += 1;
                    digit
                } else {
                    1
                };
            }
            _ => {}
        }
        let charge = self.bracket_charge()?;
        if self.peek() == Some(':') {
            let width = self.digits_after(1);
            return fail(
                SmilesErrorCode::AtomMapUnsupported,
                self.position,
                self.position + 1 + width,
            );
        }
        if self.peek() != Some(']') {
            return Err(self.bracket_fault(open));
        }
        self.position += 1;
        let span = (open, self.position);
        self.push_atom(SmilesAtom {
            element,
            aromatic,
            isotope,
            charge,
            hydrogens: Some(hydrogens),
            span,
        })
    }

    fn bracket_symbol(&mut self, open: usize) -> Result<(Element, bool), SmilesError> {
        let Some(c) = self.peek() else {
            return Err(self.bracket_fault(open));
        };
        let start = self.position;
        if c == '*' {
            return fail(SmilesErrorCode::QueryUnsupported, start, start + 1);
        }
        if c.is_ascii_uppercase() {
            if let Some(two) = self.text.get(start..start + 2)
                && two.as_bytes()[1].is_ascii_lowercase()
                && let Some(element) = Element::from_symbol(two)
            {
                self.position += 2;
                return Ok((element, false));
            }
            return match Element::from_symbol(&self.text[start..start + 1]) {
                Some(element) => {
                    self.position += 1;
                    Ok((element, false))
                }
                None => fail(SmilesErrorCode::UnknownElement, start, start + 1),
            };
        }
        if c.is_ascii_lowercase() {
            for width in [2, 1] {
                if let Some(symbol) = self.text.get(start..start + width)
                    && let Some(element) = aromatic_element(symbol)
                {
                    self.position += width;
                    return Ok((element, true));
                }
            }
            return fail(SmilesErrorCode::UnknownElement, start, start + 1);
        }
        Err(self.bracket_fault(open))
    }

    fn bracket_charge(&mut self) -> Result<i8, SmilesError> {
        let Some(sign) = self.peek().filter(|c| matches!(c, '+' | '-')) else {
            return Ok(0);
        };
        let start = self.position;
        let repeated = self.text[start..]
            .bytes()
            .take_while(|b| *b == sign as u8)
            .count();
        self.position += repeated;
        let width = self.digits_after(0);
        let magnitude = if repeated > 1 {
            if width > 0 {
                return fail(SmilesErrorCode::InvalidCharge, start, self.position + width);
            }
            repeated
        } else if width == 0 {
            1
        } else if width > 2 {
            return fail(SmilesErrorCode::InvalidCharge, start, self.position + width);
        } else {
            let value = self.text[self.position..self.position + width]
                .parse::<usize>()
                .unwrap_or(usize::MAX);
            self.position += width;
            value
        };
        let signed = i64::try_from(magnitude).unwrap_or(i64::MAX);
        let signed = if sign == '-' { -signed } else { signed };
        match i8::try_from(signed) {
            Ok(charge) if (MIN_CHARGE..=MAX_CHARGE).contains(&charge) => Ok(charge),
            _ => fail(SmilesErrorCode::ChargeOutOfRange, start, self.position),
        }
    }

    fn finish(mut self) -> Result<SmilesRead, SmilesError> {
        if self.last == Token::Bond {
            return Err(self.dangling());
        }
        if self.last == Token::Dot {
            return fail(SmilesErrorCode::EmptyComponent, self.dot, self.dot + 1);
        }
        if let Some(&(_, open)) = self.stack.first() {
            return fail(SmilesErrorCode::UnclosedBranch, open, open + 1);
        }
        if let Some(open) = self.rings.iter().flatten().min_by_key(|ring| ring.start) {
            return fail(SmilesErrorCode::UnclosedRing, open.start, open.start + 1);
        }
        let ends: Vec<_> = self.bonds.iter().map(|b| (b.begin, b.end)).collect();
        let bridge = bridge_flags(self.atoms.len(), &ends);
        let bonds: Vec<SmilesBond> = self
            .bonds
            .iter()
            .zip(&bridge)
            .map(|(pending, &bridge)| {
                let both = self.atoms[pending.begin].aromatic && self.atoms[pending.end].aromatic;
                SmilesBond {
                    begin: pending.begin,
                    end: pending.end,
                    kind: pending.explicit.unwrap_or(if both && !bridge {
                        SmilesBondKind::Aromatic
                    } else {
                        SmilesBondKind::Single
                    }),
                }
            })
            .collect();
        // An aromatic bond must lie on a cycle, and an aromatic atom needs such a bond.
        let mut has_aromatic_bond = vec![false; self.atoms.len()];
        for (bond, &bridge) in bonds.iter().zip(&bridge) {
            if bond.kind != SmilesBondKind::Aromatic {
                continue;
            }
            if !bridge {
                has_aromatic_bond[bond.begin] = true;
                has_aromatic_bond[bond.end] = true;
            }
        }
        if let Some((atom, _)) = self
            .atoms
            .iter()
            .zip(&has_aromatic_bond)
            .find(|(atom, has)| atom.aromatic && !**has)
        {
            return fail(
                SmilesErrorCode::AromaticAtomOutsideRing,
                atom.span.0,
                atom.span.1,
            );
        }
        if let Some((_, pending)) = bonds
            .iter()
            .zip(&self.bonds)
            .zip(&bridge)
            .find(|((bond, _), bridge)| bond.kind == SmilesBondKind::Aromatic && **bridge)
            .map(|((bond, pending), _)| (bond, pending))
        {
            return fail(
                SmilesErrorCode::AromaticBondOutsideRing,
                pending.span.0,
                pending.span.1,
            );
        }
        let rest = &self.text[self.position..];
        let trimmed = rest.trim_matches(is_blank);
        let ignored = (!trimmed.is_empty()).then(|| {
            let start = self.position + (rest.len() - rest.trim_start_matches(is_blank).len());
            (start, start + trimmed.len())
        });
        let atoms = std::mem::take(&mut self.atoms);
        Ok(SmilesRead {
            molecule: SmilesMolecule { atoms, bonds },
            ignored,
        })
    }
}

struct Writer<'a> {
    molecule: &'a SmilesMolecule,
    adjacent: Vec<Vec<(usize, usize)>>,
    bridge: Vec<bool>,
}

struct Plan {
    children: Vec<Vec<(usize, usize)>>,
    roots: Vec<usize>,
    /// For each bond that is not a tree edge: (ancestor, descendant).
    rings: Vec<Option<(usize, usize)>>,
}

fn element_text(atom: &SmilesAtom) -> Result<String, SmilesFormatError> {
    let symbol = atom.element.symbol();
    if atom.aromatic {
        let lower = symbol.to_ascii_lowercase();
        return if aromatic_element(&lower) == Some(atom.element) {
            Ok(lower)
        } else {
            Err(SmilesFormatError::NotWritable)
        };
    }
    Ok(symbol.to_owned())
}

fn atom_text(atom: &SmilesAtom) -> Result<String, SmilesFormatError> {
    let symbol = element_text(atom)?;
    let Some(hydrogens) = atom.hydrogens else {
        let bare = atom.charge == 0
            && atom.isotope.is_none()
            && ORGANIC_SUBSET.contains(&atom.element.symbol());
        let aromatic_ok = !atom.aromatic || AROMATIC_ORGANIC.contains(&symbol.as_str());
        return if bare && aromatic_ok {
            Ok(symbol)
        } else {
            Err(SmilesFormatError::NotWritable)
        };
    };
    if hydrogens > 9 || !(MIN_CHARGE..=MAX_CHARGE).contains(&atom.charge) {
        return Err(SmilesFormatError::NotWritable);
    }
    let mut out = String::from("[");
    if let Some(mass) = atom.isotope {
        out.push_str(&mass.to_string());
    }
    out.push_str(&symbol);
    match hydrogens {
        0 => {}
        1 => out.push('H'),
        n => {
            out.push('H');
            out.push_str(&n.to_string());
        }
    }
    match atom.charge {
        0 => {}
        1 => out.push('+'),
        -1 => out.push('-'),
        n if n > 0 => out.push_str(&format!("+{n}")),
        n => out.push_str(&format!("-{}", n.unsigned_abs())),
    }
    out.push(']');
    Ok(out)
}

fn ring_label(number: usize) -> String {
    if number < 10 {
        number.to_string()
    } else {
        format!("%{number:02}")
    }
}

impl<'a> Writer<'a> {
    fn new(molecule: &'a SmilesMolecule) -> Result<Self, SmilesFormatError> {
        if molecule.atoms.len() + molecule.bonds.len() > MAX_SMILES_ITEMS {
            return Err(SmilesFormatError::SizeLimit);
        }
        let n = molecule.atoms.len();
        let mut seen = HashSet::new();
        let mut adjacent = vec![Vec::new(); n];
        let mut ends = Vec::with_capacity(molecule.bonds.len());
        for (edge, bond) in molecule.bonds.iter().enumerate() {
            if bond.begin >= n
                || bond.end >= n
                || bond.begin == bond.end
                || !seen.insert((bond.begin.min(bond.end), bond.begin.max(bond.end)))
            {
                return Err(SmilesFormatError::NotWritable);
            }
            adjacent[bond.begin].push((bond.end, edge));
            adjacent[bond.end].push((bond.begin, edge));
            ends.push((bond.begin, bond.end));
        }
        for list in &mut adjacent {
            list.sort_unstable();
        }
        let bridge = bridge_flags(n, &ends);
        Ok(Self {
            molecule,
            adjacent,
            bridge,
        })
    }

    /// Depth-first tree in neighbour order; small subtrees come first so that nesting stays
    /// shallow. The plan depends only on the ordered graph.
    fn plan(&self) -> Plan {
        let n = self.molecule.atoms.len();
        let mut visited = vec![false; n];
        let mut parent_edge = vec![usize::MAX; n];
        let mut children = vec![Vec::new(); n];
        let mut rings = vec![None; self.molecule.bonds.len()];
        let mut roots = Vec::new();
        for root in 0..n {
            if visited[root] {
                continue;
            }
            visited[root] = true;
            roots.push(root);
            let mut stack = vec![(root, 0_usize)];
            while let Some(&(v, next)) = stack.last() {
                let Some(&(w, edge)) = self.adjacent[v].get(next) else {
                    stack.pop();
                    continue;
                };
                if let Some(top) = stack.last_mut() {
                    top.1 += 1;
                }
                if edge == parent_edge[v] {
                    continue;
                }
                if !visited[w] {
                    visited[w] = true;
                    parent_edge[w] = edge;
                    children[v].push((w, edge));
                    stack.push((w, 0));
                } else if rings[edge].is_none() {
                    // First seen from the descendant, whose ancestor `w` is still open.
                    rings[edge] = Some((w, v));
                }
            }
        }
        let mut size = vec![1_usize; n];
        for v in Self::bottom_up(&children, &roots) {
            for &(c, _) in &children[v] {
                size[v] += size[c];
            }
        }
        for list in &mut children {
            list.sort_by_key(|&(c, e)| (size[c], c, e));
        }
        Plan {
            children,
            roots,
            rings,
        }
    }

    /// Vertices so that every vertex follows all of its descendants.
    fn bottom_up(children: &[Vec<(usize, usize)>], roots: &[usize]) -> Vec<usize> {
        let mut out = Vec::with_capacity(children.len());
        let mut stack: Vec<usize> = roots.to_vec();
        while let Some(v) = stack.pop() {
            out.push(v);
            stack.extend(children[v].iter().map(|&(c, _)| c));
        }
        out.reverse();
        out
    }

    fn bond_text(&self, edge: usize) -> &'static str {
        let bond = self.molecule.bonds[edge];
        let both =
            self.molecule.atoms[bond.begin].aromatic && self.molecule.atoms[bond.end].aromatic;
        match bond.kind {
            SmilesBondKind::Single if both => "-",
            SmilesBondKind::Single => "",
            SmilesBondKind::Double => "=",
            SmilesBondKind::Triple => "#",
            SmilesBondKind::Aromatic if both && !self.bridge[edge] => "",
            SmilesBondKind::Aromatic => ":",
        }
    }

    fn run(self) -> Result<SmilesText, SmilesFormatError> {
        let plan = self.plan();
        let n = self.molecule.atoms.len();
        let mut emitter = Emitter {
            writer: &self,
            closings: vec![Vec::new(); n],
            openings: vec![Vec::new(); n],
            label_of: vec![0; self.molecule.bonds.len()],
            used: [false; MAX_WRITTEN_RING_LABELS + 1],
            out: String::new(),
            order: Vec::with_capacity(n),
        };
        for (edge, ring) in plan.rings.iter().enumerate() {
            if let Some((ancestor, descendant)) = *ring {
                emitter.openings[ancestor].push((descendant, edge));
                emitter.closings[descendant].push(edge);
            }
        }
        for list in &mut emitter.openings {
            list.sort_unstable();
        }
        for (index, &root) in plan.roots.iter().enumerate() {
            if index > 0 {
                emitter.out.push('.');
            }
            emitter.atom(root, None)?;
            // (atom, next child, whether a parenthesis closes after it)
            let mut stack = vec![(root, 0_usize, false)];
            let mut depth = 0_usize;
            while let Some(&(v, next, paren)) = stack.last() {
                if let Some(&(child, edge)) = plan.children[v].get(next) {
                    if let Some(top) = stack.last_mut() {
                        top.1 += 1;
                    }
                    let last = next + 1 == plan.children[v].len();
                    if !last {
                        depth += 1;
                        if depth > MAX_BRANCH_DEPTH {
                            return Err(SmilesFormatError::BranchDepthLimit);
                        }
                        emitter.out.push('(');
                    }
                    emitter.atom(child, Some(edge))?;
                    stack.push((child, 0, !last));
                } else {
                    stack.pop();
                    if paren {
                        depth -= 1;
                        emitter.out.push(')');
                    }
                }
            }
        }
        if emitter.out.len() > MAX_SMILES_BYTES {
            return Err(SmilesFormatError::OutputLimit);
        }
        Ok(SmilesText {
            text: emitter.out,
            atom_order: emitter.order,
        })
    }
}

struct Emitter<'w, 'a> {
    writer: &'w Writer<'a>,
    closings: Vec<Vec<usize>>,
    openings: Vec<Vec<(usize, usize)>>,
    label_of: Vec<usize>,
    used: [bool; MAX_WRITTEN_RING_LABELS + 1],
    out: String,
    order: Vec<usize>,
}

impl Emitter<'_, '_> {
    fn atom(&mut self, v: usize, via: Option<usize>) -> Result<(), SmilesFormatError> {
        if let Some(edge) = via {
            self.out.push_str(self.writer.bond_text(edge));
        }
        self.order.push(v);
        self.out
            .push_str(&atom_text(&self.writer.molecule.atoms[v])?);
        let mut closed: Vec<usize> = self.closings[v].iter().map(|&e| self.label_of[e]).collect();
        closed.sort_unstable();
        for &label in &closed {
            self.out.push_str(&ring_label(label));
        }
        // Labels closed here are free again for rings opened at this same atom.
        for &label in &closed {
            self.used[label] = false;
        }
        for index in 0..self.openings[v].len() {
            let (_, edge) = self.openings[v][index];
            let Some(label) = (1..=MAX_WRITTEN_RING_LABELS).find(|&l| !self.used[l]) else {
                return Err(SmilesFormatError::RingLabelLimit);
            };
            self.used[label] = true;
            self.label_of[edge] = label;
            self.out.push_str(self.writer.bond_text(edge));
            self.out.push_str(&ring_label(label));
        }
        Ok(())
    }
}
