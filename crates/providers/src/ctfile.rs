//! A bounded, independently implemented V2000 MOL / SDfile codec.
//!
//! Framing is parsed in context: the three header lines and a data value are never searched
//! as though they were record separators. Unsupported chemical directives are refused, not
//! ignored, because their effects on the graph cannot be inferred safely.

use std::collections::HashSet;
use std::fmt::Write as _;

use chemifree_application::ctfile::{
    CtAtom, CtBond, CtCodec, CtError, CtField, CtFormat, CtRadical, CtRecord, CtResult,
};
use chemifree_application::{Loss, LossDisposition, SemanticImpact};
use chemifree_domain::element::Element;

const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_RECORDS: usize = 128;
const MAX_FIELDS: usize = 1024;
const MAX_ITEMS: usize = 999;

#[derive(Clone, Copy, Debug, Default)]
pub struct V2000Codec;

fn error(
    code: &'static str,
    record: usize,
    line: Option<usize>,
    message: impl Into<String>,
) -> CtError {
    CtError {
        code,
        message: message.into(),
        record,
        line,
    }
}

fn loss(
    record: usize,
    path: &str,
    feature: &str,
    disposition: LossDisposition,
    impact: SemanticImpact,
    reason: &str,
) -> Loss {
    Loss {
        feature: feature.into(),
        source_path: format!("records[{record}]{path}"),
        disposition,
        reason: reason.into(),
        semantic_impact: impact,
    }
}

struct Lines<'a> {
    lines: Vec<&'a [u8]>,
    position: usize,
}

impl<'a> Lines<'a> {
    fn new(input: &'a [u8]) -> Self {
        let mut lines: Vec<_> = input.split(|b| *b == b'\n').collect();
        if input.ends_with(b"\n") {
            lines.pop();
        }
        Self { lines, position: 0 }
    }
    fn done(&self) -> bool {
        self.position == self.lines.len()
    }
    fn next(&mut self, record: usize) -> Result<&'a str, CtError> {
        let number = self.position + 1;
        let Some(&line) = self.lines.get(self.position) else {
            return Err(error(
                "truncated",
                record,
                Some(number),
                "Expected another line.",
            ));
        };
        self.position += 1;
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let line = std::str::from_utf8(line)
            .map_err(|_| error("invalid_utf8", record, Some(number), "Line is not UTF-8."))?;
        if line.contains(['\r', '\0']) {
            return Err(error(
                "invalid_line",
                record,
                Some(number),
                "A line contains a carriage return or NUL.",
            ));
        }
        Ok(line)
    }
    fn line(&self) -> Option<usize> {
        Some(self.position)
    }
}

fn simple_line(value: &str, record: usize, line: Option<usize>, name: &str) -> Result<(), CtError> {
    if value.chars().count() > 80 || value.contains(['\r', '\n', '\0']) {
        return Err(error(
            "invalid_header",
            record,
            line,
            format!("{name} must fit in 80 characters and contain no line break or NUL."),
        ));
    }
    Ok(())
}

fn title_ok(value: &str, record: usize, line: Option<usize>) -> Result<(), CtError> {
    simple_line(value, record, line, "Title")?;
    let upper = value.to_ascii_uppercase();
    if ["$MDL", "$$$$", "$RXN", "$RDFILE"]
        .iter()
        .any(|tag| upper.contains(tag))
    {
        return Err(error(
            "reserved_title",
            record,
            line,
            "Title contains a reserved CTfile tag.",
        ));
    }
    Ok(())
}

fn ascii_line(value: &str, record: usize, line: Option<usize>) -> Result<(), CtError> {
    if !value.is_ascii() || value.len() > 80 {
        return Err(error(
            "invalid_columns",
            record,
            line,
            "CTAB lines must be ASCII and at most 80 columns.",
        ));
    }
    Ok(())
}

