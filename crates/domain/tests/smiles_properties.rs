//! Seeded property-style checks of the canonical SMILES writer over a fixed corpus.
//!
//! No external generator is used: a small deterministic PRNG permutes the drawn atom and bond
//! order. For every molecule and every permutation the checks are:
//! (a) canonical text is a fixpoint of parse-then-canonicalize;
//! (b) canonical output is identical for every permutation, or refused for all of them;
//! (c) recorded output reads back to the same chemistry;
//! (d) alternate spellings of one molecule agree: identical text or refused for all.

use chemifree_domain::document::{AtomRef, Command, Document, DocumentId, NewAtomPositions, Point};
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::smiles::{self, SmilesGraph};

const PERMUTATIONS: usize = 8;
const SEED: u64 = 0x5eed_c0de_2026_1101;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = (self.next() % (i as u64 + 1)) as usize;
            items.swap(i, j);
        }
    }
}

/// The drawn graph of a parsed SMILES with atoms and bonds created in the given orders.
fn drawn(source: &SmilesGraph, atom_order: &[usize], bond_order: &[usize]) -> MoleculeGraph {
    let mut position = vec![0; atom_order.len()];
    for (new, &old) in atom_order.iter().enumerate() {
        position[old] = new;
    }
    let mut document = Document::new(DocumentId::parse("smiles-properties").unwrap(), "t").unwrap();
    let mut commands: Vec<_> = atom_order
        .iter()
        .map(|&old| Command::AddAtom {
            atom: source.atoms[old].atom.clone(),
            positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
            bond_to: None,
        })
        .collect();
    commands.extend(bond_order.iter().map(|&index| {
        let bond = &source.bonds[index];
        Command::AddBond {
            begin: AtomRef::Created(position[bond.begin]),
            end: AtomRef::Created(position[bond.end]),
            order: bond.order.clone(),
        }
    }));
    document.apply(&commands).unwrap();
    MoleculeGraph::from_document(&document)
}

fn identity_order(source: &SmilesGraph) -> (Vec<usize>, Vec<usize>) {
    (
        (0..source.atoms.len()).collect(),
        (0..source.bonds.len()).collect(),
    )
}

fn canonical_of(source: &SmilesGraph, atoms: &[usize], bonds: &[usize]) -> Option<String> {
    smiles::write_canonical(&drawn(source, atoms, bonds)).ok()
}

/// Atom symbols/charges/isotopes, bond count and sorted degrees: order-independent chemistry
/// summary used when canonical output is refused.
fn summary(source: &SmilesGraph) -> (Vec<String>, usize, Vec<usize>) {
    let mut atoms: Vec<String> = source
        .atoms
        .iter()
        .map(|a| {
            format!(
                "{}{:?}{}{:?}",
                a.atom.kind.element().unwrap().symbol(),
                a.atom.isotope,
                a.atom.charge,
                a.atom.hydrogens
            )
        })
        .collect();
    atoms.sort();
    let mut degree = vec![0; source.atoms.len()];
    for bond in &source.bonds {
        degree[bond.begin] += 1;
        degree[bond.end] += 1;
    }
    degree.sort_unstable();
    (atoms, source.bonds.len(), degree)
}

