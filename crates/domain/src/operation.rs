//! Vocabulary of the versioned operation contract: which operations exist, under which
//! compatibility profile they are asked for, and how far each is supported.
//!
//! Support has two independent sides. *Representation* says whether a document and the
//! contract can hold an operation's inputs and results; *execution* says whether Chemifree
//! runs it, needs data it does not have, or does not run it. A representable feature is not
//! thereby computed, and nothing here claims chemistry that is not implemented.

/// An operation of the operation contract.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum OperationId {
    CapabilitiesInspect,
    DocumentCreate,
    DocumentSnapshot,
    DocumentTransaction,
    HistoryUndo,
    HistoryRedo,
    DocumentSave,
    DocumentClose,
    DocumentImport,
    DocumentExport,
    ChemValidate,
    ChemAromaticity,
    ChemAromaticityConvert,
    ChemIdentity,
    ChemProperties,
    ChemNameToStructure,
    ChemStructureToName,
    LayoutStructure,
    LayoutReaction,
    RenderScene,
    BiopolymerExpand,
    PredictionEvaluate,
    IntegrationInvoke,
    ScriptEvaluate,
}

impl OperationId {
    /// Every operation, in catalog order.
    pub const ALL: [Self; 24] = [
        Self::CapabilitiesInspect,
        Self::DocumentCreate,
        Self::DocumentSnapshot,
        Self::DocumentTransaction,
        Self::HistoryUndo,
        Self::HistoryRedo,
        Self::DocumentSave,
        Self::DocumentClose,
        Self::DocumentImport,
        Self::DocumentExport,
        Self::ChemValidate,
        Self::ChemAromaticity,
        Self::ChemAromaticityConvert,
        Self::ChemIdentity,
        Self::ChemProperties,
        Self::ChemNameToStructure,
        Self::ChemStructureToName,
        Self::LayoutStructure,
        Self::LayoutReaction,
        Self::RenderScene,
        Self::BiopolymerExpand,
        Self::PredictionEvaluate,
        Self::IntegrationInvoke,
        Self::ScriptEvaluate,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CapabilitiesInspect => "capabilities.inspect",
            Self::DocumentCreate => "document.create",
            Self::DocumentSnapshot => "document.snapshot",
            Self::DocumentTransaction => "document.transaction",
            Self::HistoryUndo => "history.undo",
            Self::HistoryRedo => "history.redo",
            Self::DocumentSave => "document.save",
            Self::DocumentClose => "document.close",
            Self::DocumentImport => "document.import",
            Self::DocumentExport => "document.export",
            Self::ChemValidate => "chem.validate",
            Self::ChemAromaticity => "chem.aromaticity",
            Self::ChemAromaticityConvert => "chem.aromaticity_convert",
            Self::ChemIdentity => "chem.identity",
            Self::ChemProperties => "chem.properties",
            Self::ChemNameToStructure => "chem.name_to_structure",
            Self::ChemStructureToName => "chem.structure_to_name",
            Self::LayoutStructure => "layout.structure",
            Self::LayoutReaction => "layout.reaction",
            Self::RenderScene => "render.scene",
            Self::BiopolymerExpand => "biopolymer.expand",
            Self::PredictionEvaluate => "prediction.evaluate",
            Self::IntegrationInvoke => "integration.invoke",
            Self::ScriptEvaluate => "script.evaluate",
        }
    }

    /// The operation named `name`, if there is one.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|operation| operation.as_str() == name)
    }
}

/// The compatibility goal a request is made under. Every request names one; there is no
/// hidden default.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CompatibilityProfile {
    /// Preserve chemical and document meaning, and report what is lost.
    PreserveSemantics,
    /// Reproduce behavior observed in an earlier program, for a stated version, input, and
    /// path. Chemifree has no such behavior; requests under it are unsupported.
    LegacyObserved,
}

impl CompatibilityProfile {
    pub const ALL: [Self; 2] = [Self::PreserveSemantics, Self::LegacyObserved];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PreserveSemantics => "preserve-semantics",
            Self::LegacyObserved => "legacy-observed",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|profile| profile.as_str() == name)
    }
}

/// Whether a document and the contract can hold an operation's inputs and results.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Representation {
    Supported,
    NotSupported,
}

impl Representation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::NotSupported => "not_supported",
        }
    }
}

/// Whether Chemifree runs an operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Execution {
    /// It runs and its results are checked.
    Executable,
    /// It would run, but needs external data (a dataset or model) that is not loaded.
    RequiresData,
    /// It does not run.
    NotImplemented,
    /// It deliberately never runs (for example, delegating to another program's executable).
    Rejected,
}

impl Execution {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Executable => "executable",
            Self::RequiresData => "requires_data",
            Self::NotImplemented => "not_implemented",
            Self::Rejected => "rejected",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_names_round_trip_and_are_distinct() {
        for operation in OperationId::ALL {
            assert_eq!(OperationId::parse(operation.as_str()), Some(operation));
        }
        let mut names: Vec<_> = OperationId::ALL.iter().map(|op| op.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), OperationId::ALL.len(), "names are distinct");
        assert_eq!(
            OperationId::parse("document.Create"),
            None,
            "names are exact"
        );
    }

    #[test]
    fn profiles_round_trip() {
        for profile in CompatibilityProfile::ALL {
            assert_eq!(CompatibilityProfile::parse(profile.as_str()), Some(profile));
        }
        assert_eq!(CompatibilityProfile::parse("observed"), None);
    }
}