fn column<'a>(
    value: &'a str,
    start: usize,
    end: usize,
    record: usize,
    line: Option<usize>,
) -> Result<&'a str, CtError> {
    if value.len() <= start {
        return Ok("");
    }
    value.get(start..end).ok_or_else(|| {
        error(
            "truncated_field",
            record,
            line,
            format!("Field at columns {}–{} is incomplete.", start + 1, end),
        )
    })
}

fn number(
    value: &str,
    start: usize,
    end: usize,
    record: usize,
    line: Option<usize>,
) -> Result<i32, CtError> {
    let text = column(value, start, end, record, line)?.trim();
    if text.is_empty() {
        return Ok(0);
    }
    text.parse().map_err(|_| {
        error(
            "invalid_field",
            record,
            line,
            format!("Expected an integer at columns {}–{}.", start + 1, end),
        )
    })
}

fn coordinate(
    value: &str,
    start: usize,
    record: usize,
    line: Option<usize>,
) -> Result<f64, CtError> {
    let text = column(value, start, start + 10, record, line)?.trim();
    let result: f64 = text.parse().map_err(|_| {
        error(
            "invalid_coordinate",
            record,
            line,
            "Coordinate is missing or invalid.",
        )
    })?;
    if !result.is_finite() {
        return Err(error(
            "invalid_coordinate",
            record,
            line,
            "Coordinate must be finite.",
        ));
    }
    Ok(result)
}

fn count_field(
    value: &str,
    start: usize,
    record: usize,
    line: Option<usize>,
) -> Result<usize, CtError> {
    usize::try_from(number(value, start, start + 3, record, line)?)
        .ok()
        .filter(|v| *v <= MAX_ITEMS)
        .ok_or_else(|| {
            error(
                "invalid_count",
                record,
                line,
                "A count must be between 0 and 999.",
            )
        })
}

fn parse_atom(line: &str, record: usize, line_no: Option<usize>) -> Result<(CtAtom, i32), CtError> {
    ascii_line(line, record, line_no)?;
    if line.len() < 34 {
        return Err(error(
            "truncated_atom",
            record,
            line_no,
            "Atom line ends before its element symbol.",
        ));
    }
    if &line[30..31] != " " {
        return Err(error(
            "invalid_columns",
            record,
            line_no,
            "Atom symbol is not in its V2000 field.",
        ));
    }
    let element = column(line, 31, 34, record, line_no)?.trim();
    if Element::from_symbol(element).is_none() {
        return Err(error(
            "unsupported_atom",
            record,
            line_no,
            format!("Unsupported or invalid element symbol {element:?}."),
        ));
    }
    let mass = number(line, 34, 36, record, line_no)?;
    if !(-3..=4).contains(&mass) {
        return Err(error(
            "invalid_isotope",
            record,
            line_no,
            "Compact isotope difference is outside -3..4.",
        ));
    }
    let code = number(line, 36, 39, record, line_no)?;
    let (charge, radical) = match code {
        0 => (0, CtRadical::None),
        1 => (3, CtRadical::None),
        2 => (2, CtRadical::None),
        3 => (1, CtRadical::None),
        4 => (0, CtRadical::Doublet),
        5 => (-1, CtRadical::None),
        6 => (-2, CtRadical::None),
        7 => (-3, CtRadical::None),
        _ => {
            return Err(error(
                "invalid_charge",
                record,
                line_no,
                "Invalid compact charge code.",
            ));
        }
    };
    for start in (39..69).step_by(3) {
        if number(line, start, start + 3, record, line_no)? != 0 {
            return Err(error(
                "unsupported_atom_field",
                record,
                line_no,
                format!(
                    "Nonzero atom field at column {} is outside the basic profile.",
                    start + 1
                ),
            ));
        }
    }
    if line
        .get(69..)
        .is_some_and(|suffix| !suffix.trim().is_empty())
    {
        return Err(error(
            "unsupported_atom_field",
            record,
            line_no,
            "Unrecognized atom line suffix.",
        ));
    }
    Ok((
        CtAtom {
            element: element.into(),
            x: coordinate(line, 0, record, line_no)?,
            y: coordinate(line, 10, record, line_no)?,
            z: coordinate(line, 20, record, line_no)?,
            charge,
            isotope: None,
            radical,
        },
        mass,
    ))
}

