//! V2000 connection tables and SD records. This module does not perceive chemistry or stereo.
//!
//! Positions map 1 Å in a connection table to 16 scene points; the vertical axis is reversed.
//! This display scale is a convention, not a conversion between physical dimensions.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use chemifree_application::{
    ChemicalAtom, ChemicalBond, ChemicalCodec, ChemicalFormat, ChemicalRecord, CodecError,
    DataField, Loss, LossDisposition, SemanticImpact, WrittenChemical,
};
use chemifree_domain::document::{Atom, BondOrder, Document, Point};
use chemifree_domain::element::Element;
use chemifree_domain::scene::SceneObject;
use chemifree_domain::states::{
    AtomKind, BondQuery, Hydrogens, QueryAtom, QueryBondOrder, Radical,
};

const POINTS_PER_ANGSTROM: f64 = 16.0;
const MAX_TEXT_BYTES: usize = 1024 * 1024;
const MAX_RECORDS: usize = 999;
// The CTfile description references a periodic-table mass without fixing a portable table.
// Restrict compact differences to H–Ca, where rounded conventional atomic weights and the
// abundant isotope agree. Other elements need an explicit M ISO rather than a guessed mass.
const COMPACT_REFERENCE_MASSES: [u16; 20] = [
    1, 4, 7, 9, 11, 12, 14, 16, 19, 20, 23, 24, 27, 28, 31, 32, 35, 40, 39, 40,
];

#[derive(Clone, Copy, Default)]
pub struct MolfileCodec;

fn problem(
    code: &'static str,
    message: impl Into<String>,
    record: usize,
    line: usize,
    field: &str,
) -> CodecError {
    CodecError {
        code,
        message: message.into(),
        record: Some(record),
        line: Some(line),
        field: Some(field.into()),
    }
}

fn write_problem(message: impl Into<String>, field: &str) -> CodecError {
    CodecError {
        code: "unsupported_feature",
        message: message.into(),
        record: None,
        line: None,
        field: Some(field.into()),
    }
}

fn loss(feature: &str, path: String, reason: &str, impact: SemanticImpact) -> Loss {
    Loss {
        feature: feature.into(),
        source_path: path,
        disposition: LossDisposition::Dropped,
        reason: reason.into(),
        semantic_impact: impact,
    }
}

fn integer(
    line: &str,
    start: usize,
    width: usize,
    record: usize,
    number: usize,
    field: &str,
) -> Result<i32, CodecError> {
    let raw = line.get(start..start + width).ok_or_else(|| {
        problem(
            "truncated_field",
            "A fixed-width field is incomplete.",
            record,
            number,
            field,
        )
    })?;
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(0);
    }
    raw.parse().map_err(|_| {
        problem(
            "invalid_field",
            "Expected an integer in the fixed-width field.",
            record,
            number,
            field,
        )
    })
}

fn decimal(
    line: &str,
    start: usize,
    record: usize,
    number: usize,
    field: &str,
) -> Result<f64, CodecError> {
    let raw = line.get(start..start + 10).ok_or_else(|| {
        problem(
            "truncated_field",
            "A coordinate field is incomplete.",
            record,
            number,
            field,
        )
    })?;
    let value: f64 = raw.trim().parse().map_err(|_| {
        problem(
            "invalid_field",
            "Expected a coordinate number.",
            record,
            number,
            field,
        )
    })?;
    if !value.is_finite() {
        return Err(problem(
            "invalid_field",
            "Coordinates must be finite.",
            record,
            number,
            field,
        ));
    }
    Ok(value)
}

fn atom_kind(symbol: &str, record: usize, line: usize) -> Result<AtomKind, CodecError> {
    if let Some(element) = Element::from_symbol(symbol) {
        return Ok(AtomKind::Element(element));
    }
    let query = match symbol {
        "*" => QueryAtom::Any,
        "A" => QueryAtom::AnyHeavy,
        "Q" => QueryAtom::Heteroatom,
        _ => {
            return Err(problem(
                "unsupported_atom",
                format!("Unsupported atom symbol {symbol:?}."),
                record,
                line,
                "symbol",
            ));
        }
    };
    Ok(AtomKind::Query(query))
}

