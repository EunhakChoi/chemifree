//! Opt-in comparison with the official executable, using independently written V2000 inputs.
//!
//! Run with --ignored --nocapture and explicit CHEMIFREE_TEST_INCHI_LIBRARY,
//! CHEMIFREE_TEST_INCHI_SHA256, CHEMIFREE_TEST_INCHI_EXECUTABLE registrations. Without all
//! three, this test prints SKIP and performs zero native/reference executions. The official
//! executable and library must both be trusted builds of the pinned public 1.07.5 release.

use std::fmt::Write;
use std::path::PathBuf;
use std::process::Command;

use chemifree_inchi_native::{
    Atom, Bond, BondOrder, Hydrogens, Molecule, NativeInchi, Parity, Radical, Registration, Stereo,
    VERSION,
};

struct Fixture {
    name: &'static str,
    molecule: Molecule,
    positions: Vec<(f64, f64)>,
    /// V2000 stereo flag by bond index. Only the independent reference drawing uses these.
    marks: Vec<(usize, u8)>,
}

fn atom(element: &str) -> Atom {
    Atom {
        element: element.into(),
        isotope: None,
        charge: 0,
        radical: Radical::None,
        hydrogens: Hydrogens::Automatic,
    }
}
fn bond(begin: usize, end: usize, order: BondOrder) -> Bond {
    Bond { begin, end, order }
}

fn fixtures() -> Vec<Fixture> {
    let ethanol = Fixture {
        name: "ethanol",
        molecule: Molecule {
            atoms: vec![atom("C"), atom("C"), atom("O")],
            bonds: vec![bond(0, 1, BondOrder::Single), bond(1, 2, BondOrder::Single)],
            stereo: Vec::new(),
        },
        positions: vec![(0.0, 0.0), (1.0, 0.0), (2.0, 1.0)],
        marks: Vec::new(),
    };
    let mut acetate = Fixture {
        name: "isotopic-acetate",
        molecule: Molecule {
            atoms: vec![atom("C"), atom("C"), atom("O"), atom("O")],
            bonds: vec![
                bond(0, 1, BondOrder::Single),
                bond(1, 2, BondOrder::Double),
                bond(1, 3, BondOrder::Single),
            ],
            stereo: Vec::new(),
        },
        positions: vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (2.0, 0.0)],
        marks: Vec::new(),
    };
    acetate.molecule.atoms[0].isotope = Some(13);
    acetate.molecule.atoms[3].charge = -1;
    // The drawing is the hand-derived C/F/Cl/Br fan: a wedge toward Cl is S.
    // SDK's implicit-first order [center,F,Cl,Br] has Odd parity for this drawing.
    let tetra = |name, parity, flag| Fixture {
        name,
        molecule: Molecule {
            atoms: vec![atom("C"), atom("F"), atom("Cl"), atom("Br")],
            bonds: vec![
                bond(0, 1, BondOrder::Single),
                bond(0, 2, BondOrder::Single),
                bond(0, 3, BondOrder::Single),
            ],
            stereo: vec![Stereo::Tetrahedral {
                center: 0,
                neighbors: [0, 1, 2, 3],
                parity,
            }],
        },
        positions: vec![(0.0, 0.0), (0.0, 1.0), (1.0, -0.5), (-1.0, -0.5)],
        marks: vec![(1, flag)],
    };
    let alkene = |name, parity, last_y| Fixture {
        name,
        molecule: Molecule {
            atoms: vec![atom("C"), atom("C"), atom("F"), atom("Cl")],
            bonds: vec![
                bond(0, 1, BondOrder::Double),
                bond(0, 2, BondOrder::Single),
                bond(1, 3, BondOrder::Single),
            ],
            stereo: vec![Stereo::DoubleBond {
                neighbors: [2, 0, 1, 3],
                parity,
            }],
        },
        positions: vec![(0.0, 0.0), (1.0, 0.0), (-0.5, 1.0), (1.5, last_y)],
        marks: Vec::new(),
    };
    vec![
        ethanol,
        acetate,
        tetra("tetra-wedge", Parity::Odd, 1),
        tetra("tetra-hash", Parity::Even, 6),
        alkene("opposite-alkene", Parity::Even, -1.0),
        alkene("same-side-alkene", Parity::Odd, 1.0),
    ]
}