fn parse_bond(
    line: &str,
    atom_count: usize,
    record: usize,
    line_no: Option<usize>,
) -> Result<CtBond, CtError> {
    ascii_line(line, record, line_no)?;
    if line.len() < 9 {
        return Err(error(
            "truncated_bond",
            record,
            line_no,
            "Bond line is incomplete.",
        ));
    }
    let begin = count_field(line, 0, record, line_no)?;
    let end = count_field(line, 3, record, line_no)?;
    let order = number(line, 6, 9, record, line_no)?;
    if begin == 0 || end == 0 || begin > atom_count || end > atom_count || begin == end {
        return Err(error(
            "invalid_bond",
            record,
            line_no,
            "Bond endpoints must be distinct existing atoms.",
        ));
    }
    if !(1..=3).contains(&order) {
        return Err(error(
            "unsupported_bond",
            record,
            line_no,
            "Only single, double and triple bonds are supported.",
        ));
    }
    for start in (9..21).step_by(3) {
        if number(line, start, start + 3, record, line_no)? != 0 {
            return Err(error(
                "unsupported_bond_field",
                record,
                line_no,
                format!(
                    "Bond stereo, topology and reaction fields are unsupported (column {}).",
                    start + 1
                ),
            ));
        }
    }
    if line
        .get(21..)
        .is_some_and(|suffix| !suffix.trim().is_empty())
    {
        return Err(error(
            "unsupported_bond_field",
            record,
            line_no,
            "Unrecognized bond line suffix.",
        ));
    }
    Ok(CtBond {
        begin: begin - 1,
        end: end - 1,
        order: u8::try_from(order).expect("checked order"),
    })
}

// Both common independent readers agree on these H–Ca baselines; the CTfile guide does
// not identify a universal table. Other compact differences are refused unless M ISO
// supersedes them. M ISO itself is absolute; its PTABLE.DAT-relative validity is unknown.
fn compact_baseline(element: &str) -> Option<i32> {
    let atomic = Element::from_symbol(element)?.atomic_number();
    let values = [
        1, 4, 7, 9, 11, 12, 14, 16, 19, 20, 23, 24, 27, 28, 31, 32, 35, 40, 39, 40,
    ];
    values.get(usize::from(atomic) - 1).copied()
}

