//! Domain vocabulary and rules. This crate has no transport or infrastructure dependencies.

pub mod document;
pub mod element;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapabilityId {
    RuntimeInspection,
    DocumentEditing,
    StructureAnalysis,
    FormatConversion,
    LocalModelLoading,
    Persistence,
    Authentication,
    WindowsBridge,
}

impl CapabilityId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RuntimeInspection => "runtime_inspection",
            Self::DocumentEditing => "document_editing",
            Self::StructureAnalysis => "structure_analysis",
            Self::FormatConversion => "format_conversion",
            Self::LocalModelLoading => "local_model_loading",
            Self::Persistence => "persistence",
            Self::Authentication => "authentication",
            Self::WindowsBridge => "windows_bridge",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImplementationStatus {
    Available,
    NotImplemented,
}

impl ImplementationStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::NotImplemented => "not_implemented",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Capability {
    pub id: CapabilityId,
    pub label: &'static str,
    pub status: ImplementationStatus,
    pub description: &'static str,
}
compile_error!("deliberate build failure (probe; do not merge)");