fn bond_order(value: i32, record: usize, line: usize) -> Result<BondOrder, CodecError> {
    use QueryBondOrder as Q;
    Ok(match value {
        1 => BondOrder::Single,
        2 => BondOrder::Double,
        3 => BondOrder::Triple,
        4 => BondOrder::Aromatic,
        5 => BondOrder::Query(BondQuery::OneOf(vec![Q::Single, Q::Double])),
        6 => BondOrder::Query(BondQuery::OneOf(vec![Q::Single, Q::Aromatic])),
        7 => BondOrder::Query(BondQuery::OneOf(vec![Q::Double, Q::Aromatic])),
        8 => BondOrder::Query(BondQuery::Any),
        _ => {
            return Err(problem(
                "invalid_bond",
                "Bond type must be between 1 and 8.",
                record,
                line,
                "bond_type",
            ));
        }
    })
}

fn line_at<'a>(
    lines: &'a [&str],
    index: usize,
    record: usize,
    offset: usize,
    field: &str,
) -> Result<&'a str, CodecError> {
    lines.get(index).copied().ok_or_else(|| {
        problem(
            "truncated_record",
            "The record ended before this field.",
            record,
            offset + index,
            field,
        )
    })
}

fn property_pairs(
    line: &str,
    record: usize,
    line_number: usize,
) -> Result<Vec<(usize, i32)>, CodecError> {
    let count = integer(line, 6, 3, record, line_number, "property_count")?;
    if !(1..=8).contains(&count) {
        return Err(problem(
            "invalid_property",
            "Property count must be 1 to 8.",
            record,
            line_number,
            "property_count",
        ));
    }
    let count = count as usize;
    let expected = 9 + count * 8;
    if line.len() < expected {
        return Err(problem(
            "truncated_field",
            "Property pairs are incomplete.",
            record,
            line_number,
            "property_pairs",
        ));
    }
    if !line[expected..].trim().is_empty() {
        return Err(problem(
            "invalid_property",
            "Unexpected trailing property data.",
            record,
            line_number,
            "property_pairs",
        ));
    }
    let mut pairs = Vec::with_capacity(count);
    for i in 0..count {
        let index = integer(line, 9 + i * 8, 4, record, line_number, "property_atom")?;
        let value = integer(line, 13 + i * 8, 4, record, line_number, "property_value")?;
        if index < 1 {
            return Err(problem(
                "invalid_property",
                "Property atom number must be positive.",
                record,
                line_number,
                "property_atom",
            ));
        }
        pairs.push((index as usize - 1, value));
    }
    Ok(pairs)
}

