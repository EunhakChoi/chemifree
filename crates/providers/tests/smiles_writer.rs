//! The SMILES writers: the text they give, what they refuse and why, and the invariant that every
//! string they return is accepted by the same codec's reader.

use chemifree_application::smiles::{SmilesCode, SmilesCodec, SmilesStyle, SmilesWrite};
use chemifree_domain::document::{Atom, AtomId, Bond, BondId, BondOrder};
use chemifree_domain::element::Element;
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::states::{AtomKind, AttachmentPoints, Hydrogens, QueryAtom, Radical};
use chemifree_providers::{MAX_BRANCH_DEPTH, MAX_RING_LABEL, OpenSmilesCodec};

fn id(index: usize) -> AtomId {
    AtomId::from_serial(u32::try_from(index + 1).unwrap()).unwrap()
}

fn graph_of(text: &str) -> MoleculeGraph {
    let read = OpenSmilesCodec
        .read(text)
        .unwrap_or_else(|error| panic!("{text}: {error}"));
    build(
        read.atoms.into_iter().map(|a| a.atom).collect(),
        read.bonds
            .into_iter()
            .map(|b| (b.begin, b.end, b.order))
            .collect(),
    )
}

fn build(atoms: Vec<Atom>, bonds: Vec<(usize, usize, BondOrder)>) -> MoleculeGraph {
    MoleculeGraph::from_parts(
        atoms
            .into_iter()
            .enumerate()
            .map(|(i, a)| (id(i), a))
            .collect(),
        bonds
            .into_iter()
            .enumerate()
            .map(|(i, (begin, end, order))| {
                (
                    BondId::from_serial(u32::try_from(i + 1).unwrap()).unwrap(),
                    Bond {
                        begin: id(begin),
                        end: id(end),
                        order,
                    },
                )
            })
            .collect(),
    )
}

fn atom(symbol: &str) -> Atom {
    Atom::new(AtomKind::Element(Element::from_symbol(symbol).unwrap()))
}

fn write(graph: &MoleculeGraph, style: SmilesStyle) -> SmilesWrite {
    OpenSmilesCodec
        .write(graph, style)
        .unwrap_or_else(|error| panic!("{style:?}: {error} {error:?}"))
}

fn refusal(graph: &MoleculeGraph, style: SmilesStyle) -> SmilesCode {
    OpenSmilesCodec
        .write(graph, style)
        .expect_err("the write should be refused")
        .code
}

fn recorded(text: &str) -> String {
    write(&graph_of(text), SmilesStyle::Recorded).text
}

fn canonical(text: &str) -> String {
    write(&graph_of(text), SmilesStyle::Canonical).text
}

#[test]
fn recorded_text_is_a_depth_first_walk_in_document_order() {
    for (input, expected) in [
        ("CCO", "CCO"),
        ("CC(C)C", "CC(C)C"),
        ("C(C)(C)(C)C", "C(C)(C)(C)C"),
        ("OCC", "OCC"),
        ("C1CCCCC1", "C1CCCCC1"),
        ("C1=CC=CC=C1", "C1=CC=CC=C1"),
        ("c1ccccc1", "c1ccccc1"),
        ("C1CC1C1CC1", "C1CC1C1CC1"),
        ("C12(CCCCC1)CCCCC2", "C12(CCCCC1)CCCCC2"),
        ("[Na+].[Cl-]", "[Na+].[Cl-]"),
        ("N#CC(=O)O", "N#CC(=O)O"),
        ("c1ccccc1c1ccccc1", "c1ccccc1-c1ccccc1"),
        ("[2H]C([2H])([2H])[2H]", "[2H]C([2H])([2H])[2H]"),
        ("[NH4+]", "[NH4+]"),
        ("[O-]c1ccccc1", "[O-]c1ccccc1"),
        ("[CH3]", "[CH3]"),
        ("[CH2]", "[CH2]"),
        ("c1cc[nH]c1", "c1cc[nH]c1"),
        ("C1=CNC=C1", "C1=CNC=C1"),
        ("[H][H]", "[H][H]"),
        ("C[13CH3]", "C[13CH3]"),
    ] {
        assert_eq!(recorded(input), expected, "{input}");
    }
}

