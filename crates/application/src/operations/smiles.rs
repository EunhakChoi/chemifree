//! `document.import` and `document.export` for SMILES.
//!
//! Import reads a string, places its atoms (SMILES has no coordinates), and shares the preview,
//! digest, consent and commit flow with MOL/SDF. Export writes the drawing's graph, recorded
//! or canonical, read-only against one snapshot, and shares the consent mechanism with CDX.

use chemifree_domain::document::{AtomId, AtomRef, Command, NewAtomPositions, Point};
use serde::{Deserialize, Serialize};

use super::import_flow::{ImportAdapter, ImportRun};
use super::interchange::{consent_digest, consent_refusal, needs_consent};
use super::*;
use crate::contract::DocumentResponse;
use crate::ct_projection;
use crate::drawing_losses;
use crate::smiles_codec::{
    CANONICAL_GRAPH_MODEL, ErrorClass, RadicalReason, ReadNote, SMILES_CANONICAL_ID,
    SMILES_MODEL_ID, SmilesError, SmilesRead, SmilesStyle, SmilesWrite, SourceSpan, WriteChange,
};

/// Spacing of the generated placement, in points.
const GRID_SPACING: f64 = 40.0;
/// Gap between existing content and the imported atoms, in points.
const GRID_MARGIN: f64 = 60.0;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SmilesImportParameters {
    format: String,
    document_id: String,
    expected_revision: u64,
    source: String,
    #[serde(default)]
    preview: bool,
    #[serde(default)]
    preview_digest: Option<String>,
    #[serde(default)]
    accept_loss: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SmilesExportParameters {
    format: String,
    document_id: String,
    style: ExportStyle,
    /// The `loss_digest` of the refusal that showed the losses needing consent.
    #[serde(default)]
    accepted_losses: Option<String>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ExportStyle {
    Recorded,
    Canonical,
}

impl From<ExportStyle> for SmilesStyle {
    fn from(style: ExportStyle) -> Self {
        match style {
            ExportStyle::Recorded => Self::Recorded,
            ExportStyle::Canonical => Self::Canonical,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SmilesPreviewResult {
    /// The exact working snapshot the candidate was computed against.
    pub snapshot: DocumentResult,
    pub document: DocumentView,
    pub preview_digest: String,
    pub atoms: usize,
    pub bonds: usize,
    /// Dot-separated parts of the string.
    pub parts: usize,
    pub model_id: &'static str,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SmilesExportResult {
    /// The exact snapshot written; the write changed neither it nor its history.
    pub snapshot: DocumentResult,
    pub style: &'static str,
    pub content: String,
    /// Document atom IDs in the order the atoms appear in `content`.
    pub atom_order: Vec<String>,
    /// Explicit hydrogen atoms folded into their heavy atom (canonical); not in `atom_order`.
    pub folded_hydrogens: Vec<String>,
    pub model_id: &'static str,
    /// Present for canonical output: the form is Chemifree's own and is not compatible with
    /// any toolkit's canonical SMILES.
    pub canonical: Option<CanonicalIdentity>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CanonicalIdentity {
    pub implementation: &'static str,
    pub graph_model: &'static str,
}

/// A codec failure as a diagnostic: positions are byte offsets, `byte_end_exclusive` one past
/// the last byte of the range.
fn codec_failure(error: SmilesError) -> (Outcome, Diagnostic) {
    let mut details = Map::new();
    details.insert("class".into(), error.class.as_str().into());
    if let Some(SourceSpan { start, end }) = error.span {
        details.insert("byte_start".into(), start.into());
        details.insert("byte_end_exclusive".into(), end.into());
    }
    if let Some(detail) = error.detail {
        details.insert("reason".into(), detail.into());
    }
    let outcome = match error.class {
        ErrorClass::Syntax | ErrorClass::Limit => Outcome::Error,
        ErrorClass::Unsupported | ErrorClass::NotWritable => Outcome::Unsupported,
    };
    (outcome, diagnostic(error.code, error.message, details))
}

fn refuse_codec(known: &Known, error: SmilesError) -> OperationResponse {
    let (outcome, diagnostic) = codec_failure(error);
    known.reply(outcome, None, None, vec![diagnostic])
}

fn plain_loss(
    feature: &str,
    path: String,
    disposition: LossDisposition,
    impact: SemanticImpact,
    consent: LossConsent,
    reason: &str,
) -> Loss {
    Loss {
        consent,
        feature: feature.into(),
        source_path: path,
        disposition,
        reason: reason.into(),
        semantic_impact: impact,
    }
}

fn listed<T: std::fmt::Display>(items: impl IntoIterator<Item = T>) -> String {
    items
        .into_iter()
        .map(|item| item.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// The first and the last byte of a span, which is how a reader counts: `end` is exclusive.
fn byte_range(span: SourceSpan) -> String {
    if span.end > span.start {
        format!("{}..={}", span.start, span.end - 1)
    } else {
        format!("{}", span.start)
    }
}

fn read_losses(read: &SmilesRead) -> Vec<Loss> {
    let mut losses = Vec::new();
    for note in &read.notes {
        if let ReadNote::IgnoredText(span) = note {
            losses.push(plain_loss(
                "ignored_text",
                format!("source.bytes[{}]", byte_range(*span)),
                LossDisposition::Dropped,
                SemanticImpact::Document,
                LossConsent::Required,
                "Text after the first whitespace is not part of the SMILES and is not imported.",
            ));
        }
    }
    for (reason, text) in [
        (
            RadicalReason::OutsideModel,
            "The element or charge is outside the shared valence model, so the radical state cannot be determined; it is recorded as unknown.",
        ),
        (
            RadicalReason::Ambiguous,
            "A singlet and a triplet both fit the stated hydrogens and bonds; the radical state is recorded as unknown.",
        ),
        (
            RadicalReason::NoState,
            "No radical state fits the stated hydrogens and bonds under the shared valence model; the radical state is recorded as unknown.",
        ),
    ] {
        let atoms: Vec<usize> = read
            .notes
            .iter()
            .filter_map(|note| match note {
                ReadNote::RadicalUndetermined { atom, reason: found, .. } if *found == reason => {
                    Some(atom + 1)
                }
                _ => None,
            })
            .collect();
        if !atoms.is_empty() {
            losses.push(plain_loss(
                "radical_state",
                format!("source.atoms[{}]", listed(atoms)),
                LossDisposition::Unknown,
                SemanticImpact::Chemistry,
                LossConsent::NotRequired,
                text,
            ));
        }
    }
    if !read.atoms.is_empty() {
        losses.push(plain_loss(
            "generated_layout",
            "source".into(),
            LossDisposition::Normalized,
            SemanticImpact::Geometry,
            LossConsent::NotRequired,
            "SMILES has no coordinates. The atoms were placed on a grid in text order; this is not a depiction layout.",
        ));
    }
    losses
}

fn change_losses(changes: &[WriteChange], folded: &[AtomId]) -> Vec<Loss> {
    let atoms = |ids: &[AtomId]| format!("document.atoms[{}]", listed(ids));
    let mut losses = Vec::new();
    for change in changes {
        losses.push(match change {
            WriteChange::AttachmentPoints(ids) => plain_loss(
                "attachment_points",
                atoms(ids),
                LossDisposition::Dropped,
                SemanticImpact::Chemistry,
                LossConsent::Required,
                "SMILES has no attachment points; they are dropped.",
            ),
            WriteChange::InferredHydrogensStated(ids) => plain_loss(
                "hydrogen_state",
                atoms(ids),
                LossDisposition::Normalized,
                SemanticImpact::None,
                LossConsent::NotRequired,
                "Atoms drawn with inferred hydrogens need brackets; they are written with the hydrogen count the valence model determines, which reads back as stated.",
            ),
            WriteChange::StatedHydrogensInferred(ids) => plain_loss(
                "hydrogen_state",
                atoms(ids),
                LossDisposition::Normalized,
                SemanticImpact::None,
                LossConsent::NotRequired,
                "Atoms with a stated hydrogen count equal to the inferred one are written without brackets, so the count reads back as inferred.",
            ),
            WriteChange::HydrogensAssumed(ids) => plain_loss(
                "hydrogens_assumed",
                atoms(ids),
                LossDisposition::Unknown,
                SemanticImpact::Chemistry,
                LossConsent::Required,
                "The valence model does not cover these elements or charges, so their hydrogens are not determined; none are written, which states that the atom has none.",
            ),
            WriteChange::RadicalAssumed(ids) => plain_loss(
                "radical_assumed",
                atoms(ids),
                LossDisposition::Unknown,
                SemanticImpact::Chemistry,
                LossConsent::Required,
                "These atoms are drawn without a radical, but SMILES cannot say so for atoms outside the valence model; reading the text records their radical state as unknown.",
            ),
            WriteChange::KekuleAsAromatic(ids) => plain_loss(
                "kekule_to_aromatic",
                format!("document.bonds[{}]", listed(ids)),
                LossDisposition::Normalized,
                SemanticImpact::None,
                LossConsent::NotRequired,
                "Kekulé single and double bonds of aromatic systems are written aromatic under chemifree-aromaticity-2.",
            ),
        });
    }
    if !folded.is_empty() {
        losses.push(plain_loss(
            "explicit_hydrogens_folded",
            atoms(folded),
            LossDisposition::Normalized,
            SemanticImpact::None,
            LossConsent::NotRequired,
            "Terminal hydrogen atoms drawn as vertices are counted into their heavy atom; they are not atoms of the text.",
        ));
    }
    losses
}

/// A near-square grid in text order, to the right of existing content.
fn import_commands(read: &SmilesRead, target: &DocumentView) -> Result<Vec<Command>, Diagnostic> {
    if target.scene.instances.len() != 1 {
        return Err(diagnostic(
            "unsupported_scene",
            "Import requires exactly one molecule placement.",
            Map::new(),
        ));
    }
    let origin_x = target
        .atoms
        .iter()
        .map(|atom| atom.position.x)
        .fold(None, |most: Option<f64>, x| Some(most.map_or(x, |m| m.max(x))))
        .map_or(GRID_SPACING, |most| most + GRID_MARGIN);
    let origin_y = target
        .atoms
        .iter()
        .map(|atom| atom.position.y)
        .fold(None, |least: Option<f64>, y| Some(least.map_or(y, |m| m.min(y))))
        .unwrap_or(GRID_SPACING);
    let width = (1..).find(|w| w * w >= read.atoms.len()).unwrap_or(1);
    let mut commands = Vec::with_capacity(read.atoms.len() + read.bonds.len());
    for (index, parsed) in read.atoms.iter().enumerate() {
        let point = Point::new(
            origin_x + GRID_SPACING * (index % width) as f64,
            origin_y + GRID_SPACING * (index / width) as f64,
        )
        .map_err(|_| {
            diagnostic(
                "coordinate_out_of_range",
                format!("Atom {} cannot be placed beside the existing drawing.", index + 1),
                Map::new(),
            )
        })?;
        commands.push(Command::AddAtom {
            atom: parsed.atom.clone(),
            positions: NewAtomPositions::Single(point),
            bond_to: None,
        });
    }
    for bond in &read.bonds {
        commands.push(Command::AddBond {
            begin: AtomRef::Created(bond.begin),
            end: AtomRef::Created(bond.end),
            order: bond.order.clone(),
        });
    }
    Ok(commands)
}

struct SmilesAdapter<'a> {
    source: &'a str,
    atoms: usize,
    bonds: usize,
    parts: usize,
}

impl ImportAdapter for SmilesAdapter<'_> {
    fn refusal(&self, code: &'static str, message: &str) -> Diagnostic {
        diagnostic(code, message, Map::new())
    }

    fn failure(&self, known: &Known, error: &ApplicationError) -> OperationResponse {
        known.reply(
            Outcome::Error,
            error.current_revision,
            None,
            vec![failure(error)],
        )
    }

    fn bind(
        &self,
        generation: [u8; 16],
        snapshot: &DocumentView,
        candidate: &DocumentView,
        losses: &[Loss],
    ) -> String {
        ct_projection::digest(&(
            generation,
            "smiles",
            self.source,
            snapshot,
            candidate,
            losses,
        ))
    }

    fn preview(
        self,
        snapshot: DocumentResponse,
        candidate: DocumentView,
        digest: String,
    ) -> OperationResult {
        OperationResult::SmilesPreview(Box::new(SmilesPreviewResult {
            snapshot: snapshot.into(),
            document: candidate,
            preview_digest: digest,
            atoms: self.atoms,
            bonds: self.bonds,
            parts: self.parts,
            model_id: SMILES_MODEL_ID,
        }))
    }

    fn mapping(&self, document: &DocumentView) -> CtMapping {
        ct_mapping(document, self.atoms, self.bonds)
    }

    fn consent(&self, known: &Known, losses: &[Loss]) -> Option<OperationResponse> {
        let required: Vec<&Loss> = losses.iter().filter(|loss| needs_consent(loss)).collect();
        (!required.is_empty()).then(|| consent_refusal(known, &required, None, false))
    }
}

fn check_format(known: &Known, format: &str) -> Option<OperationResponse> {
    (format != "smiles").then(|| {
        known.invalid(diagnostic(
            "unsupported_format",
            "The format must be \"smiles\" here.",
            Map::new(),
        ))
    })
}

impl<C: CapabilityCatalog, R: DocumentRepository> Application<C, R> {
    pub(super) fn smiles_import(&self, known: &Known, value: Value) -> OperationResponse {
        let p: SmilesImportParameters = match parameters(value) {
            Ok(p) => p,
            Err(error) => return known.invalid(error),
        };
        if let Some(refusal) = check_format(known, &p.format) {
            return refusal;
        }
        let Some(codec) = &self.smiles_codec else {
            return known.not_running();
        };
        let fingerprint = ct_projection::digest(&p);
        let replay = if p.preview {
            None
        } else {
            match self.documents.replay_import(
                &p.document_id,
                Some(&known.request_id),
                &fingerprint,
            ) {
                Ok(replay) => replay,
                Err(error) => return known.reply(Outcome::Error, error.current_revision, None, vec![failure(&error)]),
            }
        };
        let read = match codec.read(&p.source) {
            Ok(read) => read,
            Err(error) => return refuse_codec(known, error),
        };
        let losses = read_losses(&read);
        let (atoms, bonds) = (read.atoms.len(), read.bonds.len());
        if let Some(response) = replay {
            // A completed request is answered from its record: nothing is placed again.
            let mut result: DocumentResult = response.into();
            result.import_mapping = Some(ct_mapping(&result.document, atoms, bonds));
            return converted(
                known,
                Some(result.document.revision),
                OperationResult::Document(Box::new(result)),
                losses,
            );
        }
        let target = match self.document(&p.document_id) {
            Ok(response) => response,
            Err(error) => return known.reply(Outcome::Error, error.current_revision, None, vec![failure(&error)]),
        };
        let commands = match import_commands(&read, &target.document) {
            Ok(commands) => commands,
            Err(error) => return known.invalid(error),
        };
        let mut response = self.run_import(
            known,
            ImportRun {
                document_id: &p.document_id,
                expected_revision: p.expected_revision,
                preview: p.preview,
                preview_digest: p.preview_digest.as_deref(),
                accept_loss: p.accept_loss,
                fingerprint,
                generation: self.smiles_generation,
                target: &target,
                commands: &commands,
                losses,
            },
            SmilesAdapter {
                source: &p.source,
                atoms,
                bonds,
                parts: read.parts,
            },
        );
        if response.result.is_some() {
            response.model_id = Some(SMILES_MODEL_ID.into());
        }
        response
    }

    pub(super) fn smiles_export(&self, known: &Known, value: Value) -> OperationResponse {
        let p: SmilesExportParameters = match parameters(value) {
            Ok(p) => p,
            Err(error) => return known.invalid(error),
        };
        if let Some(refusal) = check_format(known, &p.format) {
            return refusal;
        }
        let Some(codec) = &self.smiles_codec else {
            return known.not_running();
        };
        let (snapshot, graph) = match self.documents.graph_snapshot(&p.document_id) {
            Ok(pair) => pair,
            Err(error) => return known.reply(Outcome::Error, error.current_revision, None, vec![failure(&error)]),
        };
        let style = SmilesStyle::from(p.style);
        let written: SmilesWrite = match codec.write(&graph, style) {
            Ok(written) => written,
            Err(error) => return refuse_codec(known, error),
        };
        let mut losses = change_losses(&written.changes, &written.folded_hydrogens);
        drawing_losses::drawing_losses(
            &snapshot.document,
            "document",
            &drawing_losses::SMILES,
            &mut losses,
        );
        let required: Vec<&Loss> = losses.iter().filter(|loss| needs_consent(loss)).collect();
        if !required.is_empty() {
            let digest = consent_digest(&required, written.text.as_bytes());
            if p.accepted_losses.as_deref() != Some(digest.as_str()) {
                return consent_refusal(known, &required, Some(&digest), p.accepted_losses.is_some());
            }
        }
        let revision = snapshot.document.revision;
        let (model_id, canonical) = match style {
            SmilesStyle::Recorded => (SMILES_MODEL_ID, None),
            SmilesStyle::Canonical => (
                SMILES_CANONICAL_ID,
                Some(CanonicalIdentity {
                    implementation: SMILES_CANONICAL_ID,
                    graph_model: CANONICAL_GRAPH_MODEL,
                }),
            ),
        };
        let mut response = converted(
            known,
            Some(revision),
            OperationResult::SmilesExport(Box::new(SmilesExportResult {
                snapshot: snapshot.into(),
                style: style.as_str(),
                content: written.text,
                atom_order: written.atom_order.iter().map(ToString::to_string).collect(),
                folded_hydrogens: written
                    .folded_hydrogens
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
                model_id,
                canonical,
            })),
            losses,
        );
        response.model_id = Some(model_id.into());
        response
    }
}
