//! Seeded property and mutation tests for the SMILES codec: the reader never panics, every
//! string a writer returns is accepted by the reader, and canonical text depends only on the
//! chemical graph.

use std::collections::BTreeMap;

use chemifree_application::smiles::{ErrorClass, SmilesCodec, SmilesRead, SmilesStyle};
use chemifree_domain::document::{Atom, AtomId, Bond, BondId, BondOrder};
use chemifree_domain::element::Element;
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::identity;
use chemifree_domain::states::{AtomKind, Hydrogens, Radical};
use chemifree_providers::{MAX_INPUT_BYTES, OpenSmilesCodec};

/// SplitMix64: small, seeded and identical on every platform.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(bound).unwrap()).unwrap()
    }

    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }

    fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            items.swap(i, self.below(i + 1));
        }
    }
}

fn graph_of(read: &SmilesRead) -> MoleculeGraph {
    build(
        read.atoms.iter().map(|a| a.atom.clone()).collect(),
        read.bonds
            .iter()
            .map(|b| (b.begin, b.end, b.order.clone()))
            .collect(),
    )
}

fn build(atoms: Vec<Atom>, bonds: Vec<(usize, usize, BondOrder)>) -> MoleculeGraph {
    let id = |i: usize| AtomId::from_serial(u32::try_from(i + 1).unwrap()).unwrap();
    MoleculeGraph::from_parts(
        atoms
            .into_iter()
            .enumerate()
            .map(|(i, a)| (id(i), a))
            .collect(),
        bonds
            .into_iter()
            .enumerate()
            .map(|(i, (a, b, order))| {
                (
                    BondId::from_serial(u32::try_from(i + 1).unwrap()).unwrap(),
                    Bond {
                        begin: id(a),
                        end: id(b),
                        order,
                    },
                )
            })
            .collect(),
    )
}

/// The same graph with its atoms and bonds in another order and bond ends swapped.
fn permuted(graph: &MoleculeGraph, rng: &mut Rng) -> MoleculeGraph {
    let atoms: Vec<(AtomId, Atom)> = graph.atoms().map(|(id, a)| (id, a.clone())).collect();
    let mut order: Vec<usize> = (0..atoms.len()).collect();
    rng.shuffle(&mut order);
    let mut new_index = vec![0; atoms.len()];
    for (new, &old) in order.iter().enumerate() {
        new_index[old] = new;
    }
    let position: BTreeMap<AtomId, usize> = atoms
        .iter()
        .enumerate()
        .map(|(i, (id, _))| (*id, i))
        .collect();
    let mut bonds: Vec<(usize, usize, BondOrder)> = graph
        .bonds()
        .map(|(_, b)| {
            let (x, y) = (new_index[position[&b.begin]], new_index[position[&b.end]]);
            if rng.chance(50) {
                (x, y, b.order.clone())
            } else {
                (y, x, b.order.clone())
            }
        })
        .collect();
    rng.shuffle(&mut bonds);
    build(
        order.iter().map(|&old| atoms[old].1.clone()).collect(),
        bonds,
    )
}

fn element(symbol: &str) -> Atom {
    Atom::new(AtomKind::Element(Element::from_symbol(symbol).unwrap()))
}

/// Text the reader must survive: random, and mutated from valid strings.
const ALPHABET: &str = "CNOSPFIBcnopsb[]()=#:.-+%0123456789@/\\HlrKaNeu$* \t\n\ré\u{1F600}x";

const SEEDS: [&str; 14] = [
    "CCO",
    "c1ccccc1",
    "C1=CC=CC=C1",
    "[Na+].[Cl-]",
    "C(=O)(O)C",
    "[13CH3]C[2H]",
    "C12CC1C2",
    "C%12CCCCC%12",
    "c1cc[nH]c1",
    "[O-][N+](=O)c1ccccc1",
    "CC(C)(C)C",
    "[CH3]",
    "F/C=C/F",
    "CCO ethanol",
];

