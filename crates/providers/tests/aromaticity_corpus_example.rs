//! The corpus example builds one document per record; records above one transaction's
//! command bound must keep their bonds attached to the right atoms.

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
