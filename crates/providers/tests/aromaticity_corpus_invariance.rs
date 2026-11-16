//! The corpus must classify a molecule the same under two fixed renumberings of its atoms and
//! bonds, and
//! the Kekulé and aromatic spellings of one molecule must agree, except for the six pairs in
//! `DOCUMENTED_SPELLING_DIFFERENCES`, whose outcomes are pinned instead. Each record is checked
//! under two fixed renumberings: the reversed order with ends swapped, and the odd indices first.

use std::collections::{BTreeMap, BTreeSet};

use chemifree_domain::aromaticity::{self, Classification, Target};
use chemifree_domain::document::{
    Atom, AtomId, AtomRef, Bond, BondId, Command, Document, DocumentId, MAX_COMMANDS,
    NewAtomPositions, Point,
};
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::identity;
use serde_json::Value;

#[path = "../examples/aromaticity_corpus.rs"]
#[expect(dead_code, reason = "the example's main is not run by this test")]
mod example;

/// One aromaticity system in original numbering: its member atoms (sorted), classification,
/// reason, electron count and cycle rank.
type SystemKey = (
    Vec<usize>,
    &'static str,
    Option<&'static str>,
    Option<u16>,
    usize,
);

/// An aromatic conversion: its command count, updated bonds and atom edits, by original index.
type AromaticEdit = (usize, BTreeSet<usize>, BTreeMap<usize, String>);

/// What the corpus reports for one record, with each atom and bond named by its index in the
/// record as the corpus file states it.
#[derive(Debug)]
struct RecordKey {
    systems: Vec<SystemKey>,
    aromatize: Result<(), &'static str>,
    kekulize: Result<(), &'static str>,
    /// The command count, the original indices of the bonds the edit updates, and each updated
    /// atom's original index with its edit (the stated donor hydrogens).
    to_aromatic: Result<AromaticEdit, &'static str>,
    /// The command count, and the original indices of the bonds the edit updates. Which of
    /// them become double follows the numbering (both alternations of a benzene ring are valid
    /// Kekulé structures), so only the updated set is compared.
    to_kekule: Result<(usize, BTreeSet<usize>), &'static str>,
    /// The canonical code of the chemical graph, or its refusal code.
    identity: Result<String, &'static str>,
}

impl RecordKey {
    /// The names of the fields on which `self` and `other` differ.
    fn differing_fields(&self, other: &Self) -> Vec<&'static str> {
        [
            ("systems", self.systems == other.systems),
            ("aromatize", self.aromatize == other.aromatize),
            ("kekulize", self.kekulize == other.kekulize),
            ("to_aromatic", self.to_aromatic == other.to_aromatic),
            ("to_kekule", self.to_kekule == other.to_kekule),
            ("identity", self.identity == other.identity),
        ]
        .into_iter()
        .filter_map(|(field, same)| (!same).then_some(field))
        .collect()
    }
}

/// A document with the original index of each of its atoms and bonds, in iteration order.
struct Numbered {
    document: Document,
    atoms: Vec<usize>,
    bonds: Vec<usize>,
}

/// The record as the corpus states it: its own numbering is the original one.
fn as_stated(document: Document) -> Numbered {
    let atoms = (0..document.atoms().len()).collect();
    let bonds = (0..document.bonds().len()).collect();
    Numbered {
        document,
        atoms,
        bonds,
    }
}

