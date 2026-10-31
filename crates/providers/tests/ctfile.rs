use chemifree_application::ctfile::{
    CtAtom, CtBond, CtCodec, CtField, CtFormat, CtRadical, CtRecord,
};
use chemifree_application::{LossDisposition, SemanticImpact};
use chemifree_providers::V2000Codec;

// Explicit connection table independent of the writer, based on the public V2000 layout.
const MOL: &str = "Example\nProgram\nComment\n  3  2  0  0  0  0  0  0  0  0999 V2000\n    0.0000    0.0000    0.0000 C   0  3  0  0  0  0  0  0  0  0  0  0\n    1.0000    0.0000    0.0000 O   0  5  0  0  0  0  0  0  0  0  0  0\n    2.0000    1.0000    0.0000 N   0  4  0  0  0  0  0  0  0  0  0  0\n  1  2  1  0  0  0  0\n  2  3  2  0  0  0  0\nM  END\n";

fn atom(element: &str) -> CtAtom {
    CtAtom {
        element: element.into(),
        x: 0.,
        y: 0.,
        z: 0.,
        charge: 0,
        isotope: None,
        radical: CtRadical::None,
    }
}
fn record() -> CtRecord {
    CtRecord {
        title: "Water".into(),
        program: "".into(),
        comment: "".into(),
        atoms: vec![atom("O"), atom("H")],
        bonds: vec![CtBond {
            begin: 0,
            end: 1,
            order: 1,
        }],
        fields: vec![],
    }
}

#[test]
fn independently_authored_table_exposes_atoms_bonds_and_possible_stereo() {
    let result = V2000Codec.read(CtFormat::Mol, MOL.as_bytes()).unwrap();
    let molecule = &result.value[0];
    assert_eq!(
        molecule
            .atoms
            .iter()
            .map(|a| (&*a.element, a.charge, a.radical))
            .collect::<Vec<_>>(),
        [
            ("C", 1, CtRadical::None),
            ("O", -1, CtRadical::None),
            ("N", 0, CtRadical::Doublet)
        ]
    );
    assert_eq!(
        molecule.bonds,
        [
            CtBond {
                begin: 0,
                end: 1,
                order: 1
            },
            CtBond {
                begin: 1,
                end: 2,
                order: 2
            }
        ]
    );
    assert!(
        result.losses.iter().any(
            |l| l.feature == "coordinate_stereo" && l.semantic_impact == SemanticImpact::Stereo
        )
    );
}

#[test]
fn mixed_case_properties_override_all_compact_charge_radical_and_isotope_fields() {
    let text = MOL.replace(
        "M  END",
        "m  cHg  1   2   1\nM  rAd  1   3   0\nm  IsO  1   1  13\n  m  eNd  ",
    );
    let parsed = V2000Codec.read(CtFormat::Mol, text.as_bytes()).unwrap();
    let atoms = &parsed.value[0].atoms;
    assert_eq!(
        (atoms[0].charge, atoms[1].charge, atoms[2].charge),
        (0, 1, 0)
    );
    assert_eq!(
        (atoms[0].radical, atoms[1].radical, atoms[2].radical),
        (CtRadical::None, CtRadical::None, CtRadical::None)
    );
    assert_eq!(
        (atoms[0].isotope, atoms[1].isotope, atoms[2].isotope),
        (Some(13), None, None)
    );
    assert!(parsed.losses.iter().any(|l| l.feature == "isotope_validation" && l.disposition == LossDisposition::Unknown));
}

#[test]
fn unsupported_compact_baseline_is_refused_unless_absolute_isotope_overrides_it() {
    let input = "Ni\n\n\n  1  0  0  0  0  0  0  0  0  0999 V2000\n    0.0000    0.0000    0.0000 Ni  1  0  0  0  0  0  0  0  0  0  0  0\nM  END\n";
    assert_eq!(
        V2000Codec
            .read(CtFormat::Mol, input.as_bytes())
            .unwrap_err()
            .code,
        "uncertain_isotope_baseline"
    );
    let overridden = input.replace("M  END", "M  ISO  1   1  60\nM  END");
    assert_eq!(
        V2000Codec
            .read(CtFormat::Mol, overridden.as_bytes())
            .unwrap()
            .value[0]
            .atoms[0]
            .isotope,
        Some(60)
    );
}

