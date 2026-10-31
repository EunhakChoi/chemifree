//! Small executable boundary for an independent reader comparison; no reference code is linked.
use std::error::Error;
use std::fs;
use std::io;
use std::path::Path;

use chemifree_application::ctfile::{
    CtAtom, CtBond, CtCodec, CtField, CtFormat, CtRadical, CtRecord,
};
use chemifree_providers::V2000Codec;

fn atom(element: &str, x: f64) -> CtAtom {
    CtAtom {
        element: element.into(),
        x,
        y: 0.0,
        z: 0.0,
        charge: 0,
        isotope: None,
        radical: CtRadical::None,
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1);
    let mode = args.next().ok_or("expected write/read mode")?;
    let path = args.next().ok_or("expected a path")?;
    if mode == "write" {
        let mut carbon = atom("C", 0.0);
        carbon.isotope = Some(13);
        let mut oxygen = atom("O", 1.25);
        oxygen.charge = -1;
        let molecule = CtRecord {
            title: "Independent fixture".into(),
            program: String::new(),
            comment: String::new(),
            atoms: vec![carbon, oxygen],
            bonds: vec![CtBond {
                begin: 0,
                end: 1,
                order: 1,
            }],
            fields: vec![],
        };
        let mol = V2000Codec
            .write(CtFormat::Mol, std::slice::from_ref(&molecule))
            .map_err(|error| io::Error::other(format!("{}: {}", error.code, error.message)))?;
        fs::write(Path::new(&path).join("molecule.mol"), mol.value)?;
        let mut second = molecule.clone();
        second.title = "Second".into();
        second.fields.push(CtField {
            name: "Label".into(),
            value: "independent reader".into(),
        });
        let sdf = V2000Codec
            .write(CtFormat::Sdf, &[molecule, second])
            .map_err(|error| io::Error::other(format!("{}: {}", error.code, error.message)))?;
        fs::write(Path::new(&path).join("records.sdf"), sdf.value)?;
    } else if mode == "read" {
        let bytes = fs::read(path)?;
        let parsed = V2000Codec
            .read(CtFormat::Mol, &bytes)
            .map_err(|error| io::Error::other(format!("{}: {}", error.code, error.message)))?;
        println!(
            "{}",
            serde_json::to_string(
                &serde_json::json!({"records":parsed.value,"losses":parsed.losses})
            )?
        );
    } else {
        return Err("unknown mode".into());
    }
    Ok(())
}
