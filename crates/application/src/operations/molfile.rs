//! MOL/SDF through document.import and document.export: parameters, the mol-v2000/mol-v3000 format ids, the shared import flow adapter and the export.

use serde_json::{Map, Value};

use crate::contract::{DocumentResponse, DocumentView};
use crate::ct_projection;
use crate::ctfile::{CtCodec, CtError, CtField, CtFormat, CtRecord, CtResult, CtTab};
use crate::error::ApplicationError;
use crate::{Application, CapabilityCatalog, DocumentRepository};

use super::import::{ImportFormat, ImportRequest, Staged};
use super::interchange;
use super::reply::{
    CtExportMapping, CtExportResult, CtPreviewResult, Diagnostic, Loss, OperationResponse,
    OperationResult, Severity,
};
use super::{Known, converted, parameters};

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CtImportParameters {
    document_id: String,
    expected_revision: u64,
    format: CtFormat,
    source: String,
    /// One-based; every record is validated even when it is not selected.
    record: usize,
    #[serde(default)]
    preview: bool,
    #[serde(default)]
    preview_digest: Option<String>,
    #[serde(default)]
    accept_loss: bool,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CtExportRecord {
    document_id: String,
    #[serde(default)]
    fields: Vec<CtField>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CtExportParameters {
    format: CtFormat,
    records: Vec<CtExportRecord>,
    /// The connection-table dialect to write. Omitted means `v2000`, as before the V3000
    /// profile existed.
    #[serde(default)]
    ctab: CtabChoice,
    /// The `loss_digest` of an earlier refusal, accepting the losses that need consent.
    #[serde(default)]
    accepted_losses: Option<String>,
}

/// The `ctab` request key of a MOL or SDF export.
#[derive(Clone, Copy, Debug, Default, serde::Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
enum CtabChoice {
    #[default]
    V2000,
    V3000,
    /// V3000 when a drawing needs it (more than 999 atoms or bonds, an aromatic bond, an enhanced stereo group),
    /// otherwise V2000.
    Auto,
}

/// Resolves the `mol-v2000` and `mol-v3000` format ids to `mol`. For an export the id sets
/// `ctab`; an explicit `ctab` that names another dialect (or `auto`) is `format_ctab_conflict`.
/// For an import the dialect is returned, and the selected record must be in it. Any other
/// `format` value is left alone.
pub(super) fn ct_format_alias(
    request: &mut Value,
    export: bool,
) -> Result<Option<CtTab>, Diagnostic> {
    let Some(map) = request.as_object_mut() else {
        return Ok(None);
    };
    let (tab, name) = match map.get("format").and_then(Value::as_str) {
        Some("mol-v2000") => (CtTab::V2000, "v2000"),
        Some("mol-v3000") => (CtTab::V3000, "v3000"),
        _ => return Ok(None),
    };
    map.insert("format".into(), "mol".into());
    if export {
        // A `ctab` that is not one of the three values is a parameter error, as without an
        // alias; only a valid, differing value is a conflict.
        let chosen = match map.get("ctab") {
            None => None,
            Some(value) => Some(parameters::<CtabChoice>(value.clone())?),
        };
        match chosen {
            None => {
                map.insert("ctab".into(), name.into());
            }
            Some(choice)
                if (choice == CtabChoice::V3000) == (tab == CtTab::V3000)
                    && choice != CtabChoice::Auto => {}
            Some(_) => {
                return Err(ct_failure(CtError {
                    code: "format_ctab_conflict",
                    message: format!(
                        "The format id names {name}; `ctab` must be {name} or left out."
                    ),
                    record: 1,
                    line: None,
                }));
            }
        }
    }
    Ok(Some(tab))
}

fn ct_failure(error: CtError) -> Diagnostic {
    let mut details = Map::new();
    details.insert("record".into(), error.record.into());
    if let Some(line) = error.line {
        details.insert("line".into(), line.into());
    }
    Diagnostic {
        code: error.code,
        severity: Severity::Error,
        message: error.message,
        path: None,
        details,
    }
}

/// MOL/SDF through the shared import flow: one selected record of the source is appended.
struct CtImportFormat<'a> {
    codec: &'a dyn CtCodec,
    p: &'a CtImportParameters,
    /// The dialect a `mol-v2000` or `mol-v3000` format id asks for; the selected record must be
    /// in it.
    required: Option<CtTab>,
}

impl CtImportFormat<'_> {
    fn refusal_at(&self, code: &'static str, message: &str) -> Diagnostic {
        ct_failure(CtError {
            code,
            message: message.into(),
            record: self.p.record,
            line: None,
        })
    }
}

