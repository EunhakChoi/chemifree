//! Optional, explicitly registered official InChI 1.07.5 native provider.
//!
//! No library is discovered from PATH. Registration authenticates bytes against the
//! administrator's expected SHA-256; it does not prove their provenance or detect a version.
//! Load only a trusted official build. The public SDK has no runtime version query.
//! Native `W` timeout is cooperative and cannot interrupt a hung C call or library loading.
//! This first adapter supports Linux, at most 1,000 atoms, ordinary single/double/triple
//! bonds and explicit 0D tetrahedral/double-bond stereo. It supplies zero coordinates,
//! so drawn marks are never interpreted as stored configurations.

use std::fmt;
use std::path::PathBuf;

use chemifree_domain::budget::{Interrupt, Interrupted};
use chemifree_domain::element::Element;

#[cfg(target_os = "linux")]
#[allow(
    unsafe_code,
    reason = "isolated official SDK ABI and dynamic-loader boundary"
)]
mod platform;

/// Supported registered SDK release, declared by the administrator rather than detected.
pub const VERSION: &str = "1.07.5";
/// Product bound below the SDK's signed 16-bit atom-index capacity.
pub const MAX_ATOMS: usize = 1_000;

/// Trusted-library registration. The digest must be 64 lowercase hexadecimal characters.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Registration {
    pub library_path: PathBuf,
    pub version: String,
    pub sha256: String,
    /// Native cooperative timeout, constrained to 1–60 seconds; zero is never allowed.
    pub timeout_seconds: u16,
}

/// A recoverable provider failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Error {
    pub kind: ErrorKind,
    pub message: String,
    pub native_code: Option<i32>,
}

/// Failure categories independent of transport or application-layer diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorKind {
    Registration,
    Unavailable,
    InvalidInput,
    Limit,
    Native,
    InvalidOutput,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}
impl std::error::Error for Error {}

fn error(kind: ErrorKind, message: impl Into<String>) -> Error {
    Error {
        kind,
        message: message.into(),
        native_code: None,
    }
}

/// A molecular snapshot in atom-index order, with no drawing coordinates or group semantics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Molecule {
    pub atoms: Vec<Atom>,
    pub bonds: Vec<Bond>,
    pub stereo: Vec<Stereo>,
}

/// An ordinary element atom. Isotopes are absolute mass numbers, not mass differences.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Atom {
    pub element: String,
    pub isotope: Option<u16>,
    pub charge: i8,
    pub radical: Radical,
    pub hydrogens: Hydrogens,
}

/// The public SDK radical enumeration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Radical {
    None,
    Singlet,
    Doublet,
    Triplet,
}

/// Automatic implicit H perception or an explicit non-isotopic implicit H count.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Hydrogens {
    Automatic,
    Count(u8),
}

/// A bond between two distinct zero-based atom indices.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Bond {
    pub begin: usize,
    pub end: usize,
    pub order: BondOrder,
}

/// The supported ordinary bond subset; aromatic or query bonds must be refused upstream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BondOrder {
    Single,
    Double,
    Triple,
}

/// Public SDK 0D parity. Clockwise tetrahedral order is Even, anticlockwise is Odd.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Parity {
    Odd,
    Even,
    Unknown,
}

/// Explicit 0D stereochemistry, following the pinned public inchi_api.h neighbor convention.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Stereo {
    /// Four actual neighbors, or `[center, X, Y, Z]` for a three-neighbor center.
    /// The latter convention can represent an implicit H or lone pair; the caller owns
    /// whether that fourth position is valid for its chemical model.
    Tetrahedral {
        center: usize,
        neighbors: [usize; 4],
        parity: Parity,
    },
    /// `[X, A, B, Y]` for the explicitly bonded path X–A=B–Y.
    /// Same-side references have Odd parity; opposite-side references have Even parity.
    DoubleBond {
        neighbors: [usize; 4],
        parity: Parity,
    },
}

