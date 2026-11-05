//! Stored configurations are emitted relative to the actual traversal, including ring digits.

use chemifree_application::smiles::{SmilesCodec, SmilesSlot, SmilesStyle};
use chemifree_application::{LossConsent, LossDisposition, SemanticImpact};
use chemifree_domain::document::{
    AtomId, AtomRef, Command, Document, DocumentId, NewAtomPositions, Point,
};
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::stereo::{AtomConfiguration, SlotRef, Turn};
use chemifree_domain::stereo_group::GroupKind;
use chemifree_providers::OpenSmilesCodec;

fn atom(index: usize) -> AtomId {
    AtomId::from_serial(u32::try_from(index + 1).unwrap()).unwrap()
}

/// `numbering` lists input indexes in their new document order.
fn drawing(text: &str, numbering: &[usize]) -> Document {
    let read = OpenSmilesCodec.read(text).unwrap();
    let mut numbering = numbering.to_vec();
    if numbering.is_empty() {
        numbering.extend(0..read.atoms.len());
    }
    let mut mapped = vec![0; numbering.len()];
    for (new, &old) in numbering.iter().enumerate() {
        mapped[old] = new;
    }
    let reference = |old: usize| AtomRef::Id(atom(mapped[old]));
    let mut document = Document::new(DocumentId::parse("stereo-write").unwrap(), "Stereo").unwrap();
    let mut commands: Vec<Command> = numbering
        .iter()
        .map(|&old| Command::AddAtom {
            atom: read.atoms[old].atom.clone(),
            positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
            bond_to: None,
        })
        .collect();
    commands.extend(read.bonds.iter().map(|bond| Command::AddBond {
        begin: reference(bond.begin),
        end: reference(bond.end),
        order: bond.order.clone(),
    }));
    commands.extend(
        read.stereo
            .centers
            .iter()
            .map(|center| Command::SetAtomConfiguration {
                atom: reference(center.atom),
                configuration: Some(AtomConfiguration::Tetrahedral {
                    neighbors: center.slots.map(|slot| match slot {
                        SmilesSlot::Atom(old) => SlotRef::Atom(reference(old)),
                        SmilesSlot::ImplicitHydrogen => SlotRef::ImplicitHydrogen,
                    }),
                    turn: center.turn,
                }),
            }),
    );
    commands.extend(
        read.stereo
            .groups
            .iter()
            .map(|group| Command::CreateStereoGroup {
                kind: group.kind,
                number: group.number,
                atoms: group.atoms.iter().map(|&old| reference(old)).collect(),
            }),
    );
    document.apply(&commands).unwrap();
    document
}

fn write(document: &Document) -> chemifree_application::smiles::SmilesWrite {
    OpenSmilesCodec
        .write_with_groups(
            &MoleculeGraph::from_document(document),
            SmilesStyle::Recorded,
            document.stereo_groups(),
        )
        .unwrap()
}

#[test]
fn bracket_hydrogens_and_ring_digits_follow_the_emitted_order() {
    for text in [
        "N[C@H](O)C",
        "[C@@H](F)(Cl)Br",
        "F[C@](Cl)(Br)I",
        "[H][C@@](F)(Cl)Br",
        "[C@H]1(F)CCCC1Cl",
        "C1CCC[C@@H]1F",
        "C[C@H](F)C[C@@H](O)Cl",
    ] {
        let written = write(&drawing(text, &[]));
        assert_eq!(written.text, text, "{text}");
        assert!(
            !written
                .losses
                .iter()
                .any(|loss| loss.consent == LossConsent::Required),
            "{text}: {:?}",
            written.losses
        );
    }
}

#[test]
fn an_odd_neighbor_permutation_flips_the_written_mark() {
    let document = drawing("F[C@](Cl)(Br)I", &[0, 1, 3, 2, 4]);
    let written = write(&document);
    assert_eq!(written.text, "F[C@@](Br)(Cl)I");
    assert_eq!(written.atom_order, (0..5).map(atom).collect::<Vec<_>>());
}

#[test]
fn cx_group_members_follow_output_positions_after_a_branch_reordering() {
    let document = drawing("F[C@](Cl)(Br)I |&7:1|", &[2, 0, 1, 3, 4]);
    let written = write(&document);
    // Start at Cl (old 2), then center (old 1), F (old 0), Br and I.
    assert_eq!(written.text, "Cl[C@@](F)(Br)I |&7:1|");
    let again = OpenSmilesCodec.read(&written.text).unwrap();
    assert_eq!(again.stereo.groups[0].atoms, [1]);
    assert_eq!(again.stereo.groups[0].number, Some(7));
}

