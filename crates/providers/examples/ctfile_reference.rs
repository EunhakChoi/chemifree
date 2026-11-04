//! Small executable boundary for an independent reader comparison; no reference code is linked.
use std::error::Error;
use std::fs;
use std::io;
use std::path::Path;

use chemifree_application::Application;
use chemifree_application::ctfile::{
    CtAtom, CtBond, CtBondStereo, CtCodec, CtField, CtFormat, CtRadical, CtRecord, CtTab,
};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, V2000Codec};

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
                stereo: CtBondStereo::None,
            }],
            fields: vec![],
            ctab: CtTab::V2000,
            chiral: false,
            collections: vec![],
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
    } else if mode == "write-v3000" {
        let mut carbon = atom("C", 1.0e15);
        carbon.isotope = Some(13);
        let mut oxygen = atom("O", 1.25);
        oxygen.charge = -1;
        oxygen.radical = CtRadical::Doublet;
        let mut nitrogen = atom("N", 2.5);
        nitrogen.y = 1.0e15;
        nitrogen.z = -1.0e15;
        nitrogen.charge = 1;
        nitrogen.isotope = Some(15);
        let molecule = CtRecord {
            title: "Independent V3000 fixture".into(),
            program: String::new(),
            comment: String::new(),
            atoms: vec![carbon, oxygen, nitrogen],
            bonds: vec![
                CtBond {
                    begin: 0,
                    end: 1,
                    order: 1,
                    stereo: CtBondStereo::None,
                },
                CtBond {
                    begin: 1,
                    end: 2,
                    order: 4,
                    stereo: CtBondStereo::None,
                },
            ],
            fields: vec![],
            ctab: CtTab::V3000,
            chiral: false,
            collections: vec![],
        };
        let text = V2000Codec
            .write_ctab(CtFormat::Mol, CtTab::V3000, &[molecule])
            .map_err(|error| io::Error::other(format!("{}: {}", error.code, error.message)))?;
        fs::write(Path::new(&path).join("molecule-v3000.mol"), text.value)?;
    } else if mode == "read-v3000" {
        let bytes = fs::read(path)?;
        let parsed = V2000Codec
            .read_ctab(CtFormat::Mol, &bytes)
            .map_err(|error| io::Error::other(format!("{}: {}", error.code, error.message)))?;
        println!(
            "{}",
            serde_json::to_string(
                &serde_json::json!({"records":parsed.value,"losses":parsed.losses})
            )?
        );
    } else if mode == "write-marks" {
        // Drawings built through the operations and exported by them, so the narrow-end
        // swap and the projection are the ones the service uses: a center with a wedge, the
        // same center with the wedge narrow at the bond's end, and a chain with either marks.
        let directory = tempfile::tempdir()?;
        let app = Application::new(
            LocalCapabilities,
            FileDocumentRepository::new(directory.path()),
        )
        .with_ctfile_codec(V2000Codec);
        let run = |request: serde_json::Value| -> Result<serde_json::Value, Box<dyn Error>> {
            Ok(serde_json::to_value(
                app.execute(&serde_json::to_vec(&request)?),
            )?)
        };
        let atom_at = |x: f64, y: f64, element: &str, to: Option<&str>| {
            let mut command =
                serde_json::json!({"op":"add_atom","element":element,"position":{"x":x,"y":y}});
            if let Some(to) = to {
                command["bond_to"] = serde_json::json!({"atom":to,"order":"single"});
            }
            command
        };
        let drawings = [
            (
                "center",
                vec![
                    atom_at(100.0, 100.0, "C", None),
                    atom_at(100.0, 70.0, "F", Some("$0")),
                    atom_at(126.0, 115.0, "Cl", Some("$0")),
                    atom_at(74.0, 115.0, "Br", Some("$0")),
                    serde_json::json!({"op":"set_bond_mark","bond":"$2","mark":{"kind":"wedge","narrow":"begin"}}),
                ],
            ),
            (
                "center-end",
                vec![
                    atom_at(100.0, 70.0, "F", None),
                    atom_at(100.0, 100.0, "C", Some("$0")),
                    atom_at(126.0, 115.0, "Cl", Some("$1")),
                    atom_at(74.0, 115.0, "Br", Some("$1")),
                    serde_json::json!({"op":"set_bond_mark","bond":"$1","mark":{"kind":"wedge","narrow":"end"}}),
                ],
            ),
            (
                "chain",
                vec![
                    atom_at(0.0, 0.0, "C", None),
                    atom_at(30.0, 0.0, "C", Some("$0")),
                    atom_at(60.0, 0.0, "C", Some("$1")),
                    serde_json::json!({"op":"add_atom","element":"C","position":{"x":60,"y":30},"bond_to":{"atom":"$2","order":"double"}}),
                    serde_json::json!({"op":"set_bond_mark","bond":"$1","mark":{"kind":"hash","narrow":"end"}}),
                    serde_json::json!({"op":"set_bond_mark","bond":"$2","mark":{"kind":"wavy"}}),
                    serde_json::json!({"op":"set_bond_mark","bond":"$3","mark":{"kind":"crossed"}}),
                ],
            ),
        ];
        for (name, commands) in drawings {
            let created = run(
                serde_json::json!({"contract":1,"request_id":format!("c-{name}"),"profile":"preserve-semantics","operation":"document.create","parameters":{"title":name}}),
            )?;
            let id = created["result"]["document"]["id"]
                .as_str()
                .ok_or("no id")?
                .to_owned();
            let edit = run(
                serde_json::json!({"contract":1,"request_id":format!("t-{name}"),"profile":"preserve-semantics","operation":"document.transaction","parameters":{"document_id":id,"expected_revision":0,"commands":commands}}),
            )?;
            if edit["outcome"] != "ok" {
                return Err(format!("edit failed: {edit}").into());
            }
            for ctab in ["v2000", "v3000"] {
                let written = run(
                    serde_json::json!({"contract":1,"request_id":format!("e-{name}-{ctab}"),"profile":"preserve-semantics","operation":"document.export","parameters":{"format":"mol","ctab":ctab,"records":[{"document_id":id}]}}),
                )?;
                let text = written["result"]["content"]
                    .as_str()
                    .ok_or(format!("no content: {written}"))?;
                fs::write(Path::new(&path).join(format!("{name}-{ctab}.mol")), text)?;
            }
        }
    } else if mode == "write-groups" {
        // Four wedge centers (atoms 1, 5, 9 and 13) with one group each, exported as V3000
        // through the operations: absolute, and -128, and (omitted number), or 258.
        let directory = tempfile::tempdir()?;
        let app = Application::new(
            LocalCapabilities,
            FileDocumentRepository::new(directory.path()),
        )
        .with_ctfile_codec(V2000Codec);
        let run = |request: serde_json::Value| -> Result<serde_json::Value, Box<dyn Error>> {
            Ok(serde_json::to_value(
                app.execute(&serde_json::to_vec(&request)?),
            )?)
        };
        let mut commands = Vec::new();
        for block in 0..4_usize {
            let x = 100.0 + 100.0 * block as f64;
            commands.push(
                serde_json::json!({"op":"add_atom","element":"C","position":{"x":x,"y":100.0}}),
            );
            for (dx, dy, element) in [(0.0, -30.0, "F"), (26.0, 15.0, "Cl"), (-26.0, 15.0, "Br")] {
                commands.push(serde_json::json!({"op":"add_atom","element":element,
                    "position":{"x":x + dx,"y":100.0 + dy},
                    "bond_to":{"atom":format!("${}", 4 * block),"order":"single"}}));
            }
        }
        for block in 0..4_usize {
            commands.push(serde_json::json!({"op":"set_bond_mark","bond":format!("${}", 4 * block + 2),"mark":{"kind":"wedge","narrow":"begin"}}));
        }
        commands.push(serde_json::json!({"op":"set_configuration_from_drawing","atoms":["$0","$4","$8","$12"]}));
        commands.push(serde_json::json!({"op":"create_stereo_group","kind":"absolute","atoms":["a1"],"number":null}));
        commands.push(serde_json::json!({"op":"create_stereo_group","kind":"and","atoms":["a5"],"number":-128}));
        commands.push(serde_json::json!({"op":"create_stereo_group","kind":"and","atoms":["a9"],"number":null}));
        commands.push(serde_json::json!({"op":"create_stereo_group","kind":"or","atoms":["a13"],"number":258}));
        let created = run(
            serde_json::json!({"contract":1,"request_id":"c-groups","profile":"preserve-semantics","operation":"document.create","parameters":{"title":"groups"}}),
        )?;
        let id = created["result"]["document"]["id"]
            .as_str()
            .ok_or("no id")?
            .to_owned();
        let edit = run(
            serde_json::json!({"contract":1,"request_id":"t-groups","profile":"preserve-semantics","operation":"document.transaction","parameters":{"document_id":id,"expected_revision":0,"commands":commands}}),
        )?;
        if edit["outcome"] != "ok" {
            return Err(format!("edit failed: {edit}").into());
        }
        let written = run(
            serde_json::json!({"contract":1,"request_id":"e-groups","profile":"preserve-semantics","operation":"document.export","parameters":{"format":"mol","ctab":"v3000","records":[{"document_id":id}]}}),
        )?;
        let text = written["result"]["content"]
            .as_str()
            .ok_or(format!("no content: {written}"))?;
        fs::write(Path::new(&path).join("groups-v3000.mol"), text)?;
    } else if mode == "preview" {
        let source = fs::read_to_string(path)?;
        let directory = tempfile::tempdir()?;
        let app = Application::new(
            LocalCapabilities,
            FileDocumentRepository::new(directory.path()),
        )
        .with_ctfile_codec(V2000Codec);
        let id = app
            .create_document(None, Some("Reference target"))?
            .document
            .id;
        let request = serde_json::json!({"contract":1,"request_id":"reference-preview","profile":"preserve-semantics","operation":"document.import","parameters":{"format":"mol","source":source,"record":1,"document_id":id,"expected_revision":0,"preview":true}});
        println!(
            "{}",
            serde_json::to_string(&app.execute(&serde_json::to_vec(&request)?))?
        );
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