#[test]
fn recorded_atom_order_lists_the_atoms_in_text_order() {
    let graph = graph_of("C(O)C");
    let written = write(&graph, SmilesStyle::Recorded);
    assert_eq!(written.text, "C(O)C");
    let order: Vec<String> = written.atom_order.iter().map(ToString::to_string).collect();
    assert_eq!(order, ["a1", "a2", "a3"]);
    // A graph given in another document order is walked from its first atom.
    let reordered = build(
        vec![atom("O"), atom("C"), atom("C")],
        vec![(1, 0, BondOrder::Single), (1, 2, BondOrder::Single)],
    );
    let written = write(&reordered, SmilesStyle::Recorded);
    assert_eq!(written.text, "OCC");
    assert_eq!(written.atom_order, vec![id(0), id(1), id(2)]);
}

#[test]
fn the_walk_visits_neighbours_in_document_order_whatever_the_order_of_the_bonds() {
    let graph = build(
        vec![atom("C"), atom("N"), atom("O")],
        vec![(0, 2, BondOrder::Single), (0, 1, BondOrder::Single)],
    );
    let written = write(&graph, SmilesStyle::Recorded);
    assert_eq!(written.text, "C(N)O");
    assert_eq!(written.atom_order, vec![id(0), id(1), id(2)]);
}

#[test]
fn canonical_text_does_not_depend_on_the_order_of_the_atoms() {
    let spellings = [
        "OCC(=O)C",
        "CC(=O)CO",
        "C(O)C(C)=O",
        "O=C(C)CO",
        "C(=O)(CO)C",
    ];
    let first = canonical(spellings[0]);
    for spelling in spellings {
        assert_eq!(canonical(spelling), first, "{spelling}");
    }
}

#[test]
fn canonical_text_does_not_depend_on_the_kekule_or_aromatic_spelling() {
    for group in [
        &["c1ccccc1", "C1=CC=CC=C1", "C1=CC=CC=C1"][..],
        &["c1cc[nH]c1", "C1=CNC=C1", "[nH]1cccc1", "N1C=CC=C1"][..],
        &["c1ccncc1", "C1=CC=NC=C1", "n1ccccc1"][..],
        &[
            "c1ccc2ccccc2c1",
            "C1=CC2=CC=CC=C2C=C1",
            "C1=CC=C2C=CC=CC2=C1",
        ][..],
        &["Oc1ccccc1", "OC1=CC=CC=C1", "c1ccc(O)cc1", "c1(O)ccccc1"][..],
        &["c1ccoc1", "C1=COC=C1", "o1cccc1"][..],
    ] {
        let first = canonical(group[0]);
        for spelling in group {
            assert_eq!(canonical(spelling), first, "{spelling}");
        }
    }
}

#[test]
fn canonical_text_is_its_own_fixpoint() {
    for input in [
        "OCC(=O)C",
        "C1=CC=CC=C1",
        "C1=CNC=C1",
        "[CH3]",
        "[NH4+]",
        "[2H]C([2H])([2H])[2H]",
        "C12CC1C2",
        "N#CC(=O)O",
        "c1ccc2ccccc2c1",
        "[H]C([H])([H])[H]",
        "C[H]",
    ] {
        let once = canonical(input);
        let twice = canonical(&once);
        assert_eq!(once, twice, "{input}");
    }
}

#[test]
fn canonical_reports_each_representation_change() {
    let kekule = write(&graph_of("C1=CC=CC=C1"), SmilesStyle::Canonical);
    assert!(kekule.losses.iter().any(|l| l.feature == "aromatic_bonds"));
    let aromatic = write(&graph_of("c1ccccc1"), SmilesStyle::Canonical);
    assert!(
        !aromatic
            .losses
            .iter()
            .any(|l| l.feature == "aromatic_bonds"),
        "an aromatic drawing is not rewritten"
    );
    let folded = write(&graph_of("C[H]"), SmilesStyle::Canonical);
    assert_eq!(folded.text, "C");
    assert!(
        folded
            .losses
            .iter()
            .any(|l| l.feature == "folded_hydrogens")
    );
    assert_eq!(folded.folded_hydrogens.len(), 1);
    assert_eq!(folded.atom_order.len(), 1);
    assert!(!folded.atom_order.contains(&folded.folded_hydrogens[0].0));
    let stated = {
        let mut carbon = atom("C");
        carbon.hydrogens = Hydrogens::Specified(4);
        write(&build(vec![carbon], vec![]), SmilesStyle::Canonical)
    };
    assert_eq!(stated.text, "C");
    assert!(stated.losses.iter().any(|l| l.feature == "hydrogen_state"));
    let plain = write(&graph_of("CCO"), SmilesStyle::Canonical);
    assert!(plain.losses.is_empty(), "{:?}", plain.losses);
}

