use chemifree_application::{
    ChemicalCodec, ChemicalFormat, DataField, LossDisposition, SemanticImpact,
};
use chemifree_domain::document::{
    Atom, AtomRef, BondOrder, Command, Document, DocumentId, NewAtomPositions, Point,
};
use chemifree_domain::element::Element;
use chemifree_domain::states::{Hydrogens, Radical};
use chemifree_providers::MolfileCodec;

const ACETATE: &str = include_str!("data/molfile/sodium-acetate.mol");

#[test]
fn reads_a_fixed_width_independent_graph_and_charges() {
    let records = MolfileCodec.read(ChemicalFormat::Mol, ACETATE).unwrap();
    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert_eq!(record.title, "Sodium acetate");
    assert_eq!(
        record
            .atoms
            .iter()
            .map(|a| a.atom.kind.element().unwrap().symbol())
            .collect::<Vec<_>>(),
        ["C", "C", "O", "O", "Na"]
    );
    assert_eq!(
        record
            .atoms
            .iter()
            .map(|a| a.atom.charge)
            .collect::<Vec<_>>(),
        [0, 0, 0, -1, 1]
    );
    assert_eq!(
        record
            .bonds
            .iter()
            .map(|b| (b.begin, b.end, &b.order))
            .collect::<Vec<_>>(),
        [
            (0, 1, &BondOrder::Single),
            (1, 2, &BondOrder::Double),
            (1, 3, &BondOrder::Single)
        ]
    );
    assert_eq!(
        (record.atoms[0].position.x(), record.atoms[0].position.y()),
        (-24.0, 0.0)
    );
    assert_eq!(
        (record.atoms[2].position.x(), record.atoms[2].position.y()),
        (12.0, -20.784)
    );
    assert!(record.losses.is_empty());
}

#[test]
fn property_blocks_override_all_compact_charge_and_radical_values() {
    let input = ACETATE.replace("C   0  0", "C   0  3").replace(
        "M  CHG  2   4  -1   5   1",
        "M  RAD  1   2   2\nM  CHG  1   4   4\nM  ISO  1   1  13",
    );
    let record = MolfileCodec
        .read(ChemicalFormat::Mol, &input)
        .unwrap()
        .remove(0);
    assert_eq!(
        record
            .atoms
            .iter()
            .map(|a| a.atom.charge)
            .collect::<Vec<_>>(),
        [0, 0, 0, 4, 0]
    );
    assert_eq!(record.atoms[1].atom.radical, Radical::Doublet);
    assert_eq!(record.atoms[0].atom.isotope, Some(13));
}

