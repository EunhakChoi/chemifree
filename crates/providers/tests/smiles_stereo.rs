//! The SMILES reader's stereo: tetrahedral marks resolved into ordered neighbours, the refusals
//! that keep an unreadable mark from being dropped, and the CXSMILES stereo group block. Every
//! expected value is worked out by hand from the OpenSMILES rule (preceding atom, bracket
//! hydrogen, ring-closure digits in written order, then the chain), not taken from the reader.

use chemifree_application::smiles::{
    SmilesCenter, SmilesCode, SmilesCodec, SmilesRead, SmilesSlot, Span,
};
use chemifree_domain::stereo::{Turn, is_odd_permutation};
use chemifree_domain::stereo_group::GroupKind;
use chemifree_providers::OpenSmilesCodec;

use SmilesSlot::{Atom, ImplicitHydrogen as H};

fn read(text: &str) -> SmilesRead {
    OpenSmilesCodec
        .read(text)
        .unwrap_or_else(|error| panic!("{text:?}: {error} {error:?}"))
}

fn centers(text: &str) -> Vec<(usize, [SmilesSlot; 4], Turn)> {
    read(text)
        .stereo
        .centers
        .iter()
        .map(|center| (center.atom, center.slots, center.turn))
        .collect()
}

/// `(code, offset, length)` of the refusal.
fn refused(text: &str) -> (SmilesCode, usize, usize) {
    let error = OpenSmilesCodec
        .read(text)
        .expect_err(&format!("{text:?} should be refused"));
    let span = error
        .span
        .unwrap_or_else(|| panic!("{text:?}: {:?} has no span", error.code));
    (error.code, span.offset, span.length)
}

const ACW: Turn = Turn::Anticlockwise;
const CW: Turn = Turn::Clockwise;

#[test]
fn a_hydrogen_follows_the_preceding_atom() {
    // N0 C1 O2 C3: from N, then the bracket hydrogen, then O and the chain.
    assert_eq!(
        centers("N[C@H](O)C"),
        [(1, [Atom(0), H, Atom(2), Atom(3)], ACW)]
    );
    assert_eq!(
        centers("N[C@@H](O)C"),
        [(1, [Atom(0), H, Atom(2), Atom(3)], CW)]
    );
}

#[test]
fn a_hydrogen_is_first_when_the_center_starts_a_component() {
    assert_eq!(
        centers("[C@H](F)(Cl)Br"),
        [(0, [H, Atom(1), Atom(2), Atom(3)], ACW)]
    );
    // The atom after a dot has no preceding atom, and the numbering goes on past the dot.
    assert_eq!(
        centers("C.[C@H](F)(Cl)Br"),
        [(1, [H, Atom(2), Atom(3), Atom(4)], ACW)]
    );
}

#[test]
fn four_written_neighbours_keep_their_order() {
    assert_eq!(
        centers("F[C@](Cl)(Br)I"),
        [(1, [Atom(0), Atom(2), Atom(3), Atom(4)], ACW)]
    );
    // An explicit hydrogen atom is a neighbour like any other.
    assert_eq!(
        centers("[H][C@](F)(Cl)Br"),
        [(1, [Atom(0), Atom(2), Atom(3), Atom(4)], ACW)]
    );
}

#[test]
fn a_ring_label_stands_where_its_digit_is_written() {
    // C0 F1 C2 C3 C4 C5 Cl6, ring label 1 joins C0 and C5. No preceding atom: the hydrogen, the
    // label's partner C5 (digit written first), then the branch F and the chain C2.
    assert_eq!(
        centers("[C@H]1(F)CCCC1Cl"),
        [(0, [H, Atom(5), Atom(1), Atom(2)], ACW)]
    );
    // The closing end: C0 C1 C2 C3 C4 F5. Preceding C3, the hydrogen, the label's partner C0,
    // then F.
    assert_eq!(
        centers("C1CCC[C@H]1F"),
        [(4, [Atom(3), H, Atom(0), Atom(5)], ACW)]
    );
    // Two labels in the order written, not in numeric order: label 2 (partner C3) is written
    // before label 1 (partner C5).
    // Written 1 then 2, but label 2 is closed first: the order is the digits', not the closers'.
    assert_eq!(
        centers("[C@]12(F)CC2CC1"),
        [(0, [Atom(5), Atom(3), Atom(1), Atom(2)], ACW)]
    );
    assert_eq!(
        centers("[C@]21(F)CC2CC1"),
        [(0, [Atom(3), Atom(5), Atom(1), Atom(2)], ACW)]
    );
}

