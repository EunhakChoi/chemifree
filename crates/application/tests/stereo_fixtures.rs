//! The shared stereo drawing fixtures: every fixture's hand-stated configuration must equal what
//! `chemifree_domain::stereo_drawing` derives from its drawing. The same file is checked against
//! RDKit by `scripts/verify-stereo-rdkit.py` (manual, not part of `npm run check`).

use std::collections::{BTreeMap, BTreeSet};

use chemifree_domain::document::{Atom, AtomId, Bond, BondId, BondOrder, Point};
use chemifree_domain::element::Element;
use chemifree_domain::stereo::{
    BondEnd, CenterState, DoubleBondState, Relation, Slot, StereoMark, Turn,
};
use chemifree_domain::stereo_drawing::{DeriveError, Drawing};
use serde_json::Value;

fn fixtures() -> Vec<Value> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/stereo-fixtures.json"
    );
    let document: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(document["schema"], 1);
    assert_eq!(document["rdkit_version"], "2022.03.2");
    document["fixtures"].as_array().unwrap().clone()
}

fn atom_id(index: u64) -> AtomId {
    AtomId::from_serial(u32::try_from(index).unwrap() + 1).unwrap()
}

fn index_of(value: &Value) -> u64 {
    value.as_u64().expect("an atom index")
}

fn slot_of(value: &Value) -> Slot {
    match value.as_str() {
        Some("H") => Slot::ImplicitHydrogen,
        Some("LP") => Slot::LonePair,
        Some(other) => panic!("unknown slot {other}"),
        None => Slot::Atom(atom_id(index_of(value))),
    }
}

fn mark_of(text: &str) -> Option<StereoMark> {
    let end = if text.ends_with('<') {
        BondEnd::End
    } else {
        BondEnd::Begin
    };
    match text.trim_end_matches('<') {
        "" => None,
        "W" => Some(StereoMark::Wedge { narrow: end }),
        "H" => Some(StereoMark::Hash { narrow: end }),
        "w" => Some(StereoMark::Wavy),
        "x" => Some(StereoMark::Crossed),
        other => panic!("unknown mark {other}"),
    }
}

struct Drawn {
    atoms: BTreeMap<AtomId, Atom>,
    bonds: BTreeMap<BondId, Bond>,
    positions: Vec<(AtomId, Point)>,
}

fn drawn(fixture: &Value) -> Drawn {
    let mut atoms = BTreeMap::new();
    let mut positions = Vec::new();
    for (index, spec) in fixture["atoms"].as_array().unwrap().iter().enumerate() {
        let spec = spec.as_array().unwrap();
        let id = atom_id(index as u64);
        atoms.insert(
            id,
            Atom::from(Element::from_symbol(spec[0].as_str().unwrap()).unwrap()),
        );
        positions.push((
            id,
            Point::new(spec[1].as_f64().unwrap(), spec[2].as_f64().unwrap()).unwrap(),
        ));
    }
    let mut bonds = BTreeMap::new();
    for (index, spec) in fixture["bonds"].as_array().unwrap().iter().enumerate() {
        let order = match spec[2].as_str().unwrap() {
            "-" => BondOrder::Single,
            "=" => BondOrder::Double,
            other => panic!("unknown bond kind {other}"),
        };
        let mut bond = Bond::new(
            atom_id(index_of(&spec[0])),
            atom_id(index_of(&spec[1])),
            order,
        );
        bond.mark = mark_of(spec[3].as_str().unwrap());
        bonds.insert(
            BondId::from_serial(u32::try_from(index).unwrap() + 1).unwrap(),
            bond,
        );
    }
    Drawn {
        atoms,
        bonds,
        positions,
    }
}

fn turn_of(text: &str) -> Option<Turn> {
    match text {
        "clockwise" => Some(Turn::Clockwise),
        "anticlockwise" => Some(Turn::Anticlockwise),
        "unknown" => None,
        other => panic!("unknown turn {other}"),
    }
}

#[test]
fn every_fixture_derives_exactly_the_hand_stated_configuration() {
    let fixtures = fixtures();
    let names: BTreeSet<&str> = fixtures
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert_eq!(names.len(), fixtures.len(), "fixture names are unique");
    let mut checked = 0;
    for fixture in &fixtures {
        let name = fixture["name"].as_str().unwrap();
        let drawn = drawn(fixture);
        let drawing = Drawing::new(&drawn.atoms, &drawn.bonds, drawn.positions.iter().copied());
        if let Some(refused) = fixture.get("refused") {
            let reason = match drawing.derive_center(atom_id(index_of(&refused["atom"]))) {
                Err(DeriveError::NotDerivable(reason)) => reason.code(),
                other => panic!("{name}: expected a refusal, got {other:?}"),
            };
            assert_eq!(reason, refused["reason"].as_str().unwrap(), "{name}");
            checked += 1;
            continue;
        }
        let expect = &fixture["expect"];
        for center in expect["centers"].as_array().into_iter().flatten() {
            let slots: Vec<Slot> = center["order"]
                .as_array()
                .unwrap()
                .iter()
                .map(slot_of)
                .collect();
            let slots: [Slot; 4] = slots.try_into().expect("four slots");
            let expected = match turn_of(center["turn"].as_str().unwrap()) {
                Some(turn) => CenterState::tetrahedral(slots, turn).unwrap(),
                None => CenterState::unknown(slots).unwrap(),
            };
            let derived = drawing.derive_center(atom_id(index_of(&center["atom"])));
            assert_eq!(derived, Ok(expected), "{name}");
            checked += 1;
        }
        for bond in expect["double_bonds"].as_array().into_iter().flatten() {
            let refs = bond["refs"].as_array().unwrap();
            let expected = match bond["relation"].as_str().unwrap() {
                "unknown" => DoubleBondState::Unknown,
                relation => DoubleBondState::Relative {
                    begin_ref: Slot::Atom(atom_id(index_of(&refs[0]))),
                    end_ref: Slot::Atom(atom_id(index_of(&refs[1]))),
                    relation: match relation {
                        "same" => Relation::SameSide,
                        "opposite" => Relation::OppositeSides,
                        other => panic!("unknown relation {other}"),
                    },
                },
            };
            let id =
                BondId::from_serial(u32::try_from(index_of(&bond["bond"])).unwrap() + 1).unwrap();
            assert_eq!(drawing.derive_double_bond(id), Ok(expected), "{name}");
            checked += 1;
        }
    }
    assert!(
        checked >= fixtures.len(),
        "every fixture states something: {checked}"
    );
}
