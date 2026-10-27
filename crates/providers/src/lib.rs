//! Infrastructure implementations of application-owned ports.
//! No chemistry engines or model files are loaded. User documents are local JSON files.

mod file_format;
mod file_store;

use chemifree_application::CapabilityCatalog;
use chemifree_domain::{Capability, CapabilityId, ImplementationStatus};

pub use file_store::{FileDocumentRepository, MAX_DOCUMENT_BYTES, default_data_dir};

/// Actual implementation status. Change an entry only together with its real operation.
#[derive(Clone, Copy, Default)]
pub struct LocalCapabilities;

impl CapabilityCatalog for LocalCapabilities {
    fn capabilities(&self) -> Vec<Capability> {
        use CapabilityId::{
            Authentication, DocumentEditing, FormatConversion, LocalModelLoading, Persistence,
            RuntimeInspection, StructureAnalysis, WindowsBridge,
        };
        use ImplementationStatus::{Available, NotImplemented};

        vec![
            Capability {
                id: RuntimeInspection,
                label: "Runtime inspection",
                status: Available,
                description: "Read the local service health and implementation status through HTTP or CLI.",
            },
            Capability {
                id: DocumentEditing,
                label: "Document editing",
                status: Available,
                description: "Create drawings and add, edit, move, or delete atoms (element, charge, isotope) and single/double/triple bonds, with atomic transactions and undo/redo. Drawing only: no valence, aromaticity, stereo, or layout perception.",
            },
            Capability {
                id: StructureAnalysis,
                label: "Structure analysis",
                status: NotImplemented,
                description: "No chemical calculations, validation, or prediction engine is connected.",
            },
            Capability {
                id: FormatConversion,
                label: "Import and export",
                status: NotImplemented,
                description: "No chemical file readers, writers, or format conversion are connected.",
            },
            Capability {
                id: LocalModelLoading,
                label: "Local model loading",
                status: NotImplemented,
                description: "No executable vendor engines or proprietary model data are included or loaded.",
            },
            Capability {
                id: Persistence,
                label: "Saved documents",
                status: Available,
                description: "Save and reopen documents as local Chemifree JSON files with atomic replace, revision checks, cross-process save locks, autosaved recovery drafts, and save as copy. No database, sync, or sharing.",
            },
            Capability {
                id: Authentication,
                label: "Authentication",
                status: NotImplemented,
                description: "No user accounts. The loopback API guards writes with a per-process session token and Host/Origin checks, which do not separate local OS users.",
            },
            Capability {
                id: WindowsBridge,
                label: "Windows integration",
                status: NotImplemented,
                description: "No desktop automation, COM bridge, or Windows application integration is connected.",
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_connected_operations_are_available() {
        let capabilities = LocalCapabilities.capabilities();
        assert_eq!(capabilities.len(), 8);
        let available: Vec<_> = capabilities
            .iter()
            .filter(|capability| capability.status == ImplementationStatus::Available)
            .map(|capability| capability.id)
            .collect();
        assert_eq!(
            available,
            [
                CapabilityId::RuntimeInspection,
                CapabilityId::DocumentEditing,
                CapabilityId::Persistence
            ]
        );

        let ids: std::collections::HashSet<_> = capabilities
            .iter()
            .map(|capability| capability.id.as_str())
            .collect();
        assert_eq!(ids.len(), capabilities.len());
    }
}
