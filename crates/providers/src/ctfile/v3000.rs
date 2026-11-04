//! The V3000 connection table: a logical-line reader, a block parser and a writer for the core
//! subset (element atoms with coordinates, bonds of order 1 through 4, charge, absolute isotope,
//! radical, and the stereo collections carried unmapped).
//!
//! Everything outside that subset is refused with a closed `v3000_*` reason, never skipped:
//! SGroups, query atoms and bonds and query properties, R-groups, atom parity, valence,
//! atom-atom maps, 3D blocks, reactions and several molecules in one record. Every reason has a
//! diagnostic family in `ct_reason_family` (in the application's `ctfile` module).

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use chemifree_application::ctfile::{
    CtAtom, CtBond, CtCollection, CtCollectionKind, CtError, CtRadical, CtRecord,
};
use chemifree_application::{Loss, LossDisposition, SemanticImpact};
use chemifree_domain::element::Element;

use super::{Lines, ascii_line, error, loss};

/// Counts line of a V3000 record: 39 columns, `V3000` in columns 35-39, all numeric fields zero
/// (the real counts are in the `COUNTS` line).
const COUNTS_LINE: &str = "  0  0  0     0  0              0 V3000";
const PREFIX: &str = "M  V30 ";
pub(super) const MAX_ATOMS: usize = 2000;
pub(super) const MAX_BONDS: usize = 4000;
const MAX_COLLECTIONS: usize = 2000;
const MAX_LOGICAL_BYTES: usize = 16 * 1024;
const MAX_NUMBER_BYTES: usize = 24;
const MAX_PHYSICAL: usize = 80;

/// What a V3000 connection table holds, before it becomes a `CtRecord`.
pub(super) struct Table {
    pub atoms: Vec<CtAtom>,
    pub bonds: Vec<CtBond>,
    pub chiral: bool,
    pub collections: Vec<CtCollection>,
}

struct Logical {
    text: String,
    /// One-based physical line on which the logical line starts.
    line: usize,
}

/// Reads one logical line: strips the `M  V30 ` prefix and joins `-` continuations (the hyphen
/// ends the physical line; the next one repeats the prefix and its text follows directly).
fn read_logical(lines: &mut Lines<'_>, record: usize) -> Result<Logical, CtError> {
    let start = lines.position + 1;
    let mut text = body(lines, record)?;
    while text.ends_with('-') {
        text.pop();
        let next = lines.position + 1;
        if lines.done() {
            return Err(error(
                "v3000_unterminated_continuation",
                record,
                Some(start),
                "A line ends with a continuation hyphen but no line follows.",
            ));
        }
        let more = body(lines, record).map_err(|e| {
            if e.code == "v3000_missing_prefix" {
                error(
                    "v3000_unterminated_continuation",
                    record,
                    Some(next),
                    "A continued line must start with the M  V30 prefix.",
                )
            } else {
                e
            }
        })?;
        text.push_str(&more);
        if text.len() > MAX_LOGICAL_BYTES {
            return Err(error(
                "resource_limit",
                record,
                Some(start),
                "A logical V3000 line exceeds 16 KiB.",
            ));
        }
    }
    Ok(Logical { text, line: start })
}

fn body(lines: &mut Lines<'_>, record: usize) -> Result<String, CtError> {
    let line = lines.next(record)?;
    ascii_line(line, record, lines.line())?;
    let Some(text) = line.strip_prefix(PREFIX) else {
        return Err(error(
            "v3000_missing_prefix",
            record,
            lines.line(),
            "A V3000 line must start with \"M  V30 \".",
        ));
    };
    Ok(text.to_owned())
}

/// Splits on spaces and tabs, keeping a parenthesised list or a double-quoted string whole.
fn tokens(logical: &Logical, record: usize) -> Result<Vec<&str>, CtError> {
    let bad = || {
        error(
            "v3000_invalid_property",
            record,
            Some(logical.line),
            "Unbalanced parenthesis or quote.",
        )
    };
    let bytes = logical.text.as_bytes();
    let mut out = Vec::new();
    let (mut start, mut depth, mut quoted) = (None, 0_usize, false);
    for (i, &b) in bytes.iter().chain(std::iter::once(&b' ')).enumerate() {
        if !quoted && depth == 0 && (b == b' ' || b == b'\t') {
            if let Some(s) = start.take() {
                out.push(&logical.text[s..i]);
            }
            continue;
        }
        start.get_or_insert(i);
        match b {
            b'"' => quoted = !quoted,
            b'(' if !quoted => depth += 1,
            b')' if !quoted => depth = depth.checked_sub(1).ok_or_else(bad)?,
            _ => {}
        }
    }
    if quoted || depth != 0 {
        return Err(bad());
    }
    Ok(out)
}

