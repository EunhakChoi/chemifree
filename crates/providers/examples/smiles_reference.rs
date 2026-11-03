//! Small executable boundary for an independent SMILES comparison; no reference code is linked.
//!
//! `smiles_reference read <text>` prints the atoms and bonds the reader finds as JSON, or the
//! refusal. `smiles_reference write <recorded|canonical> <text>` reads the text, writes it and
//! prints the result, or the refusal and whether it came from the reader or the writer.
use std::process::ExitCode;

use chemifree_application::smiles::{SmilesCodec, SmilesStyle};
use chemifree_domain::document::{AtomId, Bond, BondId, BondOrder};
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::states::{Hydrogens, Radical};
use chemifree_providers::OpenSmilesCodec;
use serde_json::{Value, json};

fn refusal(phase: &str, error: &chemifree_application::smiles::SmilesError) -> Value {
    json!({
        "outcome": "refused",
        "phase": phase,
        "code": error.code.as_str(),
        "class": error.code.class().as_str(),
        "offset": error.span.map(|span| span.offset),
        "length": error.span.map(|span| span.length),
    })
}

fn order(order: &BondOrder) -> &'static str {
    match order {
        BondOrder::Single => "single",
        BondOrder::Double => "double",
        BondOrder::Triple => "triple",
        BondOrder::Aromatic => "aromatic",
        _ => "other",
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let codec = OpenSmilesCodec;
    let (mode, rest) = match args.split_first() {
        Some((mode, rest)) => (mode.as_str(), rest),
        None => {
            eprintln!("expected read or write");
            return ExitCode::from(2);
        }
    };
    let (style, text) = match (mode, rest) {
        ("read", [text]) => (None, text),
        ("write", [style, text]) => (
            Some(match style.as_str() {
                "recorded" => SmilesStyle::Recorded,
                "canonical" => SmilesStyle::Canonical,
                _ => {
                    eprintln!("unknown style {style}");
                    return ExitCode::from(2);
                }
            }),
            text,
        ),
        _ => {
            eprintln!("usage: smiles_reference read TEXT | write STYLE TEXT");
            return ExitCode::from(2);
        }
    };
    let read = match codec.read(text) {
        Ok(read) => read,
        Err(error) => {
            println!("{}", refusal("read", &error));
            return ExitCode::SUCCESS;
        }
    };
    let Some(style) = style else {
        let atoms: Vec<Value> = read
            .atoms
            .iter()
            .map(|entry| {
                let atom = &entry.atom;
                json!({
                    "element": atom.kind.element().map(|e| e.symbol()),
                    "charge": atom.charge,
                    "isotope": atom.isotope,
                    "hydrogens": match atom.hydrogens {
                        Hydrogens::Specified(count) => json!(count),
                        _ => Value::Null,
                    },
                    "radical": match atom.radical {
                        Radical::None => "none",
                        Radical::Doublet => "doublet",
                        Radical::Unknown => "unknown",
                        _ => "other",
                    },
                })
            })
            .collect();
        let bonds: Vec<Value> = read
            .bonds
            .iter()
            .map(|bond| json!([bond.begin, bond.end, order(&bond.order)]))
            .collect();
        println!(
            "{}",
            json!({"outcome": "read", "atoms": atoms, "bonds": bonds})
        );
        return ExitCode::SUCCESS;
    };
    let id = |index: usize| {
        AtomId::from_serial(u32::try_from(index + 1).expect("small")).expect("positive")
    };
    let graph = MoleculeGraph::from_parts(
        read.atoms
            .iter()
            .enumerate()
            .map(|(i, a)| (id(i), a.atom.clone()))
            .collect(),
        read.bonds
            .iter()
            .enumerate()
            .map(|(i, b)| {
                (
                    BondId::from_serial(u32::try_from(i + 1).expect("small")).expect("positive"),
                    Bond::new(id(b.begin), id(b.end), b.order.clone()),
                )
            })
            .collect(),
    );
    match codec.write(&graph, style) {
        Ok(written) => println!("{}", json!({"outcome": "text", "text": written.text})),
        Err(error) => println!("{}", refusal("write", &error)),
    }
    ExitCode::SUCCESS
}
