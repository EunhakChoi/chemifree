//! The SMILES reader's double-bond configurations: what `/` and `\` state, read relative to the
//! atom written before the mark, and every refusal that keeps a mark from being dropped. Each
//! expected value is derived by hand from the OpenSMILES rule (`F/C=C/F` is trans, `C(\F)=C/F`
//! is trans), not taken from the reader. Atom and bond numbers are text order from zero; a ring
//! closure is a bond numbered where its digit closes it.

use chemifree_application::smiles::{SmilesCode, SmilesCodec, SmilesRead};
use chemifree_domain::stereo::Relation;
use chemifree_providers::OpenSmilesCodec;

use Relation::{OppositeSides as Opposite, SameSide as Same};

fn read(text: &str) -> SmilesRead {
    OpenSmilesCodec
        .read(text)
        .unwrap_or_else(|error| panic!("{text:?}: {error} {error:?}"))
}

/// `(double bond, begin neighbour, end neighbour, relation)` for every configuration.
fn configurations(text: &str) -> Vec<(usize, usize, usize, Relation)> {
    read(text)
        .stereo
        .double_bonds
        .iter()
        .map(|found| {
            (
                found.bond,
                found.begin_neighbor,
                found.end_neighbor,
                found.relation,
            )
        })
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

#[test]
fn a_mark_is_relative_to_the_atom_written_before_it() {
    // The four spellings of two fluorines on one double bond: `/ /` and `\ \` are trans,
    // `\ /` and `/ \` are cis.
    assert_eq!(configurations("F/C=C/F"), [(1, 0, 3, Opposite)]);
    assert_eq!(configurations("F\\C=C\\F"), [(1, 0, 3, Opposite)]);
    assert_eq!(configurations("F\\C=C/F"), [(1, 0, 3, Same)]);
    assert_eq!(configurations("F/C=C\\F"), [(1, 0, 3, Same)]);
}

#[test]
fn a_mark_on_a_branch_flips_with_the_side_it_is_written_on() {
    // `C(\F)=C/F` is `F/C=C/F` (trans): the branch atom follows the carbon, so `\` puts it
    // below, where `F/C` puts the preceding fluorine.
    assert_eq!(configurations("C(\\F)=C/F"), [(1, 1, 3, Opposite)]);
    assert_eq!(configurations("C(/F)=C/F"), [(1, 1, 3, Same)]);
    assert_eq!(configurations("C(C(\\F)=C/F)C"), [(2, 2, 4, Opposite)]);
}

#[test]
fn two_marks_at_one_end_state_one_side_each() {
    // F0 C1 C2 C3 C4 F5: the fluorine is below C1, the branch carbon above, the far fluorine
    // above C4.
    assert_eq!(configurations("F/C(CC)=C/F"), [(3, 0, 5, Opposite)]);
    assert_eq!(configurations("F/C(/CC)=C/F"), [(3, 0, 5, Opposite)]);
    assert_eq!(configurations("F/C(/CC)=C\\F"), [(3, 0, 5, Same)]);
}

#[test]
fn each_double_bond_of_a_conjugated_chain_is_read_from_its_own_neighbours() {
    // The bond between the two double bonds serves both: below the second carbon of the first
    // and above the third of the second.
    assert_eq!(
        configurations("C/C=C/C=C/C"),
        [(1, 0, 3, Opposite), (3, 2, 5, Opposite)]
    );
    assert_eq!(
        configurations("C/C=C\\C=C/C"),
        [(1, 0, 3, Same), (3, 2, 5, Same)]
    );
    assert_eq!(
        configurations("F/C=C/C/C=C\\C"),
        [(1, 0, 3, Opposite), (4, 3, 6, Same)]
    );
}

#[test]
fn a_double_bond_whose_only_marks_serve_a_neighbour_states_nothing() {
    // OpenSMILES "Partial Stereochemistry": the second double bond has a mark at one end only,
    // and that bond is read by the first. Both sides of the shared bond.
    assert_eq!(configurations("C/C=C/C=CC"), [(1, 0, 3, Opposite)]);
    assert_eq!(configurations("CC=C/C=C/C"), [(3, 2, 5, Opposite)]);
    // A double bond no mark touches is not mentioned.
    assert_eq!(configurations("F/C=C/CC=CC"), [(1, 0, 3, Opposite)]);
    // The carbonyls of a diacid are one-sided and served by the chain's double bond.
    assert_eq!(configurations("OC(=O)/C=C/C(=O)O"), [(3, 1, 5, Opposite)]);
    assert_eq!(configurations("OC(=O)/C=C\\C(=O)O"), [(3, 1, 5, Same)]);
    // A Kekule ring double bond beside a marked chain is served by the chain's double bond too.
    assert_eq!(configurations("C1=CC=CC=C1/C=C/F"), [(7, 5, 8, Opposite)]);
}

#[test]
fn a_partly_served_double_bond_is_refused_at_the_mark_nobody_reads() {
    // F0 C1 C2 C3 C4 F5 C6: the second double bond has the shared mark (2-3, read by the first)
    // and its own mark at offset 8 on the fluorine, with nothing at its far end.
    assert_eq!(
        refused("F/C=C/C(/F)=CC"),
        (SmilesCode::DirectionalBondIncomplete, 8, 1)
    );
}

#[test]
fn explicit_and_implicit_hydrogens_are_positions_of_the_end() {
    // The hydrogen atom is a neighbour like any other.
    assert_eq!(configurations("[H]/C(F)=C/F"), [(2, 0, 4, Opposite)]);
    // A bracket hydrogen is the position opposite the marked neighbour.
    assert_eq!(configurations("F/[CH]=[CH]/F"), [(1, 0, 3, Opposite)]);
}

#[test]
fn nitrogen_ends_read_with_their_lone_pair_position() {
    assert_eq!(configurations("C/C=N/O"), [(1, 0, 3, Opposite)]);
    assert_eq!(configurations("C/C(C)=N\\O"), [(2, 0, 4, Same)]);
    assert_eq!(configurations("F/N=N/F"), [(1, 0, 3, Opposite)]);
    assert_eq!(configurations("F/N=N\\F"), [(1, 0, 3, Same)]);
}

#[test]
fn aromatic_and_stereo_neighbours_do_not_disturb_the_reading() {
    // Marks on the single bonds that join aromatic rings to the double bond.
    assert_eq!(
        configurations("c1ccccc1/C=C/c1ccccc1"),
        [(7, 5, 8, Opposite)]
    );
    // The tetrahedral centre beside the double bond is read as before.
    let read = read("C/C=C/[C@H](F)Cl");
    assert_eq!(read.stereo.centers.len(), 1);
    assert_eq!(read.stereo.double_bonds.len(), 1);
    assert_eq!(configurations("F/C=C/C#C"), [(1, 0, 3, Opposite)]);
}

#[test]
fn ring_double_bonds_need_eight_atoms() {
    // Eight atoms: cis and trans cyclooctene-like text; nine atoms too.
    assert_eq!(configurations("C1CC/C=C\\CCC1"), [(3, 2, 5, Same)]);
    assert_eq!(configurations("C1CCC/C=C/CCC1"), [(4, 3, 6, Opposite)]);
    // Seven atoms is refused, at the first mark, and so is every smaller ring.
    assert_eq!(
        refused("C1C/C=C\\CCC1"),
        (SmilesCode::DirectionalBondSmallRing, 3, 1)
    );
    assert_eq!(
        refused("C1/C=C\\CC1"),
        (SmilesCode::DirectionalBondSmallRing, 2, 1)
    );
}

#[test]
fn the_ring_size_is_that_of_the_shortest_ring_through_the_bond() {
    // Ten atoms in a ring: read.
    assert_eq!(
        configurations("C1CC/C=C\\CCCCC1").len(),
        1,
        "ten atoms, no chord"
    );
    // The same ring with a bond (ring label 2) from C2 to C5 puts the double bond in a ring of
    // the four atoms C2 C3 C4 C5, though the long way round is still ten.
    assert_eq!(
        refused("C1CC2/C=C\\C2CCCC1"),
        (SmilesCode::DirectionalBondSmallRing, 5, 1)
    );
}

#[test]
fn disconnected_components_are_read_separately() {
    assert_eq!(
        configurations("C/C=C/C.C/C=C\\C"),
        [(1, 0, 3, Opposite), (4, 4, 7, Same)]
    );
}

#[test]
fn a_nested_branch_and_a_longer_chain_read_like_a_short_one() {
    assert_eq!(configurations("CC(C)/C=C/C(C)C"), [(3, 1, 5, Opposite)]);
}

#[test]
fn a_mark_on_a_ring_closure_digit_or_an_aromatic_bond_is_refused_at_the_mark() {
    for (text, offset) in [
        ("C/1=C/CCCCCCC1", 1),
        ("C1=C/CCCCCCC/1", 12),
        ("c1ccc/cc1", 5),
    ] {
        assert_eq!(
            refused(text),
            (SmilesCode::DirectionalBondUnsupported, offset, 1),
            "{text}"
        );
    }
}

#[test]
fn a_mark_with_one_end_of_a_double_bond_marked_is_incomplete() {
    assert_eq!(
        refused("C/C=C"),
        (SmilesCode::DirectionalBondIncomplete, 1, 1)
    );
    assert_eq!(
        refused("C/C=CC"),
        (SmilesCode::DirectionalBondIncomplete, 1, 1)
    );
    // The far end's mark is the one reported when the near end has none.
    assert_eq!(
        refused("CC=C/C"),
        (SmilesCode::DirectionalBondIncomplete, 4, 1)
    );
}

#[test]
fn a_mark_beside_no_double_bond_is_refused() {
    assert_eq!(
        refused("F/CC"),
        (SmilesCode::DirectionalBondWithoutDoubleBond, 1, 1)
    );
    // Both marks are loose; the earlier one is reported.
    assert_eq!(
        refused("C/C/C"),
        (SmilesCode::DirectionalBondWithoutDoubleBond, 1, 1)
    );
    // A triple bond is no double bond.
    assert_eq!(
        refused("C/C#C"),
        (SmilesCode::DirectionalBondWithoutDoubleBond, 1, 1)
    );
}

#[test]
fn two_marks_at_one_end_on_the_same_side_conflict() {
    // C0 is below C2 (written before it with `/`) and F is below too (after it, with `\`).
    assert_eq!(
        refused("C/C(\\F)=C/C"),
        (SmilesCode::DirectionalBondConflict, 4, 1)
    );
    assert_eq!(
        refused("F/C(\\CC)=C/F"),
        (SmilesCode::DirectionalBondConflict, 4, 1)
    );
    assert_eq!(
        refused("C(/F)(/Cl)=C/F"),
        (SmilesCode::DirectionalBondConflict, 6, 1)
    );
}

#[test]
fn cumulated_double_bonds_are_refused() {
    assert_eq!(
        refused("F/C=C=C=C/F"),
        (SmilesCode::DirectionalBondCumulated, 1, 1)
    );
    assert_eq!(
        refused("F/C=C=C/F"),
        (SmilesCode::DirectionalBondCumulated, 1, 1)
    );
    // A fully marked double bond that shares its end atom with another double bond.
    assert_eq!(
        refused("F/C(=C/F)=C"),
        (SmilesCode::DirectionalBondCumulated, 1, 1)
    );
    // A triple bond at an end of the double bond is the same.
    assert_eq!(
        refused("F/C=C#C"),
        (SmilesCode::DirectionalBondCumulated, 1, 1)
    );
}

#[test]
fn ends_other_than_neutral_carbon_and_nitrogen_are_refused() {
    for (text, offset) in [
        ("F/P=C/F", 1),
        ("F/[N+](=C/F)C", 1),
        ("F/[C-]=C/F", 1),
        ("F/N(C)=C/F", 1),
        ("F/C(C)(C)=C/F", 1),
        ("F/[Si]=C/F", 1),
    ] {
        assert_eq!(
            refused(text),
            (SmilesCode::DirectionalBondEndUnsupported, offset, 1),
            "{text}"
        );
    }
}

#[test]
fn an_unsupported_end_is_reported_before_a_conflict_or_a_ring_it_would_hide() {
    // The far end is phosphorus and the near end conflicts, which comes first in the text; an
    // unsupported end is judged before any conflict, so the conflict is not what is reported.
    assert_eq!(
        refused("C/C(\\F)=P/F"),
        (SmilesCode::DirectionalBondEndUnsupported, 9, 1)
    );
}

#[test]
fn the_earliest_mark_is_the_one_reported_when_several_things_are_wrong() {
    assert_eq!(
        refused("C/C/C.F/P=C/F"),
        (SmilesCode::DirectionalBondWithoutDoubleBond, 1, 1)
    );
    assert_eq!(
        refused("F/P=C/F.C/C=C"),
        (SmilesCode::DirectionalBondEndUnsupported, 1, 1)
    );
}

#[test]
fn a_repeated_bond_symbol_stays_a_repeated_bond_symbol() {
    assert_eq!(refused("C/=C"), (SmilesCode::RepeatedBondSymbol, 2, 1));
    assert_eq!(refused("C//C"), (SmilesCode::RepeatedBondSymbol, 2, 1));
    assert_eq!(refused("C/"), (SmilesCode::DanglingBond, 1, 1));
    assert_eq!(refused("/C"), (SmilesCode::BondWithoutAtom, 0, 1));
}

#[test]
fn every_configuration_counts_as_a_command() {
    // Each `C/C=C/C` unit is 4 atoms, 3 bonds and 1 configuration: 8 commands. 62 units are
    // 496; `CC.C` adds 4 for exactly 500, the bound of one transaction, and one more atom is 501.
    let units = vec!["C/C=C/C"; 62].join(".");
    let fits = read(&format!("{units}.CC.C"));
    assert_eq!(fits.stereo.double_bonds.len(), 62);
    assert_eq!(fits.atoms.len() + fits.bonds.len() + 62, 500);
    let error = OpenSmilesCodec
        .read(&format!("{units}.CC.C.C"))
        .expect_err("501 commands");
    assert_eq!(error.code, SmilesCode::CommandLimit);
    // The last configuration is where the overflow is reported when nothing else follows.
    let error = OpenSmilesCodec
        .read(&format!("{units}.C/C=C/C"))
        .expect_err("504 commands");
    assert_eq!(error.code, SmilesCode::CommandLimit);
}

#[test]
fn text_without_marks_states_no_double_bond_configuration() {
    assert!(read("CC=CC").stereo.double_bonds.is_empty());
    assert!(read("C1=CC=CC=C1").stereo.double_bonds.is_empty());
}