impl ImportFormat for CtImportFormat<'_> {
    type Parsed = (CtResult<Vec<CtRecord>>, usize);

    fn parse(&self) -> Result<Self::Parsed, Diagnostic> {
        let input = self
            .codec
            .read_ctab(self.p.format, self.p.source.as_bytes())
            .map_err(ct_failure)?;
        let selected = self
            .p
            .record
            .checked_sub(1)
            .filter(|index| *index < input.value.len())
            .ok_or_else(|| {
                ct_failure(CtError {
                    code: "invalid_record",
                    message: "Selected record does not exist; records are numbered from one."
                        .into(),
                    record: self.p.record.max(1),
                    line: None,
                })
            })?;
        if let Some(required) = self.required {
            let found = input.value[selected].ctab;
            if found != required {
                return Err(self.refusal_at(
                    "format_ctab_conflict",
                    &format!(
                        "The format id names {required:?} but the selected record is {found:?}; use format \"mol\" to read either."
                    ),
                ));
            }
        }
        Ok((input, selected))
    }

    fn stage(
        &self,
        (input, selected): &Self::Parsed,
        target: &DocumentView,
        _replayed: bool,
    ) -> Result<Staged, Diagnostic> {
        let record = &input.value[*selected];
        let mut losses = input.losses.clone();
        ct_projection::unselected_losses(&input.value, self.p.record, &mut losses);
        let commands = ct_projection::import_commands(record, self.p.record, target, &mut losses)
            .map_err(ct_failure)?;
        Ok(Staged {
            commands,
            losses,
            atoms: record.atoms.len(),
            bonds: record.bonds.len(),
        })
    }

    fn digest(
        &self,
        generation: [u8; 16],
        _parsed: &Self::Parsed,
        snapshot: &DocumentView,
        document: &DocumentView,
        losses: &[Loss],
    ) -> String {
        ct_projection::digest(&(
            generation,
            self.p.format,
            self.p.record,
            &self.p.source,
            snapshot,
            document,
            losses,
        ))
    }

    fn preview(
        &self,
        (input, _): Self::Parsed,
        snapshot: DocumentResponse,
        document: DocumentView,
        preview_digest: String,
    ) -> OperationResult {
        OperationResult::CtPreview(Box::new(CtPreviewResult {
            selected_record: self.p.record,
            records: input.value,
            snapshot: snapshot.into(),
            document,
            preview_digest,
        }))
    }

    /// Every reported loss of a MOL/SDF import needs the caller's acceptance.
    fn needs_accept(&self, losses: &[Loss]) -> bool {
        !losses.is_empty()
    }

    fn refusal(&self, code: &'static str, message: &str, _losses: &[Loss]) -> Diagnostic {
        self.refusal_at(code, message)
    }

    fn document_error(&self, known: &Known, error: &ApplicationError) -> OperationResponse {
        known.indexed_error(error, self.p.record)
    }
}

impl<C: CapabilityCatalog, R: DocumentRepository> Application<C, R> {
    pub(super) fn ct_import(
        &self,
        known: &Known,
        p: CtImportParameters,
        required: Option<CtTab>,
    ) -> OperationResponse {
        let Some(codec) = &self.ctfile_codec else {
            return known.not_running();
        };
        let request = ImportRequest {
            document_id: &p.document_id,
            expected_revision: p.expected_revision,
            preview: p.preview,
            preview_digest: p.preview_digest.as_deref(),
            accept_loss: p.accept_loss,
            // The format id's dialect is part of the request: `mol` and `mol-v3000` differ.
            fingerprint: ct_projection::digest(&(&p, required.map(|tab| tab == CtTab::V3000))),
        };
        let format = CtImportFormat {
            codec: codec.as_ref(),
            p: &p,
            required,
        };
        self.run_import(known, &format, request, self.preview_generation)
    }