#[test]
fn framing_obeys_header_and_data_context_and_reports_separator_suffix() {
    let mol = MOL.replace("Program\nComment", "$$$$ in program\nM  END");
    let sdf = format!("{mol}>  <Note>\nM  END\n> inside a value\n\n$$$$suffix\n{MOL}$$$$\n");
    let result = V2000Codec.read(CtFormat::Sdf, sdf.as_bytes()).unwrap();
    assert_eq!(result.value.len(), 2);
    assert_eq!(result.value[0].program, "$$$$ in program");
    assert_eq!(
        result.value[0].fields,
        [CtField {
            name: "Note".into(),
            value: "M  END\n> inside a value".into()
        }]
    );
    assert!(
        result
            .losses
            .iter()
            .any(|l| l.feature == "separator_suffix" && l.source_path == "records[1].separator")
    );
}

#[test]
fn field_lengths_are_unicode_characters_and_error_names_later_record() {
    let first = format!("{MOL}$$$$\n");
    let accepted = format!("{first}{MOL}> <Label>\n{}\n\n$$$$\n", "가".repeat(200));
    assert_eq!(
        V2000Codec
            .read(CtFormat::Sdf, accepted.as_bytes())
            .unwrap()
            .value[1]
            .fields[0]
            .value
            .chars()
            .count(),
        200
    );
    let too_long = format!("{first}{MOL}> <Label>\n{}\n\n$$$$\n", "가".repeat(201));
    let err = V2000Codec
        .read(CtFormat::Sdf, too_long.as_bytes())
        .unwrap_err();
    assert_eq!((err.code, err.record), ("data_line_too_long", 2));
    let mut second = record();
    second.fields.push(CtField {
        name: "Label".into(),
        value: "가".repeat(201),
    });
    let err = V2000Codec
        .write(CtFormat::Sdf, &[record(), second])
        .unwrap_err();
    assert_eq!((err.code, err.record), ("data_line_too_long", 2));
}

#[test]
fn invalid_data_and_unknown_chemistry_are_never_silently_erased() {
    for (text, code) in [
        (
            format!("{MOL}> (123) DT12\nvalue\n\n$$$$\n"),
            "unsupported_dt_field",
        ),
        (
            format!("{MOL}> <_invalid>\nx\n\n$$$$\n"),
            "invalid_data_name",
        ),
        (format!("{MOL}> <Note>\n$$$$tail\n"), "unterminated_data"),
        (
            MOL.replace("M  END", "m  aLs  1 C\nM  END"),
            "unsupported_directive",
        ),
    ] {
        let format = if code == "unsupported_directive" {
            CtFormat::Mol
        } else {
            CtFormat::Sdf
        };
        assert_eq!(
            V2000Codec.read(format, text.as_bytes()).unwrap_err().code,
            code,
            "{text}"
        );
    }
}

#[test]
fn malformed_utf8_in_second_record_is_reported_at_its_boundary() {
    let mut input = format!("{MOL}$$$$\n{MOL}> <Name>\n").into_bytes();
    input.extend_from_slice(b"\xC3\x28\n\n$$$$\n");
    let error = V2000Codec.read(CtFormat::Sdf, &input).unwrap_err();
    assert_eq!((error.code, error.record), ("invalid_utf8", 2));
}

#[test]
fn writer_rejects_bad_later_records_without_returning_partial_bytes() {
    let mut invalid = record();
    invalid.bonds[0].end = 5;
    let error = V2000Codec
        .write(CtFormat::Sdf, &[record(), invalid])
        .unwrap_err();
    assert_eq!((error.code, error.record), ("invalid_bond", 2));
}
