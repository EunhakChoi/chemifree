//! Small command-line bridge for the independent public RDKit comparison.

use chemifree_domain::smiles::{parse, write};
use chemifree_domain::states::Hydrogens;

fn main() {
    let Some(input) = std::env::args().nth(1) else {
        eprintln!("usage: smiles_reference SMILES");
        std::process::exit(2);
    };
    let graph = match parse(&input) {
        Ok(graph) => graph,
        Err(error) => {
            eprintln!("{}:{}..{}", error.code, error.start, error.end);
            std::process::exit(1);
        }
    };
    let order: Vec<_> = (0..graph.atoms.len()).collect();
    let output = match write(&graph, &order) {
        Ok(output) => output,
        Err(error) => {
            eprintln!("write:{error:?}");
            std::process::exit(1);
        }
    };
    println!("smiles\t{output}");
    for atom in &graph.atoms {
        let symbol = atom.atom.kind.element().expect("SMILES element").symbol();
        let hydrogens = match atom.atom.hydrogens {
            Hydrogens::Inferred => "inferred".to_owned(),
            Hydrogens::Specified(value) => value.to_string(),
            Hydrogens::Unknown => "unknown".to_owned(),
        };
        println!(
            "atom\t{symbol}\t{}\t{}\t{hydrogens}\t{}",
            atom.atom.isotope.unwrap_or(0),
            atom.atom.charge,
            atom.aromatic
        );
    }
    for bond in &graph.bonds {
        println!("bond\t{}\t{}\t{:?}", bond.begin, bond.end, bond.order);
    }
}
