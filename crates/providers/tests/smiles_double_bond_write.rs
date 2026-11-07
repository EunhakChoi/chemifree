//! The recorded SMILES writer's directional bonds `/` and `\` (P29c2b). Expected texts are
//! derived by hand from the OpenSMILES rule (a mark is relative to the atom written before it),
//! and the sides of the conjugated cases are worked through in the comments, not taken from the
//! writer. Atoms are numbered from zero in text order; bond `bN` is the N-th bond of the text.

use chemifree_application::smiles::{SmilesCodec, SmilesSlot, SmilesStyle, SmilesWrite};
use chemifree_application::{LossConsent, LossDisposition, SemanticImpact};
use chemifree_domain::document::{
    AtomId, AtomRef, BondOrder, BondRef, Command, Document, DocumentId, NewAtomPositions, Point,
};
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::stereo::{AtomConfiguration, BondConfiguration, Relation, SlotRef};
use chemifree_providers::OpenSmilesCodec;

use Relation::{OppositeSides as Opposite, SameSide as Same};

fn atom(index: usize) -> AtomId {
    AtomId::from_serial(u32::try_from(index + 1).unwrap()).unwrap()
}

/// A configuration a test states by hand: the neighbour of each end (text atom numbers, `None`
/// for an implicit hydrogen) and the relation, or unknown.
#[derive(Clone, Copy)]
enum Stated {
    Unknown,
    Relative(Option<usize>, Option<usize>, Relation),
}

/// The drawing of `text` (read without the stereo it states when `with_marks` is false), with
/// the bonds added in `bond_order` (text bond numbers; every bond, once) and `stated` set on
/// top. Bond ids follow the order the bonds were added in.
fn build(
    text: &str,
    with_marks: bool,
    bond_order: Option<&[usize]>,
    stated: &[(usize, Stated)],
) -> Document {
    let read = OpenSmilesCodec.read(text).unwrap();
    let atoms = read.atoms.len();
    let order: Vec<usize> =
        bond_order.map_or_else(|| (0..read.bonds.len()).collect(), <[_]>::to_vec);
    assert_eq!(
        order.len(),
        read.bonds.len(),
        "{text}: every bond is added once"
    );
    // The command that adds text bond `k`.
    let created = |k: usize| BondRef::Created(atoms + order.iter().position(|&o| o == k).unwrap());
    let mut document = Document::new(DocumentId::parse("write-marks").unwrap(), "Marks").unwrap();
    let mut commands: Vec<Command> = read
        .atoms
        .iter()
        .map(|entry| Command::AddAtom {
            atom: entry.atom.clone(),
            positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
            bond_to: None,
        })
        .collect();
    commands.extend(order.iter().map(|&k| Command::AddBond {
        begin: AtomRef::Id(atom(read.bonds[k].begin)),
        end: AtomRef::Id(atom(read.bonds[k].end)),
        order: read.bonds[k].order.clone(),
    }));
    let slot = |neighbor: Option<usize>| {
        neighbor.map_or(SlotRef::ImplicitHydrogen, |index| {
            SlotRef::Atom(AtomRef::Id(atom(index)))
        })
    };
    if with_marks {
        commands.extend(read.stereo.double_bonds.iter().map(|found| {
            Command::SetBondConfiguration {
                bond: created(found.bond),
                configuration: Some(BondConfiguration::Relative {
                    begin_neighbor: slot(Some(found.begin_neighbor)),
                    end_neighbor: slot(Some(found.end_neighbor)),
                    relation: found.relation,
                }),
            }
        }));
    }
    commands.extend(
        stated
            .iter()
            .map(|&(bond, how)| Command::SetBondConfiguration {
                bond: created(bond),
                configuration: Some(match how {
                    Stated::Unknown => BondConfiguration::Unknown,
                    Stated::Relative(begin, end, relation) => BondConfiguration::Relative {
                        begin_neighbor: slot(begin),
                        end_neighbor: slot(end),
                        relation,
                    },
                }),
            }),
    );
    document.apply(&commands).unwrap();
    document
}