/// Molecules that should canonicalize or be refused consistently.
const CORPUS: &[&str] = &[
    // acyclic
    "C",
    "CC",
    "CCO",
    "CC(C)C",
    "CC(C)(C)C",
    "C=CC=C",
    "C#CC",
    "CC(=O)O",
    "OC(=O)CC(N)C",
    "CC(O)C(N)C(=O)O",
    "C(C)(C)(C)C",
    "CCCCCCCC",
    "CC(C)CC(C)CC(C)C",
    "N#CC(=O)N",
    "CS(=O)(=O)C",
    "CP(C)(C)=O",
    "FC(F)(F)C(Cl)(Br)I",
    "O=C=O",
    "C=C=C",
    // monocyclic saturated and unsaturated
    "C1CC1",
    "C1CCC1",
    "C1CCCC1",
    "C1CCCCC1",
    "C1CCCCCCC1",
    "C1=CC1",
    "C1=CCC1",
    "C1=CCCC1",
    "C1=CCCCC1",
    "C1=CC=CC1",
    "C1=CC=CCC1",
    "C1CCOC1",
    "C1CCNCC1",
    "C1CCSC1",
    "C1COCCN1",
    "CC1CCCCC1",
    "CC1(C)CCCCC1",
    "C1CCC(C)(C)C1",
    "OC1CCCCC1O",
    // monocyclic aromatic and heteroaromatic
    "c1ccccc1",
    "C1=CC=CC=C1",
    "Cc1ccccc1",
    "Oc1ccccc1O",
    "c1ccncc1",
    "N1=CC=CC=C1",
    "c1cc[nH]c1",
    "[nH]1cccc1",
    "N1C=CC=C1",
    "C1=CNC=C1",
    "c1ccoc1",
    "o1cccc1",
    "O1C=CC=C1",
    "c1ccsc1",
    "s1cccc1",
    "S1C=CC=C1",
    "c1c[nH]cn1",
    "N1C=CN=C1",
    "C1=CN=CN1",
    "c1cnc[nH]1",
    "c1cocn1",
    "c1cscn1",
    "c1ccncn1",
    "c1cncnc1",
    "n1ccccc1C",
    "Cn1cccc1",
    "CN1C=CC=C1",
    "[nH+]1ccccc1",
    "C[n+]1ccccc1",
    "c1cc[n-]c1",
    "c1ccc[nH+]c1",
    // fused aromatic
    "c1ccc2ccccc2c1",
    "C1=CC2=CC=CC=C2C=C1",
    "c1ccc2cc3ccccc3cc2c1",
    "c1cc2ccc3cccc4ccc(c1)c2c34",
    "c1ccc2c(c1)ccc1ccccc12",
    // fused partly saturated (refused)
    "C1CC2=CC=CC=C2C1",
    "C1CC2=C(C1)C=CC=C2",
    "C1CCC2=CC=CC=C2C1",
    "C1=CC=C2C(=C1)CC1=CC=CC=C12",
    "CC1(C)CC2=CC=CC=C12",
    // fused / bridged / spiro saturated
    "C1CCC2CCCCC2C1",
    "C1CC2CCC1C2",
    "C1CC12CC2",
    "C1CCC2(CC1)CCCC2",
    "C1CC2CC1CC2",
    "C1C2CC3CC1CC(C2)C3",
    "C1CC2(C1)CCC2",
    "C12CC1C2",
    "C1CC11CC1",
    "C1=CC2CCC1C2",
    "C1=CC2CC1C=C2",
    // charged / isotope / hydrogens
    "[13CH3-]",
    "[NH4+]",
    "[OH-]",
    "[O-]C",
    "C[O-]",
    "[Cl-]",
    "[2H]O[2H]",
    "[13CH4]",
    "[CH2-]C",
    "C[N+](C)(C)C",
    "[NH3+]CC([O-])=O",
    "[H][H]",
    "[H+]",
    "OS(=O)(=O)[O-]",
    "[N+](=O)[O-]",
    "C[N+](=O)[O-]",
    "[O-][N+](=O)c1ccccc1",
    "c1ccccc1C(=O)[O-]",
    "[13C]1CCCCC1",
    "C[13CH2]C",
    // multi-component
    "C1CC1.CCO",
    "[Na+].[Cl-]",
    "[NH4+].[Cl-]",
    "c1ccccc1.c1ccncc1",
    "CC.CC",
    "O.O",
    "[13CH3-].[NH4+]",
    // two aromatic systems
    "c1ccccc1-c1ccccc1",
    "c1ccccc1c1ccccc1",
    "c1ccccc1Cc1ccccc1",
    "c1ccncc1-c1ccccc1",
    "c1cc[nH]c1-c1ccco1",
];

/// Groups of alternate spellings of one molecule (different atom order, Kekule form, ring
/// numbering or explicit hydrogens).
const SPELLINGS: &[&[&str]] = &[
    &["c1ccccc1", "C1=CC=CC=C1", "C=1C=CC=CC=1", "c1ccc[cH]c1"],
    &[
        "[nH]1cccc1",
        "c1cc[nH]c1",
        "N1C=CC=C1",
        "C1=CNC=C1",
        "[NH]1C=CC=C1",
    ],
    &["c1ccoc1", "o1cccc1", "O1C=CC=C1", "C1=COC=C1"],
    &["c1ccsc1", "s1cccc1", "S1C=CC=C1"],
    &["c1c[nH]cn1", "N1C=CN=C1", "C1=CN=CN1", "[nH]1cncc1"],
    &["c1ccncc1", "N1=CC=CC=C1", "C1=CC=NC=C1", "n1ccccc1"],
    &[
        "c1ccc2ccccc2c1",
        "C1=CC2=CC=CC=C2C=C1",
        "C1=CC=C2C=CC=CC2=C1",
    ],
    &["C1CC1", "[CH2]1CC1", "C1CC1([H])[H]"],
    &["CC1(C)CC1", "C1CC1(C)C", "CC1(C)CC1"],
    &["C1CCC2(CC1)CCCC2", "C1CCC2(CCCCC2)C1", "C1CCC2(C1)CCCCC2"],
    &["C1CC2=CC=CC=C2C1", "C1CC2=C(C1)C=CC=C2"],
    &["OCC", "CCO", "C(O)C", "[H]OCC"],
    &["CC(=O)O", "OC(C)=O", "C(=O)(C)O", "O=C(C)O"],
    &["[NH4+]", "[N+]([H])([H])([H])[H]", "[H][N+]([H])([H])[H]"],
    &["CC(C)C", "C(C)(C)C", "C(C)(C)C"],
    &[
        "c1ccccc1-c1ccccc1",
        "c1ccccc1c1ccccc1",
        "c1cc(ccc1)-c1ccccc1",
    ],
    &["C1=CC=CC=C1.CCO", "OCC.c1ccccc1", "c1ccccc1.CCO"],
    &["C1CCC2CCCCC2C1", "C1CC2CCCCC2CC1", "C1CCCC2CCCCC12"],
    &["[13CH3-].[NH4+]", "[NH4+].[13CH3-]"],
];