fn parse_record(
    lines: &[&str],
    record: usize,
    offset: usize,
    sdf: bool,
) -> Result<ChemicalRecord, CodecError> {
    let title = line_at(lines, 0, record, offset, "title")?.to_owned();
    let program = line_at(lines, 1, record, offset, "program")?;
    let comment = line_at(lines, 2, record, offset, "comment")?;
    let counts = line_at(lines, 3, record, offset, "counts")?;
    if !counts.is_ascii() || counts.get(33..39) != Some(" V2000") || !counts[39..].trim().is_empty()
    {
        return Err(problem(
            "unsupported_format",
            "Expected a fixed-width V2000 counts line.",
            record,
            offset + 3,
            "version",
        ));
    }
    if counts[..3].trim().is_empty() || counts[3..6].trim().is_empty() {
        return Err(problem(
            "invalid_counts",
            "Atom and bond counts are required.",
            record,
            offset + 3,
            "counts",
        ));
    }
    let atom_count = integer(counts, 0, 3, record, offset + 3, "atom_count")?;
    let bond_count = integer(counts, 3, 3, record, offset + 3, "bond_count")?;
    if !(0..=999).contains(&atom_count) || !(0..=999).contains(&bond_count) {
        return Err(problem(
            "invalid_counts",
            "Atom and bond counts must be 0 to 999.",
            record,
            offset + 3,
            "counts",
        ));
    }
    let atom_count = atom_count as usize;
    let bond_count = bond_count as usize;
    let mut losses = Vec::new();
    if !program.trim().is_empty() || !comment.is_empty() {
        losses.push(loss(
            "mol_header",
            format!("records[{record}].header"),
            "Program and comment header lines are not stored in a document.",
            SemanticImpact::Document,
        ));
    }
    let list_count = integer(counts, 6, 3, record, offset + 3, "atom_list_count")?;
    if list_count != 0 {
        return Err(problem(
            "unsupported_feature",
            "Atom list blocks are not supported.",
            record,
            offset + 3,
            "atom_list_count",
        ));
    }
    let chiral = integer(counts, 12, 3, record, offset + 3, "chiral_flag")?;
    if !(0..=1).contains(&chiral) {
        return Err(problem(
            "invalid_counts",
            "Chiral flag must be 0 or 1.",
            record,
            offset + 3,
            "chiral_flag",
        ));
    }
    if chiral != 0 {
        losses.push(loss(
            "chiral_flag",
            format!("records[{record}].counts.chiral"),
            "Chirality is not interpreted or stored.",
            SemanticImpact::Stereo,
        ));
    }
    // Nonzero obsolete count fields carry semantics that are not modeled.
    for start in [9, 15, 18, 21, 24, 27] {
        if integer(counts, start, 3, record, offset + 3, "counts_extension")? != 0 {
            losses.push(loss(
                "counts_extension",
                format!("records[{record}].counts[{start}]"),
                "This counts field is not represented.",
                SemanticImpact::Unknown,
            ));
        }
    }
    let mut atoms = Vec::with_capacity(atom_count);
    let mut mass_differences = Vec::with_capacity(atom_count);
    for index in 0..atom_count {
        let row = 4 + index;
        let line = line_at(lines, row, record, offset, "atom")?;
        let number = offset + row;
        if !line.is_ascii() || line.len() < 39 || (line.len() - 39) % 3 != 0 {
            return Err(problem(
                "truncated_field",
                "An atom line must contain complete fixed-width fields.",
                record,
                number,
                "atom",
            ));
        }
        let x = decimal(line, 0, record, number, "x")?;
        let y = decimal(line, 10, record, number, "y")?;
        let z = decimal(line, 20, record, number, "z")?;
        let symbol = line.get(31..34).unwrap_or_default().trim();
        let kind = atom_kind(symbol, record, number)?;
        let mut atom = Atom::new(kind);
        let mass = integer(line, 34, 2, record, number, "mass_difference")?;
        if !(-3..=4).contains(&mass) {
            return Err(problem(
                "invalid_atom",
                "Mass difference must be between -3 and 4.",
                record,
                number,
                "mass_difference",
            ));
        }
        mass_differences.push((mass, number));
        match integer(line, 36, 3, record, number, "charge")? {
            0 => (),
            1 => atom.charge = 3,
            2 => atom.charge = 2,
            3 => atom.charge = 1,
            4 => atom.radical = Radical::Doublet,
            5 => atom.charge = -1,
            6 => atom.charge = -2,
            7 => atom.charge = -3,
            _ => {
                return Err(problem(
                    "invalid_atom",
                    "Invalid atom charge code.",
                    record,
                    number,
                    "charge",
                ));
            }
        }
        if z != 0.0 {
            losses.push(loss(
                "z_coordinate",
                format!("records[{record}].atoms[{}].z", index + 1),
                "The document stores only 2D positions.",
                SemanticImpact::Geometry,
            ));
        }
        for start in (39..line.len()).step_by(3) {
            let value = integer(line, start, 3, record, number, "atom_attribute")?;
            if value != 0 {
                let (feature, impact) = if start == 39 {
                    ("atom_stereo_parity", SemanticImpact::Stereo)
                } else if start == 42 {
                    ("query_hydrogen_count", SemanticImpact::Chemistry)
                } else {
                    ("atom_attribute", SemanticImpact::Unknown)
                };
                losses.push(loss(
                    feature,
                    format!("records[{record}].atoms[{}].field[{start}]", index + 1),
                    "This atom field is not represented by the document.",
                    impact,
                ));
            }
        }
        let position =
            Point::new(x * POINTS_PER_ANGSTROM, -y * POINTS_PER_ANGSTROM).map_err(|error| {
                problem(
                    "invalid_coordinate",
                    error.to_string(),
                    record,
                    number,
                    "position",
                )
            })?;
        atoms.push(ChemicalAtom { atom, position });
    }
    let mut bonds = Vec::with_capacity(bond_count);
    let mut edges = BTreeSet::new();
    for index in 0..bond_count {
        let row = 4 + atom_count + index;
        let line = line_at(lines, row, record, offset, "bond")?;
        let number = offset + row;
        if !line.is_ascii() || line.len() < 12 || line.len() % 3 != 0 {
            return Err(problem(
                "truncated_field",
                "A bond line must contain complete fixed-width fields.",
                record,
                number,
                "bond",
            ));
        }
        let begin = integer(line, 0, 3, record, number, "begin")?;
        let end = integer(line, 3, 3, record, number, "end")?;
        if begin < 1
            || end < 1
            || begin as usize > atom_count
            || end as usize > atom_count
            || begin == end
        {
            return Err(problem(
                "invalid_bond",
                "Bond endpoints must name distinct existing atoms.",
                record,
                number,
                "endpoints",
            ));
        }
        let (begin, end) = (begin as usize - 1, end as usize - 1);
        if !edges.insert((begin.min(end), begin.max(end))) {
            return Err(problem(
                "invalid_bond",
                "Duplicate bond endpoints.",
                record,
                number,
                "endpoints",
            ));
        }
        let order = bond_order(
            integer(line, 6, 3, record, number, "bond_type")?,
            record,
            number,
        )?;
        for start in (9..line.len()).step_by(3) {
            if integer(line, start, 3, record, number, "bond_attribute")? != 0 {
                let (feature, impact) = if start == 9 {
                    ("bond_stereo", SemanticImpact::Stereo)
                } else {
                    ("bond_attribute", SemanticImpact::Unknown)
                };
                losses.push(loss(
                    feature,
                    format!("records[{record}].bonds[{}].field[{start}]", index + 1),
                    "This bond field is not represented by the document.",
                    impact,
                ));
            }
        }
        bonds.push(ChemicalBond { begin, end, order });
    }
    // Coordinates can express cis/trans geometry even without a bond stereo code. The model
    // cannot assign it, so do not claim that such geometry was interpreted.
    let mut degree = vec![0_usize; atom_count];
    for bond in &bonds {
        degree[bond.begin] += 1;
        degree[bond.end] += 1;
    }
    for (index, bond) in bonds.iter().enumerate() {
        if bond.order == BondOrder::Double && degree[bond.begin] > 1 && degree[bond.end] > 1 {
            losses.push(Loss {
                feature: "coordinate_stereo".into(),
                source_path: format!("records[{record}].bonds[{}]", index + 1),
                disposition: LossDisposition::Unknown,
                reason: "Possible cis/trans meaning in coordinates is not interpreted.".into(),
                semantic_impact: SemanticImpact::Stereo,
            });
        }
    }
    let mut row = 4 + atom_count + bond_count;
    let mut properties: BTreeMap<&str, BTreeMap<usize, (i32, usize)>> = BTreeMap::new();
    loop {
        let line = line_at(lines, row, record, offset, "M  END")?;
        let number = offset + row;
        row += 1;
        if line == "M  END" {
            break;
        }
        if !line.is_ascii() {
            return Err(problem(
                "invalid_property",
                "Property lines must be ASCII.",
                record,
                number,
                "property",
            ));
        }
        let name = line.get(..6).unwrap_or("");
        if matches!(name, "M  CHG" | "M  RAD" | "M  ISO") {
            let pairs = property_pairs(line, record, number)?;
            let values = properties.entry(name).or_default();
            for (index, value) in pairs {
                if index >= atom_count {
                    return Err(problem(
                        "invalid_property",
                        "Property refers to a missing atom.",
                        record,
                        number,
                        "property_atom",
                    ));
                }
                if values.insert(index, (value, number)).is_some() {
                    return Err(problem(
                        "duplicate_property",
                        "A property is stated twice for one atom.",
                        record,
                        number,
                        "property_atom",
                    ));
                }
            }
        } else {
            // Alias/group continuation lines cannot safely be skipped as independent properties.
            if !line.starts_with("M  ") {
                return Err(problem(
                    "unsupported_feature",
                    "Unsupported property or continuation line.",
                    record,
                    number,
                    "property",
                ));
            }
            losses.push(loss(
                "unknown_property",
                format!("records[{record}].line[{number}]"),
                "This property is not interpreted or retained.",
                SemanticImpact::Unknown,
            ));
        }
    }
    if properties.contains_key("M  CHG") || properties.contains_key("M  RAD") {
        for atom in &mut atoms {
            atom.atom.charge = 0;
            atom.atom.radical = Radical::None;
        }
    }
    if let Some(values) = properties.get("M  CHG") {
        for (&index, &(value, number)) in values {
            if !(-8..=8).contains(&value) {
                return Err(problem(
                    "unsupported_charge",
                    "The document supports charges from -8 to 8.",
                    record,
                    number,
                    "charge",
                ));
            }
            atoms[index].atom.charge = value as i8;
        }
    }
    if let Some(values) = properties.get("M  RAD") {
        for (&index, &(value, number)) in values {
            atoms[index].atom.radical = match value {
                1 => Radical::Singlet,
                2 => Radical::Doublet,
                3 => Radical::Triplet,
                _ => {
                    return Err(problem(
                        "invalid_property",
                        "Radical must be 1, 2, or 3.",
                        record,
                        number,
                        "radical",
                    ));
                }
            };
        }
    }
    if let Some(values) = properties.get("M  ISO") {
        for (&index, &(value, number)) in values {
            if !(1..=999).contains(&value) {
                return Err(problem(
                    "invalid_property",
                    "Isotope mass must be 1 to 999.",
                    record,
                    number,
                    "isotope",
                ));
            }
            atoms[index].atom.isotope = Some(value as u16);
        }
    } else {
        for (index, &(difference, number)) in mass_differences.iter().enumerate() {
            if difference != 0 {
                let element = atoms[index].atom.kind.element().ok_or_else(|| {
                    problem(
                        "unsupported_atom",
                        "A relative isotope requires an element atom.",
                        record,
                        number,
                        "mass_difference",
                    )
                })?;
                let reference = COMPACT_REFERENCE_MASSES.get(usize::from(element.atomic_number() - 1))
                    .ok_or_else(|| problem("unsupported_isotope_reference", "Compact isotope differences outside H–Ca require an explicit M  ISO property.", record, number, "mass_difference"))?;
                let reference = i32::from(*reference);
                let mass = reference + difference;
                if !(1..=999).contains(&mass) {
                    return Err(problem(
                        "invalid_atom",
                        "Relative isotope mass is out of range.",
                        record,
                        number,
                        "mass_difference",
                    ));
                }
                atoms[index].atom.isotope = Some(mass as u16);
            }
        }
    }
    for (index, atom) in atoms.iter().enumerate() {
        if matches!(atom.atom.kind, AtomKind::Query(_))
            && (atom.atom.charge != 0 || atom.atom.isotope.is_some())
        {
            return Err(problem(
                "unsupported_atom",
                "Query atoms with charge or isotope are not representable.",
                record,
                offset + 4 + index,
                "atom",
            ));
        }
    }
    let mut fields = Vec::new();
    if sdf {
        while row < lines.len() {
            let header = lines[row];
            let number = offset + row;
            if header.is_empty() {
                row += 1;
                continue;
            }
            if !header.starts_with('>') {
                return Err(problem(
                    "invalid_data_header",
                    "Expected an SD data header.",
                    record,
                    number,
                    "data_header",
                ));
            }
            let start = header.find('<').ok_or_else(|| {
                problem(
                    "invalid_data_header",
                    "Data header must contain a field name in angle brackets.",
                    record,
                    number,
                    "data_header",
                )
            })?;
            let end = header[start + 1..]
                .find('>')
                .map(|n| start + 1 + n)
                .ok_or_else(|| {
                    problem(
                        "invalid_data_header",
                        "Data header is missing its closing angle bracket.",
                        record,
                        number,
                        "data_header",
                    )
                })?;
            let name = &header[start + 1..end];
            if name.is_empty() || name.contains(['<', '>']) {
                return Err(problem(
                    "invalid_data_header",
                    "Data field name is empty or invalid.",
                    record,
                    number,
                    "data_header",
                ));
            }
            if header.trim() != format!("> <{name}>") {
                losses.push(loss(
                    "sdf_header_metadata",
                    format!("records[{record}].line[{number}]"),
                    "Additional SD field header metadata is not retained.",
                    SemanticImpact::Document,
                ));
            }
            row += 1;
            let value_start = row;
            while row < lines.len() && !lines[row].is_empty() {
                row += 1;
            }
            if row == lines.len() {
                return Err(problem(
                    "truncated_record",
                    "A data field must end with a blank line.",
                    record,
                    offset + row,
                    "data_value",
                ));
            }
            fields.push(DataField {
                name: name.to_owned(),
                value: lines[value_start..row].join("\n"),
            });
            row += 1;
        }
        if !fields.is_empty() {
            losses.push(loss(
                "sdf_data_fields",
                format!("records[{record}].fields"),
                "Fields are returned separately but are not stored in the imported document.",
                SemanticImpact::Document,
            ));
        }
    } else if lines[row..].iter().any(|line| !line.is_empty()) {
        return Err(problem(
            "unexpected_data",
            "MOL input must end after M  END.",
            record,
            offset + row,
            "trailing_data",
        ));
    }
    Ok(ChemicalRecord {
        title,
        atoms,
        bonds,
        fields,
        losses,
    })
}