/// The drawing a text with marks states.
fn drawing(text: &str) -> Document {
    build(text, true, None, &[])
}

fn write(document: &Document) -> SmilesWrite {
    OpenSmilesCodec
        .write_with_groups(
            &MoleculeGraph::from_document(document),
            SmilesStyle::Recorded,
            document.stereo_groups(),
        )
        .unwrap()
}

/// `(feature, source path)` of every loss that needs consent.
fn consent_needed(written: &SmilesWrite) -> Vec<(&str, &str)> {
    written
        .losses
        .iter()
        .filter(|loss| loss.consent == LossConsent::Required)
        .map(|loss| (loss.feature.as_str(), loss.source_path.as_str()))
        .collect()
}

#[test]
fn the_four_spellings_of_two_fluorines_are_written_with_the_first_mark_a_slash() {
    // Trans is `/ /` or `\ \`, cis `/ \` or `\ /`; the writer states the same relation with
    // its own first mark, a `/`.
    for (input, expected) in [
        ("F/C=C/F", "F/C=C/F"),
        ("F\\C=C\\F", "F/C=C/F"),
        ("F\\C=C/F", "F/C=C\\F"),
        ("F/C=C\\F", "F/C=C\\F"),
    ] {
        let written = write(&drawing(input));
        assert_eq!(written.text, expected, "{input}");
        assert!(consent_needed(&written).is_empty(), "{input}: {written:?}");
    }
}

#[test]
fn a_branch_atom_is_marked_from_the_side_it_is_written_on() {
    // `C(\F)=C/F` is trans. Written, the branch fluorine follows the carbon: a first `/` puts it
    // above, so the far fluorine, trans to it, is below the second carbon: `\`.
    assert_eq!(write(&drawing("C(\\F)=C/F")).text, "C(/F)=C\\F");
    assert_eq!(write(&drawing("C(/F)=C/F")).text, "C(/F)=C/F");
    // Only one mark is written at an end that has two neighbours.
    assert_eq!(write(&drawing("F/C(/CC)=C/F")).text, "F/C(CC)=C/F");
    assert_eq!(write(&drawing("CC(C)/C=C/C(C)C")).text, "CC(C)/C=C/C(C)C");
}

#[test]
fn the_mark_names_the_first_neighbour_in_the_text_whichever_is_stored() {
    // Cl0 - C1 - C2(F3)=C4 - F5, numbered so that the stored neighbour of C2 is F3 (the lower
    // identifier) but the text reaches C1 first: Cl0 C1 / C2(F3)... The molecule is
    // `ClC/C(F)=C/F`: C1 and F5 trans, so F3 and F5 cis. The identifiers are a1 Cl, a2 F,
    // a3 C, a4 C, a5 C, a6 F: the double bond is a4=a5 and a4's neighbours are a2 (F) and a3.
    let document = {
        let mut document = Document::new(DocumentId::parse("flip").unwrap(), "Flip").unwrap();
        let element = |symbol: &str| OpenSmilesCodec.read(symbol).unwrap().atoms.remove(0).atom;
        let mut commands: Vec<Command> = ["Cl", "F", "C", "C", "C", "F"]
            .iter()
            .map(|symbol| Command::AddAtom {
                atom: element(symbol),
                positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
                bond_to: None,
            })
            .collect();
        // Cl-C(a3), C(a3)-C(a4), C(a4)-F(a2), C(a4)=C(a5), C(a5)-F(a6)
        for (begin, end, order) in [
            (0, 2, BondOrder::Single),
            (2, 3, BondOrder::Single),
            (3, 1, BondOrder::Single),
            (3, 4, BondOrder::Double),
            (4, 5, BondOrder::Single),
        ] {
            commands.push(Command::AddBond {
                begin: AtomRef::Id(atom(begin)),
                end: AtomRef::Id(atom(end)),
                order,
            });
        }
        // F(a2) and F(a6) are on the same side.
        commands.push(Command::SetBondConfiguration {
            bond: BondRef::Created(6 + 3),
            configuration: Some(BondConfiguration::Relative {
                begin_neighbor: SlotRef::Atom(AtomRef::Id(atom(1))),
                end_neighbor: SlotRef::Atom(AtomRef::Id(atom(5))),
                relation: Same,
            }),
        });
        document.apply(&commands).unwrap();
        document
    };
    // Walking from a1: Cl, C(a3), C(a4) reached from a3; its other neighbours are F (a2) and
    // the double-bonded carbon. The mark sits on a3 (reached first), which is the position
    // opposite the stored F, so it is trans to F(a6): `/` below the carbon, `/` above.
    let written = write(&document);
    assert_eq!(written.text, "ClC/C(F)=C/F");
    assert!(consent_needed(&written).is_empty(), "{written:?}");
}

