//! Catalog and capability wording for the connected file and text formats. Each connected
//! codec contributes its own fragment, and the earlier MOL/SDF and CDXML wording is kept
//! as it was when SMILES is absent.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

/// Which format providers are connected.
#[derive(Clone, Copy)]
pub(crate) struct Formats {
    pub ctfile: bool,
    pub interchange: bool,
    pub smiles: bool,
}

/// Wording is `'static` in the catalog types; each distinct text is stored once.
fn intern(text: String) -> &'static str {
    static TEXTS: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    let mut texts = TEXTS
        .get_or_init(Mutex::default)
        .lock()
        .expect("the text table is never held across a panic");
    if let Some(existing) = texts.get(text.as_str()) {
        return existing;
    }
    let leaked: &'static str = Box::leak(text.into_boxed_str());
    texts.insert(leaked);
    leaked
}

fn join(base: Option<&'static str>, fragment: Option<&'static str>) -> Option<&'static str> {
    match (base, fragment) {
        (Some(base), Some(fragment)) => Some(intern(format!("{base} {fragment}"))),
        (base, fragment) => base.or(fragment),
    }
}

impl Formats {
    fn files(self) -> (bool, bool) {
        (self.ctfile, self.interchange)
    }

    /// The label of the `format_conversion` capability, e.g. "MOL/SDF and CDXML subsets".
    pub(crate) fn label(self) -> Option<&'static str> {
        let mut names = Vec::new();
        if self.ctfile {
            names.push("MOL/SDF");
        }
        if self.interchange {
            names.push("CDXML");
        }
        if self.smiles {
            names.push("SMILES");
        }
        let text = match names.as_slice() {
            [] => return None,
            [only] => format!("{only} subset"),
            [head @ .., last] => format!("{} and {last} subsets", head.join(", ")),
        };
        Some(intern(text))
    }

    pub(crate) fn description(self) -> Option<&'static str> {
        let files = match self.files() {
            (true, true) => Some(
                "Read and write bounded V2000 MOL/SDF and rich CDXML subsets with explicit losses. Advanced chemistry is limited; other formats are unsupported.",
            ),
            (true, false) => Some(
                "Read and write a bounded V2000 MOL/SDF subset with explicit loss reports. Other formats, stereo and advanced objects remain unsupported.",
            ),
            (false, true) => Some(
                "Read and write a bounded CDXML subset and preview editor projection with explicit losses. Other formats and advanced objects remain unsupported.",
            ),
            (false, false) => None,
        };
        let smiles = self.smiles.then_some(if files.is_some() {
            "SMILES connectivity is also read and written; it has no stereo, query or reaction support and no InChI."
        } else {
            "Read and write SMILES connectivity (branches, rings, components, isotopes, charges, hydrogens, aromatic atoms) with explicit losses; canonical output is Chemifree's own. No stereo, query or reaction SMILES and no InChI. Other formats are unsupported."
        });
        join(files, smiles)
    }

    pub(crate) fn import_note(self) -> Option<&'static str> {
        let files = match self.files() {
            (true, true) => Some(
                "Preview and import bounded MOL/SDF records or rich CDXML with explicit losses.",
            ),
            (true, false) => Some(
                "Preview and append one selected record of a bounded V2000 MOL/SDF subset, with explicit losses and revision-checked consent.",
            ),
            (false, true) => {
                Some("Read CDXML as a rich tree or preview a loss-accounted editor projection.")
            }
            (false, false) => None,
        };
        let smiles = self.smiles.then_some("SMILES text is previewed and appended after loss consent; it reads connectivity only, and stereo, query and reaction SMILES are refused with a position.");
        join(files, smiles)
    }

    pub(crate) fn export_note(self) -> Option<&'static str> {
        let files = match self.files() {
            (true, true) => Some(
                "Write bounded MOL/SDF or rich CDXML with explicit losses and opaque-content safety.",
            ),
            (true, false) => Some(
                "Write a bounded V2000 MOL/SDF subset with per-record losses; other formats and advanced chemistry are unsupported.",
            ),
            (false, true) => {
                Some("Write rich CDXML with opaque-content safety and explicit loss reporting.")
            }
            (false, false) => None,
        };
        let smiles = self.smiles.then_some("SMILES is written in recorded order or Chemifree's own canonical order (not compatible with other toolkits); drawings the shared valence and aromaticity models cannot interpret are refused with a reason.");
        join(files, smiles)
    }
}