#[test]
fn all_supported_group_kinds_are_written_and_ungrouped_centers_stay_ungrouped() {
    let text = "F[C@H](Cl)Br.F[C@@H](Cl)Br.F[C@H](Cl)Br.F[C@@H](Cl)Br |a:1,&0:5,o9:9|";
    let written = write(&drawing(text, &[]));
    assert_eq!(written.text, text);
    let read = OpenSmilesCodec.read(&written.text).unwrap();
    assert_eq!(read.stereo.centers.len(), 4);
    assert_eq!(read.stereo.groups.len(), 3);
    let identity = written
        .losses
        .iter()
        .find(|loss| loss.feature == "stereo_group_identity")
        .unwrap();
    assert_eq!(identity.source_path, "groups[e1, e2, e3]");
    assert_eq!(identity.consent, LossConsent::NotRequired);
    assert_eq!(identity.disposition, LossDisposition::Dropped);
    assert_eq!(identity.semantic_impact, SemanticImpact::Stereo);
    assert_eq!(
        written
            .losses
            .iter()
            .filter(|loss| loss.feature == "stereo_group_identity")
            .count(),
        1
    );
}

#[test]
fn negative_group_numbers_are_renumbered_and_missing_numbers_normalize_to_zero() {
    let mut document = drawing("F[C@H](Cl)Br.F[C@@H](Cl)Br", &[]);
    document
        .apply(&[
            Command::CreateStereoGroup {
                kind: GroupKind::And,
                number: Some(-2),
                atoms: vec![AtomRef::Id(atom(1))],
            },
            Command::CreateStereoGroup {
                kind: GroupKind::Or,
                number: None,
                atoms: vec![AtomRef::Id(atom(5))],
            },
        ])
        .unwrap();
    let written = write(&document);
    assert_eq!(written.text, "F[C@H](Cl)Br.F[C@@H](Cl)Br |&1:1,o0:5|");
    assert!(
        !written
            .losses
            .iter()
            .any(|loss| loss.consent == LossConsent::Required)
    );
    let normalized = written
        .losses
        .iter()
        .find(|loss| loss.feature == "stereo_group_number" && loss.source_path == "groups[e2]")
        .unwrap();
    assert_eq!(normalized.source_path, "groups[e2]");
    assert_eq!(normalized.consent, LossConsent::NotRequired);
}

#[test]
fn an_unknown_center_is_dropped_with_required_consent() {
    let mut document = drawing("F[C@H](Cl)Br", &[]);
    document
        .apply(&[Command::SetAtomConfiguration {
            atom: AtomRef::Id(atom(1)),
            configuration: Some(AtomConfiguration::Unknown {
                neighbors: [
                    SlotRef::Atom(AtomRef::Id(atom(0))),
                    SlotRef::ImplicitHydrogen,
                    SlotRef::Atom(AtomRef::Id(atom(2))),
                    SlotRef::Atom(AtomRef::Id(atom(3))),
                ],
            }),
        }])
        .unwrap();
    let written = write(&document);
    assert_eq!(written.text, "FC(Cl)Br");
    let loss = written
        .losses
        .iter()
        .find(|loss| loss.feature == "stereo_configuration")
        .unwrap();
    assert_eq!(loss.source_path, "atoms[a2]");
    assert_eq!(loss.consent, LossConsent::Required);
    assert_eq!(
        OpenSmilesCodec
            .read(&written.text)
            .unwrap()
            .stereo
            .centers
            .len(),
        0
    );
}

#[test]
fn renumbered_groups_reserve_existing_numbers_without_merging_memberships() {
    let mut document = drawing("F[C@H](Cl)Br.F[C@@H](Cl)Br", &[]);
    document
        .apply(&[
            Command::CreateStereoGroup {
                kind: GroupKind::And,
                number: Some(-1),
                atoms: vec![AtomRef::Id(atom(1))],
            },
            Command::CreateStereoGroup {
                kind: GroupKind::And,
                number: Some(1),
                atoms: vec![AtomRef::Id(atom(5))],
            },
        ])
        .unwrap();
    let written = write(&document);
    assert_eq!(written.text, "F[C@H](Cl)Br.F[C@@H](Cl)Br |&2:1,&1:5|");
    let groups = OpenSmilesCodec.read(&written.text).unwrap().stereo.groups;
    assert_eq!(groups.len(), 2);
    assert_eq!((groups[0].number, &groups[0].atoms), (Some(2), &vec![1]));
    assert_eq!((groups[1].number, &groups[1].atoms), (Some(1), &vec![5]));
}