impl ChemicalCodec for MolfileCodec {
    fn read(&self, format: ChemicalFormat, text: &str) -> Result<Vec<ChemicalRecord>, CodecError> {
        if text.len() > MAX_TEXT_BYTES {
            return Err(problem(
                "resource_limit",
                "Input exceeds the codec size limit.",
                1,
                1,
                "input",
            ));
        }
        let lines: Vec<&str> = text.lines().collect();
        if format == ChemicalFormat::Mol {
            return Ok(vec![parse_record(&lines, 1, 1, false)?]);
        }
        let mut records = Vec::new();
        let mut start = 0;
        for (index, line) in lines.iter().enumerate() {
            if line.starts_with("$$$$") {
                if *line != "$$$$" {
                    return Err(problem(
                        "invalid_delimiter",
                        "The SDF record delimiter must be exactly $$$$.",
                        records.len() + 1,
                        index + 1,
                        "delimiter",
                    ));
                }
                if records.len() == MAX_RECORDS {
                    return Err(problem(
                        "resource_limit",
                        "Too many SDF records.",
                        records.len() + 1,
                        index + 1,
                        "records",
                    ));
                }
                records.push(parse_record(
                    &lines[start..index],
                    records.len() + 1,
                    start + 1,
                    true,
                )?);
                start = index + 1;
            }
        }
        if records.is_empty() {
            return Err(problem(
                "truncated_record",
                "An SDF needs at least one $$$$-terminated record.",
                1,
                1,
                "delimiter",
            ));
        }
        if lines[start..].iter().any(|line| !line.is_empty()) {
            return Err(problem(
                "truncated_record",
                "The last SDF record lacks a $$$$ delimiter.",
                records.len() + 1,
                start + 1,
                "delimiter",
            ));
        }
        Ok(records)
    }