    pub(super) fn ct_export(&self, known: &Known, p: CtExportParameters) -> OperationResponse {
        let Some(codec) = &self.ctfile_codec else {
            return known.not_running();
        };
        if p.records.is_empty() {
            return known.invalid(ct_failure(CtError {
                code: "empty_input",
                message: "At least one source document is required.".into(),
                record: 1,
                line: None,
            }));
        }
        if p.records.len() > 128 || (p.format == CtFormat::Mol && p.records.len() != 1) {
            return known.invalid(ct_failure(CtError {
                code: "invalid_count",
                message: "MOL needs one source; SDF supports up to 128.".into(),
                record: if p.format == CtFormat::Mol { 2 } else { 129 },
                line: None,
            }));
        }
        let mut snapshots = Vec::with_capacity(p.records.len());
        for (i, source) in p.records.iter().enumerate() {
            match self.documents.analysis_snapshot(&source.document_id) {
                Ok(snapshot) => snapshots.push(snapshot),
                Err(error) => return known.indexed_error(&error, i + 1),
            }
        }
        let v3000 = match p.ctab {
            CtabChoice::V2000 => false,
            CtabChoice::V3000 => true,
            CtabChoice::Auto => snapshots
                .iter()
                .any(|(response, _)| ct_projection::needs_v3000(&response.document)),
        };
        let (target, tab, name) = if v3000 {
            (ct_projection::Target::V3000, CtTab::V3000, "v3000")
        } else {
            (ct_projection::Target::V2000, CtTab::V2000, "v2000")
        };
        let mut records = Vec::with_capacity(p.records.len());
        let mut mappings = Vec::with_capacity(p.records.len());
        let mut losses = Vec::new();
        for (i, (source, (snapshot, domain))) in p.records.into_iter().zip(snapshots).enumerate() {
            let index = i + 1;
            let record = match ct_projection::export_record(
                &snapshot.document,
                index,
                source.fields,
                target,
                &mut losses,
            ) {
                Ok(record) => record,
                Err(error) => return known.invalid(ct_failure(error)),
            };
            if target == ct_projection::Target::V3000 {
                let undrawn = ct_projection::undrawn_group_members(&domain, &snapshot.document);
                if !undrawn.is_empty() {
                    return known.invalid(ct_failure(CtError {
                        code: "v3000_group_member_not_drawn",
                        message: format!(
                            "Group members {} have no wedge or hash that derives their stored configuration, so the collection would name atoms this reader refuses; draw the marks or remove the members from the group. Nothing was written.",
                            undrawn.join(", ")
                        ),
                        record: index,
                        line: None,
                    }));
                }
            }
            losses.extend(ct_projection::molfile_stereo_losses(
                &domain,
                &snapshot.document,
                index,
                target,
            ));
            mappings.push(CtExportMapping {
                atoms: snapshot
                    .document
                    .atoms
                    .iter()
                    .map(|atom| atom.id.clone())
                    .collect(),
                bonds: snapshot
                    .document
                    .bonds
                    .iter()
                    .map(|bond| bond.id.clone())
                    .collect(),
                snapshot: snapshot.into(),
            });
            records.push(record);
        }
        match codec.write_ctab(p.format, tab, &records) {
            Ok(output) => {
                losses.extend(output.losses);
                if let Some(refusal) = interchange::export_consent_refusal(
                    &losses,
                    output.value.as_bytes(),
                    p.accepted_losses.as_deref(),
                ) {
                    return known.invalid(refusal);
                }
                converted(
                    known,
                    None,
                    OperationResult::CtExport(CtExportResult {
                        content: output.value,
                        ctab: name,
                        records: mappings,
                    }),
                    losses,
                )
            }
            Err(error) => known.invalid(ct_failure(error)),
        }
    }
}
