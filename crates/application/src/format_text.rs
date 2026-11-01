//! The capability and catalog wording for the connected import and export formats.
//!
//! One sentence for every combination of connected codecs, built once per application when a
//! codec is connected and owned by it. Every limit quoted here comes from the codec that
//! enforces it, so the number is stated once, in the codec.

use crate::smiles_codec::SmilesLimits;

/// A byte limit in words: `1 MiB` when it is a whole number of mebibytes, else `N bytes`.
fn describe_limit(bytes: usize) -> String {
    const MIB: usize = 1024 * 1024;
    if bytes != 0 && bytes.is_multiple_of(MIB) {
        format!("{} MiB", bytes / MIB)
    } else {
        format!("{bytes} bytes")
    }
}

/// "a", "a and b", "a, b and c".
fn list(items: &[String], last: &str) -> String {
    match items {
        [] => String::new(),
        [only] => only.clone(),
        [init @ .., tail] => format!("{} {last} {tail}", init.join(", ")),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FormatText {
    pub label: String,
    pub description: String,
    pub import_note: String,
    pub export_note: String,
}

impl FormatText {
    /// The wording for the connected codecs, or `None` when no format is connected.
    pub(crate) fn new(
        ctfile: bool,
        binary_limit: Option<usize>,
        smiles: Option<SmilesLimits>,
    ) -> Option<Self> {
        let rich = binary_limit.map(describe_limit);
        match (ctfile, rich, smiles) {
            (false, None, None) => None,
            (true, None, None) => Some(Self {
                label: "MOL/SDF subset".into(),
                description: "Read and write a bounded V2000 MOL/SDF subset with explicit loss reports. Other formats, stereo and advanced objects remain unsupported.".into(),
                import_note: "Preview and append one selected record of a bounded V2000 MOL/SDF subset, with explicit losses and revision-checked consent.".into(),
                export_note: "Write a bounded V2000 MOL/SDF subset with per-record losses; other formats and advanced chemistry are unsupported.".into(),
            }),
            (false, Some(limit), None) => Some(Self {
                label: "CDXML and CDX subsets".into(),
                description: format!("Read and write rich CDXML and a bounded CDX subset (base64 over JSON, at most {limit} of CDX; HTTP and CLI limits are lower), and preview an editor projection, with explicit losses. Other formats and advanced objects remain unsupported."),
                import_note: format!("Read CDXML or a bounded CDX subset (canonical base64, at most {limit}) as a rich tree, or preview a loss-accounted editor projection."),
                export_note: "Write rich CDXML or a bounded CDX subset with opaque-content safety, explicit loss reporting and accepted_losses consent for rounded or dropped CDX content.".into(),
            }),
            (true, Some(limit), None) => Some(Self {
                label: "MOL/SDF, CDXML and CDX subsets".into(),
                description: format!("Read and write bounded V2000 MOL/SDF, rich CDXML and a bounded CDX subset (base64 over JSON, at most {limit} of CDX; HTTP and CLI limits are lower) with explicit losses. Advanced chemistry and other CDX objects are unsupported; other formats are unsupported."),
                import_note: format!("Preview and import bounded MOL/SDF records, or read, preview and create from rich CDXML or a bounded CDX subset (canonical base64, at most {limit}), with explicit losses."),
                export_note: "Write bounded MOL/SDF, rich CDXML or a bounded CDX subset with explicit losses, opaque-content safety and accepted_losses consent for rounded or dropped CDX content.".into(),
            }),
            (ctfile, rich, Some(smiles)) => {
                let mut names = Vec::new();
                let mut described = Vec::new();
                let mut reads = Vec::new();
                let mut writes = Vec::new();
                let mut consenting = Vec::new();
                if ctfile {
                    names.push("MOL/SDF".to_owned());
                    described.push("a bounded V2000 MOL/SDF subset".to_owned());
                    reads.push("bounded MOL/SDF records".to_owned());
                    writes.push("bounded MOL/SDF".to_owned());
                }
                if let Some(limit) = &rich {
                    names.push("CDXML".to_owned());
                    names.push("CDX".to_owned());
                    described.push(format!("rich CDXML and a bounded CDX subset (base64 over JSON, at most {limit} of CDX; HTTP and CLI limits are lower)"));
                    reads.push("rich CDXML".to_owned());
                    reads.push(format!("a bounded CDX subset (canonical base64, at most {limit})"));
                    writes.push("rich CDXML".to_owned());
                    writes.push("a bounded CDX subset".to_owned());
                    consenting.push("CDX".to_owned());
                }
                names.push("SMILES".to_owned());
                described.push(format!("SMILES without stereochemistry (at most {} bytes and {} atoms plus bonds per string, recorded-order or canonical output)", smiles.max_input_bytes, smiles.max_commands));
                reads.push(format!("a SMILES string (at most {} bytes)", smiles.max_input_bytes));
                writes.push("SMILES (recorded-order or canonical)".to_owned());
                consenting.push("SMILES".to_owned());
                let formats = if names.len() == 1 { "subset" } else { "subsets" };
                Some(Self {
                    label: format!("{} {formats}", list(&names, "and")),
                    description: format!("Read and write {} with explicit losses. Stereochemistry, queries, reactions, InChI and formats not listed here are unsupported.", list(&described, "and")),
                    import_note: format!("Read, preview and import {}, with explicit losses and revision-checked consent.", list(&reads, "and")),
                    export_note: format!("Write {}, reporting every loss; a write that would drop or round {} content needs the loss_digest repeated as accepted_losses.", list(&writes, "and"), list(&consenting, "or")),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn smiles() -> SmilesLimits {
        SmilesLimits {
            max_input_bytes: 4096,
            max_commands: 500,
            max_branch_depth: 100,
            max_open_rings: 99,
            canonical_max_atoms: 128,
        }
    }

    #[test]
    fn describes_whole_mebibytes_and_other_sizes() {
        assert_eq!(describe_limit(1024 * 1024), "1 MiB");
        assert_eq!(describe_limit(1000), "1000 bytes");
        assert_eq!(describe_limit(0), "0 bytes");
    }

    #[test]
    fn lists_read_naturally() {
        assert_eq!(list(&["a".into()], "and"), "a");
        assert_eq!(list(&["a".into(), "b".into()], "or"), "a or b");
        assert_eq!(list(&["a".into(), "b".into(), "c".into()], "and"), "a, b and c");
    }

    #[test]
    fn nothing_connected_has_no_text() {
        assert_eq!(FormatText::new(false, None, None), None);
    }

    /// Every combination that includes SMILES, pinned whole: no sentence calls other formats
    /// unsupported and then lists one more, and the plural follows the number of formats.
    #[test]
    fn smiles_combinations_are_pinned() {
        let limit = Some(1024 * 1024);
        let cases = [
            (false, None, "SMILES subset", "Read and write SMILES without stereochemistry (at most 4096 bytes and 500 atoms plus bonds per string, recorded-order or canonical output) with explicit losses. Stereochemistry, queries, reactions, InChI and formats not listed here are unsupported."),
            (true, None, "MOL/SDF and SMILES subsets", "Read and write a bounded V2000 MOL/SDF subset and SMILES without stereochemistry (at most 4096 bytes and 500 atoms plus bonds per string, recorded-order or canonical output) with explicit losses. Stereochemistry, queries, reactions, InChI and formats not listed here are unsupported."),
            (false, limit, "CDXML, CDX and SMILES subsets", "Read and write rich CDXML and a bounded CDX subset (base64 over JSON, at most 1 MiB of CDX; HTTP and CLI limits are lower) and SMILES without stereochemistry (at most 4096 bytes and 500 atoms plus bonds per string, recorded-order or canonical output) with explicit losses. Stereochemistry, queries, reactions, InChI and formats not listed here are unsupported."),
            (true, limit, "MOL/SDF, CDXML, CDX and SMILES subsets", "Read and write a bounded V2000 MOL/SDF subset, rich CDXML and a bounded CDX subset (base64 over JSON, at most 1 MiB of CDX; HTTP and CLI limits are lower) and SMILES without stereochemistry (at most 4096 bytes and 500 atoms plus bonds per string, recorded-order or canonical output) with explicit losses. Stereochemistry, queries, reactions, InChI and formats not listed here are unsupported."),
        ];
        for (ct, rich, label, description) in cases {
            let text = FormatText::new(ct, rich, Some(smiles())).expect("a format is connected");
            assert_eq!(text.label, label);
            assert_eq!(text.description, description, "{label}");
            assert!(!text.description.contains("Other formats"), "{label}");
        }
        let all = FormatText::new(true, limit, Some(smiles())).unwrap();
        assert_eq!(all.import_note, "Read, preview and import bounded MOL/SDF records, rich CDXML, a bounded CDX subset (canonical base64, at most 1 MiB) and a SMILES string (at most 4096 bytes), with explicit losses and revision-checked consent.");
        assert_eq!(all.export_note, "Write bounded MOL/SDF, rich CDXML, a bounded CDX subset and SMILES (recorded-order or canonical), reporting every loss; a write that would drop or round CDX or SMILES content needs the loss_digest repeated as accepted_losses.");
        let only = FormatText::new(false, None, Some(smiles())).unwrap();
        assert_eq!(only.import_note, "Read, preview and import a SMILES string (at most 4096 bytes), with explicit losses and revision-checked consent.");
        assert_eq!(only.export_note, "Write SMILES (recorded-order or canonical), reporting every loss; a write that would drop or round SMILES content needs the loss_digest repeated as accepted_losses.");
    }
}