#[test]
fn doublet_radicals_have_a_canonical_form_and_other_radicals_do_not() {
    assert_eq!(canonical("[CH3]"), "[CH3]");
    assert_eq!(canonical("[OH]"), "[OH]");
    for text in ["[CH2]", "[C]", "[CH]"] {
        let code = refusal(&graph_of(text), SmilesStyle::Canonical);
        assert_eq!(code, SmilesCode::CanonicalUnsupported, "{text}");
    }
    // A drawn singlet carbene has valence-consistent hydrogens, but no SMILES states it.
    let mut carbene = atom("C");
    carbene.radical = Radical::Singlet;
    let graph = build(vec![carbene], vec![]);
    assert_eq!(
        refusal(&graph, SmilesStyle::Recorded),
        SmilesCode::RadicalNotWritable
    );
    assert_eq!(
        refusal(&graph, SmilesStyle::Canonical),
        SmilesCode::RadicalNotWritable
    );
}

#[test]
fn ions_and_metals_are_written_with_named_consent_required_losses() {
    let sodium = {
        let mut sodium = atom("Na");
        sodium.charge = 1;
        sodium
    };
    let written = write(&build(vec![sodium], vec![]), SmilesStyle::Recorded);
    assert_eq!(written.text, "[Na+]");
    let features: Vec<&str> = written.losses.iter().map(|l| l.feature.as_str()).collect();
    assert!(features.contains(&"hydrogen_assumption"), "{features:?}");
    assert!(features.contains(&"radical_assumption"), "{features:?}");
    assert!(
        written
            .losses
            .iter()
            .filter(|l| l.feature.ends_with("_assumption"))
            .all(|l| l.consent == chemifree_application::LossConsent::Required)
    );
    let iron = write(&build(vec![atom("Fe")], vec![]), SmilesStyle::Recorded);
    assert_eq!(iron.text, "[Fe]");
    // With the hydrogens stated and the radical unknown, nothing is assumed.
    let mut stated = atom("Fe");
    stated.hydrogens = Hydrogens::Specified(0);
    stated.radical = Radical::Unknown;
    let clean = write(&build(vec![stated], vec![]), SmilesStyle::Recorded);
    assert!(clean.losses.is_empty(), "{:?}", clean.losses);
    // The canonical form is bound to the valence model, which has no row for them.
    assert_eq!(
        refusal(&build(vec![atom("Fe")], vec![]), SmilesStyle::Canonical),
        SmilesCode::CanonicalUnsupported
    );
}

#[test]
fn a_bare_atom_that_reads_back_differently_is_bracketed_with_its_count() {
    // A charged nitrogen with inferred hydrogens: the valence model fills it to four.
    let mut ammonium = atom("N");
    ammonium.charge = 1;
    let written = write(&build(vec![ammonium], vec![]), SmilesStyle::Recorded);
    assert_eq!(written.text, "[NH4+]");
    assert!(written.losses.iter().any(|l| l.feature == "hydrogen_state"));
    // A doublet radical carbon with two bonds: the count and the radical both come from the
    // shared model.
    let mut radical = atom("C");
    radical.radical = Radical::Doublet;
    let graph = build(
        vec![radical, atom("C"), atom("C")],
        vec![(0, 1, BondOrder::Single), (0, 2, BondOrder::Single)],
    );
    assert_eq!(write(&graph, SmilesStyle::Recorded).text, "[CH](C)C");
}

