//! The CXSMILES extension block after the first whitespace, reduced to the enhanced stereo
//! groups.
//!
//! Syntax source: the RDKit Book, "Support for Enhanced Stereochemistry"
//! (<https://www.rdkit.org/docs/RDKit_Book.html>), whose examples are
//! `C[C@H](F)C[C@H](O)Cl |a:4,&1:1|` and `|a:3,11,o1:14|`: a block is `|` fields `|`, the fields
//! are comma separated, a field name is `a:` (absolute), `o<n>:` (or) or `&<n>:` (and), and the
//! comma separated numbers after a name are 0-based atom indexes in the order the atoms are
//! written. That page shows no field without a number after `o` or `&`, so `o:` and `&:` are
//! refused as unsupported rather than guessed. Every other field is refused with its position.

use chemifree_application::smiles::{SmilesCode, SmilesError, SmilesGroup, Span};
use chemifree_domain::stereo_group::GroupKind;

fn at(code: SmilesCode, span: Span) -> SmilesError {
    SmilesError::at(code, span)
}

/// One field's entry as written: the group it names and its indexes, each with its span.
struct Entry {
    kind: GroupKind,
    number: Option<i32>,
    span: Span,
    atoms: Vec<(usize, Span)>,
}

/// The group header at the start of `token`: kind, number and the text after the colon.
/// `None` for a field this reader does not support.
fn header(token: &str) -> Option<(GroupKind, Option<i32>, &str)> {
    let (name, rest) = token.split_once(':')?;
    if name == "a" {
        return Some((GroupKind::Absolute, None, rest));
    }
    let kind = match name.as_bytes().first()? {
        b'o' => GroupKind::Or,
        b'&' => GroupKind::And,
        _ => return None,
    };
    let digits = &name[1..];
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // A number that does not fit is not a group number the drawing can hold.
    let number: i32 = digits.parse().ok()?;
    Some((kind, Some(number), rest))
}

/// Reads the block `source[open..=close]` (`open` and `close` are the bars) into groups.
/// `atom_count` is the number of atoms of the text and `is_center` tells whether an atom carries
/// a tetrahedral mark.
pub(super) fn parse(
    source: &str,
    open: usize,
    close: usize,
    atom_count: usize,
    is_center: impl Fn(usize) -> bool,
) -> Result<Vec<SmilesGroup>, SmilesError> {
    let malformed = |span| at(SmilesCode::CxsmilesMalformed, span);
    let body_start = open + 1;
    let body = &source[body_start..close];
    if body.is_empty() {
        return Err(malformed(Span::new(open, close + 1 - open)));
    }
    let mut entries: Vec<Entry> = Vec::new();
    let mut offset = body_start;
    for token in body.split(',') {
        let span = Span::new(offset, token.len());
        offset += token.len() + 1;
        if token.is_empty() {
            return Err(malformed(span));
        }
        if token.bytes().all(|b| b.is_ascii_digit()) {
            let Some(entry) = entries.last_mut() else {
                return Err(malformed(span));
            };
            let index = token.parse().map_err(|_| malformed(span))?;
            entry.atoms.push((index, span));
            continue;
        }
        let Some((kind, number, rest)) = header(token) else {
            return Err(at(SmilesCode::CxsmilesFieldUnsupported, span));
        };
        if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_digit()) {
            return Err(malformed(span));
        }
        let first = token.len() - rest.len();
        entries.push(Entry {
            kind,
            number,
            span,
            atoms: vec![(
                rest.parse().map_err(|_| malformed(span))?,
                Span::new(span.offset + first, rest.len()),
            )],
        });
    }
    let mut groups: Vec<SmilesGroup> = Vec::new();
    let mut seen = vec![false; atom_count];
    for entry in entries {
        let position = groups
            .iter()
            .position(|group| group.kind == entry.kind && group.number == entry.number);
        let group = match position {
            Some(position) => &mut groups[position],
            None => {
                groups.push(SmilesGroup {
                    kind: entry.kind,
                    number: entry.number,
                    atoms: Vec::new(),
                    span: entry.span,
                });
                groups.last_mut().expect("a group was just pushed")
            }
        };
        for (index, span) in entry.atoms {
            if index >= atom_count {
                return Err(at(SmilesCode::CxsmilesAtomOutOfRange, span));
            }
            if !is_center(index) {
                return Err(at(SmilesCode::CxsmilesGroupMemberNotStereocenter, span));
            }
            if std::mem::replace(&mut seen[index], true) {
                return Err(at(SmilesCode::CxsmilesMemberRepeated, span));
            }
            group.atoms.push(index);
        }
    }
    Ok(groups)
}