#[test]
fn a_conjugated_chain_shares_the_marks_of_its_middle_bonds() {
    for input in [
        "C/C=C/C=C/C",
        "C/C=C\\C=C/C",
        "C/C=C/C=C/C=C/C",
        "C/C=C\\C=C/C=C\\C",
    ] {
        let written = write(&drawing(input));
        assert_eq!(written.text, input);
        assert!(consent_needed(&written).is_empty(), "{input}");
    }
    // The second double bond states nothing: its only mark serves the first.
    assert_eq!(write(&drawing("C/C=C/C=CC")).text, "C/C=C/C=CC");
}

#[test]
fn hydrogens_of_an_end_are_positions_not_marks() {
    // An explicit hydrogen atom is a neighbour like another and can carry the mark; a bracket
    // hydrogen is written bare (the count is the valence model's) and the end has no other
    // neighbour to mark.
    assert_eq!(write(&drawing("[H]/C(F)=C/F")).text, "[H]/C(F)=C/F");
    assert_eq!(write(&drawing("F/[CH]=[CH]/F")).text, "F/C=C/F");
    assert_eq!(write(&drawing("C/C=N/O")).text, "C/C=N/O");
    assert_eq!(write(&drawing("C/C(C)=N\\O")).text, "C/C(C)=N\\O");
    assert_eq!(write(&drawing("F/N=N/F")).text, "F/N=N/F");
    assert_eq!(write(&drawing("F/N=N\\F")).text, "F/N=N\\F");
}

#[test]
fn rings_of_eight_atoms_or_more_are_written_and_smaller_ones_are_not() {
    assert_eq!(write(&drawing("C1CC/C=C\\CCC1")).text, "C1CC/C=C\\CCC1");
    assert_eq!(write(&drawing("C1CCC/C=C/CCC1")).text, "C1CCC/C=C/CCC1");
    // The same double bond of a seven-membered ring: the reader would refuse the marks.
    let document = build(
        "C1CC=CCCC1",
        false,
        None,
        &[(2, Stated::Relative(Some(1), Some(4), Same))],
    );
    let written = write(&document);
    assert_eq!(written.text, "C1CC=CCCC1");
    assert_eq!(
        consent_needed(&written),
        [("stereo_configuration", "bonds[b3]")]
    );
    assert!(
        written.losses[0]
            .reason
            .contains("ring of fewer than eight atoms"),
        "{:?}",
        written.losses
    );
    // And the same ring at eight atoms is written.
    let document = build(
        "C1CC=CCCCC1",
        false,
        None,
        &[(2, Stated::Relative(Some(1), Some(4), Same))],
    );
    assert_eq!(write(&document).text, "C1C/C=C\\CCCC1");
}

#[test]
fn an_aromatic_chain_neighbour_and_other_components_are_written_like_any_other() {
    for input in [
        "c1ccccc1/C=C/c1ccccc1",
        "C1=CC=CC=C1/C=C/F",
        "C/C=C/C.C/C=C\\C",
        "C/C=C/C(F)Cl",
        "OC(=O)/C=C/C(=O)O",
        "OC(=O)/C=C\\C(=O)O",
        "F/C=C/C/C=C\\C",
        "F/C=C/CC=CC",
    ] {
        assert_eq!(write(&drawing(input)).text, input, "{input}");
    }
}

