//! Capability and catalog wording for the interchange codec, built from the limit the codec
//! reports so the number is stated once, in the codec.
//!
//! The descriptions are `&'static str`, so each is built once when the codec is connected and
//! kept for the life of the process: six short strings per connected application.

/// A byte limit in words: `1 MiB` when it is a whole number of mebibytes, else `N bytes`.
fn describe_limit(bytes: usize) -> String {
    const MIB: usize = 1024 * 1024;
    if bytes != 0 && bytes.is_multiple_of(MIB) {
        format!("{} MiB", bytes / MIB)
    } else {
        format!("{bytes} bytes")
    }
}

fn keep(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

/// Wording that quotes the codec's binary limit.
#[derive(Clone, Copy, Debug)]
pub(crate) struct InterchangeNotes {
    /// `format_conversion` description with the V2000 provider connected too.
    pub combined_description: &'static str,
    /// `format_conversion` description with only the interchange codec.
    pub interchange_description: &'static str,
    pub combined_import: &'static str,
    pub interchange_import: &'static str,
    pub combined_export: &'static str,
    pub interchange_export: &'static str,
}

impl InterchangeNotes {
    pub(crate) fn new(binary_limit: usize) -> Self {
        let limit = describe_limit(binary_limit);
        Self {
            combined_description: keep(format!("Read and write bounded V2000 MOL/SDF, rich CDXML and a bounded CDX subset (base64 over JSON, at most {limit} of CDX; HTTP and CLI limits are lower) with explicit losses. Advanced chemistry and other CDX objects are unsupported; other formats are unsupported.")),
            interchange_description: keep(format!("Read and write rich CDXML and a bounded CDX subset (base64 over JSON, at most {limit} of CDX; HTTP and CLI limits are lower), and preview an editor projection, with explicit losses. Other formats and advanced objects remain unsupported.")),
            combined_import: keep(format!("Preview and import bounded MOL/SDF records, or read, preview and create from rich CDXML or a bounded CDX subset (canonical base64, at most {limit}), with explicit losses.")),
            interchange_import: keep(format!("Read CDXML or a bounded CDX subset (canonical base64, at most {limit}) as a rich tree, or preview a loss-accounted editor projection.")),
            combined_export: keep("Write bounded MOL/SDF, rich CDXML or a bounded CDX subset with explicit losses, opaque-content safety and accepted_losses consent for rounded or dropped CDX content.".to_owned()),
            interchange_export: keep("Write rich CDXML or a bounded CDX subset with opaque-content safety, explicit loss reporting and accepted_losses consent for rounded or dropped CDX content.".to_owned()),
        }
    }
}