fn int(token: &str, code: &'static str, record: usize, line: usize) -> Result<i32, CtError> {
    if token.len() > MAX_NUMBER_BYTES {
        return Err(error(
            code,
            record,
            Some(line),
            "A number has more than 24 characters.",
        ));
    }
    token.parse().map_err(|_| {
        error(
            code,
            record,
            Some(line),
            format!("{token:?} is not a 32-bit integer."),
        )
    })
}

fn real(token: &str, record: usize, line: usize) -> Result<f64, CtError> {
    let value = if token.len() > MAX_NUMBER_BYTES {
        None
    } else {
        token.parse::<f64>().ok().filter(|v| v.is_finite())
    };
    value.ok_or_else(|| {
        error(
            "v3000_invalid_number",
            record,
            Some(line),
            format!("{token:?} is not a finite number of at most 24 characters."),
        )
    })
}

/// Reads the lines of a block up to its `END <name>`.
fn block(
    lines: &mut Lines<'_>,
    record: usize,
    name: &str,
    limit: usize,
) -> Result<Vec<Logical>, CtError> {
    let mut out = Vec::new();
    loop {
        let item = read_logical(lines, record)?;
        let mut words = item.text.split_ascii_whitespace();
        match (words.next(), words.next(), words.next()) {
            (Some("END"), Some(n), None) if n == name => return Ok(out),
            (Some("END"), Some("CTAB"), None) => {
                return Err(error(
                    "v3000_block_not_closed",
                    record,
                    Some(item.line),
                    format!("Block {name} is not closed before END CTAB."),
                ));
            }
            (Some("BEGIN" | "END"), ..) => {
                return Err(error(
                    "v3000_block_mismatch",
                    record,
                    Some(item.line),
                    format!("Expected END {name}."),
                ));
            }
            _ => {}
        }
        if out.len() == limit {
            return Err(error(
                "v3000_count_mismatch",
                record,
                Some(item.line),
                format!("Block {name} has more lines than declared."),
            ));
        }
        out.push(item);
    }
}

fn counts(
    lines: &mut Lines<'_>,
    record: usize,
    losses: &mut Vec<Loss>,
) -> Result<(usize, usize, bool, usize), CtError> {
    let item = read_logical(lines, record)?;
    let words = tokens(&item, record)?;
    let line = item.line;
    let bad = |m: &str| {
        error(
            "v3000_invalid_counts_line",
            record,
            Some(line),
            m.to_owned(),
        )
    };
    if words.first() != Some(&"COUNTS") || !(6..=7).contains(&words.len()) {
        return Err(bad("Expected COUNTS with 5 or 6 numbers."));
    }
    let number = |i: usize| int(words[i], "v3000_invalid_counts_line", record, line);
    let (atoms, bonds, sgroups, three_d, chiral) =
        (number(1)?, number(2)?, number(3)?, number(4)?, number(5)?);
    if words.len() == 7 {
        number(6)?;
        losses.push(loss(
            record,
            ".counts.regno",
            "registry_number",
            LossDisposition::Dropped,
            SemanticImpact::Document,
            "The COUNTS registry number is not retained.",
        ));
    }
    if [atoms, bonds, sgroups, three_d].iter().any(|n| *n < 0) || !(0..=1).contains(&chiral) {
        return Err(bad(
            "COUNTS numbers must be nonnegative and the chiral flag 0 or 1.",
        ));
    }
    let limit = |n: i32, max: usize, what: &str| {
        usize::try_from(n)
            .ok()
            .filter(|n| *n <= max)
            .ok_or_else(|| {
                error(
                    "resource_limit",
                    record,
                    Some(line),
                    format!("More than {max} {what}."),
                )
            })
    };
    let (atoms, bonds) = (
        limit(atoms, MAX_ATOMS, "atoms")?,
        limit(bonds, MAX_BONDS, "bonds")?,
    );
    if sgroups > 0 {
        return Err(error(
            "v3000_sgroup_not_supported",
            record,
            Some(line),
            "SGroups change the compound and are not supported.",
        ));
    }
    if three_d > 0 {
        return Err(error(
            "v3000_unsupported_3d",
            record,
            Some(line),
            "3D objects are not supported.",
        ));
    }
    Ok((atoms, bonds, chiral == 1, line))
}