fn mutate(rng: &mut Rng, seed: &str) -> String {
    let mut chars: Vec<char> = seed.chars().collect();
    let alphabet: Vec<char> = ALPHABET.chars().collect();
    for _ in 0..=rng.below(4) {
        let at = if chars.is_empty() {
            0
        } else {
            rng.below(chars.len() + 1)
        };
        let pick = alphabet[rng.below(alphabet.len())];
        match rng.below(4) {
            0 if at < chars.len() => chars[at] = pick,
            1 => chars.insert(at.min(chars.len()), pick),
            2 if at < chars.len() => {
                chars.remove(at);
            }
            _ if chars.len() > 1 => {
                let other = rng.below(chars.len());
                let first = at.min(chars.len() - 1);
                chars.swap(first, other);
            }
            _ => {}
        }
    }
    chars.into_iter().collect()
}

/// Whatever the reader says, a span must lie on character boundaries inside the input, and a
/// string it accepts must write and read again.
fn check_reading(text: &str) {
    match OpenSmilesCodec.read(text) {
        Ok(read) => {
            for atom in &read.atoms {
                let span = atom.span;
                assert!(text.is_char_boundary(span.offset), "{text:?}");
                assert!(text.is_char_boundary(span.offset + span.length), "{text:?}");
            }
            if let Some(span) = read.ignored_text {
                assert!(
                    text.is_char_boundary(span.offset)
                        && text.is_char_boundary(span.offset + span.length)
                );
            }
            let graph = graph_of(&read);
            for style in [SmilesStyle::Recorded, SmilesStyle::Canonical] {
                match OpenSmilesCodec.write(&graph, style) {
                    Ok(written) => {
                        let again = OpenSmilesCodec.read(&written.text).unwrap_or_else(|error| {
                            panic!("{text:?} -> {:?} is not read: {error:?}", written.text)
                        });
                        assert_eq!(again.atoms.len(), written.atom_order.len());
                    }
                    Err(error) => assert!(
                        matches!(
                            error.code.class(),
                            ErrorClass::NotWritable | ErrorClass::Limit
                        ),
                        "{text:?}: {error:?}"
                    ),
                }
            }
        }
        Err(error) => {
            if let Some(span) = error.span {
                assert!(span.offset <= text.len(), "{text:?}: {error:?}");
                assert!(text.is_char_boundary(span.offset), "{text:?}: {error:?}");
                assert!(
                    text.is_char_boundary(span.offset + span.length),
                    "{text:?}: {error:?}"
                );
            }
            assert!(!error.code.message().is_empty());
        }
    }
}

#[test]
fn the_reader_never_panics_on_odd_input() {
    for text in [
        "",
        " ",
        "\n",
        "\0",
        "   CCO",
        "é",
        "[é]",
        "[C\u{1F600}]",
        "C\u{1F600}C",
        "%",
        "%%",
        "[%",
        "(((((",
        ")))))",
        "C(C(C(C",
        "[[[[",
        "]]]]",
        "...",
        "====",
        "C1C1C1C1C1",
        "\u{FEFF}C",
        "C\u{0301}",
    ] {
        check_reading(text);
    }
    let long = "C".repeat(MAX_INPUT_BYTES * 4);
    check_reading(&long);
    check_reading(&"(".repeat(MAX_INPUT_BYTES));
    check_reading(&"é".repeat(MAX_INPUT_BYTES));
    check_reading(&format!("C{}", "1".repeat(MAX_INPUT_BYTES)));
    check_reading(&format!("[{}", "9".repeat(MAX_INPUT_BYTES)));
    check_reading(&format!("[{}C]", "9".repeat(5000)));
}

#[test]
fn mutated_and_random_strings_never_panic_and_keep_their_spans_valid() {
    let mut rng = Rng(0x5EED_0001);
    let alphabet: Vec<char> = ALPHABET.chars().collect();
    for round in 0..4000 {
        let text = if round % 2 == 0 {
            let seed = SEEDS[rng.below(SEEDS.len())];
            mutate(&mut rng, seed)
        } else {
            (0..rng.below(40))
                .map(|_| alphabet[rng.below(alphabet.len())])
                .collect()
        };
        check_reading(&text);
    }
}