/// Aromatic-bonded inputs the reader refuses because the aromaticity model cannot assign them.
const REFUSED_AT_PARSE: &[&str] = &[
    "c1ccc2c(c1)CCC2",
    "c1ccc2c(c1)CCCC2",
    "c1ccc2[nH]ccc2c1",
    "c1ccc2occc2c1",
    "c1ccc2sccc2c1",
    "c1cnc2[nH]ccc2c1",
    "C(C)1(C)CC1",
];

#[test]
fn inputs_outside_the_aromatic_model_or_grammar_are_refused_by_the_reader() {
    for &input in REFUSED_AT_PARSE {
        assert!(smiles::parse(input).is_err(), "{input}");
    }
}

fn parse(input: &str) -> SmilesGraph {
    smiles::parse(input).unwrap_or_else(|e| panic!("corpus entry {input:?} must parse: {e:?}"))
}

#[test]
fn canonical_output_is_stable_across_permutations_fixpoints_and_recorded_round_trips() {
    let mut rng = Rng(SEED);
    let mut permutations = 0_usize;
    let mut accepted = 0_usize;
    let mut refused = 0_usize;
    for &input in CORPUS {
        let source = parse(input);
        let (atoms, bonds) = identity_order(&source);
        let baseline = canonical_of(&source, &atoms, &bonds);
        match &baseline {
            Some(_) => accepted += 1,
            None => refused += 1,
        }
        for _ in 0..PERMUTATIONS {
            let mut atom_order = atoms.clone();
            let mut bond_order = bonds.clone();
            rng.shuffle(&mut atom_order);
            rng.shuffle(&mut bond_order);
            permutations += 1;
            assert_eq!(
                canonical_of(&source, &atom_order, &bond_order),
                baseline,
                "(b) {input}: permutation atoms={atom_order:?}"
            );
        }
        if let Some(text) = &baseline {
            // (a) fixpoint
            let reparsed = smiles::parse(text)
                .unwrap_or_else(|e| panic!("(a) {input}: canonical {text} does not parse: {e:?}"));
            let (a2, b2) = identity_order(&reparsed);
            assert_eq!(
                canonical_of(&reparsed, &a2, &b2).as_deref(),
                Some(text.as_str()),
                "(a) {input}: {text}"
            );
        }
        // (c) recorded round trip: the text reads back and agrees chemically.
        let graph = drawn(&source, &atoms, &bonds);
        if let Ok(recorded) = smiles::write_recorded(&graph) {
            let reparsed = smiles::parse(&recorded)
                .unwrap_or_else(|e| panic!("(c) {input}: recorded {recorded} unreadable: {e:?}"));
            let (a2, b2) = identity_order(&reparsed);
            match &baseline {
                Some(text) => assert_eq!(
                    canonical_of(&reparsed, &a2, &b2).as_deref(),
                    Some(text.as_str()),
                    "(c) {input}: {recorded}"
                ),
                None => assert_eq!(
                    summary(&reparsed),
                    summary(&source),
                    "(c) {input}: {recorded}"
                ),
            }
        }
    }
    eprintln!(
        "corpus {} molecules, {} permutations, {} canonical, {} refused",
        CORPUS.len(),
        permutations,
        accepted,
        refused
    );
    assert!(
        accepted > 100,
        "corpus should mostly canonicalize ({accepted})"
    );
    assert!(refused > 5, "corpus should exercise refusals ({refused})");
}

#[test]
fn alternate_spellings_agree_or_are_all_refused() {
    let mut rng = Rng(SEED ^ 0xa11);
    let mut results = 0_usize;
    for group in SPELLINGS {
        let mut outcomes = Vec::new();
        for &input in *group {
            let source = parse(input);
            let (atoms, bonds) = identity_order(&source);
            outcomes.push((input, canonical_of(&source, &atoms, &bonds)));
            for _ in 0..PERMUTATIONS {
                let mut atom_order = atoms.clone();
                let mut bond_order = bonds.clone();
                rng.shuffle(&mut atom_order);
                rng.shuffle(&mut bond_order);
                outcomes.push((input, canonical_of(&source, &atom_order, &bond_order)));
            }
        }
        results += outcomes.len();
        let first = &outcomes[0];
        for outcome in &outcomes {
            assert_eq!(
                outcome.1, first.1,
                "(d) {:?} disagrees with {:?}",
                outcome.0, first.0
            );
        }
    }
    eprintln!(
        "{} spelling groups, {} canonicalizations",
        SPELLINGS.len(),
        results
    );
}