/// Parses a V3000 connection table starting after the counts line, through `M  END`.
pub(super) fn read_table(
    lines: &mut Lines<'_>,
    record: usize,
    losses: &mut Vec<Loss>,
) -> Result<Table, CtError> {
    let first = read_logical(lines, record)?;
    if tokens(&first, record)? != ["BEGIN", "CTAB"] {
        return Err(error(
            "v3000_missing_block",
            record,
            Some(first.line),
            "Expected BEGIN CTAB.",
        ));
    }
    let (atom_count, bond_count, chiral, counts_line) = counts(lines, record, losses)?;
    let mut atoms: Option<(Vec<CtAtom>, HashMap<i32, usize>)> = None;
    let mut bonds: Option<Vec<CtBond>> = None;
    let mut collections: Option<Vec<CtCollection>> = None;
    loop {
        let item = read_logical(lines, record)?;
        let words = tokens(&item, record)?;
        let line = item.line;
        let refuse = |code: &'static str, m: &str| error(code, record, Some(line), m.to_owned());
        // Blocks are parsed as they close, so a bond that precedes the atom block, or names an
        // atom that block does not define, is a dangling reference.
        match words.as_slice() {
            ["END", "CTAB"] => break,
            ["BEGIN", "ATOM"] if atoms.is_none() => {
                let items = block(lines, record, "ATOM", atom_count)?;
                require_count(items.len(), atom_count, "atoms", counts_line, record)?;
                atoms = Some(read_atoms(&items, record, losses)?);
            }
            ["BEGIN", "BOND"] if bonds.is_none() => {
                let items = block(lines, record, "BOND", bond_count)?;
                require_count(items.len(), bond_count, "bonds", counts_line, record)?;
                let known = atoms.as_ref().map(|(_, index)| index);
                bonds = Some(read_bonds(&items, known, record)?);
            }
            ["BEGIN", "COLLECTION"] if collections.is_none() => {
                let items = block(lines, record, "COLLECTION", MAX_COLLECTIONS)?;
                let known = atoms.as_ref().map(|(_, index)| index);
                collections = Some(read_collections(&items, known, record)?);
            }
            ["BEGIN", "ATOM" | "BOND" | "COLLECTION"] => {
                return Err(refuse(
                    "v3000_duplicate_block",
                    "A block occurs more than once.",
                ));
            }
            ["BEGIN", "SGROUP"] => {
                return Err(refuse(
                    "v3000_sgroup_not_supported",
                    "SGroups change the compound and are not supported.",
                ));
            }
            ["BEGIN", "RGROUP"] => {
                return Err(refuse(
                    "v3000_unsupported_rgroup",
                    "R-group definitions are not supported.",
                ));
            }
            ["BEGIN", "OBJ3D"] => {
                return Err(refuse(
                    "v3000_unsupported_3d",
                    "3D objects are not supported.",
                ));
            }
            ["BEGIN", "REACTANT" | "PRODUCT" | "INTERMEDIATE"] => {
                return Err(refuse(
                    "v3000_unsupported_reaction",
                    "Reaction blocks are not supported.",
                ));
            }
            ["BEGIN", other] => {
                return Err(refuse(
                    "v3000_unsupported_block",
                    &format!("Block {other} is not supported."),
                ));
            }
            _ => {
                return Err(refuse(
                    "v3000_block_mismatch",
                    "Expected BEGIN <block> or END CTAB.",
                ));
            }
        }
    }
    // A block that is absent holds zero items.
    if atoms.is_none() {
        require_count(0, atom_count, "atoms", counts_line, record)?;
    }
    if bonds.is_none() {
        require_count(0, bond_count, "bonds", counts_line, record)?;
    }
    end_of_table(lines, record)?;
    Ok(Table {
        atoms: atoms.map(|(atoms, _)| atoms).unwrap_or_default(),
        bonds: bonds.unwrap_or_default(),
        chiral,
        collections: collections.unwrap_or_default(),
    })
}