fn random_atom(rng: &mut Rng) -> Atom {
    const SYMBOLS: [&str; 16] = [
        "C", "C", "C", "C", "N", "O", "S", "P", "F", "Cl", "Br", "I", "B", "Na", "Fe", "H",
    ];
    let mut atom = element(SYMBOLS[rng.below(SYMBOLS.len())]);
    if rng.chance(15) {
        atom.charge = i8::try_from(rng.below(5)).unwrap() - 2;
    }
    if rng.chance(10) {
        atom.isotope = Some(u16::try_from(1 + rng.below(250)).unwrap());
    }
    if rng.chance(20) {
        atom.hydrogens = Hydrogens::Specified(u8::try_from(rng.below(5)).unwrap());
    }
    if rng.chance(8) {
        atom.radical = [
            Radical::Doublet,
            Radical::Singlet,
            Radical::Triplet,
            Radical::Unknown,
        ][rng.below(4)];
    }
    atom
}

fn random_molecule(rng: &mut Rng) -> MoleculeGraph {
    let count = 1 + rng.below(10);
    let atoms: Vec<Atom> = (0..count).map(|_| random_atom(rng)).collect();
    let mut bonds: Vec<(usize, usize, BondOrder)> = Vec::new();
    let mut taken = std::collections::BTreeSet::new();
    let order = |rng: &mut Rng| match rng.below(20) {
        0 => BondOrder::Aromatic,
        1 => BondOrder::Dative,
        2..=3 => BondOrder::Triple,
        4..=7 => BondOrder::Double,
        _ => BondOrder::Single,
    };
    for i in 1..count {
        if rng.chance(90) {
            let parent = rng.below(i);
            taken.insert((parent, i));
            bonds.push((parent, i, order(rng)));
        }
    }
    for _ in 0..rng.below(4) {
        let (a, b) = (rng.below(count), rng.below(count));
        if a != b && taken.insert((a.min(b), a.max(b))) {
            bonds.push((a, b, order(rng)));
        }
    }
    build(atoms, bonds)
}

#[test]
fn writers_return_only_text_the_reader_accepts_or_a_specific_refusal() {
    let mut rng = Rng(0x5EED_0002);
    let (mut written, mut refused) = (0, 0);
    for _ in 0..3000 {
        let graph = random_molecule(&mut rng);
        for style in [SmilesStyle::Recorded, SmilesStyle::Canonical] {
            match OpenSmilesCodec.write(&graph, style) {
                Ok(output) => {
                    written += 1;
                    let read = OpenSmilesCodec
                        .read(&output.text)
                        .unwrap_or_else(|error| panic!("{:?}: {error:?}", output.text));
                    assert_eq!(read.atoms.len(), output.atom_order.len(), "{}", output.text);
                    assert_eq!(read.ignored_text, None);
                }
                Err(error) => {
                    refused += 1;
                    assert!(
                        matches!(
                            error.code.class(),
                            ErrorClass::NotWritable | ErrorClass::Limit
                        ),
                        "{error:?}"
                    );
                }
            }
        }
    }
    assert!(
        written > 500,
        "only {written} strings were written ({refused} refused)"
    );
    assert!(refused > 100, "only {refused} refusals ({written} written)");
}