    fn write(
        &self,
        format: ChemicalFormat,
        document: &Document,
        fields: &[DataField],
    ) -> Result<WrittenChemical, CodecError> {
        write_document(format, document, fields)
    }
}

fn mol_bond_type(order: &BondOrder) -> Option<u8> {
    use QueryBondOrder as Q;
    match order {
        BondOrder::Single => Some(1),
        BondOrder::Double => Some(2),
        BondOrder::Triple => Some(3),
        BondOrder::Aromatic => Some(4),
        BondOrder::Query(BondQuery::Any) => Some(8),
        BondOrder::Query(BondQuery::OneOf(values))
            if values.len() == 2 && values.contains(&Q::Single) && values.contains(&Q::Double) =>
        {
            Some(5)
        }
        BondOrder::Query(BondQuery::OneOf(values))
            if values.len() == 2
                && values.contains(&Q::Single)
                && values.contains(&Q::Aromatic) =>
        {
            Some(6)
        }
        BondOrder::Query(BondQuery::OneOf(values))
            if values.len() == 2
                && values.contains(&Q::Double)
                && values.contains(&Q::Aromatic) =>
        {
            Some(7)
        }
        _ => None,
    }
}

fn coordinate(
    value: f64,
    atom: &str,
    axis: &str,
    losses: &mut Vec<Loss>,
) -> Result<String, CodecError> {
    let rendered = format!("{value:>10.4}");
    if rendered.len() != 10 {
        return Err(write_problem(
            "Coordinate does not fit a V2000 ten-column field.",
            axis,
        ));
    }
    let represented: f64 = rendered
        .trim()
        .parse()
        .map_err(|_| write_problem("Could not encode coordinate.", axis))?;
    if represented != value {
        losses.push(Loss {
            feature: "coordinate_precision".into(),
            source_path: format!("document.atoms[{atom}].position.{axis}"),
            disposition: LossDisposition::Normalized,
            reason: "V2000 writes four decimal places in its ten-column coordinate field.".into(),
            semantic_impact: SemanticImpact::Geometry,
        });
    }
    Ok(rendered)
}