fn require_count(
    found: usize,
    declared: usize,
    what: &str,
    line: usize,
    record: usize,
) -> Result<(), CtError> {
    if found == declared {
        return Ok(());
    }
    Err(error(
        "v3000_count_mismatch",
        record,
        Some(line),
        format!("COUNTS declares {declared} {what} but {found} are present."),
    ))
}

fn end_of_table(lines: &mut Lines<'_>, record: usize) -> Result<(), CtError> {
    let line = lines.next(record)?;
    ascii_line(line, record, lines.line())?;
    let parts: Vec<_> = line.split_whitespace().collect();
    if parts.len() == 2
        && parts[0].eq_ignore_ascii_case("M")
        && parts[1].eq_ignore_ascii_case("END")
    {
        return Ok(());
    }
    let (code, message) = if line.starts_with(PREFIX) {
        (
            "v3000_multiple_structures",
            "Only one connection table per record is supported.",
        )
    } else if parts.first().is_some_and(|p| p.eq_ignore_ascii_case("M"))
        && parts.get(1).is_some_and(|p| p.eq_ignore_ascii_case("END"))
    {
        (
            "invalid_m_end",
            "M END may contain only trailing whitespace.",
        )
    } else {
        ("missing_m_end", "Expected M  END after END CTAB.")
    };
    Err(error(code, record, lines.line(), message))
}

fn unique_index(
    token: &str,
    seen: &mut HashSet<i32>,
    record: usize,
    line: usize,
) -> Result<i32, CtError> {
    let value = int(token, "v3000_invalid_number", record, line)?;
    if value <= 0 {
        return Err(error(
            "v3000_invalid_index",
            record,
            Some(line),
            "An index must be a positive integer.",
        ));
    }
    if !seen.insert(value) {
        return Err(error(
            "v3000_duplicate_index",
            record,
            Some(line),
            format!("Index {value} is used twice."),
        ));
    }
    Ok(value)
}

/// Splits `NAME=value` properties of an atom or bond line, refusing a repeated name.
fn properties<'a>(
    words: &[&'a str],
    record: usize,
    line: usize,
) -> Result<Vec<(&'a str, &'a str)>, CtError> {
    let mut out: Vec<(&str, &str)> = Vec::new();
    for word in words {
        let (key, value) = word
            .split_once('=')
            .filter(|(k, v)| !k.is_empty() && !v.is_empty())
            .ok_or_else(|| {
                error(
                    "v3000_invalid_property",
                    record,
                    Some(line),
                    format!("{word:?} is not NAME=value."),
                )
            })?;
        if out.iter().any(|(k, _)| *k == key) {
            return Err(error(
                "v3000_duplicate_property",
                record,
                Some(line),
                format!("Property {key} is given twice."),
            ));
        }
        out.push((key, value));
    }
    Ok(out)
}