#[test]
fn both_hands_and_the_two_centers_of_the_rr_example() {
    assert_eq!(
        centers("F[C@H](Cl)[C@H](Br)I"),
        [
            (1, [Atom(0), H, Atom(2), Atom(3)], ACW),
            (3, [Atom(1), H, Atom(4), Atom(5)], ACW),
        ]
    );
}

/// The turn a one-center text states when its neighbours are listed by `ranking` (an element
/// symbol per neighbour, the hydrogen as `"H"`): the text's turn, flipped once for every odd
/// reordering. Two spellings of one molecule give the same value.
fn turn_by_ranking(text: &str, ranking: [&str; 4]) -> Turn {
    let read = read(text);
    let [center] = read.stereo.centers.as_slice() else {
        panic!("{text}: one center expected");
    };
    let label = |slot: SmilesSlot| match slot {
        H => "H".to_owned(),
        Atom(index) => read.atoms[index]
            .atom
            .kind
            .element()
            .unwrap()
            .symbol()
            .to_owned(),
    };
    let ranks: Vec<usize> = center
        .slots
        .iter()
        .map(|slot| {
            let name = label(*slot);
            ranking.iter().position(|r| *r == name).unwrap()
        })
        .collect();
    if is_odd_permutation(&ranks).unwrap() {
        center.turn.flipped()
    } else {
        center.turn
    }
}

#[test]
fn respellings_of_one_molecule_state_the_same_configuration() {
    let ranking = ["F", "H", "Cl", "Br"];
    let expected = turn_by_ranking("F[C@H](Cl)Br", ranking);
    for respelled in [
        "[C@@H](F)(Cl)Br",
        "Br[C@@H](Cl)F",
        "Cl[C@@H](F)Br",
        "Br[C@H](F)Cl",
        "[H][C@@](F)(Cl)Br",
        "F[C@@](Cl)([H])Br",
    ] {
        assert_eq!(turn_by_ranking(respelled, ranking), expected, "{respelled}");
    }
    // The mirror image is not one of them.
    assert_ne!(turn_by_ranking("F[C@@H](Cl)Br", ranking), expected);
}

#[test]
fn a_ring_closure_respelling_states_the_same_configuration() {
    // First: center C1 reads F, H, ring partner O6, chain C2, `@`. Second: center C5 reads the
    // chain C4, H, ring partner O0, F6, `@@`. Swapping F and C is one odd step, which `@@`
    // answers.
    let ranking = ["F", "H", "O", "C"];
    let expected = turn_by_ranking("F[C@H]1CCCCO1", ranking);
    assert_eq!(turn_by_ranking("O1CCCC[C@@H]1F", ranking), expected);
    assert_ne!(turn_by_ranking("O1CCCC[C@H]1F", ranking), expected);
}

#[test]
fn unmarked_atoms_state_nothing() {
    assert!(read("CC(F)Cl").stereo.centers.is_empty());
    assert!(read("F[C@H](Cl)Br").stereo.groups.is_empty());
}

#[test]
fn marks_the_drawing_cannot_hold_are_refused_with_their_span() {
    for (text, code, offset, length) in [
        // Two bonds and a hydrogen: three neighbours counting the hydrogen, none stored.
        ("F[C@H]Cl", SmilesCode::StereoNeighborCount, 3, 1),
        // Five neighbours.
        ("[C@H](F)(Cl)(Br)I", SmilesCode::StereoNeighborCount, 2, 1),
        ("C[C@@](F)", SmilesCode::StereoNeighborCount, 3, 2),
        // Three bonds and no hydrogen leave a lone pair, which is not read.
        (
            "[S@](=O)(C)CC",
            SmilesCode::StereoCenterUnsupportedHere,
            2,
            1,
        ),
        ("C[N@](C)CC", SmilesCode::StereoCenterUnsupportedHere, 3, 1),
        // Four positions that the valence model does not accept: a neutral nitrogen with a
        // hydrogen and three bonds, and a charged carbon.
        (
            "[N@H](F)(Cl)Br",
            SmilesCode::StereoCenterUnsupportedHere,
            2,
            1,
        ),
        (
            "[C@H+](F)(Cl)Br",
            SmilesCode::StereoCenterUnsupportedHere,
            2,
            1,
        ),
        // A double bond on the center: the valence model reads its real order.
        (
            "[C@H](=C)(F)Cl",
            SmilesCode::StereoCenterUnsupportedHere,
            2,
            1,
        ),
        // The valence model names no fourth position for a carbocation.
        ("C[C@+](F)Cl", SmilesCode::StereoCenterUnsupportedHere, 3, 1),
        // The other classes, whole with their digits.
        ("F[C@TH1H](Cl)Br", SmilesCode::StereoClassUnsupported, 3, 4),
        (
            "F[C@SP2](Cl)(Br)I",
            SmilesCode::StereoClassUnsupported,
            3,
            4,
        ),
        (
            "F[C@TB12](Cl)(Br)(I)N",
            SmilesCode::StereoClassUnsupported,
            3,
            5,
        ),
        (
            "F[C@OH3](Cl)(Br)(I)N",
            SmilesCode::StereoClassUnsupported,
            3,
            4,
        ),
        ("F[C@AL1](Cl)Br", SmilesCode::StereoClassUnsupported, 3, 4),
        // A mark outside a bracket is not SMILES at all.
        ("C@C", SmilesCode::UnexpectedCharacter, 1, 1),
        // A directional bond beside no double bond is not a configuration (tests/smiles_double_bonds.rs).
        ("F/CC", SmilesCode::DirectionalBondWithoutDoubleBond, 1, 1),
    ] {
        assert_eq!(refused(text), (code, offset, length), "{text}");
    }
}