fn parse_property(
    line: &str,
    atoms: &mut [CtAtom],
    seen_charge: &mut HashSet<usize>,
    seen_radical: &mut HashSet<usize>,
    seen_isotope: &mut HashSet<usize>,
    record: usize,
    line_no: Option<usize>,
) -> Result<&'static str, CtError> {
    ascii_line(line, record, line_no)?;
    let parts: Vec<_> = line.split_whitespace().collect();
    if parts.len() < 2 || !parts[0].eq_ignore_ascii_case("M") {
        return Err(error(
            "unsupported_directive",
            record,
            line_no,
            "Unsupported CTAB property directive.",
        ));
    }
    let kind = if parts[1].eq_ignore_ascii_case("CHG") {
        "CHG"
    } else if parts[1].eq_ignore_ascii_case("RAD") {
        "RAD"
    } else if parts[1].eq_ignore_ascii_case("ISO") {
        "ISO"
    } else {
        return Err(error(
            "unsupported_directive",
            record,
            line_no,
            format!("Unsupported CTAB directive {:?}.", parts[1]),
        ));
    };
    let count = parts
        .get(2)
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|n| (1..=8).contains(n))
        .ok_or_else(|| {
            error(
                "invalid_property",
                record,
                line_no,
                "Property count must be 1 through 8.",
            )
        })?;
    if parts.len() != 3 + 2 * count {
        return Err(error(
            "invalid_property",
            record,
            line_no,
            "Property count does not match its atom/value pairs.",
        ));
    }
    for pair in parts[3..].chunks_exact(2) {
        let index = pair[0]
            .parse::<usize>()
            .ok()
            .filter(|n| *n >= 1 && *n <= atoms.len())
            .ok_or_else(|| {
                error(
                    "invalid_property",
                    record,
                    line_no,
                    "Property names an invalid atom index.",
                )
            })?
            - 1;
        let value = pair[1].parse::<i32>().map_err(|_| {
            error(
                "invalid_property",
                record,
                line_no,
                "Property value is not an integer.",
            )
        })?;
        let seen = match kind {
            "CHG" => &mut *seen_charge,
            "RAD" => &mut *seen_radical,
            _ => &mut *seen_isotope,
        };
        if !seen.insert(index) {
            return Err(error(
                "duplicate_property",
                record,
                line_no,
                "The same property names one atom more than once.",
            ));
        }
        match kind {
            "CHG" if (-15..=15).contains(&value) => {
                atoms[index].charge = i8::try_from(value).expect("checked charge")
            }
            "RAD" => {
                atoms[index].radical = match value {
                    0 => CtRadical::None,
                    1 => CtRadical::Singlet,
                    2 => CtRadical::Doublet,
                    3 => CtRadical::Triplet,
                    _ => {
                        return Err(error(
                            "invalid_radical",
                            record,
                            line_no,
                            "M RAD must be 0, 1, 2 or 3.",
                        ));
                    }
                }
            }
            "ISO" if (1..=999).contains(&value) => {
                atoms[index].isotope = Some(u16::try_from(value).expect("checked isotope"))
            }
            _ => {
                return Err(error(
                    "invalid_property",
                    record,
                    line_no,
                    format!("{kind} value is outside the supported range."),
                ));
            }
        }
    }
    Ok(kind)
}