fn read_atoms(
    items: &[Logical],
    record: usize,
    losses: &mut Vec<Loss>,
) -> Result<(Vec<CtAtom>, HashMap<i32, usize>), CtError> {
    let mut atoms = Vec::with_capacity(items.len());
    let mut index = HashMap::new();
    let mut seen = HashSet::new();
    for item in items {
        let line = item.line;
        let refuse = |code: &'static str, m: String| error(code, record, Some(line), m);
        let words = tokens(item, record)?;
        if words.len() < 6 {
            return Err(refuse(
                "v3000_invalid_atom",
                "An atom line has index, type, x, y, z and aamap.".into(),
            ));
        }
        let file_index = unique_index(words[0], &mut seen, record, line)?;
        let symbol = words[1];
        if Element::from_symbol(symbol).is_none() {
            let (code, what) = match symbol {
                "R#" => ("v3000_unsupported_rgroup", "R-group atoms"),
                "A" | "Q" | "X" | "*" => ("v3000_unsupported_query", "Query atoms"),
                s if s.starts_with('[') || s == "NOT" => ("v3000_unsupported_query", "Atom lists"),
                _ => ("v3000_unsupported_atom", "This atom type"),
            };
            return Err(refuse(
                code,
                format!("{what} are not supported ({symbol:?})."),
            ));
        }
        let mut atom = CtAtom {
            element: symbol.into(),
            x: real(words[2], record, line)?,
            y: real(words[3], record, line)?,
            z: real(words[4], record, line)?,
            charge: 0,
            isotope: None,
            radical: CtRadical::None,
        };
        if int(words[5], "v3000_invalid_number", record, line)? != 0 {
            return Err(refuse(
                "v3000_unsupported_atom_property",
                "A nonzero atom-atom mapping number is not supported.".into(),
            ));
        }
        for (key, value) in properties(&words[6..], record, line)? {
            let number = |code| int(value, code, record, line);
            match key {
                "CHG" => {
                    atom.charge = i8::try_from(number("v3000_invalid_charge")?).map_err(|_| {
                        refuse("v3000_invalid_charge", "CHG is outside -128..127.".into())
                    })?;
                }
                "RAD" => {
                    atom.radical = match number("v3000_invalid_radical")? {
                        0 => CtRadical::None,
                        1 => CtRadical::Singlet,
                        2 => CtRadical::Doublet,
                        3 => CtRadical::Triplet,
                        _ => {
                            return Err(refuse(
                                "v3000_invalid_radical",
                                "RAD must be 0, 1, 2 or 3.".into(),
                            ));
                        }
                    };
                }
                "MASS" => {
                    atom.isotope = Some(
                        u16::try_from(number("v3000_invalid_isotope")?)
                            .ok()
                            .filter(|m| (1..=999).contains(m))
                            .ok_or_else(|| {
                                refuse(
                                    "v3000_invalid_isotope",
                                    "MASS must be 1 through 999.".into(),
                                )
                            })?,
                    );
                    losses.push(loss(record, &format!(".atoms[{file_index}].isotope"), "isotope_validation", LossDisposition::Unknown, SemanticImpact::Chemistry, "The public guide does not identify the PTABLE.DAT revision needed to verify the absolute mass bound."));
                }
                "HCOUNT" | "SUBST" | "UNSAT" | "RBCNT" => {
                    return Err(refuse(
                        "v3000_unsupported_query",
                        format!("Query property {key} changes what a query matches."),
                    ));
                }
                "ATTCHPT" | "RGROUPS" | "ATTCHORD" | "LOGIC" => {
                    return Err(refuse(
                        "v3000_unsupported_rgroup",
                        format!("R-group property {key} is not supported."),
                    ));
                }
                _ => {
                    return Err(refuse(
                        "v3000_unsupported_atom_property",
                        format!("Atom property {key} is not supported."),
                    ));
                }
            }
        }
        index.insert(file_index, atoms.len());
        atoms.push(atom);
    }
    Ok((atoms, index))
}

fn read_bonds(
    items: &[Logical],
    atoms: Option<&HashMap<i32, usize>>,
    record: usize,
) -> Result<Vec<CtBond>, CtError> {
    let mut bonds = Vec::with_capacity(items.len());
    let mut seen = HashSet::new();
    let mut pairs = HashSet::new();
    for item in items {
        let line = item.line;
        let refuse = |code: &'static str, m: String| error(code, record, Some(line), m);
        let words = tokens(item, record)?;
        if words.len() < 4 {
            return Err(refuse(
                "v3000_invalid_bond",
                "A bond line has index, type and two atoms.".into(),
            ));
        }
        unique_index(words[0], &mut seen, record, line)?;
        let order = match int(words[1], "v3000_invalid_number", record, line)? {
            order @ 1..=4 => u8::try_from(order).expect("matched 1..=4"),
            5..=8 => {
                return Err(refuse(
                    "v3000_unsupported_query",
                    "Query bond types are not supported.".into(),
                ));
            }
            other => {
                return Err(refuse(
                    "v3000_unsupported_bond",
                    format!("Bond type {other} is not supported."),
                ));
            }
        };
        let endpoint = |token: &str| -> Result<usize, CtError> {
            let value = int(token, "v3000_invalid_number", record, line)?;
            atoms
                .and_then(|known| known.get(&value))
                .copied()
                .ok_or_else(|| {
                    refuse(
                        "v3000_unknown_atom_index",
                        format!("Atom {value} is not defined in the atom block."),
                    )
                })
        };
        let (begin, end) = (endpoint(words[2])?, endpoint(words[3])?);
        if begin == end {
            return Err(refuse(
                "v3000_invalid_bond",
                "A bond joins two distinct atoms.".into(),
            ));
        }
        if let Some((key, _)) = properties(&words[4..], record, line)?.first() {
            let code = match *key {
                "TOPO" | "ENDPTS" | "ATTACH" => "v3000_unsupported_query",
                _ => "v3000_unsupported_bond_property",
            };
            return Err(refuse(
                code,
                format!("Bond property {key} is not supported."),
            ));
        }
        if !pairs.insert((begin.min(end), begin.max(end))) {
            return Err(refuse(
                "v3000_duplicate_bond",
                "More than one bond connects the same atoms.".into(),
            ));
        }
        bonds.push(CtBond { begin, end, order });
    }
    Ok(bonds)
}