/// Owned native output. Warnings and the native log are never silently discarded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Output {
    /// Unmodified GetStdINCHI return code (0 success, 1 warning).
    pub return_code: i32,
    pub inchi: String,
    pub inchikey: String,
    pub aux_info: Option<String>,
    pub message: Option<String>,
    pub log: Option<String>,
    pub warning: bool,
}

/// A loaded registered library. All calls across instances are serialized process-wide.
pub struct NativeInchi {
    registration: Registration,
    #[cfg(target_os = "linux")]
    library: platform::Library,
}

impl NativeInchi {
    /// Verifies the registration and loads its explicit absolute library path.
    ///
    /// # Errors
    /// Returns registration, file, digest, platform or symbol-loading errors. The declared
    /// version is checked against VERSION; no runtime version detection is claimed.
    pub fn load(registration: Registration) -> Result<Self, Error> {
        if registration.version != VERSION
            || !registration.library_path.is_absolute()
            || registration.sha256.len() != 64
            || !registration
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || !(1..=60).contains(&registration.timeout_seconds)
        {
            return Err(error(
                ErrorKind::Registration,
                "InChI requires an absolute path, registered version 1.07.5, lowercase SHA-256, and timeout 1–60 seconds.",
            ));
        }
        #[cfg(target_os = "linux")]
        {
            let library = platform::Library::load(&registration)?;
            Ok(Self {
                registration,
                library,
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(error(
                ErrorKind::Unavailable,
                "The registered native InChI adapter currently supports Linux only.",
            ))
        }
    }

    /// The exact metadata registered by the administrator, including the checked digest.
    #[must_use]
    pub fn registration(&self) -> &Registration {
        &self.registration
    }

    /// The sole effective option: a finite native cooperative timeout in seconds.
    #[must_use]
    pub fn options(&self) -> Vec<String> {
        vec![format!("-W{}", self.registration.timeout_seconds)]
    }

    /// Computes Standard InChI and its Standard InChIKey from explicit molecular input.
    ///
    /// `stop` is asked once the native serialization lock is held, right before the native
    /// call. It is never asked after that call begins, so a finished call is returned as is.
    ///
    /// # Errors
    /// The inner error refuses invalid indices, topology, unsupported element names, oversized
    /// inputs, poisoned serialization locks, native errors and nonstandard or malformed output.
    /// `Err(Interrupted)` means `stop` ended the call before the native library ran.
    pub fn generate(
        &self,
        molecule: &Molecule,
        stop: &dyn Interrupt,
    ) -> Result<Result<Output, Error>, Interrupted> {
        if let Err(refusal) = validate(molecule) {
            return Ok(Err(refusal));
        }
        #[cfg(target_os = "linux")]
        {
            self.library
                .generate(molecule, self.registration.timeout_seconds, stop)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = stop;
            Ok(Err(error(
                ErrorKind::Unavailable,
                "Native InChI is unavailable on this platform.",
            )))
        }
    }
}

fn validate(molecule: &Molecule) -> Result<(), Error> {
    use std::collections::BTreeSet;
    if molecule.atoms.is_empty() {
        return Err(error(
            ErrorKind::InvalidInput,
            "InChI requires a nonempty molecule.",
        ));
    }
    if molecule.atoms.len() > MAX_ATOMS
        || molecule.stereo.len() > MAX_ATOMS
        || molecule.bonds.len() > MAX_ATOMS * 10
    {
        return Err(error(
            ErrorKind::Limit,
            "Native InChI accepts at most 1,000 atoms, 1,000 stereo elements and 10,000 bonds.",
        ));
    }
    for atom in &molecule.atoms {
        if Element::from_symbol(&atom.element).is_none()
            || atom.isotope.is_some_and(|mass| mass == 0 || mass >= 10_000)
            || matches!(atom.hydrogens, Hydrogens::Count(count) if count > i8::MAX as u8)
        {
            return Err(error(
                ErrorKind::InvalidInput,
                "An atom has an invalid element, isotope or hydrogen count.",
            ));
        }
    }
    let mut edges = BTreeSet::new();
    let mut neighbors = vec![BTreeSet::new(); molecule.atoms.len()];
    let mut doubles = BTreeSet::new();
    for bond in &molecule.bonds {
        if bond.begin >= molecule.atoms.len()
            || bond.end >= molecule.atoms.len()
            || bond.begin == bond.end
        {
            return Err(error(
                ErrorKind::InvalidInput,
                "A bond has invalid atom indices.",
            ));
        }
        let edge = (bond.begin.min(bond.end), bond.begin.max(bond.end));
        if !edges.insert(edge) {
            return Err(error(
                ErrorKind::InvalidInput,
                "Duplicate bonds are not accepted.",
            ));
        }
        neighbors[bond.begin].insert(bond.end);
        neighbors[bond.end].insert(bond.begin);
        if bond.order == BondOrder::Double {
            doubles.insert(edge);
        }
    }
    if neighbors.iter().any(|adjacent| adjacent.len() > 20) {
        return Err(error(
            ErrorKind::Limit,
            "The SDK allows at most 20 neighbors per atom.",
        ));
    }
    let mut centers = BTreeSet::new();
    let mut stereo_bonds = BTreeSet::new();
    for stereo in &molecule.stereo {
        let slots = match stereo {
            Stereo::Tetrahedral { neighbors, .. } | Stereo::DoubleBond { neighbors, .. } => {
                neighbors
            }
        };
        if slots.iter().any(|index| *index >= molecule.atoms.len())
            || slots.iter().collect::<BTreeSet<_>>().len() != 4
        {
            return Err(error(
                ErrorKind::InvalidInput,
                "Stereo neighbors must be four distinct valid atom indices.",
            ));
        }
        match stereo {
            Stereo::Tetrahedral {
                center,
                neighbors: slots,
                ..
            } => {
                if *center >= molecule.atoms.len() || !centers.insert(*center) {
                    return Err(error(
                        ErrorKind::InvalidInput,
                        "Invalid or duplicate tetrahedral center.",
                    ));
                }
                let adjacent: BTreeSet<usize> = slots
                    .iter()
                    .copied()
                    .filter(|index| index != center)
                    .collect();
                if adjacent != neighbors[*center] || (slots.contains(center) && slots[0] != *center)
                {
                    return Err(error(
                        ErrorKind::InvalidInput,
                        "Tetrahedral slots must match actual neighbors, with an implicit fourth position encoded first as the center.",
                    ));
                }
            }
            Stereo::DoubleBond {
                neighbors: [x, a, b, y],
                ..
            } => {
                let edge = ((*a).min(*b), (*a).max(*b));
                if !doubles.contains(&edge)
                    || !neighbors[*a].contains(x)
                    || !neighbors[*b].contains(y)
                    || !stereo_bonds.insert(edge)
                {
                    return Err(error(
                        ErrorKind::InvalidInput,
                        "Double-bond stereo must describe one unique X–A=B–Y path.",
                    ));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn carbon() -> Atom {
        Atom {
            element: "C".into(),
            isotope: None,
            charge: 0,
            radical: Radical::None,
            hydrogens: Hydrogens::Automatic,
        }
    }
    fn molecule() -> Molecule {
        Molecule {
            atoms: vec![carbon()],
            bonds: Vec::new(),
            stereo: Vec::new(),
        }
    }
    fn registration(path: PathBuf) -> Registration {
        Registration {
            library_path: path,
            version: VERSION.into(),
            sha256: "0".repeat(64),
            timeout_seconds: 5,
        }
    }
    fn load_error(registration: Registration) -> Error {
        match NativeInchi::load(registration) {
            Ok(_) => panic!("invalid registration unexpectedly loaded"),
            Err(cause) => cause,
        }
    }

    #[test]
    fn registration_rejects_relative_paths_versions_digest_syntax_and_unlimited_timeout() {
        let directory = tempfile::tempdir().unwrap();
        let base = registration(directory.path().join("missing.so"));
        let mut cases = Vec::new();
        let mut bad = base.clone();
        bad.library_path = "libinchi.so".into();
        cases.push(bad);
        let mut bad = base.clone();
        bad.version = "1.07.4".into();
        cases.push(bad);
        let mut bad = base.clone();
        bad.sha256 = "A".repeat(64);
        cases.push(bad);
        let mut bad = base.clone();
        bad.sha256 = "g".repeat(64);
        cases.push(bad);
        let mut bad = base.clone();
        bad.sha256 = "0".repeat(63);
        cases.push(bad);
        for timeout in [0, 61, u16::MAX] {
            let mut bad = base.clone();
            bad.timeout_seconds = timeout;
            cases.push(bad);
        }
        for case in cases {
            assert_eq!(
                load_error(case.clone()).kind,
                ErrorKind::Registration,
                "{case:?}"
            );
        }
    }

    #[test]
    fn absent_registered_library_is_honestly_unavailable() {
        let directory = tempfile::tempdir().unwrap();
        assert_eq!(
            load_error(registration(directory.path().join("missing.so"))).kind,
            ErrorKind::Unavailable
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn fifo_registration_refuses_without_waiting_for_a_writer() {
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};

        const PROBE_PATH: &str = "CHEMIFREE_TEST_INCHI_FIFO_PROBE_PATH";
        // Run the actual load in a child: a regression to blocking open cannot hang
        // the main test process. No process-global environment is mutated.
        if let Some(path) = std::env::var_os(PROBE_PATH) {
            let failure = load_error(registration(PathBuf::from(path)));
            assert_eq!(failure.kind, ErrorKind::Registration);
            assert_eq!(
                failure.message,
                "The registered InChI path must be a regular file."
            );
            return;
        }
        let directory = tempfile::tempdir().unwrap();
        let fifo = directory.path().join("registered-library.fifo");
        rustix::fs::mkfifoat(
            rustix::fs::CWD,
            &fifo,
            rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
        )
        .unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tests::fifo_registration_refuses_without_waiting_for_a_writer",
                "--nocapture",
            ])
            .env(PROBE_PATH, &fifo)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let output = child.wait_with_output().unwrap();
                    assert!(
                        status.success(),
                        "FIFO probe failed: {} {}",
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    );
                    break;
                }
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                other => {
                    child.kill().unwrap();
                    let output = child.wait_with_output().unwrap();
                    panic!(
                        "FIFO registration did not finish within its bounded subprocess deadline ({other:?}): {} {}",
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn digest_mismatch_precedes_loading_and_matching_nonlibrary_bytes_do_not_load() {
        use sha2::{Digest, Sha256};
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("not-a-library.so");
        std::fs::write(&path, b"not a native library").unwrap();
        let mut candidate = registration(path);
        assert_eq!(load_error(candidate.clone()).kind, ErrorKind::Registration);
        candidate.sha256 = format!("{:x}", Sha256::digest(b"not a native library"));
        assert_eq!(load_error(candidate).kind, ErrorKind::Unavailable);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn oversized_registered_file_refuses_before_reading_its_contents() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("oversized.so");
        std::fs::File::create(&path)
            .unwrap()
            .set_len(128 * 1024 * 1024 + 1)
            .unwrap();
        assert_eq!(load_error(registration(path)).kind, ErrorKind::Limit);
    }

    #[test]
    fn validation_accepts_supported_graph_and_refuses_atom_field_boundaries() {
        assert_eq!(validate(&molecule()), Ok(()));
        for atom in [
            Atom {
                element: "Zz".into(),
                ..carbon()
            },
            Atom {
                isotope: Some(0),
                ..carbon()
            },
            // 10000 is the SDK's isotope-shift flag, so cannot encode absolute mass 10000.
            Atom {
                isotope: Some(10_000),
                ..carbon()
            },
            Atom {
                hydrogens: Hydrogens::Count(128),
                ..carbon()
            },
        ] {
            let invalid = Molecule {
                atoms: vec![atom],
                ..molecule()
            };
            assert_eq!(
                validate(&invalid).unwrap_err().kind,
                ErrorKind::InvalidInput,
                "{invalid:?}"
            );
        }
        assert_eq!(
            validate(&Molecule {
                atoms: Vec::new(),
                ..molecule()
            })
            .unwrap_err()
            .kind,
            ErrorKind::InvalidInput
        );
        assert_eq!(
            validate(&Molecule {
                atoms: vec![carbon(); MAX_ATOMS + 1],
                ..molecule()
            })
            .unwrap_err()
            .kind,
            ErrorKind::Limit
        );
    }

    #[test]
    fn validation_refuses_self_out_of_range_duplicate_bonds_and_excess_neighbors() {
        for bonds in [
            vec![Bond {
                begin: 0,
                end: 0,
                order: BondOrder::Single,
            }],
            vec![Bond {
                begin: 0,
                end: 2,
                order: BondOrder::Single,
            }],
            vec![
                Bond {
                    begin: 0,
                    end: 1,
                    order: BondOrder::Single,
                },
                Bond {
                    begin: 1,
                    end: 0,
                    order: BondOrder::Double,
                },
            ],
        ] {
            let invalid = Molecule {
                atoms: vec![carbon(); 2],
                bonds,
                stereo: Vec::new(),
            };
            assert_eq!(
                validate(&invalid).unwrap_err().kind,
                ErrorKind::InvalidInput,
                "{invalid:?}"
            );
        }
        let mut crowded = Molecule {
            atoms: vec![carbon(); 22],
            bonds: (1..21)
                .map(|end| Bond {
                    begin: 0,
                    end,
                    order: BondOrder::Single,
                })
                .collect(),
            stereo: Vec::new(),
        };
        assert_eq!(validate(&crowded), Ok(()));
        crowded.bonds.push(Bond {
            begin: 0,
            end: 21,
            order: BondOrder::Single,
        });
        assert_eq!(validate(&crowded).unwrap_err().kind, ErrorKind::Limit);
    }

    #[test]
    fn tetrahedral_validation_checks_real_neighbors_and_first_implicit_slot() {
        let mut graph = Molecule {
            atoms: vec![carbon(); 4],
            bonds: (1..4)
                .map(|end| Bond {
                    begin: 0,
                    end,
                    order: BondOrder::Single,
                })
                .collect(),
            stereo: vec![Stereo::Tetrahedral {
                center: 0,
                neighbors: [0, 1, 2, 3],
                parity: Parity::Even,
            }],
        };
        assert_eq!(validate(&graph), Ok(()));
        for neighbors in [[1, 0, 2, 3], [0, 1, 1, 3], [0, 1, 2, 4]] {
            graph.stereo = vec![Stereo::Tetrahedral {
                center: 0,
                neighbors,
                parity: Parity::Even,
            }];
            assert_eq!(
                validate(&graph).unwrap_err().kind,
                ErrorKind::InvalidInput,
                "{neighbors:?}"
            );
        }
        graph.stereo = vec![
            Stereo::Tetrahedral {
                center: 0,
                neighbors: [0, 1, 2, 3],
                parity: Parity::Odd
            };
            2
        ];
        assert_eq!(validate(&graph).unwrap_err().kind, ErrorKind::InvalidInput);
    }

    #[test]
    fn double_bond_validation_requires_the_actual_unique_reference_path() {
        let mut graph = Molecule {
            atoms: vec![carbon(); 4],
            bonds: vec![
                Bond {
                    begin: 0,
                    end: 1,
                    order: BondOrder::Single,
                },
                Bond {
                    begin: 1,
                    end: 2,
                    order: BondOrder::Double,
                },
                Bond {
                    begin: 2,
                    end: 3,
                    order: BondOrder::Single,
                },
            ],
            stereo: vec![Stereo::DoubleBond {
                neighbors: [0, 1, 2, 3],
                parity: Parity::Even,
            }],
        };
        assert_eq!(validate(&graph), Ok(()));
        graph.stereo.push(graph.stereo[0].clone());
        assert_eq!(validate(&graph).unwrap_err().kind, ErrorKind::InvalidInput);
        graph.stereo.truncate(1);
        graph.bonds[1].order = BondOrder::Single;
        assert_eq!(validate(&graph).unwrap_err().kind, ErrorKind::InvalidInput);
    }
}