fn parse_record(
    lines: &mut Lines<'_>,
    record: usize,
    losses: &mut Vec<Loss>,
) -> Result<CtRecord, CtError> {
    let title = lines.next(record)?.to_owned();
    title_ok(&title, record, lines.line())?;
    let program = lines.next(record)?.to_owned();
    simple_line(&program, record, lines.line(), "Program")?;
    let comment = lines.next(record)?.to_owned();
    simple_line(&comment, record, lines.line(), "Comment")?;
    let counts = lines.next(record)?;
    ascii_line(counts, record, lines.line())?;
    if counts.len() < 39 {
        return Err(error(
            "truncated_counts",
            record,
            lines.line(),
            "V2000 counts line is incomplete.",
        ));
    }
    if !counts[33..39].trim().eq_ignore_ascii_case("V2000") {
        return Err(error(
            "unsupported_version",
            record,
            lines.line(),
            "Only V2000 connection tables are supported.",
        ));
    }
    if !counts[39..].trim().is_empty() {
        return Err(error(
            "invalid_counts",
            record,
            lines.line(),
            "Unexpected counts line suffix.",
        ));
    }
    let atom_count = count_field(counts, 0, record, lines.line())?;
    let bond_count = count_field(counts, 3, record, lines.line())?;
    for start in (6..30).step_by(3) {
        if number(counts, start, start + 3, record, lines.line())? != 0 {
            return Err(error(
                "unsupported_counts",
                record,
                lines.line(),
                "Query, chiral or obsolete counts fields are outside this profile.",
            ));
        }
    }
    let property_count = number(counts, 30, 33, record, lines.line())?;
    if !(0..=999).contains(&property_count) {
        return Err(error("invalid_counts", record, lines.line(), "The obsolete property count must be nonnegative."));
    }
    if property_count != 0 && property_count != 999 {
        losses.push(loss(record, ".counts.property_count", "obsolete_property_count", LossDisposition::Dropped, SemanticImpact::Document, "The obsolete count of property lines is not retained."));
    }
    let mut atoms = Vec::with_capacity(atom_count);
    let mut masses = Vec::with_capacity(atom_count);
    for _ in 0..atom_count {
        let line = lines.next(record)?;
        let (atom, mass) = parse_atom(line, record, lines.line())?;
        atoms.push(atom);
        masses.push(mass);
    }
    let mut bonds = Vec::with_capacity(bond_count);
    let mut pairs = HashSet::new();
    for _ in 0..bond_count {
        let line = lines.next(record)?;
        let bond = parse_bond(line, atom_count, record, lines.line())?;
        let pair = (bond.begin.min(bond.end), bond.begin.max(bond.end));
        if !pairs.insert(pair) {
            return Err(error(
                "duplicate_bond",
                record,
                lines.line(),
                "More than one bond connects the same atoms.",
            ));
        }
        bonds.push(bond);
    }
    let compact: Vec<_> = atoms.iter().map(|a| (a.charge, a.radical)).collect();
    let mut charges = HashSet::new();
    let mut radicals = HashSet::new();
    let mut isotopes = HashSet::new();
    let mut has_charge_or_radical = false;
    let mut has_isotope = false;
    loop {
        let line = lines.next(record)?;
        ascii_line(line, record, lines.line())?;
        let parts: Vec<_> = line.split_whitespace().collect();
        if line.starts_with("$$$$") {
            return Err(error(
                "missing_m_end",
                record,
                lines.line(),
                "Record separator occurred before M END.",
            ));
        }
        if parts.len() >= 2
            && parts[0].eq_ignore_ascii_case("M")
            && parts[1].eq_ignore_ascii_case("END")
        {
            if parts.len() == 2 {
                break;
            }
            return Err(error(
                "invalid_m_end",
                record,
                lines.line(),
                "M END may contain only trailing whitespace.",
            ));
        }
        let kind = parse_property(
            line,
            &mut atoms,
            &mut charges,
            &mut radicals,
            &mut isotopes,
            record,
            lines.line(),
        )?;
        if kind == "ISO" {
            has_isotope = true;
        } else {
            has_charge_or_radical = true;
        }
    }
    for (index, atom) in atoms.iter_mut().enumerate() {
        if has_charge_or_radical {
            if !charges.contains(&index) {
                atom.charge = 0;
            }
            if !radicals.contains(&index) {
                atom.radical = CtRadical::None;
            }
        } else {
            (atom.charge, atom.radical) = compact[index];
        }
        if !has_isotope && masses[index] != 0 {
            let base = compact_baseline(&atom.element).ok_or_else(|| error("uncertain_isotope_baseline", record, None, format!("Compact isotope baseline for {} is not defined by this profile; use M ISO.", atom.element)))?;
            let value = base + masses[index];
            atom.isotope = Some(
                u16::try_from(value)
                    .ok()
                    .filter(|n| *n > 0)
                    .ok_or_else(|| {
                        error(
                            "invalid_isotope",
                            record,
                            None,
                            "Compact isotope mass is not positive.",
                        )
                    })?,
            );
        }
        if atom.isotope.is_some() {
            losses.push(loss(record, &format!(".atoms[{}].isotope", index + 1), "isotope_validation", LossDisposition::Unknown, SemanticImpact::Chemistry, "The public guide does not identify the PTABLE.DAT revision needed to verify the absolute mass bound."));
        }
    }
    stereo_losses(&bonds, record, losses);
    Ok(CtRecord {
        title,
        program,
        comment,
        atoms,
        bonds,
        fields: Vec::new(),
    })
}

fn stereo_losses(bonds: &[CtBond], record: usize, losses: &mut Vec<Loss>) {
    for (index, bond) in bonds.iter().enumerate() {
        if bond.order == 2 {
            losses.push(loss(record, &format!(".bonds[{}]", index + 1), "coordinate_stereo", LossDisposition::Unknown, SemanticImpact::Stereo, "V2000 double-bond stereo code 0 can be interpreted from coordinates; this codec does not infer or preserve an E/Z assignment."));
        }
    }
}

fn valid_field_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_alphabetic())
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