/// The reference executable reads an ordinary coordinate/mark molfile, not the FFI buffers.
fn molfile(fixture: &Fixture) -> String {
    let mut text = format!(
        "{}\n  Chemifree independent fixture\n\n{:3}{:3}  0  0  0  0  0  0  0  0999 V2000\n",
        fixture.name,
        fixture.molecule.atoms.len(),
        fixture.molecule.bonds.len()
    );
    for (atom, (x, y)) in fixture.molecule.atoms.iter().zip(&fixture.positions) {
        writeln!(
            text,
            "{x:10.4}{y:10.4}{:10.4} {:<3} 0  0  0  0  0  0  0  0  0  0  0  0",
            0.0, atom.element
        )
        .unwrap();
    }
    for (index, bond) in fixture.molecule.bonds.iter().enumerate() {
        let order = match bond.order {
            BondOrder::Single => 1,
            BondOrder::Double => 2,
            BondOrder::Triple => 3,
        };
        let mark = fixture
            .marks
            .iter()
            .find(|(candidate, _)| *candidate == index)
            .map_or(0, |(_, flag)| *flag);
        writeln!(
            text,
            "{:3}{:3}{order:3}{mark:3}  0  0  0",
            bond.begin + 1,
            bond.end + 1
        )
        .unwrap();
    }
    for (index, atom) in fixture.molecule.atoms.iter().enumerate() {
        if atom.charge != 0 {
            writeln!(text, "M  CHG  1{:4}{:4}", index + 1, atom.charge).unwrap();
        }
        if let Some(mass) = atom.isotope {
            writeln!(text, "M  ISO  1{:4}{mass:4}", index + 1).unwrap();
        }
    }
    text.push_str("M  END\n");
    text
}

#[test]
#[ignore = "requires explicit official InChI 1.07.5 library, SHA-256 and executable; run with --ignored --nocapture"]
fn registered_native_output_matches_official_executable_exactly() {
    let values: Option<Vec<String>> = [
        "CHEMIFREE_TEST_INCHI_LIBRARY",
        "CHEMIFREE_TEST_INCHI_SHA256",
        "CHEMIFREE_TEST_INCHI_EXECUTABLE",
    ]
    .iter()
    .map(|name| std::env::var(name).ok())
    .collect();
    let Some(values) = values else {
        eprintln!(
            "SKIP official InChI comparison: all three explicit registration environment variables are required; executed=0"
        );
        return;
    };
    let executable = PathBuf::from(&values[2]);
    assert!(
        executable.is_absolute(),
        "official executable path must be absolute"
    );
    let native = NativeInchi::load(Registration {
        library_path: PathBuf::from(&values[0]),
        version: VERSION.into(),
        sha256: values[1].clone(),
        timeout_seconds: 5,
    })
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let mut identifiers = Vec::new();
    let cases = fixtures();
    for fixture in &cases {
        let input = directory.path().join(format!("{}.mol", fixture.name));
        let output = directory.path().join(format!("{}.inchi", fixture.name));
        let log = directory.path().join(format!("{}.log", fixture.name));
        let problem = directory
            .path()
            .join(format!("{}.problem.mol", fixture.name));
        std::fs::write(&input, molfile(fixture)).unwrap();
        let execution = Command::new(&executable)
            .args([
                input.as_os_str(),
                output.as_os_str(),
                log.as_os_str(),
                problem.as_os_str(),
            ])
            .args(["-W5", "-Key"])
            .output()
            .unwrap();
        assert!(
            execution.status.success(),
            "{}: official executable failed: {}",
            fixture.name,
            String::from_utf8_lossy(&execution.stderr)
        );
        let reference = std::fs::read_to_string(&output).unwrap();
        let expected_inchi = reference
            .lines()
            .find(|line| line.starts_with("InChI=1S/"))
            .unwrap_or_else(|| {
                panic!(
                    "{}: official output had no Standard InChI: {reference}",
                    fixture.name
                )
            });
        let expected_key = reference
            .lines()
            .find_map(|line| line.strip_prefix("InChIKey="))
            .unwrap_or_else(|| {
                panic!(
                    "{}: official output had no InChIKey: {reference}",
                    fixture.name
                )
            });
        let result = native.generate(&fixture.molecule).unwrap();
        assert_eq!(
            result.inchi.as_bytes(),
            expected_inchi.as_bytes(),
            "{}: exact Standard InChI bytes",
            fixture.name
        );
        assert_eq!(
            result.inchikey.as_bytes(),
            expected_key.as_bytes(),
            "{}: exact Standard InChIKey bytes",
            fixture.name
        );
        identifiers.push(result.inchi);
        eprintln!("EXECUTED official/native comparison: {}", fixture.name);
    }
    assert_eq!(
        identifiers[0], "InChI=1S/C2H6O/c1-2-3/h3H,2H2,1H3",
        "independent ethanol reference"
    );
    assert_ne!(
        identifiers[2], identifiers[3],
        "opposite tetrahedral parities must remain distinct"
    );
    assert_ne!(
        identifiers[4], identifiers[5],
        "opposite double-bond parities must remain distinct"
    );
    eprintln!(
        "Official InChI {VERSION} comparison executed={} skipped=0",
        cases.len()
    );
}
