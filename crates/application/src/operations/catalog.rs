//! The operation catalog: the profile table, each operation's row with the connected
//! providers' wording, and the catalog reply.

use chemifree_domain::operation::{CompatibilityProfile, Execution, OperationId};

use crate::{Application, CapabilityCatalog, DocumentRepository};

use super::OPERATION_CONTRACT;
use super::catalog_table::{Row, entry, runs};
use super::cip;
use super::reply::{OperationCatalog, ProfileSupport};

fn profile_support(profile: CompatibilityProfile) -> ProfileSupport {
    let (execution, note) = match profile {
        CompatibilityProfile::PreserveSemantics => (
            Execution::Executable,
            "Operations report conversion losses and uncertainty; imports require consent for reported losses.",
        ),
        CompatibilityProfile::LegacyObserved => (
            Execution::NotImplemented,
            "No observed behavior of another program is reproduced. Only capabilities.inspect answers under it.",
        ),
    };
    ProfileSupport {
        id: profile.as_str(),
        execution: execution.as_str(),
        note,
    }
}

impl<C: CapabilityCatalog, R: DocumentRepository> Application<C, R> {
    /// The one row of the support table for an operation, with the wording of the format
    /// codecs connected to this application. The catalog reports it and the dispatcher runs
    /// exactly what it marks executable.
    pub(super) fn row(&self, operation: OperationId) -> Row<'_> {
        if operation == OperationId::ChemInchi && self.inchi_provider.is_some() {
            return Row {
                entry: runs(
                    "chem.inchi",
                    "Read-only bounded Standard InChI/InChIKey generation with checked provider version, build, effective options, exact snapshot, explicit losses and consent. Unsupported chemistry and AND/OR groups are refused. Stored configurations are used; drawn marks are not substituted. It runs under a time limit (time_limit_ms) that is asked before the native call and again once the native-call lock is held; the native call itself is not interrupted.",
                ),
                parts: &[],
                note: None,
            };
        }
        if let Some(text) = &self.format_text {
            let note = match operation {
                OperationId::DocumentImport => Some(text.import_note.as_str()),
                OperationId::DocumentExport => Some(text.export_note.as_str()),
                _ => None,
            };
            if let Some(note) = note {
                return Row {
                    entry: runs(operation.as_str(), ""),
                    parts: &[],
                    note: Some(note),
                };
            }
        }
        let (entry, parts) = entry(operation);
        Row {
            entry,
            parts,
            note: None,
        }
    }

    /// What this service supports of the operation contract, per operation and sub-operation.
    pub fn operation_catalog(&self) -> OperationCatalog {
        OperationCatalog {
            implementation: "chemifree",
            version: env!("CARGO_PKG_VERSION"),
            contracts: vec![OPERATION_CONTRACT],
            profiles: CompatibilityProfile::ALL
                .into_iter()
                .map(profile_support)
                .collect(),
            operations: OperationId::ALL
                .into_iter()
                .map(|operation| {
                    let row = self.row(operation);
                    let mut support = if operation == OperationId::ChemCip {
                        let note = cip::catalog_note(self.cip_labeler.as_ref());
                        row.entry.view(Some(&note), row.parts)
                    } else {
                        row.entry.view(row.note, row.parts)
                    };
                    if operation == OperationId::ChemInchi {
                        if let Some(provider) = &self.inchi_provider {
                            let identity = provider.identity();
                            support.note = format!("{} Registered provider: {}; version: {}; build: {}; effective options: {:?}. {}", support.note, identity.name, identity.version, identity.build, identity.options, crate::inchi::STANDARD_SEMANTICS).into();
                        } else if let Some(error) = &self.inchi_provider_error {
                            support.note = format!("{} Registration failure {}: {}", support.note, error.code, error.message).into();
                        }
                    }
                    if operation == OperationId::ChemProperties {
                        for part in &mut support.sub_operations {
                            let available = match part.id {
                                "mass" => self.property_data_readiness.mass,
                                "isotope_distribution" => self.property_data_readiness.distribution,
                                _ => continue,
                            };
                            if available {
                                part.execution = Execution::Executable.as_str();
                            }
                        }
                    }
                    support
                })
                .collect(),
        }
    }
}