fn group(text: &str) -> Vec<(GroupKind, Option<i32>, Vec<usize>)> {
    read(text)
        .stereo
        .groups
        .iter()
        .map(|group| (group.kind, group.number, group.atoms.clone()))
        .collect()
}

#[test]
fn cx_groups_map_kind_number_and_members_as_stated() {
    let text = "F[C@H](Cl)[C@H](Br)[C@H](I)N |a:1,o1:3,&2:5|";
    assert_eq!(
        group(text),
        [
            (GroupKind::Absolute, None, vec![1]),
            (GroupKind::Or, Some(1), vec![3]),
            (GroupKind::And, Some(2), vec![5]),
        ]
    );
    // The block is read, not ignored.
    assert_eq!(read(text).ignored_text, None);
    // Atom indexes count atoms, not ring labels, and run on past a dot.
    assert_eq!(
        group("C1CC1.F[C@H](Cl)Br |o3:4|"),
        [(GroupKind::Or, Some(3), vec![4])]
    );
}

#[test]
fn entries_of_one_kind_and_number_are_one_group() {
    assert_eq!(
        group("F[C@H](Cl)[C@H](Br)[C@H](I)N |o1:1,3,o1:5|"),
        [(GroupKind::Or, Some(1), vec![1, 3, 5])]
    );
    assert_eq!(
        group("F[C@H](Cl)[C@H](Br)[C@H](I)N |a:1,o1:3,a:5|"),
        [
            (GroupKind::Absolute, None, vec![1, 5]),
            (GroupKind::Or, Some(1), vec![3]),
        ]
    );
    // Same number, different kind: two groups. Number zero is kept as stated.
    assert_eq!(
        group("F[C@H](Cl)[C@H](Br)I |o0:1,&0:3|"),
        [
            (GroupKind::Or, Some(0), vec![1]),
            (GroupKind::And, Some(0), vec![3]),
        ]
    );
}

#[test]
fn a_center_in_no_group_states_nothing_more() {
    let read = read("F[C@H](Cl)[C@H](Br)I |o1:1|");
    assert_eq!(read.stereo.centers.len(), 2);
    assert_eq!(read.stereo.groups.len(), 1);
    assert!(read.losses.is_empty(), "{:?}", read.losses);
}

#[test]
fn text_after_the_block_is_still_reported_not_read() {
    let read = read("F[C@H](Cl)Br |a:1| name");
    assert_eq!(read.ignored_text, Some(Span::new(19, 4)));
    assert_eq!(read.stereo.groups.len(), 1);
    // Text that is not a block is ignored as before.
    assert_eq!(
        self::read("F[C@H](Cl)Br name").ignored_text,
        Some(Span::new(13, 4))
    );
}

#[test]
fn every_other_cx_field_is_refused_at_its_position() {
    for (text, offset) in [
        ("F[C@H](Cl)Br |r|", 14),
        ("F[C@H](Cl)Br |a:1,$|", 18),
        ("F[C@H](Cl)Br |c:0,1|", 14),
        ("F[C@H](Cl)Br |(0,0,;1,1,)|", 14),
        ("F[C@H](Cl)Br |a:1,w:1.2|", 18),
        // The number-less forms are not in the public description of the fields.
        ("F[C@H](Cl)Br |o:1|", 14),
        ("F[C@H](Cl)Br |&:1|", 14),
        ("F[C@H](Cl)Br |o+1:1|", 14),
        ("F[C@H](Cl)Br |a:1,^1:2|", 18),
    ] {
        let (code, found, _) = refused(text);
        assert_eq!(
            (code, found),
            (SmilesCode::CxsmilesFieldUnsupported, offset),
            "{text}"
        );
    }
}