/// Collections are carried unmapped. A name other than the three stereo kinds, or a key other
/// than `ATOMS`, is refused rather than dropped.
fn read_collections(
    items: &[Logical],
    atoms: Option<&HashMap<i32, usize>>,
    record: usize,
) -> Result<Vec<CtCollection>, CtError> {
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let line = item.line;
        let refuse = |code: &'static str, m: String| error(code, record, Some(line), m);
        let unsupported = |m: &str| refuse("v3000_collection_not_supported_yet", m.to_owned());
        let invalid = |m: &str| refuse("v3000_invalid_collection", m.to_owned());
        let words = tokens(item, record)?;
        let name = words
            .first()
            .ok_or_else(|| invalid("A collection line is empty."))?
            .strip_prefix("MDLV30/")
            .ok_or_else(|| unsupported("Only MDLV30/STEABS, STERAC and STEREL are carried."))?;
        let number = |digits: &str| {
            digits
                .parse::<i32>()
                .map_err(|_| invalid("The collection number is not a 32-bit integer."))
        };
        let (kind, number) = if name == "STEABS" {
            (CtCollectionKind::Absolute, None)
        } else if let Some(digits) = name.strip_prefix("STERAC") {
            (CtCollectionKind::And, Some(number(digits)?))
        } else if let Some(digits) = name.strip_prefix("STEREL") {
            (CtCollectionKind::Or, Some(number(digits)?))
        } else {
            return Err(unsupported(
                "Only MDLV30/STEABS, STERAC and STEREL are carried.",
            ));
        };
        let mut list = None;
        for word in &words[1..] {
            match word.split_once('=') {
                Some(("ATOMS", value)) if list.is_none() => list = Some(value),
                Some(("ATOMS", _)) => return Err(invalid("ATOMS is given twice.")),
                _ => {
                    return Err(unsupported(
                        "Only the ATOMS list of a collection is carried.",
                    ));
                }
            }
        }
        let inner = list
            .and_then(|v| v.strip_prefix('(')?.strip_suffix(')'))
            .ok_or_else(|| invalid("A collection needs ATOMS=(n i1 ... in)."))?;
        let mut numbers = inner.split_ascii_whitespace();
        let declared = int(
            numbers.next().unwrap_or(""),
            "v3000_invalid_collection",
            record,
            line,
        )?;
        let members: Vec<_> = numbers.collect();
        if usize::try_from(declared).ok() != Some(members.len()) {
            return Err(refuse(
                "v3000_count_mismatch",
                "The ATOMS count does not match its list.".into(),
            ));
        }
        let mut positions = Vec::with_capacity(members.len());
        let mut seen = HashSet::new();
        for member in members {
            let value = int(member, "v3000_invalid_number", record, line)?;
            let position = atoms
                .and_then(|known| known.get(&value))
                .copied()
                .ok_or_else(|| {
                    refuse(
                        "v3000_unknown_atom_index",
                        format!("Atom {value} is not defined in the atom block."),
                    )
                })?;
            if !seen.insert(value) {
                return Err(refuse(
                    "v3000_duplicate_index",
                    format!("Atom {value} is listed twice in one collection."),
                ));
            }
            positions.push(position);
        }
        out.push(CtCollection {
            kind,
            number,
            atoms: positions,
        });
    }
    Ok(out)
}

/// Writes one logical line, breaking between tokens with `-` continuations so that no physical
/// line passes 80 columns.
fn put(out: &mut String, text: &str) {
    let mut rest = text;
    while PREFIX.len() + rest.len() > MAX_PHYSICAL {
        // A continued line holds the prefix, the text, a space and the hyphen.
        let room = MAX_PHYSICAL - PREFIX.len() - 2;
        let cut = rest[..=room]
            .rfind(' ')
            .expect("tokens are shorter than a line");
        out.push_str(PREFIX);
        out.push_str(&rest[..=cut]);
        out.push_str("-\n");
        rest = &rest[cut + 1..];
    }
    out.push_str(PREFIX);
    out.push_str(rest);
    out.push('\n');
}