#[test]
fn stereo_configuration_commands_count_toward_the_readback_bound() {
    let mut document = drawing(&format!("{}C[C@H](F)Cl", "C[C@H](F)".repeat(70)), &[]);
    let carbon = OpenSmilesCodec.read("C").unwrap().atoms[0].atom.clone();
    document
        .apply(
            &(0..3)
                .map(|_| Command::AddAtom {
                    atom: carbon.clone(),
                    positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
                    bond_to: None,
                })
                .collect::<Vec<_>>(),
        )
        .unwrap();
    let error = OpenSmilesCodec
        .write_with_groups(
            &MoleculeGraph::from_document(&document),
            SmilesStyle::Recorded,
            document.stereo_groups(),
        )
        .unwrap_err();
    assert_eq!(
        error.code,
        chemifree_application::smiles::SmilesCode::CommandLimit
    );
}

#[test]
fn a_group_with_a_lone_pair_member_is_dropped_whole_while_supported_centers_survive() {
    for kind in [GroupKind::Absolute, GroupKind::And, GroupKind::Or] {
        // N has three single bonds, no hydrogen, and a stored tetrahedral lone-pair slot.
        // The disconnected carbon has a representable bracket-hydrogen configuration.
        let mut document = drawing("FN(Cl)Br.F[C@H](Cl)Br", &[]);
        document
            .apply(&[
                Command::SetAtomConfiguration {
                    atom: AtomRef::Id(atom(1)),
                    configuration: Some(AtomConfiguration::Tetrahedral {
                        neighbors: [
                            SlotRef::Atom(AtomRef::Id(atom(0))),
                            SlotRef::Atom(AtomRef::Id(atom(2))),
                            SlotRef::Atom(AtomRef::Id(atom(3))),
                            SlotRef::LonePair,
                        ],
                        turn: Turn::Clockwise,
                    }),
                },
                Command::CreateStereoGroup {
                    kind,
                    number: if kind == GroupKind::Absolute {
                        None
                    } else {
                        Some(3)
                    },
                    atoms: vec![AtomRef::Id(atom(1)), AtomRef::Id(atom(5))],
                },
            ])
            .unwrap();
        let written = write(&document);
        assert_eq!(written.text, "FN(Cl)Br.F[C@H](Cl)Br", "{kind:?}");
        let read = OpenSmilesCodec.read(&written.text).unwrap();
        assert_eq!(
            read.stereo.groups,
            [],
            "{kind:?}: the surviving member must not become a singleton group"
        );
        assert_eq!(read.stereo.centers.len(), 1, "{kind:?}");
        assert_eq!(read.stereo.centers[0].atom, 5, "{kind:?}");
        assert_eq!(read.stereo.centers[0].turn, Turn::Anticlockwise, "{kind:?}");
        assert_eq!(
            read.stereo.centers[0].slots,
            [
                SmilesSlot::Atom(4),
                SmilesSlot::ImplicitHydrogen,
                SmilesSlot::Atom(6),
                SmilesSlot::Atom(7)
            ],
            "{kind:?}"
        );
        let stereo_losses: Vec<_> = written
            .losses
            .iter()
            .filter(|loss| loss.semantic_impact == SemanticImpact::Stereo)
            .map(|loss| {
                (
                    loss.feature.as_str(),
                    loss.source_path.as_str(),
                    loss.consent,
                    loss.disposition,
                )
            })
            .collect();
        assert_eq!(
            stereo_losses,
            [
                (
                    "stereo_configuration",
                    "atoms[a2]",
                    LossConsent::Required,
                    LossDisposition::Dropped
                ),
                (
                    "stereo_group",
                    "groups[e1]",
                    LossConsent::Required,
                    LossDisposition::Dropped
                ),
            ],
            "{kind:?}"
        );
    }
}
