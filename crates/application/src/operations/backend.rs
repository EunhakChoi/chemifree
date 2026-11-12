//! Which provider serves a document.import or document.export request.

use serde_json::{Map, Value};

use crate::{Application, CapabilityCatalog, DocumentRepository};

use super::diagnostic;
use super::reply::Diagnostic;

/// Which provider serves a `document.import` or `document.export` request.
pub(super) enum Backend {
    Smiles,
    Interchange,
    Ct,
    /// The bounded SVG drawing: export only. It has no codec of its own, but it is served only
    /// by an application with a format codec connected.
    Svg,
    /// The format names a provider that is not connected.
    NotConnected(&'static str),
}

pub(super) fn not_connected(provider: &str) -> Diagnostic {
    diagnostic(
        "unsupported_format",
        format!("The {provider} provider is not connected."),
        Map::new(),
    )
}

impl<C: CapabilityCatalog, R: DocumentRepository> Application<C, R> {
    /// The provider for the request's `format`, decided once for import and export alike. A
    /// format that no provider names goes to MOL/SDF, which refuses it as a parameter error;
    /// with only the rich interchange provider connected, that provider refuses it.
    pub(super) fn format_backend(&self, parameters: &Value) -> Backend {
        let (backend, provider, connected) = match parameters.get("format").and_then(Value::as_str)
        {
            Some("smiles") => (Backend::Smiles, "SMILES", self.smiles_codec.is_some()),
            Some("cdxml" | "cdx") => (
                Backend::Interchange,
                "rich interchange",
                self.interchange_codec.is_some(),
            ),
            Some("mol" | "sdf" | "mol-v2000" | "mol-v3000") => {
                (Backend::Ct, "MOL/SDF", self.ctfile_codec.is_some())
            }
            // SVG has no provider of its own, so it has no `connected` check. Document import and
            // export are executable only when a format codec is attached (see `with_format_text`).
            // Under preserve-semantics, a bare `Application::new` refuses both as not_running
            // before this call; legacy-observed refuses them earlier as profile_unavailable.
            Some("svg") => return Backend::Svg,
            _ if self.ctfile_codec.is_none() && self.interchange_codec.is_some() => {
                return Backend::Interchange;
            }
            _ => return Backend::Ct,
        };
        if connected {
            backend
        } else {
            Backend::NotConnected(provider)
        }
    }
}
