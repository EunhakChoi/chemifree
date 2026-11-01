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

impl Formats {
    fn files(self) -> (bool, bool) {
        (self.ctfile, self.interchange)
    }

    /// The label of the `format_conversion` capability, e.g. "MOL/SDF and CDXML subsets".
    fn label(self) -> Option<&'static str> {
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
        if self.smiles {
            let list = self.list()?;
            return Some(intern(format!(
                "Read and write bounded {list} subsets with explicit losses.{} Other formats, stereochemistry and advanced objects are unsupported.",
                self.clauses(&[
                    (self.ctfile, "MOL/SDF is the V2000 subset."),
                    (self.interchange, "CDXML includes a loss-accounted editor projection."),
                    (true, "SMILES is connectivity only, with no query or reaction support and no InChI."),
                ])
            )));
        }
        match self.files() {
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
        }
    }

    pub(crate) fn import_note(self) -> Option<&'static str> {
        if self.smiles {
            let list = self.list()?;
            return Some(intern(format!(
                "Preview and import {list} with explicit losses.{}",
                self.clauses(&[
                    (self.ctfile, "MOL/SDF appends one selected record under revision-checked consent."),
                    (self.interchange, "CDXML reads a rich tree or previews an editor projection."),
                    (true, "SMILES appends connectivity after loss consent; stereo, query and reaction SMILES are refused with a position."),
                ])
            )));
        }
        match self.files() {
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
        }
    }

    pub(crate) fn export_note(self) -> Option<&'static str> {
        if self.smiles {
            let list = self.list()?;
            return Some(intern(format!(
                "Write {list} with explicit losses.{}",
                self.clauses(&[
                    (self.ctfile, "MOL/SDF is the V2000 subset with per-record losses."),
                    (self.interchange, "CDXML keeps opaque content safe."),
                    (true, "SMILES is written in recorded order or Chemifree's own canonical order (not compatible with other toolkits), and drawings the shared valence and aromaticity models cannot interpret are refused with a reason."),
                ])
            )));
        }
        match self.files() {
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
        }
    }

    /// The connected formats as a list, "MOL/SDF, CDXML and SMILES".
    fn list(self) -> Option<String> {
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
        match names.as_slice() {
            [] => None,
            [only] => Some((*only).to_owned()),
            [head @ .., last] => Some(format!("{} and {last}", head.join(", "))),
        }
    }

    /// Each connected codec's sentence, each preceded by a space.
    fn clauses(self, fragments: &[(bool, &str)]) -> String {
        fragments
            .iter()
            .filter(|(connected, _)| *connected)
            .map(|(_, text)| format!(" {text}"))
            .collect()
    }

    /// All four texts, computed once for a set of connected formats.
    pub(crate) fn texts(self) -> FormatTexts {
        FormatTexts {
            label: self.label(),
            description: self.description(),
            import_note: self.import_note(),
            export_note: self.export_note(),
        }
    }
}

/// The wording for one set of connected formats; built when a codec is connected, not per request.
#[derive(Clone, Copy, Default)]
pub(crate) struct FormatTexts {
    pub label: Option<&'static str>,
    pub description: Option<&'static str>,
    pub import_note: Option<&'static str>,
    pub export_note: Option<&'static str>,
}