/// The corpus the canonical properties run over: acyclic, saturated rings, quaternary and spiro
/// atoms, bridged systems, heteroaromatics, fused aromatics, fused partially saturated systems,
/// norbornene, alpha-pinene, charged, isotopic and multi-component molecules. Each line is one
/// graph in one or more spellings.
const CORPUS: &[&[&str]] = &[
    &["CCO", "OCC", "C(O)C"],
    &["CC(=O)O", "OC(C)=O", "C(=O)(O)C"],
    &["CC(C)(C)C", "C(C)(C)(C)C"],
    &["C1CCCCC1", "C1CCCCC1"],
    &["C1CC1", "C1CC1"],
    &["C1CCC1C", "CC1CCC1"],
    &["C1CCCC1C", "CC1CCCC1"],
    &["C1CCC2(CC1)CC2", "C1CC12CCCCC2", "C1CCCCC12CC2"],
    &["C1CC2CCC1C2", "C12CCC(CC1)C2"],
    &["C1=CC2CCC1C2", "C1=CC2CC1CC2", "C1CC2C=CC1C2"],
    &[
        "CC1=CCC2CC1C2(C)C",
        "C1C=C(C)C2CC1C2(C)C",
        "CC1(C)C2CC=C(C)C1C2",
    ],
    &["c1ccccc1", "C1=CC=CC=C1", "C1=CC=CC=C1"],
    &["Cc1ccccc1", "CC1=CC=CC=C1", "c1ccccc1C"],
    &["c1ccncc1", "C1=CC=NC=C1", "n1ccccc1"],
    &["c1cc[nH]c1", "C1=CNC=C1", "[nH]1cccc1"],
    &["c1ccoc1", "C1=COC=C1", "o1cccc1"],
    &["c1ccsc1", "C1=CSC=C1"],
    &[
        "c1ccc2ccccc2c1",
        "C1=CC2=CC=CC=C2C=C1",
        "C1=CC=C2C=CC=CC2=C1",
    ],
    &[
        "c1ccc2[nH]ccc2c1",
        "C1=CC=C2NC=CC2=C1",
        "C1=CC2=C(C=C1)NC=C2",
    ],
    &["c1ccc2c(c1)ccc1ccccc12", "C1=CC=C2C(=C1)C=CC1=CC=CC=C12"],
    &["c1ccc2c(c1)CCCC2", "C1=CC=C2CCCCC2=C1", "C1CCC2=CC=CC=C2C1"],
    &["c1ccc2c(c1)CC=CC2", "C1=CC2=C(C=C1)CC=CC2"],
    &["O=C1C=CC(=O)C=C1", "C1(=O)C=CC(=O)C=C1"],
    &["[NH4+]", "[NH4+]"],
    &["C[N+](C)(C)C", "C[N+](C)(C)C"],
    &["[O-]c1ccccc1", "[O-]C1=CC=CC=C1"],
    &["c1cc[n+]([O-])cc1", "C1=CC=[N+]([O-])C=C1"],
    &["[CH3]", "[CH3]"],
    &["[2H]C([2H])([2H])[2H]", "[2H]C([2H])([2H])[2H]"],
    &["[13CH3]C", "C[13CH3]", "[13CH3]C"],
    &["[H]C([H])([H])[H]", "C", "[H]C([H])([H])[H]"],
    &["N#CC(=O)O", "OC(=O)C#N"],
    &["[Cl-].[NH4+]", "[NH4+].[Cl-]"],
    &["CCO.CC(=O)O", "OC(C)=O.CCO", "CCO.OC(C)=O"],
    &["c1ccccc1.c1ccncc1", "c1ccncc1.c1ccccc1"],
    &["C1=CC=CC=C1.O", "O.c1ccccc1"],
];

type Outcome = Result<String, String>;

fn canonical_outcome(graph: &MoleculeGraph) -> Outcome {
    OpenSmilesCodec
        .write(graph, SmilesStyle::Canonical)
        .map(|written| written.text)
        .map_err(|error| error.code.as_str().to_owned())
}

/// What a spelling comes to: its canonical text, or a refusal. A spelling the reader refuses
/// (an aromatic system the model cannot class) and a graph the canonical form refuses are both
/// refusals: the rule is one text or no text, whichever door refuses.
fn spelling_outcome(spelling: &str, graph: Option<&MoleculeGraph>) -> Outcome {
    match (OpenSmilesCodec.read(spelling), graph) {
        (Err(_), _) => Err("refused".to_owned()),
        (Ok(_), Some(graph)) => canonical_outcome(graph).map_err(|_| "refused".to_owned()),
        (Ok(read), None) => canonical_outcome(&graph_of(&read)).map_err(|_| "refused".to_owned()),
    }
}

#[test]
fn canonical_text_depends_only_on_the_chemical_graph() {
    let mut rng = Rng(0x5EED_0003);
    let (mut texts, mut refusals) = (0, 0);
    for group in CORPUS {
        let mut outcomes: Vec<(String, Outcome)> = Vec::new();
        for spelling in *group {
            let Ok(read) = OpenSmilesCodec.read(spelling) else {
                outcomes.push((
                    format!("{spelling} (read)"),
                    spelling_outcome(spelling, None),
                ));
                continue;
            };
            let graph = graph_of(&read);
            for round in 0..6 {
                let candidate = if round == 0 {
                    graph.clone()
                } else {
                    permuted(&graph, &mut rng)
                };
                outcomes.push((
                    format!("{spelling} (round {round})"),
                    spelling_outcome(spelling, Some(&candidate)),
                ));
            }
        }
        let (first_name, first) = &outcomes[0];
        for (name, outcome) in &outcomes {
            assert_eq!(
                outcome, first,
                "{name} differs from {first_name}: every spelling must give one text or all be refused"
            );
        }
        match first {
            Ok(text) => {
                texts += 1;
                // The fixpoint: canonical text read and written again is the same text.
                let again = canonical_outcome(&graph_of(&OpenSmilesCodec.read(text).unwrap()));
                assert_eq!(again.as_ref(), Ok(text), "{group:?}");
            }
            Err(_) => refusals += 1,
        }
    }
    assert!(
        texts >= 25,
        "only {texts} groups have a canonical form ({refusals} refused)"
    );
    assert!(
        refusals >= 2,
        "the corpus should include refused systems, got {refusals}"
    );
}