/// Rebuilds `original` with its atoms added in `atom_order` and its bonds added in
/// `bond_order`; each order is a permutation of the original indices. With `swap_ends`, every
/// bond names its atoms the other way round. Atoms keep their element, charge, radical and
/// hydrogens; positions are not copied, since perception and identity ignore them.
fn renumber(
    original: &Document,
    atom_order: &[usize],
    bond_order: &[usize],
    swap_ends: bool,
) -> Numbered {
    let stated_atoms: Vec<&Atom> = original.atoms().map(|(_, atom)| atom).collect();
    let stated_bonds: Vec<&Bond> = original.bonds().map(|(_, bond)| bond).collect();
    assert_permutation(atom_order, stated_atoms.len());
    assert_permutation(bond_order, stated_bonds.len());
    let stated_index: BTreeMap<AtomId, usize> = original
        .atoms()
        .enumerate()
        .map(|(index, (id, _))| (id, index))
        .collect();
    // position[old] is where the original atom `old` was added.
    let mut position = vec![0; atom_order.len()];
    for (new, &old) in atom_order.iter().enumerate() {
        position[old] = new;
    }

    let mut document = Document::new(
        DocumentId::parse("renumbered").expect("a valid document ID"),
        "renumbered",
    )
    .expect("an empty document");
    for chunk in atom_order.chunks(MAX_COMMANDS) {
        let commands: Vec<Command> = chunk
            .iter()
            .map(|&old| Command::AddAtom {
                atom: stated_atoms[old].clone(),
                positions: NewAtomPositions::Single(Point::new(0.0, 0.0).expect("finite")),
                bond_to: None,
            })
            .collect();
        document
            .apply(&commands)
            .expect("adding the renumbered atoms");
    }
    // Atoms are stored in ID order, which is the order they were added in.
    let new_atoms: Vec<AtomId> = document.atoms().map(|(id, _)| id).collect();
    let mut bond_commands: Vec<Command> = Vec::new();
    for &old in bond_order {
        let bond = stated_bonds[old];
        let (mut begin, mut end) = (stated_index[&bond.begin], stated_index[&bond.end]);
        if swap_ends {
            (begin, end) = (end, begin);
        }
        bond_commands.push(Command::AddBond {
            begin: AtomRef::Id(new_atoms[position[begin]]),
            end: AtomRef::Id(new_atoms[position[end]]),
            order: bond.order.clone(),
        });
    }
    for chunk in bond_commands.chunks(MAX_COMMANDS) {
        document.apply(chunk).expect("adding the renumbered bonds");
    }

    // The copy must hold the same atoms and bonds as the original, named through the mapping.
    // Bond ends are compared unordered, because swapping them is part of the renumbering.
    let copied_index: BTreeMap<AtomId, usize> = document
        .atoms()
        .enumerate()
        .map(|(index, (id, _))| (id, index))
        .collect();
    for (new, (_, atom)) in document.atoms().enumerate() {
        assert_eq!(
            atom, stated_atoms[atom_order[new]],
            "atom at new index {new}"
        );
    }
    for (new, (_, bond)) in document.bonds().enumerate() {
        let stated = stated_bonds[bond_order[new]];
        assert_eq!(bond.order, stated.order, "bond order at new index {new}");
        let mut copied_ends = [
            atom_order[copied_index[&bond.begin]],
            atom_order[copied_index[&bond.end]],
        ];
        let mut stated_ends = [stated_index[&stated.begin], stated_index[&stated.end]];
        copied_ends.sort_unstable();
        stated_ends.sort_unstable();
        assert_eq!(
            copied_ends, stated_ends,
            "bond at new index {new} joins other atoms"
        );
    }

    Numbered {
        document,
        atoms: atom_order.to_vec(),
        bonds: bond_order.to_vec(),
    }
}

fn assert_permutation(order: &[usize], len: usize) {
    let mut sorted = order.to_vec();
    sorted.sort_unstable();
    assert!(
        sorted.iter().copied().eq(0..len),
        "{order:?} is not an order of {len} items"
    );
}

/// The two fixed renumberings every record is checked under, by label.
fn numberings(original: &Document) -> [(&'static str, Numbered); 2] {
    let atoms = original.atoms().len();
    let bonds = original.bonds().len();
    let reversed = |n: usize| (0..n).rev().collect::<Vec<_>>();
    let odd_first = |n: usize| {
        (1..n)
            .step_by(2)
            .chain((0..n).step_by(2))
            .collect::<Vec<_>>()
    };
    [
        (
            "reversed numbering with ends swapped",
            renumber(original, &reversed(atoms), &reversed(bonds), true),
        ),
        (
            "odd indices first, ends kept",
            renumber(original, &odd_first(atoms), &odd_first(bonds), false),
        ),
    ]
}

fn corpus() -> Vec<Value> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/verification/aromaticity-corpus.json"
    );
    let text = std::fs::read_to_string(path).expect("the corpus file is readable");
    serde_json::from_str(&text).expect("the corpus file is a JSON array")
}

fn name(entry: &Value) -> &str {
    entry["name"].as_str().expect("every record has a name")
}

fn spelling(entry: &Value) -> &str {
    entry["spelling"]
        .as_str()
        .expect("every record has a spelling")
}

fn classification_label(classification: Classification) -> &'static str {
    match classification {
        Classification::Aromatic => "aromatic",
        Classification::NonAromatic => "non_aromatic",
        Classification::Unsupported => "unsupported",
    }
}