#[test]
fn malformed_blocks_are_refused_not_ignored() {
    for (text, offset) in [
        ("F[C@H](Cl)Br |a:1", 13),
        ("F[C@H](Cl)Br ||", 13),
        ("F[C@H](Cl)Br |a:|", 14),
        ("F[C@H](Cl)Br |a:1,,1|", 18),
        ("F[C@H](Cl)Br |1|", 14),
        ("F[C@H](Cl)Br |a:x|", 14),
        ("F[C@H](Cl)Br |a:99999999999999999999|", 14),
    ] {
        let (code, found, _) = refused(text);
        assert_eq!(
            (code, found),
            (SmilesCode::CxsmilesMalformed, offset),
            "{text}"
        );
    }
}

#[test]
fn members_must_be_marked_centers_listed_once() {
    assert_eq!(
        refused("F[C@H](Cl)Br |a:0|"),
        (SmilesCode::CxsmilesGroupMemberNotStereocenter, 16, 1)
    );
    assert_eq!(
        refused("F[C@H](Cl)Br |a:1,o1:0|"),
        (SmilesCode::CxsmilesGroupMemberNotStereocenter, 21, 1)
    );
    assert_eq!(
        refused("F[C@H](Cl)Br |a:9|"),
        (SmilesCode::CxsmilesAtomOutOfRange, 16, 1)
    );
    assert_eq!(
        refused("F[C@H](Cl)Br |a:1,o1:1|"),
        (SmilesCode::CxsmilesMemberRepeated, 21, 1)
    );
    assert_eq!(
        refused("F[C@H](Cl)Br |o1:1,1|"),
        (SmilesCode::CxsmilesMemberRepeated, 19, 1)
    );
}

#[test]
fn a_stereo_import_that_exceeds_the_command_bound_is_refused() {
    // Each `C[C@H](F)` unit is 3 atoms, 3 bonds (one joins the next unit) and 1 configuration.
    // 71 units and a closing `C[C@H](F)Cl` are 217 atoms and 216 bonds, under 500 together, but
    // with 72 configurations they are 505 commands. One unit fewer is 498.
    let unit = "C[C@H](F)";
    let end = "C[C@H](F)Cl";
    let error = OpenSmilesCodec
        .read(&format!("{}{end}", unit.repeat(71)))
        .expect_err("505 commands");
    assert_eq!(error.code, SmilesCode::CommandLimit);
    let fits = OpenSmilesCodec
        .read(&format!("{}{end}", unit.repeat(70)))
        .expect("498 commands");
    assert_eq!(fits.stereo.centers.len(), 71);
}

#[test]
fn center_values_are_plain_data() {
    let center: SmilesCenter = read("F[C@H](Cl)Br").stereo.centers[0].clone();
    assert_eq!(center.span, Span::new(3, 1));
}

#[test]
fn an_aromatic_atom_is_refused_for_what_it_is_not_for_a_lone_pair() {
    let error = OpenSmilesCodec
        .read("[c@]1(F)ccccc1")
        .expect_err("aromatic centers are not read");
    assert_eq!(error.code, SmilesCode::StereoCenterUnsupportedHere);
    assert_eq!(
        error.reason.as_deref(),
        Some("an aromatic atom is not a tetrahedral center")
    );
}

#[test]
fn only_the_five_named_classes_are_refused_whole_and_other_letters_are_not_a_class() {
    // The named classes are `stereo_class_unsupported` with their digits, also after `@@`.
    assert_eq!(
        refused("F[C@@TH2H](Cl)Br"),
        (SmilesCode::StereoClassUnsupported, 3, 5)
    );
    assert_eq!(
        refused("F[C@TH3H](Cl)Br"),
        (SmilesCode::StereoClassUnsupported, 3, 4)
    );
    // Letters that name no class are not part of the grammar: the bracket is malformed.
    assert_eq!(
        refused("F[C@XXH](Cl)Br"),
        (SmilesCode::InvalidBracketContent, 4, 1)
    );
    assert_eq!(
        refused("F[C@Z](Cl)Br"),
        (SmilesCode::InvalidBracketContent, 4, 1)
    );
}