#[test]
fn every_accepted_fixture_reads_back_as_the_configurations_it_was_written_from() {
    for input in [
        "F/C=C/F",
        "F\\C=C\\F",
        "F\\C=C/F",
        "F/C=C\\F",
        "C(\\F)=C/F",
        "C(/F)=C/F",
        "F/C(CC)=C/F",
        "F/C(/CC)=C/F",
        "C/C=C/C=C/C",
        "C/C=C\\C=C/C",
        "C/C=C/C=CC",
        "F/C=C/C/C=C\\C",
        "F/C=C/CC=CC",
        "[H]/C(F)=C/F",
        "F/[CH]=[CH]/F",
        "C/C=N/O",
        "C/C(C)=N\\O",
        "F/N=N/F",
        "F/N=N\\F",
        "OC(=O)/C=C/C(=O)O",
        "OC(=O)/C=C\\C(=O)O",
        "c1ccccc1/C=C/c1ccccc1",
        "C/C=C/C(F)Cl",
        "C1CC/C=C\\CCC1",
        "C1CCC/C=C/CCC1",
        "C/C=C/C.C/C=C\\C",
        "CC(C)/C=C/C(C)C",
        "C(C(\\F)=C/F)C",
        "C(C)(/F)=C/F",
        "C1=CC=CC=C1/C=C/F",
        "Cl/C=C\\F",
        "C/C=C/C=C/C=C/C",
        "C1CC/C=C\\C=C/CC1",
    ] {
        let before = drawing(input);
        let written = write(&before);
        assert!(consent_needed(&written).is_empty(), "{input}: {written:?}");
        let after = drawing(&written.text);
        // The same atoms and bonds in the same order, so the stored records compare directly.
        let shape = |document: &Document| {
            document
                .bonds()
                .map(|(_, bond)| (bond.begin, bond.end, bond.order.clone()))
                .collect::<Vec<_>>()
        };
        assert_eq!(shape(&before), shape(&after), "{input} -> {}", written.text);
        assert_eq!(
            before.stereo(),
            after.stereo(),
            "{input} -> {}",
            written.text
        );
        assert!(!before.stereo().is_empty() || input == "C/C=C/C=CC");
    }
}

#[test]
fn an_unconfigured_middle_bond_of_a_conjugated_chain_costs_the_second_outer_configuration() {
    // C0 C1=C2 C3=C4 C5=C6 C7; b2 and b6 are stored, b4 is not. Greedy in bond order: b2 takes
    // the marks C0-C1 and C2-C3. b6 would take C4-C5 and C6-C7, which would mark both ends of
    // b4, a bond the drawing says nothing about: b6 is left out, with a consent loss.
    let document = build(
        "CC=CC=CC=CC",
        false,
        None,
        &[
            (1, Stated::Relative(Some(0), Some(3), Opposite)),
            (5, Stated::Relative(Some(4), Some(7), Opposite)),
        ],
    );
    let written = write(&document);
    assert_eq!(written.text, "C/C=C/C=CC=CC");
    assert_eq!(
        consent_needed(&written),
        [("stereo_configuration", "bonds[b6]")]
    );
    let loss = &written.losses[0];
    assert_eq!(loss.disposition, LossDisposition::Dropped);
    assert_eq!(loss.semantic_impact, SemanticImpact::Stereo);
    assert!(
        loss.reason
            .contains("also state a configuration for double bond b4"),
        "{}",
        loss.reason
    );
    // Reading the text back finds exactly the one configuration that was kept.
    let read = OpenSmilesCodec.read(&written.text).unwrap();
    assert_eq!(read.stereo.double_bonds.len(), 1);
    assert_eq!(read.stereo.double_bonds[0].bond, 1);
}