/// The outcome of a conversion without its payload.
fn outcome<T>(result: &Result<T, &'static str>) -> Result<(), &'static str> {
    result.as_ref().map(|_| ()).map_err(|reason| *reason)
}

fn record_key(numbered: &Numbered) -> RecordKey {
    let document = &numbered.document;
    let graph = MoleculeGraph::from_document(document);
    let atom_origin: BTreeMap<AtomId, usize> = document
        .atoms()
        .enumerate()
        .map(|(position, (id, _))| (id, numbered.atoms[position]))
        .collect();
    let bond_origin: BTreeMap<BondId, usize> = document
        .bonds()
        .enumerate()
        .map(|(position, (id, _))| (id, numbered.bonds[position]))
        .collect();
    let interpretation = aromaticity::perceive(&graph);
    let mut systems: Vec<SystemKey> = interpretation
        .systems
        .iter()
        .map(|system| {
            let mut members: Vec<usize> = system.atoms.iter().map(|id| atom_origin[id]).collect();
            members.sort_unstable();
            (
                members,
                classification_label(system.classification),
                system.reason,
                system.electrons,
                system.cycle_rank,
            )
        })
        .collect();
    systems.sort_unstable();
    let to_aromatic = aromaticity::conversion_commands(&graph, Target::Aromatic).map(|commands| {
        let updated: BTreeSet<usize> = commands
            .iter()
            .filter_map(|command| {
                let Command::UpdateBond { bond, .. } = command else {
                    return None;
                };
                Some(bond_origin[bond])
            })
            .collect();
        let atoms: BTreeMap<usize, String> = commands
            .iter()
            .filter_map(|command| {
                let Command::UpdateAtom { atom, edit } = command else {
                    return None;
                };
                Some((atom_origin[atom], format!("{edit:?}")))
            })
            .collect();
        (commands.len(), updated, atoms)
    });
    RecordKey {
        systems,
        aromatize: interpretation.aromatize().map(|_| ()),
        kekulize: interpretation.kekulize().map(|_| ()),
        to_aromatic,
        to_kekule: aromaticity::conversion_commands(&graph, Target::Kekule).map(|commands| {
            let assigned: BTreeSet<usize> = commands
                .iter()
                .filter_map(|command| {
                    let Command::UpdateBond { bond, .. } = command else {
                        return None;
                    };
                    Some(bond_origin[bond])
                })
                .collect();
            (commands.len(), assigned)
        }),
        identity: identity::canonicalize_chemical(&graph)
            .map(|canonical| canonical.code)
            .map_err(|error| error.code()),
    }
}

fn has_search_limit(key: &RecordKey) -> bool {
    key.systems
        .iter()
        .any(|(_, _, reason, _, _)| *reason == Some("search_limit"))
}

#[test]
fn corpus_results_do_not_depend_on_numbering() {
    let corpus = corpus();
    let mut compared = 0;
    for entry in &corpus {
        let stated = as_stated(
            example::document(entry).unwrap_or_else(|error| panic!("{}: {error}", name(entry))),
        );
        let stated_key = record_key(&stated);
        for (label, numbered) in numberings(&stated.document) {
            let key = record_key(&numbered);
            if has_search_limit(&stated_key) || has_search_limit(&key) {
                // A search that stops at its budget depends on the order it searched in; the
                // corpus test that asserts there is no such system reports it.
                continue;
            }
            compared += 1;
            let fields = key.differing_fields(&stated_key);
            assert!(
                fields.is_empty(),
                "{} ({}) under the {label}: differs in {fields:?}",
                name(entry),
                spelling(entry)
            );
        }
    }
    assert_eq!(
        compared,
        2 * corpus.len(),
        "every record is compared under both numberings"
    );
}

/// The pinned outcome of one spelling of a documented pair.
struct PinnedSpelling {
    /// Each system's classification and reason, as the record key reports them.
    systems: &'static [(&'static str, Option<&'static str>)],
    /// `Ok(())` for a canonical identity, or the refusal code.
    identity: Result<(), &'static str>,
}

/// A corpus pair whose two spellings the model reads differently, with each outcome pinned.
struct DocumentedDifference {
    name: &'static str,
    kekule: PinnedSpelling,
    aromatic: PinnedSpelling,
    /// The Kekulé spelling's aromatic conversion: `Ok(())` or the refusal reason.
    kekule_to_aromatic: Result<(), &'static str>,
    /// The aromatic spelling's Kekulé conversion: `Ok(())` or the refusal reason.
    aromatic_to_kekule: Result<(), &'static str>,
}

/// Documented in docs/CONTRACT.md, "Ring and aromaticity interpretation" bullets.
const DOCUMENTED_SPELLING_DIFFERENCES: [DocumentedDifference; 6] = [
    DocumentedDifference {
        name: "azulene",
        kekule: PinnedSpelling {
            systems: &[("unsupported", Some("fused_system_outside_model"))],
            identity: Err("fused_system_outside_model"),
        },
        aromatic: PinnedSpelling {
            systems: &[("unsupported", Some("mixed_ring_bonds"))],
            identity: Err("mixed_ring_bonds"),
        },
        kekule_to_aromatic: Err("fused_system_outside_model"),
        aromatic_to_kekule: Err("mixed_ring_bonds"),
    },
    DocumentedDifference {
        name: "biphenylene",
        kekule: PinnedSpelling {
            systems: &[("unsupported", Some("fused_system_outside_model"))],
            identity: Err("fused_system_outside_model"),
        },
        aromatic: PinnedSpelling {
            systems: &[("unsupported", Some("mixed_ring_bonds"))],
            identity: Err("mixed_ring_bonds"),
        },
        kekule_to_aromatic: Err("fused_system_outside_model"),
        aromatic_to_kekule: Err("mixed_ring_bonds"),
    },
    DocumentedDifference {
        name: "heptalene",
        kekule: PinnedSpelling {
            systems: &[("unsupported", Some("fused_system_outside_model"))],
            identity: Err("fused_system_outside_model"),
        },
        aromatic: PinnedSpelling {
            systems: &[("unsupported", Some("mixed_ring_bonds"))],
            identity: Err("mixed_ring_bonds"),
        },
        kekule_to_aromatic: Err("fused_system_outside_model"),
        aromatic_to_kekule: Err("mixed_ring_bonds"),
    },
    DocumentedDifference {
        name: "cyclotriazane",
        kekule: PinnedSpelling {
            systems: &[("non_aromatic", Some("not_conjugated"))],
            identity: Ok(()),
        },
        aromatic: PinnedSpelling {
            systems: &[("unsupported", Some("adjacent_donors"))],
            identity: Err("adjacent_donors"),
        },
        kekule_to_aromatic: Ok(()),
        aromatic_to_kekule: Err("adjacent_donors"),
    },
    DocumentedDifference {
        name: "five-membered O-S ring (trioxadithiolane)",
        kekule: PinnedSpelling {
            systems: &[("non_aromatic", Some("not_conjugated"))],
            identity: Ok(()),
        },
        aromatic: PinnedSpelling {
            systems: &[("unsupported", Some("adjacent_donors"))],
            identity: Err("adjacent_donors"),
        },
        kekule_to_aromatic: Ok(()),
        aromatic_to_kekule: Err("adjacent_donors"),
    },
    DocumentedDifference {
        name: "triazane fused to CH2 ring",
        kekule: PinnedSpelling {
            systems: &[("non_aromatic", Some("not_conjugated"))],
            identity: Ok(()),
        },
        aromatic: PinnedSpelling {
            systems: &[("unsupported", Some("fused_partially_aromatic"))],
            identity: Err("fused_partially_aromatic"),
        },
        kekule_to_aromatic: Ok(()),
        aromatic_to_kekule: Err("fused_partially_aromatic"),
    },
];

/// Pins the observed outcome of a documented pair to its table entry.
fn assert_documented(documented: &DocumentedDifference, kekule: &RecordKey, aromatic: &RecordKey) {
    let molecule = documented.name;
    assert_spelling(molecule, "kekule", &documented.kekule, kekule);
    assert_spelling(molecule, "aromatic", &documented.aromatic, aromatic);
    assert_eq!(
        outcome(&kekule.to_aromatic),
        documented.kekule_to_aromatic,
        "{molecule}: the Kekulé spelling's aromatic conversion differs from the documented outcome"
    );
    assert_eq!(
        outcome(&aromatic.to_kekule),
        documented.aromatic_to_kekule,
        "{molecule}: the aromatic spelling's Kekulé conversion differs from the documented outcome"
    );
}

fn assert_spelling(molecule: &str, spelling: &str, pinned: &PinnedSpelling, key: &RecordKey) {
    let systems: Vec<(&str, Option<&str>)> = key
        .systems
        .iter()
        .map(|(_, classification, reason, _, _)| (*classification, *reason))
        .collect();
    assert_eq!(
        systems, pinned.systems,
        "{molecule} ({spelling}): systems differ from the documented outcome"
    );
    assert_eq!(
        outcome(&key.identity),
        pinned.identity,
        "{molecule} ({spelling}): chemical identity differs from the documented outcome"
    );
}

#[test]
fn kekule_and_aromatic_spellings_of_a_molecule_agree() {
    let corpus = corpus();
    let mut spellings: BTreeMap<&str, BTreeMap<&str, &Value>> = BTreeMap::new();
    for entry in &corpus {
        let previous = spellings
            .entry(name(entry))
            .or_default()
            .insert(spelling(entry), entry);
        assert!(
            previous.is_none(),
            "{} has two {} records",
            name(entry),
            spelling(entry)
        );
    }
    let pairs: Vec<(&Value, &Value)> = spellings
        .values()
        .filter_map(|group| Some((*group.get("kekule")?, *group.get("aromatic")?)))
        .collect();
    let mut agreeing = 0;
    let mut pinned_names = BTreeSet::new();
    for (kekule_entry, aromatic_entry) in pairs {
        let molecule = name(kekule_entry);
        let kekule = as_stated(
            example::document(kekule_entry)
                .unwrap_or_else(|error| panic!("{molecule} (kekule): {error}")),
        );
        let mut aromatic = as_stated(
            example::document(aromatic_entry)
                .unwrap_or_else(|error| panic!("{molecule} (aromatic): {error}")),
        );
        let kekule_key = record_key(&kekule);
        let aromatic_key = record_key(&aromatic);
        if let Some(documented) = DOCUMENTED_SPELLING_DIFFERENCES
            .iter()
            .find(|documented| documented.name == molecule)
        {
            assert_documented(documented, &kekule_key, &aromatic_key);
            pinned_names.insert(molecule);
            continue;
        }
        assert_eq!(
            kekule_key.systems, aromatic_key.systems,
            "{molecule}: system keys differ between the spellings"
        );
        assert_eq!(
            kekule_key.identity, aromatic_key.identity,
            "{molecule}: chemical identity differs between the spellings"
        );
        // `kekulize` is not compared: a Kekulé spelling has no aromatic bond to assign, so it
        // succeeds there whatever the aromatic spelling does; `to_kekule` above covers it.
        assert_eq!(
            kekule_key.aromatize, aromatic_key.aromatize,
            "{molecule}: aromatize outcomes differ between the spellings"
        );
        let forward = outcome(&kekule_key.to_aromatic);
        let backward = outcome(&aromatic_key.to_kekule);
        assert_eq!(
            forward.is_ok(),
            backward.is_ok(),
            "{molecule}: Kekulé to aromatic gives {forward:?} but aromatic to Kekulé gives {backward:?}"
        );
        if let (Err(forward_reason), Err(backward_reason)) = (forward, backward) {
            assert_eq!(
                forward_reason, backward_reason,
                "{molecule}: refusal reasons differ between the spellings"
            );
        }
        if let Ok(commands) = aromaticity::conversion_commands(
            &MoleculeGraph::from_document(&aromatic.document),
            Target::Kekule,
        ) {
            if !commands.is_empty() {
                aromatic.document.apply(&commands).unwrap_or_else(|error| {
                    panic!("{molecule}: applying the Kekulé conversion failed: {error:?}")
                });
            }
            let converted = record_key(&aromatic);
            assert_eq!(
                converted.systems, aromatic_key.systems,
                "{molecule}: classification changes after the aromatic spelling is converted to Kekulé"
            );
            assert_eq!(
                converted.identity, aromatic_key.identity,
                "{molecule}: chemical identity changes after the aromatic spelling is converted to Kekulé"
            );
        }
        agreeing += 1;
    }
    assert_eq!(
        agreeing, 74,
        "{agreeing} pairs agree outside the documented table; expected 74 (80 pairs less the 6 documented)"
    );
    for documented in &DOCUMENTED_SPELLING_DIFFERENCES {
        assert!(
            pinned_names.contains(documented.name),
            "{} is not a kekule/aromatic pair in the corpus",
            documented.name
        );
    }
}

#[test]
fn no_corpus_record_reaches_the_search_limit() {
    for entry in corpus() {
        let stated = as_stated(
            example::document(&entry).unwrap_or_else(|error| panic!("{}: {error}", name(&entry))),
        );
        let renumbered = numberings(&stated.document);
        let all = std::iter::once(("stated numbering", stated)).chain(renumbered);
        for (label, numbered) in all {
            let graph = MoleculeGraph::from_document(&numbered.document);
            let stopped = aromaticity::perceive(&graph)
                .systems
                .iter()
                .filter(|system| system.reason == Some("search_limit"))
                .count();
            assert_eq!(
                stopped,
                0,
                "{} ({}) under the {label}: {stopped} system(s) reached search_limit",
                name(&entry),
                spelling(&entry)
            );
        }
    }
}
