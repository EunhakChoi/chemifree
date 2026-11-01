//! Capability and catalog wording for the connected format codecs, built once per application
//! from whichever codecs are connected and from the limits those codecs report, so each number
//! is stated once, in its codec.
//!
//! Every combination reads as one description: what can be read and written, listed once, then
//! what is not supported. A format is never called unsupported in a sentence that elsewhere
//! lists it.

use crate::smiles::SmilesLimits;

/// A byte limit in words: `1 MiB` or `16 KiB` when it is a whole number of those, else
/// `N bytes`.
fn describe_limit(bytes: usize) -> String {
    const KIB: usize = 1024;
    const MIB: usize = 1024 * KIB;
    if bytes != 0 && bytes.is_multiple_of(MIB) {
        format!("{} MiB", bytes / MIB)
    } else if bytes != 0 && bytes.is_multiple_of(KIB) {
        format!("{} KiB", bytes / KIB)
    } else {
        format!("{bytes} bytes")
    }
}

/// `a`, `a and b`, `a, b and c`.
fn list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [only] => only.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// Which codecs an application has, with the limits they report.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Connected {
    pub ctfile: bool,
    /// The most bytes of CDX the interchange codec accepts.
    pub interchange_binary_limit: Option<usize>,
    pub smiles: Option<SmilesLimits>,
}

/// The wording for the `format_conversion` capability and the `document.import` and
/// `document.export` catalog rows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FormatText {
    pub label: String,
    pub description: String,
    pub import_note: String,
    pub export_note: String,
}

impl FormatText {
    /// `None` when no format codec is connected: the static wording then applies.
    pub(crate) fn new(connected: Connected) -> Option<Self> {
        let mut names = Vec::new();
        let mut read_write = Vec::new();
        let mut import = Vec::new();
        let mut export = Vec::new();
        if connected.ctfile {
            names.push("MOL/SDF".to_owned());
            read_write.push("bounded V2000 MOL/SDF".to_owned());
            import.push("append one selected record of a bounded V2000 MOL/SDF subset".to_owned());
            export.push("bounded V2000 MOL/SDF with per-record losses".to_owned());
        }
        if let Some(limit) = connected.interchange_binary_limit {
            let limit = describe_limit(limit);
            names.push("CDXML".to_owned());
            names.push("CDX".to_owned());
            read_write.push("rich CDXML".to_owned());
            read_write.push(format!("a bounded CDX subset (base64 over JSON, at most {limit} of CDX; HTTP and CLI limits are lower)"));
            import.push(format!("read CDXML or a bounded CDX subset (canonical base64, at most {limit}) as a rich tree, or create a drawing from its loss-accounted editor projection"));
            export.push("rich CDXML or a bounded CDX subset with opaque-content safety and accepted_losses consent for rounded or dropped CDX content".to_owned());
        }
        if let Some(limits) = connected.smiles {
            let limit = describe_limit(limits.max_input_bytes);
            names.push("SMILES".to_owned());
            read_write.push(format!("SMILES text (a bounded OpenSMILES subset of at most {limit} and {} nested branches, written in recorded or canonical form)", limits.max_branch_depth));
            import.push(format!("append one SMILES string (at most {limit})"));
            export.push("one SMILES string, recorded or canonical, with accepted_losses consent for the assumptions it must make".to_owned());
        }
        if names.is_empty() {
            return None;
        }
        let subsets = if names.len() == 1 {
            "subset"
        } else {
            "subsets"
        };
        Some(Self {
            label: format!("{} {subsets}", list(&names)),
            description: format!(
                "Read and write {} with explicit losses. Stereochemistry, query and reaction forms and advanced chemistry are not read or written; other formats are unsupported.",
                list(&read_write)
            ),
            import_note: format!(
                "Preview, then import with explicit losses and revision-checked consent: {}.",
                import.join("; ")
            ),
            export_note: format!("Write with explicit losses: {}.", export.join("; ")),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_are_described_in_the_largest_whole_unit() {
        assert_eq!(describe_limit(1024 * 1024), "1 MiB");
        assert_eq!(describe_limit(16 * 1024), "16 KiB");
        assert_eq!(describe_limit(1000), "1000 bytes");
        assert_eq!(describe_limit(0), "0 bytes");
    }

    #[test]
    fn lists_read_naturally() {
        let items = |n: usize| (0..n).map(|i| format!("x{i}")).collect::<Vec<_>>();
        assert_eq!(list(&items(1)), "x0");
        assert_eq!(list(&items(2)), "x0 and x1");
        assert_eq!(list(&items(3)), "x0, x1 and x2");
    }
}