#[test]
fn an_unknown_middle_bond_is_left_out_and_so_is_what_would_mark_it() {
    let document = build(
        "CC=CC=CC=CC",
        false,
        None,
        &[
            (1, Stated::Relative(Some(0), Some(3), Opposite)),
            (3, Stated::Unknown),
            (5, Stated::Relative(Some(4), Some(7), Opposite)),
        ],
    );
    let written = write(&document);
    assert_eq!(written.text, "C/C=C/C=CC=CC");
    assert_eq!(
        consent_needed(&written),
        [
            ("stereo_configuration", "bonds[b4]"),
            ("stereo_configuration", "bonds[b6]"),
        ]
    );
    assert!(
        written.losses[0].reason.contains("unknown"),
        "{:?}",
        written.losses
    );
}

#[test]
fn a_fully_configured_chain_is_written_whole() {
    let document = build(
        "CC=CC=CC=CC",
        false,
        None,
        &[
            (1, Stated::Relative(Some(0), Some(3), Opposite)),
            (3, Stated::Relative(Some(2), Some(5), Opposite)),
            (5, Stated::Relative(Some(4), Some(7), Opposite)),
        ],
    );
    let written = write(&document);
    assert_eq!(written.text, "C/C=C/C=C/C=C/C");
    assert!(consent_needed(&written).is_empty(), "{written:?}");
}

#[test]
fn bond_identifiers_decide_which_configuration_of_a_chain_is_kept() {
    // The outer double bonds (text b2, b6) get the identifiers b1 and b2 and the middle one
    // (text b4) b3, so the outer ones are placed first and the middle one last. With the outer
    // ones trans the marks they leave on the middle bond already state it trans.
    let order = [1, 5, 3, 0, 2, 4, 6];
    let trans = build(
        "CC=CC=CC=CC",
        false,
        Some(&order),
        &[
            (1, Stated::Relative(Some(0), Some(3), Opposite)),
            (3, Stated::Relative(Some(2), Some(5), Opposite)),
            (5, Stated::Relative(Some(4), Some(7), Opposite)),
        ],
    );
    let written = write(&trans);
    assert_eq!(written.text, "C/C=C/C=C/C=C/C");
    assert!(consent_needed(&written).is_empty(), "{written:?}");
    // With the middle one cis the outer marks would state it trans: the sixth bond, placed
    // second, is the one left out, and the middle bond is written cis from the first mark.
    let cis = build(
        "CC=CC=CC=CC",
        false,
        Some(&order),
        &[
            (1, Stated::Relative(Some(0), Some(3), Opposite)),
            (3, Stated::Relative(Some(2), Some(5), Same)),
            (5, Stated::Relative(Some(4), Some(7), Opposite)),
        ],
    );
    let written = write(&cis);
    assert_eq!(written.text, "C/C=C/C=C\\C=CC");
    // Bond ids: text b6 was added second, so it is the drawing's b2.
    assert_eq!(
        consent_needed(&written),
        [("stereo_configuration", "bonds[b2]")]
    );
    assert!(
        written.losses[0]
            .reason
            .contains("opposite configuration for double bond b3"),
        "{}",
        written.losses[0].reason
    );
}

#[test]
fn marks_that_would_put_both_neighbours_of_an_end_on_one_side_are_not_written() {
    // F0 C1=C2 C3(=C4 C5) C6=C7 F8: the middle carbon C3 is the end of its own double bond and
    // is joined to the ends of both others. b2 (cis) puts C2 above C3, b4 is written from
    // that mark, and b7 would put C6 above C3 as well: both neighbours of one end of b4 on
    // the same side, which the reader refuses. b7 is left out.
    let document = build(
        "FC=CC(=CC)C=CF",
        false,
        None,
        &[
            (1, Stated::Relative(Some(0), Some(3), Same)),
            (3, Stated::Relative(Some(2), Some(5), Opposite)),
            (6, Stated::Relative(Some(3), Some(8), Opposite)),
        ],
    );
    let written = write(&document);
    assert_eq!(written.text, "F/C=C\\C(=C\\C)C=CF");
    assert_eq!(
        consent_needed(&written),
        [("stereo_configuration", "bonds[b7]")]
    );
    assert!(
        written.losses[0].reason.contains("same side"),
        "{}",
        written.losses[0].reason
    );
}