#[test]
fn refusals_name_their_real_cause() {
    let mut cases: Vec<(MoleculeGraph, SmilesCode)> = Vec::new();
    cases.push((build(vec![], vec![]), SmilesCode::EmptyGraph));
    let mut query = atom("C");
    query.kind = AtomKind::Query(QueryAtom::Any);
    cases.push((build(vec![query], vec![]), SmilesCode::QueryAtomNotWritable));
    let mut charged = atom("C");
    charged.charge = 9;
    cases.push((build(vec![charged], vec![]), SmilesCode::ChargeNotWritable));
    let mut heavy = atom("C");
    heavy.isotope = Some(1000);
    cases.push((build(vec![heavy], vec![]), SmilesCode::IsotopeNotWritable));
    let mut zero = atom("C");
    zero.isotope = Some(0);
    cases.push((build(vec![zero], vec![]), SmilesCode::IsotopeNotWritable));
    let mut attached = atom("C");
    attached.attachment_points = AttachmentPoints::new(&[1]).unwrap();
    cases.push((
        build(vec![attached], vec![]),
        SmilesCode::AttachmentPointsNotWritable,
    ));
    let mut unknown = atom("C");
    unknown.hydrogens = Hydrogens::Unknown;
    cases.push((
        build(vec![unknown], vec![]),
        SmilesCode::HydrogensNotWritable,
    ));
    let mut many = atom("C");
    many.hydrogens = Hydrogens::Specified(10);
    cases.push((build(vec![many], vec![]), SmilesCode::HydrogensNotWritable));
    cases.push((
        build(vec![atom("C"), atom("C")], vec![(0, 1, BondOrder::Dative)]),
        SmilesCode::BondKindNotWritable,
    ));
    cases.push((
        build(vec![atom("C"), atom("C")], vec![(0, 1, BondOrder::Unknown)]),
        SmilesCode::BondKindNotWritable,
    ));
    // An aromatic bond that is a chain bond, and a lone aromatic bond between two atoms.
    cases.push((
        build(
            vec![atom("C"), atom("C")],
            vec![(0, 1, BondOrder::Aromatic)],
        ),
        SmilesCode::AromaticNotWritable,
    ));
    // An aromatic ring of five carbons is not an aromatic system under the model.
    let five: Vec<(usize, usize, BondOrder)> = (0..5)
        .map(|i| (i, (i + 1) % 5, BondOrder::Aromatic))
        .collect();
    cases.push((
        build((0..5).map(|_| atom("C")).collect(), five),
        SmilesCode::AromaticNotWritable,
    ));
    for (graph, code) in cases {
        assert_eq!(refusal(&graph, SmilesStyle::Recorded), code, "recorded");
    }
}

#[test]
fn an_aromatic_element_without_a_lower_case_form_is_refused_before_any_text() {
    // Six-membered aromatic ring of silicon: no lower-case symbol exists for it.
    let ring: Vec<(usize, usize, BondOrder)> = (0..6)
        .map(|i| (i, (i + 1) % 6, BondOrder::Aromatic))
        .collect();
    let graph = build((0..6).map(|_| atom("Si")).collect(), ring);
    let code = refusal(&graph, SmilesStyle::Recorded);
    assert!(
        matches!(
            code,
            SmilesCode::AromaticNotWritable | SmilesCode::AromaticElementNotWritable
        ),
        "{code:?}"
    );
}

#[test]
fn refusals_explain_unsupported_aromatic_systems() {
    let five: Vec<(usize, usize, BondOrder)> = (0..5)
        .map(|i| (i, (i + 1) % 5, BondOrder::Aromatic))
        .collect();
    let graph = build((0..5).map(|_| atom("C")).collect(), five);
    let error = OpenSmilesCodec
        .write(&graph, SmilesStyle::Recorded)
        .unwrap_err();
    assert_eq!(error.code, SmilesCode::AromaticNotWritable);
    assert!(error.reason.is_some());
}

#[test]
fn ring_labels_are_freed_before_new_ones_open_at_the_same_atom() {
    // Two triangles sharing an atom, drawn so the shared atom closes one ring and opens the
    // next: the closing label is reused there.
    let graph = graph_of("C1CC11CC1");
    let text = write(&graph, SmilesStyle::Recorded).text;
    assert_eq!(text, "C1CC11CC1");
}

/// A hub atom bonded to every atom of a path that starts beside it: the walk opens one ring
/// label per spoke at the hub, `spokes` of them at once, and each path atom closes one.
fn fan(spokes: usize) -> (Vec<Atom>, Vec<(usize, usize, BondOrder)>) {
    let path = spokes + 1;
    let atoms: Vec<Atom> = (0..=path).map(|_| atom("C")).collect();
    let mut bonds: Vec<_> = (0..path).map(|i| (i, i + 1, BondOrder::Single)).collect();
    bonds.extend((2..=path).map(|i| (0, i, BondOrder::Single)));
    (atoms, bonds)
}

#[test]
fn ring_labels_run_out_at_the_reader_limit_and_not_before() {
    let (atoms, bonds) = fan(MAX_RING_LABEL);
    let text = write(&build(atoms, bonds), SmilesStyle::Recorded).text;
    assert!(text.contains(&format!("%{MAX_RING_LABEL}")), "{text}");
    let (atoms, bonds) = fan(MAX_RING_LABEL + 1);
    assert_eq!(
        refusal(&build(atoms, bonds), SmilesStyle::Recorded),
        SmilesCode::RingLabelLimit
    );
}