#[test]
fn reports_unrepresented_stereo_and_three_dimensional_geometry() {
    let input = ACETATE
        .replacen(
            "   -1.5000    0.0000    0.0000 C   0  0  0",
            "   -1.5000    0.0000    1.0000 C   0  0  1",
            1,
        )
        .replacen("  1  2  1  0", "  1  2  1  6", 1);
    let record = MolfileCodec
        .read(ChemicalFormat::Mol, &input)
        .unwrap()
        .remove(0);
    let losses = record
        .losses
        .iter()
        .map(|loss| {
            (
                loss.feature.as_str(),
                loss.disposition,
                loss.semantic_impact,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        losses,
        [
            (
                "z_coordinate",
                LossDisposition::Dropped,
                SemanticImpact::Geometry
            ),
            (
                "atom_stereo_parity",
                LossDisposition::Dropped,
                SemanticImpact::Stereo
            ),
            (
                "bond_stereo",
                LossDisposition::Dropped,
                SemanticImpact::Stereo
            )
        ]
    );
}

#[test]
fn reads_multiple_sdf_records_and_ordered_duplicate_fields() {
    let first = format!("{ACETATE}> <NAME>\nline one\nline two\n\n> <NAME>\n\n$$$$\n");
    let input = format!("{first}{ACETATE}$$$$\n");
    let records = MolfileCodec.read(ChemicalFormat::Sdf, &input).unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(
        records[0].fields,
        [
            DataField {
                name: "NAME".into(),
                value: "line one\nline two".into()
            },
            DataField {
                name: "NAME".into(),
                value: String::new()
            }
        ]
    );
    assert_eq!(records[0].losses[0].feature, "sdf_data_fields");
    assert_eq!(records[1].fields, []);
}

#[test]
fn refuses_invalid_and_truncated_input_with_field_location() {
    let cases = [
        ("", "truncated_record", "title"),
        ("name\n\n\n", "truncated_record", "counts"),
        (
            "name\n\n\n  1  0  0  0  0  0  0  0  0  0999 V3000\n",
            "unsupported_format",
            "version",
        ),
        (
            "name\n\n\n  1  0  0  0  0  0  0  0  0  0999 V2000\nshort\n",
            "truncated_field",
            "atom",
        ),
    ];
    for (input, code, field) in cases {
        let error = MolfileCodec.read(ChemicalFormat::Mol, input).unwrap_err();
        assert_eq!(
            (error.code, error.field.as_deref(), error.record),
            (code, Some(field), Some(1)),
            "{input:?}"
        );
        assert!(error.line.is_some());
    }
    let invalid = ACETATE.replacen("  1  2  1  0", "  1  9  1  0", 1);
    let error = MolfileCodec
        .read(ChemicalFormat::Mol, &invalid)
        .unwrap_err();
    assert_eq!(
        (error.code, error.line, error.field.as_deref()),
        ("invalid_bond", Some(10), Some("endpoints"))
    );
    let duplicate = ACETATE.replace("M  END", "M  CHG  1   4   1\nM  END");
    let error = MolfileCodec
        .read(ChemicalFormat::Mol, &duplicate)
        .unwrap_err();
    assert_eq!(error.code, "duplicate_property");
    let no_delimiter = format!("{ACETATE}> <name>\nvalue\n\n");
    assert_eq!(
        MolfileCodec
            .read(ChemicalFormat::Sdf, &no_delimiter)
            .unwrap_err()
            .field
            .as_deref(),
        Some("delimiter")
    );
    let no_blank = format!("{ACETATE}> <name>\nvalue\n$$$$\n");
    assert_eq!(
        MolfileCodec
            .read(ChemicalFormat::Sdf, &no_blank)
            .unwrap_err()
            .field
            .as_deref(),
        Some("data_value")
    );
}

fn document() -> Document {
    let id = DocumentId::parse("mol-test").unwrap();
    let mut document = Document::new(id, "Test").unwrap();
    let mut carbon = Atom::from(Element::from_symbol("C").unwrap());
    carbon.isotope = Some(13);
    carbon.charge = 4;
    carbon.radical = Radical::Triplet;
    carbon.hydrogens = Hydrogens::Specified(2);
    let oxygen = Atom::from(Element::from_symbol("O").unwrap());
    document
        .apply(&[
            Command::AddAtom {
                atom: carbon,
                positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
                bond_to: None,
            },
            Command::AddAtom {
                atom: oxygen,
                positions: NewAtomPositions::Single(Point::new(24.0, -16.0).unwrap()),
                bond_to: None,
            },
            Command::AddBond {
                begin: AtomRef::Created(0),
                end: AtomRef::Created(1),
                order: BondOrder::Double,
            },
        ])
        .unwrap();
    document
}

#[test]
fn writes_fixed_width_fields_and_reports_unrepresentable_hydrogen_state() {
    let result = MolfileCodec
        .write(
            ChemicalFormat::Sdf,
            &document(),
            &[DataField {
                name: "NAME".into(),
                value: "two\nlines".into(),
            }],
        )
        .unwrap();
    assert!(
        result.text.contains("M  CHG  1   1   4\n"),
        "{}",
        result.text
    );
    assert!(
        result.text.contains("M  ISO  1   1  13\n"),
        "{}",
        result.text
    );
    assert!(
        result.text.contains("M  RAD  1   1   3\n"),
        "{}",
        result.text
    );
    assert!(result.text.ends_with("> <NAME>\ntwo\nlines\n\n$$$$\n"));
    assert!(
        result
            .losses
            .iter()
            .any(|loss| loss.feature == "hydrogen_state"
                && loss.semantic_impact == SemanticImpact::Chemistry)
    );
}

#[test]
fn rejects_output_that_cannot_be_encoded_without_corruption() {
    let mut doc = document();
    doc.apply(&[Command::UpdateBond {
        bond: chemifree_domain::document::BondId::from_serial(1).unwrap(),
        order: BondOrder::Dative,
    }])
    .unwrap();
    let error = MolfileCodec
        .write(ChemicalFormat::Mol, &doc, &[])
        .unwrap_err();
    assert_eq!(
        (error.code, error.field.as_deref()),
        ("unsupported_feature", Some("bond.order"))
    );
    let invalid = DataField {
        name: "NAME".into(),
        value: "line\n\nother".into(),
    };
    let error = MolfileCodec
        .write(ChemicalFormat::Sdf, &doc, &[invalid])
        .unwrap_err();
    assert_eq!(
        (error.code, error.field.as_deref()),
        ("unsupported_feature", Some("fields.value"))
    );
}

#[test]
fn compact_isotope_mass_is_interpreted_and_property_block_supersedes_it() {
    let compact = ACETATE.replacen("C   0  0", "C   1  0", 1);
    let record = MolfileCodec
        .read(ChemicalFormat::Mol, &compact)
        .unwrap()
        .remove(0);
    assert_eq!(record.atoms[0].atom.isotope, Some(13));
    let overridden = compact.replace("M  END", "M  ISO  1   2  14\nM  END");
    let record = MolfileCodec
        .read(ChemicalFormat::Mol, &overridden)
        .unwrap()
        .remove(0);
    assert_eq!(record.atoms[0].atom.isotope, None);
    assert_eq!(record.atoms[1].atom.isotope, Some(14));
}

#[test]
fn invalid_property_reports_the_offending_property_line() {
    let input = ACETATE.replace("M  CHG  2   4  -1   5   1", "M  CHG  1   4  15");
    let error = MolfileCodec.read(ChemicalFormat::Mol, &input).unwrap_err();
    assert_eq!(
        (error.code, error.line, error.field.as_deref()),
        ("unsupported_charge", Some(13), Some("charge"))
    );
}

#[test]
fn unknown_properties_and_query_hydrogen_have_explicit_losses() {
    let input = ACETATE
        .replacen("C   0  0  0  0", "C   0  0  0  2", 1)
        .replace("M  END", "M  XYZ  1   1   5\nM  END");
    let record = MolfileCodec
        .read(ChemicalFormat::Mol, &input)
        .unwrap()
        .remove(0);
    assert_eq!(
        record
            .losses
            .iter()
            .map(|loss| loss.feature.as_str())
            .collect::<Vec<_>>(),
        ["query_hydrogen_count", "unknown_property"]
    );
    assert_eq!(record.atoms[0].atom.hydrogens, Hydrogens::Inferred);
}

#[test]
fn unsupported_continuation_and_dangling_properties_are_refused() {
    let alias = ACETATE.replace("M  END", "A    1\nAc\nM  END");
    let error = MolfileCodec.read(ChemicalFormat::Mol, &alias).unwrap_err();
    assert_eq!(
        (error.code, error.field.as_deref()),
        ("unsupported_feature", Some("property"))
    );
    let dangling = ACETATE.replace("M  END", "M  ISO  1   6  13\nM  END");
    let error = MolfileCodec
        .read(ChemicalFormat::Mol, &dangling)
        .unwrap_err();
    assert_eq!(
        (error.code, error.field.as_deref()),
        ("invalid_property", Some("property_atom"))
    );
}

#[test]
fn incomplete_atom_bond_and_property_fields_are_not_padded() {
    let cases = [
        ACETATE.replacen("C   0  0  0", "C   0  0  ", 1),
        ACETATE.replacen("  1  2  1  0  0  0  0", "  1  2  1  ", 1),
        ACETATE.replacen("M  CHG  2   4  -1   5   1", "M  CHG  2   4  -1   5", 1),
    ];
    for input in cases {
        assert_eq!(
            MolfileCodec
                .read(ChemicalFormat::Mol, &input)
                .unwrap_err()
                .code,
            "truncated_field"
        );
    }
}

#[test]
fn compact_isotope_baselines_outside_hydrogen_through_calcium_are_not_guessed() {
    for symbol in ["Ni", "Br"] {
        let input = ACETATE.replacen("C   0  0", &format!("{symbol:<3} 1  0"), 1);
        let error = MolfileCodec.read(ChemicalFormat::Mol, &input).unwrap_err();
        assert_eq!(
            (error.code, error.line, error.field.as_deref()),
            (
                "unsupported_isotope_reference",
                Some(5),
                Some("mass_difference")
            ),
            "{symbol}"
        );
        let explicit = input.replace("M  END", "M  ISO  1   1  81\nM  END");
        let records = MolfileCodec.read(ChemicalFormat::Mol, &explicit).unwrap();
        assert_eq!(
            records[0].atoms[0].atom.isotope,
            Some(81),
            "M ISO takes precedence for {symbol}"
        );
    }
}

#[test]
fn radical_property_alone_supersedes_compact_charge_on_all_atoms() {
    let input = ACETATE
        .replacen("C   0  0", "C   0  3", 1)
        .replace("M  CHG  2   4  -1   5   1", "M  RAD  1   2   2");
    let record = MolfileCodec
        .read(ChemicalFormat::Mol, &input)
        .unwrap()
        .remove(0);
    assert_eq!(record.atoms[0].atom.charge, 0);
    assert_eq!(record.atoms[1].atom.radical, Radical::Doublet);
}

#[test]
fn invalid_atoms_and_bond_orders_have_specific_diagnostics() {
    let cases = [
        (
            ACETATE.replacen(" C   0", " Xx  0", 1),
            "unsupported_atom",
            "symbol",
        ),
        (
            ACETATE.replacen("   -1.5000", "       NaN", 1),
            "invalid_field",
            "x",
        ),
        (
            ACETATE.replacen("  1  2  1  0", "  1  2  9  0", 1),
            "invalid_bond",
            "bond_type",
        ),
        (
            ACETATE.replacen("  1  2  1  0", "  1  1  1  0", 1),
            "invalid_bond",
            "endpoints",
        ),
    ];
    for (input, code, field) in cases {
        let error = MolfileCodec.read(ChemicalFormat::Mol, &input).unwrap_err();
        assert_eq!((error.code, error.field.as_deref()), (code, Some(field)));
    }
}

#[test]
fn output_reports_coordinate_rounding() {
    let mut doc = document();
    doc.apply(&[Command::MoveAtoms {
        atoms: vec![chemifree_domain::document::AtomId::from_serial(1).unwrap()],
        delta: chemifree_domain::document::Vector {
            dx: 0.00001,
            dy: 0.0,
        },
        instance: None,
    }])
    .unwrap();
    let result = MolfileCodec.write(ChemicalFormat::Mol, &doc, &[]).unwrap();
    assert!(
        result
            .losses
            .iter()
            .any(|loss| loss.feature == "coordinate_precision"
                && loss.disposition == LossDisposition::Normalized)
    );
}