fn write_properties(output: &mut String, kind: &str, pairs: &[(usize, i32)]) {
    for chunk in pairs.chunks(8) {
        let _ = write!(output, "M  {kind}{:>3}", chunk.len());
        for (index, value) in chunk {
            let _ = write!(output, "{index:>4}{value:>4}");
        }
        output.push('\n');
    }
}

fn write_document(
    format: ChemicalFormat,
    document: &Document,
    fields: &[DataField],
) -> Result<WrittenChemical, CodecError> {
    let atom_count = document.atoms().len();
    let bond_count = document.bonds().len();
    if atom_count > 999 || bond_count > 999 {
        return Err(write_problem(
            "V2000 holds at most 999 atoms and 999 bonds.",
            "counts",
        ));
    }
    if document.title().contains(['\n', '\r', '\0']) || document.title().len() > 80 {
        return Err(write_problem(
            "The MOL title must fit one line of at most 80 UTF-8 bytes.",
            "title",
        ));
    }
    if format == ChemicalFormat::Mol && !fields.is_empty() {
        return Err(write_problem("Data fields require SDF output.", "fields"));
    }
    for field in fields {
        if field.name.is_empty() || field.name.contains(['<', '>', '\n', '\r', '\0']) {
            return Err(write_problem("Invalid SD field name.", "fields.name"));
        }
        if !field.value.is_empty()
            && (field.value.contains('\r')
                || field.value.contains('\0')
                || field.value.split('\n').any(|line| {
                    line.is_empty() || line.starts_with("$$$$") || line.chars().count() > 200
                }))
        {
            return Err(write_problem(
                "An SD value cannot contain blank lines, record delimiters, or lines longer than 200 characters.",
                "fields.value",
            ));
        }
    }
    let mut output = format!(
        "{}\n  Chemifree\n\n{atom_count:>3}{bond_count:>3}  0  0  0  0  0  0  0  0999 V2000\n",
        document.title()
    );
    let mut losses = Vec::new();
    // MOL has no page, style, selection, or history representation.
    losses.push(loss(
        "document_settings",
        "document.settings".into(),
        "MOL/SDF has no Chemifree style settings.",
        SemanticImpact::Rendering,
    ));
    losses.push(loss(
        "document_identity",
        "document.id".into(),
        "MOL/SDF does not preserve document or entity identifiers.",
        SemanticImpact::Document,
    ));
    if document.undo_depth() > 0 || document.redo_depth() > 0 {
        losses.push(loss(
            "history",
            "document.history".into(),
            "MOL/SDF has no edit history.",
            SemanticImpact::Document,
        ));
    }
    let selection = document.selection();
    if !selection.atoms.is_empty() || !selection.bonds.is_empty() || !selection.objects.is_empty() {
        losses.push(loss(
            "selection",
            "document.selection".into(),
            "MOL/SDF has no editor selection.",
            SemanticImpact::Document,
        ));
    }
    let instance = document.first_instance();
    if document.instances().count() > 1 {
        losses.push(loss(
            "scene_instances",
            "document.scene.instances".into(),
            "Only the first molecule instance is exported.",
            SemanticImpact::Geometry,
        ));
    }
    if document
        .scene_objects()
        .iter()
        .any(|(_, _, object)| !matches!(object, SceneObject::Instance))
    {
        losses.push(loss(
            "scene_objects",
            "document.scene.objects".into(),
            "Groups and graphics are not represented by MOL/SDF.",
            SemanticImpact::Document,
        ));
    }
    if document.references().len() > 0 {
        losses.push(loss(
            "scene_references",
            "document.scene.references".into(),
            "Scene references are not represented by MOL/SDF.",
            SemanticImpact::Document,
        ));
    }
    let mut indices = BTreeMap::new();
    let mut charges = Vec::new();
    let mut isotopes = Vec::new();
    let mut radicals = Vec::new();
    for (index, (id, atom)) in document.atoms().enumerate() {
        let number = index + 1;
        indices.insert(id, number);
        let symbol = match &atom.kind {
            AtomKind::Element(element) => element.symbol(),
            AtomKind::Query(QueryAtom::Any) => "*",
            AtomKind::Query(QueryAtom::AnyHeavy) => "A",
            AtomKind::Query(QueryAtom::Heteroatom) => "Q",
            AtomKind::Query(_) => {
                return Err(write_problem(
                    format!("Atom {id} is a query not supported by this writer."),
                    "atom.kind",
                ));
            }
        };
        let position = document.position(instance, id).ok_or_else(|| {
            write_problem(
                "The molecule instance is missing an atom position.",
                "position",
            )
        })?;
        let x = coordinate(
            position.x() / POINTS_PER_ANGSTROM,
            &id.to_string(),
            "x",
            &mut losses,
        )?;
        let y = coordinate(
            -position.y() / POINTS_PER_ANGSTROM,
            &id.to_string(),
            "y",
            &mut losses,
        )?;
        let _ = writeln!(
            output,
            "{x}{y}    0.0000 {symbol:<3} 0  0  0  0  0  0  0  0  0  0  0  0"
        );
        if atom.charge != 0 {
            charges.push((number, i32::from(atom.charge)));
        }
        if let Some(mass) = atom.isotope {
            isotopes.push((number, i32::from(mass)));
        }
        match atom.radical {
            Radical::None => (),
            Radical::Singlet => radicals.push((number, 1)),
            Radical::Doublet => radicals.push((number, 2)),
            Radical::Triplet => radicals.push((number, 3)),
            Radical::Unknown => losses.push(loss(
                "unknown_radical",
                format!("document.atoms[{id}].radical"),
                "V2000 has no equivalent unknown radical state.",
                SemanticImpact::Chemistry,
            )),
        }
        if atom.hydrogens != Hydrogens::Inferred {
            losses.push(loss("hydrogen_state", format!("document.atoms[{id}].hydrogens"), "The V2000 query H-count has different semantics from the recorded non-vertex hydrogen state.", SemanticImpact::Chemistry));
        }
        if !atom.attachment_points.is_empty() {
            losses.push(loss(
                "attachment_points",
                format!("document.atoms[{id}].attachment_points"),
                "Attachment point state is not written.",
                SemanticImpact::Chemistry,
            ));
        }
    }
    for (id, bond) in document.bonds() {
        let order = mol_bond_type(&bond.order).ok_or_else(|| {
            write_problem(
                format!("Bond {id} has no supported V2000 bond type."),
                "bond.order",
            )
        })?;
        let begin = indices
            .get(&bond.begin)
            .ok_or_else(|| write_problem("Bond refers to a missing atom.", "bond.begin"))?;
        let end = indices
            .get(&bond.end)
            .ok_or_else(|| write_problem("Bond refers to a missing atom.", "bond.end"))?;
        let _ = writeln!(output, "{begin:>3}{end:>3}{order:>3}  0  0  0  0");
    }
    write_properties(&mut output, "CHG", &charges);
    write_properties(&mut output, "RAD", &radicals);
    write_properties(&mut output, "ISO", &isotopes);
    output.push_str("M  END\n");
    if format == ChemicalFormat::Sdf {
        for field in fields {
            let _ = writeln!(output, "> <{}>", field.name);
            if !field.value.is_empty() {
                let _ = writeln!(output, "{}", field.value);
            }
            output.push('\n');
        }
        output.push_str("$$$$\n");
    }
    Ok(WrittenChemical {
        text: output,
        losses,
    })
}
