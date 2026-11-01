//! Classifies a corpus of MOL/V2000 records and prints one JSON line per record: the
//! aromaticity systems, the chemical identity (or its refusal) and the conversion outcomes.
//!
//! Usage: `aromaticity_corpus <corpus.json>`. The corpus is a JSON array of
//! `{ name, class, spelling, molblock, stated_hydrogens, aromatic_bonds }`. Only API shared by model
//! revisions 1 and 2 is used, so the same file runs against an older checkout and the
//! output of the two runs can be compared (`scripts/aromaticity_corpus_diff.py`).
use std::error::Error;
use std::fs;
use std::io;

use chemifree_application::ctfile::{CtCodec, CtFormat};
use chemifree_domain::aromaticity::{self, Classification};
use chemifree_domain::document::{
    Atom, AtomRef, BondOrder, Command, Document, DocumentId, NewAtomPositions, Point,
};
use chemifree_domain::element::Element;
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::identity;
use chemifree_domain::states::Hydrogens;
use chemifree_providers::V2000Codec;
use serde_json::{Value, json};

/// FNV-1a: a short, stable stand-in for the canonical code in the printed comparison.
fn digest(code: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in code.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn graph(entry: &Value) -> Result<MoleculeGraph, Box<dyn Error>> {
    let source = entry["molblock"]
        .as_str()
        .ok_or("molblock must be a string")?;
    let parsed = V2000Codec
        .read(CtFormat::Mol, source.as_bytes())
        .map_err(|error| io::Error::other(format!("{}: {}", error.code, error.message)))?;
    let record = parsed.value.first().ok_or("no record")?;
    let stated = entry["stated_hydrogens"]
        .as_object()
        .ok_or("stated_hydrogens must be an object")?;
    let mut commands: Vec<Command> = Vec::new();
    for (index, ct) in record.atoms.iter().enumerate() {
        let mut atom = Atom::from(Element::from_symbol(&ct.element).ok_or("unknown element")?);
        atom.charge = ct.charge;
        if let Some(count) = stated.get(&index.to_string()).and_then(Value::as_u64) {
            atom.hydrogens = Hydrogens::Specified(u8::try_from(count)?);
        }
        commands.push(Command::AddAtom {
            atom,
            positions: NewAtomPositions::Single(Point::new(0.0, 0.0)?),
            bond_to: None,
        });
    }
    let aromatic: Vec<u64> = entry["aromatic_bonds"]
        .as_array()
        .ok_or("aromatic_bonds must be an array")?
        .iter()
        .filter_map(Value::as_u64)
        .collect();
    for (index, bond) in record.bonds.iter().enumerate() {
        commands.push(Command::AddBond {
            begin: AtomRef::Created(bond.begin),
            end: AtomRef::Created(bond.end),
            order: match bond.order {
                _ if aromatic.contains(&(index as u64)) => BondOrder::Aromatic,
                1 => BondOrder::Single,
                2 => BondOrder::Double,
                3 => BondOrder::Triple,
                other => return Err(format!("unsupported bond type {other}").into()),
            },
        });
    }
    let mut document = Document::new(DocumentId::parse("corpus").ok_or("invalid id")?, "corpus")?;
    for chunk in commands.chunks(500) {
        document.apply(chunk)?;
    }
    Ok(MoleculeGraph::from_document(&document))
}

fn classify(graph: &MoleculeGraph) -> Value {
    let interpreted = aromaticity::perceive(graph);
    let mut systems: Vec<Value> = interpreted
        .systems
        .iter()
        .map(|system| {
            let label = match system.classification {
                Classification::Aromatic => "aromatic",
                Classification::NonAromatic => "non_aromatic",
                Classification::Unsupported => "unsupported",
            };
            json!({"atoms": system.atoms.len(), "classification": label, "reason": system.reason})
        })
        .collect();
    systems.sort_by_key(Value::to_string);
    let to_aromatic = interpreted.aromatize().err();
    let to_kekule = interpreted.kekulize().err();
    json!({"systems": systems, "to_aromatic_refused": to_aromatic, "to_kekule_refused": to_kekule})
}

fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args_os().nth(1).ok_or("expected a corpus path")?;
    let corpus: Vec<Value> = serde_json::from_str(&fs::read_to_string(path)?)?;
    for entry in corpus {
        let graph = graph(&entry)?;
        let identity = match identity::canonicalize_chemical(&graph) {
            Ok(result) => json!({"code": digest(&result.code)}),
            Err(error) => json!({"refused": error.code()}),
        };
        println!(
            "{}",
            json!({
                "name": entry["name"], "class": entry["class"], "spelling": entry["spelling"],
                "aromaticity": classify(&graph), "identity": identity,
            })
        );
    }
    Ok(())
}
