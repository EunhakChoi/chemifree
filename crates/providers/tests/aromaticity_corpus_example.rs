//! The corpus example builds one document per record; records above one transaction's
//! command bound must keep their bonds attached to the right atoms.

use chemifree_domain::aromaticity::{self, Target};
use chemifree_domain::document::Document;
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::identity;
use chemifree_domain::rings;
use serde_json::json;

#[path = "../examples/aromaticity_corpus.rs"]
#[expect(dead_code, reason = "the example's main is not run by this test")]
mod example;

/// A V2000 ring of `n` carbons.
fn molblock_ring(n: usize) -> String {
    let mut text = format!("ring\n\n\n{n:3}{n:3}  0  0  0  0  0  0  0  0999 V2000\n");
    for i in 0..n {
        text.push_str(&format!(
            "{:10.4}{:10.4}{:10.4} C   0  0  0  0  0  0  0  0  0  0  0  0\n",
            i as f64, 0.0, 0.0
        ));
    }
    for i in 0..n {
        text.push_str(&format!("{:3}{:3}  1  0\n", i + 1, (i + 1) % n + 1));
    }
    text.push_str("M  END\n");
    text
}

#[test]
fn a_record_above_the_transaction_bound_keeps_its_bonds_on_the_right_atoms() {
    let entry = json!({
        "name": "large ring", "class": "test", "spelling": "kekule",
        "molblock": molblock_ring(400), "stated_hydrogens": {}, "aromatic_bonds": [],
    });
    let graph = example::graph(&entry).unwrap();
    assert_eq!(graph.atoms().len(), 400);
    let systems = rings::perceive(&graph);
    assert_eq!(systems.len(), 1, "one 400-membered ring");
    assert_eq!(systems[0].atoms.len(), 400);
}

fn state(document: &Document) -> MoleculeGraph {
    MoleculeGraph::from_document(document)
}

/// Systematic check over the whole corpus: whatever the Kekule spelling converts to must
/// classify like the original, convert back to the same atoms (stated hydrogens included)
/// and the same chemical identity, and never leave a document that cannot convert back.
#[test]
fn every_corpus_kekule_spelling_that_converts_converts_back() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/verification/aromaticity-corpus.json"
    );
    let corpus: Vec<serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let (mut converted, mut refused) = (0, 0);
    for entry in corpus.iter().filter(|e| e["spelling"] == "kekule") {
        let name = entry["name"].as_str().unwrap();
        let mut document = example::document(entry).unwrap();
        let original = state(&document);
        let Ok(forward) = aromaticity::conversion_commands(&original, Target::Aromatic) else {
            refused += 1;
            continue;
        };
        converted += 1;
        if !forward.is_empty() {
            document.apply(&forward).unwrap();
        }
        let aromatic = state(&document);
        let summary = |graph: &MoleculeGraph| -> Vec<_> {
            aromaticity::perceive(graph)
                .systems
                .iter()
                .map(|s| (s.classification, s.reason, s.electrons, s.atoms.len()))
                .collect()
        };
        assert_eq!(
            summary(&original),
            summary(&aromatic),
            "{name}: classification"
        );
        let backward = aromaticity::conversion_commands(&aromatic, Target::Kekule)
            .unwrap_or_else(|reason| panic!("{name}: cannot convert back: {reason}"));
        if !backward.is_empty() {
            document.apply(&backward).unwrap();
        }
        let restored = state(&document);
        let atoms = |graph: &MoleculeGraph| -> Vec<_> {
            graph.atoms().map(|(id, atom)| (id, atom.clone())).collect()
        };
        assert_eq!(atoms(&original), atoms(&restored), "{name}: atoms");
        assert_eq!(
            identity::canonicalize_chemical(&original).map(|c| c.code),
            identity::canonicalize_chemical(&restored).map(|c| c.code),
            "{name}: chemical identity"
        );
    }
    assert!(
        converted > 30,
        "the corpus should exercise conversions: {converted}"
    );
    assert!(refused > 10, "and refusals: {refused}");
}