#[test]
fn a_label_that_closes_at_an_atom_is_free_for_rings_opening_there_at_the_limit() {
    // Every label is open when the first spoke closes at a2; a2 also opens a ring of its own.
    // The writer must free the closing label before it allocates the new one.
    let (mut atoms, mut bonds) = fan(MAX_RING_LABEL);
    let first = atoms.len();
    atoms.push(atom("C"));
    atoms.push(atom("C"));
    bonds.extend([
        (2, first, BondOrder::Single),
        (first, first + 1, BondOrder::Single),
        (first + 1, 2, BondOrder::Single),
    ]);
    let graph = build(atoms, bonds);
    let written = write(&graph, SmilesStyle::Recorded);
    let reread = OpenSmilesCodec.read(&written.text).unwrap();
    assert_eq!(reread.atoms.len(), graph.atoms().len());
    assert_eq!(reread.bonds.len(), graph.bonds().len());
}

#[test]
fn branches_nest_to_the_reader_limit_and_not_beyond() {
    // Each spine atom's spine child has a lower index than its leaf, so the child is the
    // parenthesised branch and the leaf continues the chain: depth grows with the spine.
    let comb = |spine: usize| {
        let atoms: Vec<Atom> = (0..2 * spine).map(|_| atom("C")).collect();
        let mut bonds: Vec<_> = (1..spine).map(|i| (i - 1, i, BondOrder::Single)).collect();
        bonds.extend((0..spine).map(|i| (i, spine + i, BondOrder::Single)));
        build(atoms, bonds)
    };
    let deepest = write(&comb(MAX_BRANCH_DEPTH + 1), SmilesStyle::Recorded);
    assert_eq!(deepest.text.matches('(').count(), MAX_BRANCH_DEPTH);
    assert!(OpenSmilesCodec.read(&deepest.text).is_ok());
    assert_eq!(
        refusal(&comb(MAX_BRANCH_DEPTH + 2), SmilesStyle::Recorded),
        SmilesCode::BranchDepthLimit
    );
}

#[test]
fn a_graph_with_more_commands_than_one_import_takes_is_refused() {
    let atoms: Vec<Atom> = (0..300).map(|_| atom("C")).collect();
    let bonds: Vec<_> = (1..300).map(|i| (i - 1, i, BondOrder::Single)).collect();
    let graph = build(atoms, bonds);
    assert_eq!(
        refusal(&graph, SmilesStyle::Recorded),
        SmilesCode::CommandLimit
    );
    // The canonical form is limited by the identity model's atom bound before that.
    assert_eq!(
        refusal(&graph, SmilesStyle::Canonical),
        SmilesCode::CanonicalLimit
    );
}

#[test]
fn every_written_string_is_read_by_the_same_reader() {
    for input in [
        "CCO",
        "C1=CC=CC=C1",
        "c1ccccc1",
        "[Na+].[Cl-]",
        "CC(C)(C)c1ccc(O)cc1",
        "C[N+](C)(C)C",
        "[O-][N+](=O)c1ccccc1",
        "C#CC=CC",
    ] {
        let graph = graph_of(input);
        for style in [SmilesStyle::Recorded, SmilesStyle::Canonical] {
            if let Ok(written) = OpenSmilesCodec.write(&graph, style) {
                OpenSmilesCodec
                    .read(&written.text)
                    .unwrap_or_else(|e| panic!("{input} {style:?} -> {}: {e}", written.text));
            }
        }
    }
}

#[test]
fn inferred_hydrogens_a_bracket_must_state_are_a_loss_only_in_recorded_text() {
    let mut ammonium = atom("N");
    ammonium.charge = 1;
    let graph = build(vec![ammonium], vec![]);
    let recorded = write(&graph, SmilesStyle::Recorded);
    assert_eq!(recorded.text, "[NH4+]");
    let loss = recorded
        .losses
        .iter()
        .find(|loss| loss.feature == "hydrogen_state")
        .expect("recorded text states what the drawing inferred");
    assert!(
        loss.reason.contains("the valence model determines"),
        "{}",
        loss.reason
    );
    assert_eq!(
        loss.consent,
        chemifree_application::LossConsent::NotRequired
    );
    // A canonical text states counts wherever it needs a bracket; nothing about them changed.
    let canonical = write(&graph, SmilesStyle::Canonical);
    assert_eq!(canonical.text, "[NH4+]");
    assert!(canonical.losses.is_empty(), "{:?}", canonical.losses);
    // A radical atom whose count the drawing stated is not a loss either way.
    let mut methyl = atom("C");
    methyl.radical = Radical::Doublet;
    methyl.hydrogens = Hydrogens::Specified(3);
    let written = write(&build(vec![methyl], vec![]), SmilesStyle::Canonical);
    assert_eq!(written.text, "[CH3]");
    assert!(written.losses.is_empty(), "{:?}", written.losses);
}

