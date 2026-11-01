//! Capability and catalog wording for the connected format codecs.
//!
//! One function builds every sentence from the set of connected codecs and the limits the
//! codecs and the domain report, so no number is written twice and no combination can say that
//! a format is unsupported in one sentence and supported in the next. The result is built when
//! a codec is connected and kept in the `Application`, which owns it.

use chemifree_domain::document::MAX_COMMANDS;
use chemifree_domain::smiles::MAX_INPUT_BYTES;

/// A byte limit in words: `1 MiB` when it is a whole number of mebibytes, else `N bytes`.
fn describe_limit(bytes: usize) -> String {
    const MIB: usize = 1024 * 1024;
    if bytes != 0 && bytes.is_multiple_of(MIB) {
        format!("{} MiB", bytes / MIB)
    } else {
        format!("{bytes} bytes")
    }
}

/// `a`, `a and b`, `a, b and c`.
fn join(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [only] => only.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// Which format codecs are connected, with the limits the interchange codec reports.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Codecs {
    pub ctfile: bool,
    /// The interchange codec's binary input limit, when it is connected.
    pub interchange_binary_limit: Option<usize>,
    pub smiles: bool,
}

/// The wording of the `format_conversion` capability and of `document.import` and
/// `document.export` for one combination of connected codecs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FormatText {
    pub label: String,
    pub description: String,
    pub import_note: String,
    pub export_note: String,
}

impl FormatText {
    /// `None` when no codec is connected: the built-in catalog text then applies.
    pub(crate) fn new(codecs: Codecs) -> Option<Self> {
        let limit = codecs.interchange_binary_limit.map(describe_limit);
        let mut names = Vec::new();
        let mut described = Vec::new();
        let mut import = Vec::new();
        let mut export = Vec::new();
        if codecs.ctfile {
            names.push("MOL/SDF".to_owned());
            described.push("a bounded V2000 MOL/SDF subset".to_owned());
            import.push(
                "preview and append one selected record of a bounded V2000 MOL/SDF subset"
                    .to_owned(),
            );
            export.push("write a bounded V2000 MOL/SDF subset".to_owned());
        }
        if let Some(limit) = &limit {
            names.push("CDXML".to_owned());
            names.push("CDX".to_owned());
            described.push(format!("rich CDXML and a bounded CDX subset (base64 over JSON, at most {limit} of CDX; HTTP and CLI limits are lower)"));
            import.push(format!("read, preview and create from rich CDXML or a bounded CDX subset (canonical base64, at most {limit})"));
            export.push("write rich CDXML or a bounded CDX subset with opaque-content safety and accepted_losses consent for rounded or dropped CDX content".to_owned());
        }
        if codecs.smiles {
            names.push("SMILES".to_owned());
            described.push(format!("SMILES text (at most {MAX_INPUT_BYTES} bytes and {MAX_COMMANDS} atoms and bonds; no stereo; recorded-order or canonical output)"));
            import.push(format!("preview and append one SMILES string (at most {MAX_INPUT_BYTES} bytes), bound to the previewed revision"));
            export.push("write one drawing as recorded-order or canonical SMILES with accepted_losses consent for dropped content".to_owned());
        }
        if names.is_empty() {
            return None;
        }
        let plural = if names.len() == 1 {
            "subset"
        } else {
            "subsets"
        };
        Some(Self {
            label: format!("{} {plural}", join(&names)),
            description: format!(
                "Read and write {} with explicit loss reports. Stereo and advanced chemistry are unsupported; other formats are unsupported.",
                join(&described)
            ),
            import_note: format!(
                "{}; losses are reported and must be accepted before anything is created or appended.",
                capitalized(&import.join("; "))
            ),
            export_note: format!(
                "{}; every export reports its losses.",
                capitalized(&export.join("; "))
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(ctfile: bool, rich: bool, smiles: bool) -> Option<FormatText> {
        FormatText::new(Codecs {
            ctfile,
            interchange_binary_limit: rich.then_some(1024 * 1024),
            smiles,
        })
    }

    #[test]
    fn every_combination_has_one_consistent_text() {
        let labels = [
            ((false, false, false), None),
            ((true, false, false), Some("MOL/SDF subset")),
            ((false, true, false), Some("CDXML and CDX subsets")),
            ((false, false, true), Some("SMILES subset")),
            ((true, true, false), Some("MOL/SDF, CDXML and CDX subsets")),
            ((true, false, true), Some("MOL/SDF and SMILES subsets")),
            ((false, true, true), Some("CDXML, CDX and SMILES subsets")),
            (
                (true, true, true),
                Some("MOL/SDF, CDXML, CDX and SMILES subsets"),
            ),
        ];
        for ((ct, rich, smiles), label) in labels {
            let built = text(ct, rich, smiles);
            assert_eq!(
                built.as_ref().map(|t| t.label.as_str()),
                label,
                "{ct} {rich} {smiles}"
            );
            let Some(built) = built else { continue };
            // Each connected format is named, and no unconnected one is.
            for (named, present) in [("MOL/SDF", ct), ("CDXML", rich), ("SMILES", smiles)] {
                assert_eq!(
                    built.description.contains(named),
                    present,
                    "{named}: {}",
                    built.description
                );
            }
            assert_eq!(built.description.contains("1 MiB"), rich);
            assert_eq!(built.import_note.contains("1 MiB"), rich);
            assert_eq!(
                built.export_note.contains("accepted_losses"),
                rich || smiles
            );
            // The closing sentence comes last, so nothing supported follows "unsupported".
            assert!(
                built
                    .description
                    .ends_with("other formats are unsupported."),
                "{}",
                built.description
            );
            assert_eq!(
                built.description.matches("unsupported").count(),
                2,
                "{}",
                built.description
            );
            for note in [&built.import_note, &built.export_note] {
                assert!(!note.contains("unsupported"), "{note}");
                assert!(note.ends_with('.'), "{note}");
            }
        }
    }

    #[test]
    fn the_exact_sentences_are_pinned() {
        let all = text(true, true, true).unwrap();
        assert_eq!(
            all.description,
            "Read and write a bounded V2000 MOL/SDF subset, rich CDXML and a bounded CDX subset (base64 over JSON, at most 1 MiB of CDX; HTTP and CLI limits are lower) and SMILES text (at most 4096 bytes and 500 atoms and bonds; no stereo; recorded-order or canonical output) with explicit loss reports. Stereo and advanced chemistry are unsupported; other formats are unsupported."
        );
        assert_eq!(
            text(false, false, true).unwrap().description,
            "Read and write SMILES text (at most 4096 bytes and 500 atoms and bonds; no stereo; recorded-order or canonical output) with explicit loss reports. Stereo and advanced chemistry are unsupported; other formats are unsupported."
        );
        assert_eq!(
            text(true, false, false).unwrap().import_note,
            "Preview and append one selected record of a bounded V2000 MOL/SDF subset; losses are reported and must be accepted before anything is created or appended."
        );
        assert_eq!(
            text(false, false, true).unwrap().export_note,
            "Write one drawing as recorded-order or canonical SMILES with accepted_losses consent for dropped content; every export reports its losses."
        );
    }

    #[test]
    fn a_limit_in_bytes_is_not_rounded_to_mebibytes() {
        let built = FormatText::new(Codecs {
            ctfile: false,
            interchange_binary_limit: Some(1500),
            smiles: false,
        })
        .unwrap();
        assert!(built.description.contains("at most 1500 bytes of CDX"));
    }
}