#[test]
fn a_ring_closure_digit_never_carries_a_mark() {
    // An end whose only other neighbour is written as a ring-closure digit has nothing to mark.
    let document = build(
        "C1=CCCCCCC1",
        false,
        None,
        &[(0, Stated::Relative(Some(7), Some(2), Same))],
    );
    let written = write(&document);
    assert_eq!(written.text, "C1=CCCCCCC1");
    assert_eq!(
        consent_needed(&written),
        [("stereo_configuration", "bonds[b1]")]
    );
    assert!(
        written.losses[0].reason.contains("ring-closure digit"),
        "{}",
        written.losses[0].reason
    );
    // An end with a second neighbour marks that one, skipping the digit: C0 has the ring
    // closure to C8, the fluorine and the double bond. F1 and C3 are trans.
    let document = build(
        "C1(F)=CCCCCCC1",
        false,
        None,
        &[(1, Stated::Relative(Some(1), Some(3), Opposite))],
    );
    let written = write(&document);
    assert_eq!(written.text, "C1(/F)=C\\CCCCCC1");
    assert!(consent_needed(&written).is_empty(), "{written:?}");
}

#[test]
fn a_conjugated_ring_is_written_with_the_marks_its_digit_leaves_free() {
    // Cyclooctatetraene with every double bond cis in the ring. b1 (C0=C1) has C7 only through the
    // ring-closure digit, and b7 (C6=C7) has C0 only through it; the two inner bonds b3 and b5
    // are written. b3 takes C1-C2 and C3-C4; b5 shares C3-C4 and adds C5-C6.
    let all_cis = |bond: usize, begin: usize, end: usize| {
        (bond, Stated::Relative(Some(begin), Some(end), Same))
    };
    let document = build(
        "C1=CC=CC=CC=C1",
        false,
        None,
        &[
            all_cis(0, 7, 2),
            all_cis(2, 1, 4),
            all_cis(4, 3, 6),
            all_cis(6, 5, 0),
        ],
    );
    let written = write(&document);
    assert_eq!(written.text, "C1=C/C=C\\C=C/C=C1");
    assert_eq!(
        consent_needed(&written),
        [
            ("stereo_configuration", "bonds[b1]"),
            ("stereo_configuration", "bonds[b7]"),
        ]
    );
}

#[test]
fn what_the_reader_would_refuse_or_cannot_name_is_left_out_with_a_loss() {
    // An end that only has an implicit hydrogen to name: C=N with the nitrogen's lone pair.
    let document = build(
        "CC=N",
        false,
        None,
        &[(1, Stated::Relative(Some(0), None, Same))],
    );
    let written = write(&document);
    assert_eq!(written.text, "CC=N");
    assert_eq!(
        consent_needed(&written),
        [("stereo_configuration", "bonds[b2]")]
    );
    assert!(
        written.losses[0].reason.contains("not a bonded atom"),
        "{}",
        written.losses[0].reason
    );
    // A phosphorus end is not read.
    let document = build(
        "FP=CF",
        false,
        None,
        &[(1, Stated::Relative(Some(0), Some(3), Same))],
    );
    let written = write(&document);
    assert_eq!(written.text, "FP=CF");
    assert!(
        written.losses[0]
            .reason
            .contains("only carbon and nitrogen ends"),
        "{}",
        written.losses[0].reason
    );
}