#[test]
fn fused_partially_saturated_systems_are_refused_in_every_spelling() {
    // The Kekule spellings read, and their canonical form is refused; the aromatic spellings
    // are refused already by the reader, because the same model classes the system.
    for kekule in [
        "C1=CC=C2CCCCC2=C1",
        "C1CCC2=CC=CC=C2C1",
        "C1=CC2=C(C=C1)CC=CC2",
    ] {
        let graph = graph_of(&OpenSmilesCodec.read(kekule).unwrap());
        assert_eq!(
            canonical_outcome(&graph),
            Err("canonical_unsupported".to_owned()),
            "{kekule}"
        );
    }
    for aromatic in ["c1ccc2c(c1)CCCC2", "c1ccc2c(c1)CC=CC2"] {
        let error = OpenSmilesCodec.read(aromatic).unwrap_err();
        assert_eq!(
            error.code.as_str(),
            "aromatic_system_unsupported",
            "{aromatic}"
        );
    }
}

#[test]
fn recorded_text_reads_back_as_the_same_graph() {
    let mut rng = Rng(0x5EED_0004);
    for group in CORPUS {
        for spelling in *group {
            // A spelling the reader refuses has no graph to write.
            let Ok(read) = OpenSmilesCodec.read(spelling) else {
                continue;
            };
            let graph = permuted(&graph_of(&read), &mut rng);
            let written = OpenSmilesCodec
                .write(&graph, SmilesStyle::Recorded)
                .unwrap();
            let again = graph_of(&OpenSmilesCodec.read(&written.text).unwrap());
            // Written atom k is the document atom `atom_order[k]`; its bonds must match.
            let source_atoms: BTreeMap<AtomId, &Atom> = graph.atoms().collect();
            let again_atoms: Vec<&Atom> = again.atoms().map(|(_, a)| a).collect();
            assert_eq!(again_atoms.len(), written.atom_order.len(), "{spelling}");
            let mut slot: BTreeMap<AtomId, usize> = BTreeMap::new();
            for (k, id) in written.atom_order.iter().enumerate() {
                slot.insert(*id, k);
                let (a, b) = (source_atoms[id], again_atoms[k]);
                assert_eq!(
                    (&a.kind, a.charge, a.isotope),
                    (&b.kind, b.charge, b.isotope),
                    "{spelling}"
                );
            }
            let normalise =
                |a: usize, b: usize, order: &BondOrder| (a.min(b), a.max(b), order.clone());
            let mut source_bonds: Vec<_> = graph
                .bonds()
                .map(|(_, b)| normalise(slot[&b.begin], slot[&b.end], &b.order))
                .collect();
            let again_ids: Vec<AtomId> = again.atoms().map(|(id, _)| id).collect();
            let index: BTreeMap<AtomId, usize> = again_ids
                .iter()
                .enumerate()
                .map(|(i, id)| (*id, i))
                .collect();
            let mut again_bonds: Vec<_> = again
                .bonds()
                .map(|(_, b)| normalise(index[&b.begin], index[&b.end], &b.order))
                .collect();
            let key = |bond: &(usize, usize, BondOrder)| (bond.0, bond.1);
            source_bonds.sort_by_key(key);
            again_bonds.sort_by_key(key);
            assert_eq!(source_bonds, again_bonds, "{spelling} -> {}", written.text);
            // Where the model can judge both, the chemical identity is the same too.
            if let (Ok(a), Ok(b)) = (
                identity::canonicalize_chemical(&graph),
                identity::canonicalize_chemical(&again),
            ) {
                assert_eq!(a.code, b.code, "{spelling} -> {}", written.text);
            }
        }
    }
}