#[test]
fn a_hydrogen_count_the_model_does_not_determine_is_never_written_as_if_it_were() {
    // A labelled carbon with five single bonds is outside the valence model: its inferred
    // hydrogens have no count, only the guess zero, and the label needs a bracket to state one.
    let mut centre = atom("C");
    centre.isotope = Some(13);
    let mut atoms = vec![centre];
    atoms.extend((0..5).map(|_| atom("C")));
    let bonds = (1..=5).map(|i| (0, i, BondOrder::Single)).collect();
    let graph = build(atoms, bonds);
    let error = OpenSmilesCodec
        .write(&graph, SmilesStyle::Recorded)
        .unwrap_err();
    assert_eq!(error.code, SmilesCode::HydrogensNotWritable);
    assert!(error.reason.unwrap().contains("a guess"));
    // Unlabelled, the same carbon is written bare and states nothing.
    let mut atoms = vec![atom("C")];
    atoms.extend((0..5).map(|_| atom("C")));
    let bonds = (1..=5).map(|i| (0, i, BondOrder::Single)).collect();
    assert_eq!(
        write(&build(atoms, bonds), SmilesStyle::Recorded).text,
        "C(C)(C)(C)(C)C"
    );
}

#[test]
fn a_bond_that_joins_no_two_atoms_is_refused_not_walked() {
    for style in [SmilesStyle::Recorded, SmilesStyle::Canonical] {
        let to_itself = build(vec![atom("C")], vec![(0, 0, BondOrder::Single)]);
        assert_eq!(refusal(&to_itself, style), SmilesCode::BondEndpointsInvalid);
        let dangling = MoleculeGraph::from_parts(
            vec![(id(0), atom("C"))],
            vec![(
                BondId::from_serial(1).unwrap(),
                Bond {
                    begin: id(0),
                    end: id(5),
                    order: BondOrder::Single,
                },
            )],
        );
        assert_eq!(refusal(&dangling, style), SmilesCode::BondEndpointsInvalid);
    }
}

fn two_carbenes(first: Radical, second: Radical) -> MoleculeGraph {
    let mut a = atom("C");
    a.radical = first;
    let mut b = atom("C");
    b.radical = second;
    build(vec![a, b], vec![])
}

#[test]
fn the_lowest_numbered_faulty_atom_decides_the_code_and_is_named() {
    // Both atoms read back differently; a bracket cannot repair either. Whichever order the
    // two kinds of fault come in, the error names the lower-numbered atom (a1).
    for graph in [
        two_carbenes(Radical::Singlet, Radical::Triplet),
        two_carbenes(Radical::Triplet, Radical::Singlet),
    ] {
        let error = OpenSmilesCodec
            .write(&graph, SmilesStyle::Recorded)
            .unwrap_err();
        assert_eq!(error.code, SmilesCode::RadicalNotWritable);
        let reason = error.reason.unwrap();
        assert!(reason.starts_with("a1:"), "{reason}");
    }
}

#[test]
fn stated_hydrogens_on_an_over_valent_atom_are_kept_and_its_real_cause_is_reported() {
    // A carbon with five single bonds and two stated hydrogens. Dropping the stated count
    // would let the bare atom read back as an over-valent carbon with none, and be written.
    let mut centre = atom("C");
    centre.hydrogens = Hydrogens::Specified(2);
    let mut atoms = vec![centre];
    atoms.extend((0..5).map(|_| atom("C")));
    let bonds = (1..=5).map(|i| (0, i, BondOrder::Single)).collect();
    let error = OpenSmilesCodec
        .write(&build(atoms, bonds), SmilesStyle::Recorded)
        .unwrap_err();
    assert_eq!(error.code, SmilesCode::RadicalNotWritable);
    assert!(
        error
            .reason
            .unwrap()
            .contains("valence is outside the valence model"),
        "the real cause is named"
    );
}