// A DT-numbered field may be accompanied by a registry number in parentheses. Do not
// mistake the contents of that registry number for the data-field identifier.
fn has_dt_number(header: &str) -> bool {
    let bytes = header.as_bytes();
    let mut depth = 0_usize;
    for (index, byte) in bytes.iter().copied().enumerate() {
        match byte {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            _ if depth == 0 && byte.eq_ignore_ascii_case(&b'D') => {
                let next = bytes.get(index + 1).copied();
                let digit = bytes.get(index + 2).copied();
                if next.is_some_and(|b| b.eq_ignore_ascii_case(&b'T'))
                    && digit.is_some_and(|b| b.is_ascii_digit())
                {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

fn parse_data(
    lines: &mut Lines<'_>,
    record: usize,
    item: &mut CtRecord,
    losses: &mut Vec<Loss>,
) -> Result<(), CtError> {
    loop {
        let line = lines.next(record)?;
        if let Some(suffix) = line.strip_prefix("$$$$") {
            if !suffix.is_empty() {
                losses.push(loss(
                    record,
                    ".separator",
                    "separator_suffix",
                    LossDisposition::Dropped,
                    SemanticImpact::Document,
                    "Text after the four-dollar separator is ignored.",
                ));
            }
            return Ok(());
        }
        if !line.starts_with('>') {
            return Err(error(
                "invalid_data_header",
                record,
                lines.line(),
                "Expected an SD data header or record separator.",
            ));
        }
        let Some(start) = line.find('<') else {
            let numbered = has_dt_number(line);
            return Err(error(
                if numbered {
                    "unsupported_dt_field"
                } else {
                    "invalid_data_header"
                },
                record,
                lines.line(),
                "A named <field> is required; DT-numbered fields are not supported.",
            ));
        };
        let end = line[start + 1..]
            .find('>')
            .map(|p| p + start + 1)
            .ok_or_else(|| {
                error(
                    "invalid_data_header",
                    record,
                    lines.line(),
                    "Field name has no closing >.",
                )
            })?;
        let name = &line[start + 1..end];
        if !valid_field_name(name) {
            return Err(error(
                "invalid_data_name",
                record,
                lines.line(),
                "Field name must start with an ASCII letter and contain only ASCII letters, digits or underscore.",
            ));
        }
        if !line[1..start].trim().is_empty() || !line[end + 1..].trim().is_empty() {
            losses.push(loss(
                record,
                &format!(".fields[{}].header", item.fields.len() + 1),
                "data_header_metadata",
                LossDisposition::Dropped,
                SemanticImpact::Document,
                "Optional SD header information is not represented.",
            ));
        }
        if item.fields.len() == MAX_FIELDS {
            return Err(error(
                "resource_limit",
                record,
                lines.line(),
                "Too many SD fields.",
            ));
        }
        let mut values = Vec::new();
        loop {
            let value = lines.next(record)?;
            if value.is_empty() {
                break;
            }
            if value.starts_with("$$$$") {
                return Err(error(
                    "unterminated_data",
                    record,
                    lines.line(),
                    "A data item must end with a blank line before a separator.",
                ));
            }
            if value.chars().count() > 200 {
                return Err(error(
                    "data_line_too_long",
                    record,
                    lines.line(),
                    "SD data value line exceeds 200 Unicode characters.",
                ));
            }
            values.push(value);
        }
        item.fields.push(CtField {
            name: name.into(),
            value: values.join("\n"),
        });
    }
}

impl CtCodec for V2000Codec {
    fn read(&self, format: CtFormat, input: &[u8]) -> Result<CtResult<Vec<CtRecord>>, CtError> {
        if input.is_empty() {
            return Err(error("empty_input", 1, None, "No MOL record was supplied."));
        }
        if input.len() > MAX_BYTES {
            return Err(error("resource_limit", 1, None, "Input exceeds 4 MiB."));
        }
        let mut lines = Lines::new(input);
        let mut records = Vec::new();
        let mut losses = Vec::new();
        while !lines.done() {
            let index = records.len() + 1;
            if index > MAX_RECORDS {
                return Err(error(
                    "resource_limit",
                    index,
                    None,
                    "More than 128 records.",
                ));
            }
            let mut item = parse_record(&mut lines, index, &mut losses)?;
            if format == CtFormat::Sdf {
                parse_data(&mut lines, index, &mut item, &mut losses)?;
            }
            records.push(item);
            if format == CtFormat::Mol {
                while !lines.done() {
                    if !lines.next(index)?.trim().is_empty() {
                        return Err(error(
                            "trailing_content",
                            index,
                            lines.line(),
                            "Unexpected content after M END in MOL file.",
                        ));
                    }
                }
            }
        }
        if records.is_empty() {
            return Err(error("empty_input", 1, None, "No MOL record was supplied."));
        }
        Ok(CtResult {
            value: records,
            losses,
        })
    }

    fn write(&self, format: CtFormat, records: &[CtRecord]) -> Result<CtResult<String>, CtError> {
        if records.is_empty() {
            return Err(error(
                "empty_input",
                1,
                None,
                "At least one record is required.",
            ));
        }
        if format == CtFormat::Mol && records.len() != 1 {
            return Err(error(
                "invalid_count",
                2,
                None,
                "MOL contains exactly one record.",
            ));
        }
        let mut output = String::new();
        let mut losses = Vec::new();
        for (index, record) in records.iter().enumerate() {
            let number = index + 1;
            if number > MAX_RECORDS {
                return Err(error(
                    "resource_limit",
                    number,
                    None,
                    "More than 128 records.",
                ));
            }
            write_record(&mut output, record, format, number, &mut losses)?;
            if output.len() > MAX_BYTES {
                return Err(error(
                    "resource_limit",
                    number,
                    None,
                    "Output exceeds 4 MiB.",
                ));
            }
        }
        Ok(CtResult {
            value: output,
            losses,
        })
    }
}

fn write_record(
    out: &mut String,
    item: &CtRecord,
    format: CtFormat,
    record: usize,
    losses: &mut Vec<Loss>,
) -> Result<(), CtError> {
    title_ok(&item.title, record, None)?;
    simple_line(&item.program, record, None, "Program")?;
    simple_line(&item.comment, record, None, "Comment")?;
    if item.atoms.len() > MAX_ITEMS || item.bonds.len() > MAX_ITEMS {
        return Err(error(
            "resource_limit",
            record,
            None,
            "V2000 supports at most 999 atoms and bonds.",
        ));
    }
    if item.fields.len() > MAX_FIELDS {
        return Err(error("resource_limit", record, None, "Too many SD fields."));
    }
    if format == CtFormat::Mol && !item.fields.is_empty() {
        return Err(error(
            "unsupported_fields",
            record,
            None,
            "MOL cannot contain SD fields.",
        ));
    }
    writeln!(out, "{}\n{}\n{}", item.title, item.program, item.comment)
        .expect("String writes do not fail");
    writeln!(
        out,
        "{:>3}{:>3}  0  0  0  0  0  0  0  0999 V2000",
        item.atoms.len(),
        item.bonds.len()
    )
    .expect("String writes do not fail");
    let mut charges = Vec::new();
    let mut radicals = Vec::new();
    let mut isotopes = Vec::new();
    for (i, atom) in item.atoms.iter().enumerate() {
        if Element::from_symbol(&atom.element).is_none() {
            return Err(error(
                "unsupported_atom",
                record,
                None,
                format!("Invalid element {:?}.", atom.element),
            ));
        }
        if !(-15..=15).contains(&atom.charge) {
            return Err(error(
                "invalid_charge",
                record,
                None,
                "Charge exceeds V2000 range -15..15.",
            ));
        }
        let mut coords = Vec::new();
        for value in [atom.x, atom.y, atom.z] {
            if !value.is_finite() {
                return Err(error(
                    "invalid_coordinate",
                    record,
                    None,
                    "Coordinate must be finite.",
                ));
            }
            let formatted = format!("{value:>10.4}");
            if formatted.len() != 10 {
                return Err(error(
                    "invalid_coordinate",
                    record,
                    None,
                    "Coordinate does not fit ten columns.",
                ));
            }
            if formatted.trim().parse::<f64>().ok() != Some(value) {
                losses.push(loss(
                    record,
                    &format!(".atoms[{}].coordinates", i + 1),
                    "coordinate_precision",
                    LossDisposition::Normalized,
                    SemanticImpact::Geometry,
                    "V2000 coordinates are rounded to four decimal places.",
                ));
            }
            coords.push(formatted);
        }
        writeln!(
            out,
            "{}{}{} {:<3} 0  0  0  0  0  0  0  0  0  0  0  0",
            coords[0], coords[1], coords[2], atom.element
        )
        .expect("String writes do not fail");
        if atom.charge != 0 {
            charges.push((i + 1, i32::from(atom.charge)));
        }
        let rad = match atom.radical {
            CtRadical::None => 0,
            CtRadical::Singlet => 1,
            CtRadical::Doublet => 2,
            CtRadical::Triplet => 3,
        };
        if rad != 0 {
            radicals.push((i + 1, rad));
        }
        if let Some(mass) = atom.isotope {
            if mass == 0 || mass > 999 {
                return Err(error(
                    "invalid_isotope",
                    record,
                    None,
                    "Isotope mass must be 1 through 999.",
                ));
            }
            isotopes.push((i + 1, i32::from(mass)));
            losses.push(loss(record, &format!(".atoms[{}].isotope", i + 1), "isotope_validation", LossDisposition::Unknown, SemanticImpact::Chemistry, "The public guide does not identify the PTABLE.DAT revision needed to verify the absolute mass bound."));
        }
    }
    let mut pairs = HashSet::new();
    for bond in &item.bonds {
        if bond.begin >= item.atoms.len() || bond.end >= item.atoms.len() || bond.begin == bond.end
        {
            return Err(error(
                "invalid_bond",
                record,
                None,
                "Bond endpoints must be distinct existing atoms.",
            ));
        }
        if !(1..=3).contains(&bond.order) {
            return Err(error(
                "unsupported_bond",
                record,
                None,
                "Only single, double and triple bonds are supported.",
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
        writeln!(
            out,
            "{:>3}{:>3}{:>3}  0  0  0  0",
            bond.begin + 1,
            bond.end + 1,
            bond.order
        )
        .expect("String writes do not fail");
    }
    for (tag, pairs) in [("CHG", charges), ("RAD", radicals), ("ISO", isotopes)] {
        for chunk in pairs.chunks(8) {
            write!(out, "M  {tag}{:>3}", chunk.len()).expect("String writes do not fail");
            for (atom, value) in chunk {
                write!(out, "{atom:>4}{value:>4}").expect("String writes do not fail");
            }
            out.push('\n');
        }
    }
    out.push_str("M  END\n");
    if format == CtFormat::Sdf {
        for field in &item.fields {
            if !valid_field_name(&field.name) {
                return Err(error(
                    "invalid_data_name",
                    record,
                    None,
                    "Field name must start with an ASCII letter and contain only ASCII letters, digits or underscore.",
                ));
            }
            writeln!(out, ">  <{}>", field.name).expect("String writes do not fail");
            if !field.value.is_empty() {
                for line in field.value.split('\n') {
                    if line.is_empty() || line.contains(['\r', '\0']) || line.starts_with("$$$$") {
                        return Err(error(
                            "invalid_data_value",
                            record,
                            None,
                            "SD values cannot contain a blank line, CR, NUL, or a line beginning with $$$$.",
                        ));
                    }
                    if line.chars().count() > 200 {
                        return Err(error(
                            "data_line_too_long",
                            record,
                            None,
                            "SD data value line exceeds 200 Unicode characters.",
                        ));
                    }
                    writeln!(out, "{line}").expect("String writes do not fail");
                    if out.len() > MAX_BYTES {
                        return Err(error(
                            "resource_limit",
                            record,
                            None,
                            "Output exceeds 4 MiB.",
                        ));
                    }
                }
            }
            out.push('\n');
        }
        out.push_str("$$$$\n");
    }
    stereo_losses(&item.bonds, record, losses);
    Ok(())
}
