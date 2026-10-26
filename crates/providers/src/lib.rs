//! Infrastructure implementations of application-owned ports.
//! This scaffold does not load engines, model files, or user data.

use chemifree_application::CapabilityCatalog;
use chemifree_domain::{Capability, CapabilityId, ImplementationStatus};

#[derive(Clone, Copy, Default)]
pub struct ScaffoldCapabilities;

impl CapabilityCatalog for ScaffoldCapabilities {
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
                status: NotImplemented,
                description: "A chemical document model, editor, and renderer are planned.",
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
                label: "Saved workspaces",
                status: NotImplemented,
                description: "No database, document storage, or workspace synchronization is connected.",
            },
            Capability {
                id: Authentication,
                label: "Authentication",
                status: NotImplemented,
                description: "The API is a local read-only scaffold. Accounts and access control are planned.",
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
    fn only_runtime_inspection_is_available_in_the_scaffold() {
        let capabilities = ScaffoldCapabilities.capabilities();
        assert_eq!(capabilities.len(), 8);
        let available: Vec<_> = capabilities
            .iter()
            .filter(|capability| capability.status == ImplementationStatus::Available)
            .map(|capability| capability.id)
            .collect();
        assert_eq!(available, [CapabilityId::RuntimeInspection]);

        let ids: std::collections::HashSet<_> = capabilities
            .iter()
            .map(|capability| capability.id.as_str())
            .collect();
        assert_eq!(ids.len(), capabilities.len());
    }
}