/// Writes the counts line through `M  END` for a V3000 record.
pub(super) fn write_table(
    out: &mut String,
    item: &CtRecord,
    record: usize,
    losses: &mut Vec<Loss>,
) -> Result<(), CtError> {
    if item.atoms.len() > MAX_ATOMS || item.bonds.len() > MAX_BONDS {
        return Err(error(
            "resource_limit",
            record,
            None,
            "A V3000 record holds at most 2000 atoms and 4000 bonds.",
        ));
    }
    out.push_str(COUNTS_LINE);
    out.push('\n');
    put(out, "BEGIN CTAB");
    put(
        out,
        &format!("COUNTS {} {} 0 0 0", item.atoms.len(), item.bonds.len()),
    );
    if !item.atoms.is_empty() {
        put(out, "BEGIN ATOM");
    }
    for (i, atom) in item.atoms.iter().enumerate() {
        if Element::from_symbol(&atom.element).is_none() {
            return Err(error(
                "unsupported_atom",
                record,
                None,
                format!("Invalid element {:?}.", atom.element),
            ));
        }
        let mut text = format!("{} {}", i + 1, atom.element);
        for value in [atom.x, atom.y, atom.z] {
            let formatted = format!("{value:.4}");
            if !value.is_finite() || formatted.len() > MAX_NUMBER_BYTES {
                return Err(error(
                    "invalid_coordinate",
                    record,
                    None,
                    "Coordinate must be finite and fit 24 characters.",
                ));
            }
            if formatted.parse::<f64>().ok() != Some(value) {
                losses.push(loss(
                    record,
                    &format!(".atoms[{}].coordinates", i + 1),
                    "coordinate_precision",
                    LossDisposition::Normalized,
                    SemanticImpact::Geometry,
                    "V3000 coordinates are rounded to four decimal places.",
                ));
            }
            write!(text, " {formatted}").expect("String writes do not fail");
        }
        text.push_str(" 0");
        if atom.charge != 0 {
            write!(text, " CHG={}", atom.charge).expect("String writes do not fail");
        }
        let radical = match atom.radical {
            CtRadical::None => 0,
            CtRadical::Singlet => 1,
            CtRadical::Doublet => 2,
            CtRadical::Triplet => 3,
        };
        if radical != 0 {
            write!(text, " RAD={radical}").expect("String writes do not fail");
        }
        if let Some(mass) = atom.isotope {
            if !(1..=999).contains(&mass) {
                return Err(error(
                    "invalid_isotope",
                    record,
                    None,
                    "Isotope mass must be 1 through 999.",
                ));
            }
            write!(text, " MASS={mass}").expect("String writes do not fail");
            losses.push(loss(record, &format!(".atoms[{}].isotope", i + 1), "isotope_validation", LossDisposition::Unknown, SemanticImpact::Chemistry, "The public guide does not identify the PTABLE.DAT revision needed to verify the absolute mass bound."));
        }
        put(out, &text);
    }
    if !item.atoms.is_empty() {
        put(out, "END ATOM");
    }
    if !item.bonds.is_empty() {
        put(out, "BEGIN BOND");
    }
    let mut pairs = HashSet::new();
    for (i, bond) in item.bonds.iter().enumerate() {
        if bond.begin >= item.atoms.len() || bond.end >= item.atoms.len() || bond.begin == bond.end
        {
            return Err(error(
                "invalid_bond",
                record,
                None,
                "Bond endpoints must be distinct existing atoms.",
            ));
        }
        if !(1..=4).contains(&bond.order) {
            return Err(error(
                "unsupported_bond",
                record,
                None,
                "Only bond orders 1 through 4 are supported.",
            ));
        }
        if !pairs.insert((bond.begin.min(bond.end), bond.begin.max(bond.end))) {
            return Err(error(
                "duplicate_bond",
                record,
                None,
                "More than one bond connects the same atoms.",
            ));
        }
        put(
            out,
            &format!(
                "{} {} {} {}",
                i + 1,
                bond.order,
                bond.begin + 1,
                bond.end + 1
            ),
        );
    }
    if !item.bonds.is_empty() {
        put(out, "END BOND");
    }
    put(out, "END CTAB");
    out.push_str("M  END\n");
    Ok(())
}
