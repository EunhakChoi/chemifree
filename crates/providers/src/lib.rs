//! Infrastructure implementations of application-owned ports.
//! No prediction engines are included here. User documents and preferences are local JSON files;
//! explicitly selected external data is checked through an application-owned registry.

mod data_directory;
mod file_format;
mod file_store;
mod preference_store;

use chemifree_application::CapabilityCatalog;
use chemifree_domain::{Capability, CapabilityId, ImplementationStatus};

pub use data_directory::{
    DirectoryDataSource, configure_selected_registry, load_selected_registry,
};
pub use file_store::{FileDocumentRepository, MAX_DOCUMENT_BYTES, default_data_dir};
pub use preference_store::FilePreferenceStore;

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
                description: "Create and edit drawings with atomic transactions and undo/redo. Edits record states as drawn; bounded valence inspection is a separate operation.",
            },
            Capability {
                id: StructureAnalysis,
                label: "Structure analysis",
                status: NotImplemented,
                description: "Read-only valence inspection covers a limited model; broader chemical analysis and prediction are not implemented.",
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
                description: "A trusted local registry can load a limited set of external data profiles. No model engine or model data is included; verified data alone does not run a calculation.",
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