#[test]
fn hydrogens_stated_in_brackets_count_as_neighbours_of_an_end() {
    // `F[CH](Cl)=CF` states one hydrogen on a carbon that already has F, Cl and the double
    // bond: the reader counts three other neighbours and refuses the end, so the writer leaves
    // the configuration out rather than produce text the reader refuses.
    let document = {
        let mut read = OpenSmilesCodec.read("F[CH](Cl)=CF").unwrap();
        assert_eq!(read.atoms.len(), 5);
        let mut document = Document::new(DocumentId::parse("bracket-h").unwrap(), "H").unwrap();
        let mut commands: Vec<Command> = read
            .atoms
            .drain(..)
            .map(|entry| Command::AddAtom {
                atom: entry.atom,
                positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
                bond_to: None,
            })
            .collect();
        commands.extend(read.bonds.iter().map(|bond| Command::AddBond {
            begin: AtomRef::Id(atom(bond.begin)),
            end: AtomRef::Id(atom(bond.end)),
            order: bond.order.clone(),
        }));
        commands.push(Command::SetBondConfiguration {
            bond: BondRef::Created(5 + 2),
            configuration: Some(BondConfiguration::Relative {
                begin_neighbor: SlotRef::Atom(AtomRef::Id(atom(0))),
                end_neighbor: SlotRef::Atom(AtomRef::Id(atom(4))),
                relation: Opposite,
            }),
        });
        document.apply(&commands).unwrap();
        document
    };
    let written = write(&document);
    assert_eq!(written.text, "F[CH](Cl)=CF");
    assert_eq!(
        consent_needed(&written),
        [("stereo_configuration", "bonds[b3]")]
    );
    assert!(
        written.losses[0]
            .reason
            .contains("at most two other neighbours"),
        "{}",
        written.losses[0].reason
    );
}

#[test]
fn the_output_is_the_same_every_time_and_reads_back_to_the_same_output() {
    let document = build(
        "CC=CC=CC=CC",
        false,
        Some(&[1, 5, 3, 0, 2, 4, 6]),
        &[
            (1, Stated::Relative(Some(0), Some(3), Opposite)),
            (3, Stated::Relative(Some(2), Some(5), Same)),
            (5, Stated::Relative(Some(4), Some(7), Opposite)),
        ],
    );
    let first = write(&document);
    for _ in 0..3 {
        let again = write(&document);
        assert_eq!(again.text, first.text);
        assert_eq!(again.losses, first.losses);
    }
    // The text re-read and re-written keeps its own configurations and text.
    let reread = write(&drawing(&first.text));
    assert_eq!(reread.text, first.text);
    assert!(consent_needed(&reread).is_empty(), "{reread:?}");
}

#[test]
fn canonical_output_still_writes_no_directional_bond() {
    let document = drawing("F/C=C/F");
    let written = OpenSmilesCodec
        .write(
            &MoleculeGraph::from_document(&document),
            SmilesStyle::Canonical,
        )
        .unwrap();
    assert!(!written.text.contains(['/', '\\']), "{}", written.text);
}

/// The drawing of `text` with its tetrahedral centres and its directional marks.
fn drawing_with_centres(text: &str) -> Document {
    let mut document = drawing(text);
    let read = OpenSmilesCodec.read(text).unwrap();
    let commands: Vec<Command> = read
        .stereo
        .centers
        .iter()
        .map(|center| Command::SetAtomConfiguration {
            atom: AtomRef::Id(atom(center.atom)),
            configuration: Some(AtomConfiguration::Tetrahedral {
                neighbors: center.slots.map(|slot| match slot {
                    SmilesSlot::Atom(old) => SlotRef::Atom(AtomRef::Id(atom(old))),
                    SmilesSlot::ImplicitHydrogen => SlotRef::ImplicitHydrogen,
                }),
                turn: center.turn,
            }),
        })
        .collect();
    document.apply(&commands).unwrap();
    document
}

#[test]
fn a_tetrahedral_centre_and_directional_marks_round_trip_together() {
    // The centre's `@` is written in the emitted neighbour order and the marks are placed in the
    // same text, so the second read keeps both the centre and the double-bond configuration.
    for text in ["C/C=C/[C@H](F)Cl", "F[C@H](Cl)/C=C/C"] {
        let first = write(&drawing_with_centres(text));
        assert!(first.text.contains('@'), "{text} -> {}", first.text);
        assert!(first.text.contains(['/', '\\']), "{text} -> {}", first.text);
        assert!(consent_needed(&first).is_empty(), "{text}: {first:?}");
        let reread = write(&drawing_with_centres(&first.text));
        assert_eq!(reread.text, first.text, "{text}");
        assert!(consent_needed(&reread).is_empty(), "{text}: {reread:?}");
    }
}
