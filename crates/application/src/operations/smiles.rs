//! SMILES import and export as `document.import` and `document.export` with `format: "smiles"`.
//!
//! Import is the shared preview, consent and commit flow. Export is read-only: it writes one
//! document's graph as recorded or canonical text and returns the exact snapshot it wrote.

use chemifree_domain::chemistry::VALENCE_MODEL;
use chemifree_domain::document::{AtomRef, Command, NewAtomPositions, Point};
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::identity::CHEMICAL_MODEL_ID;
use chemifree_domain::states::AttachmentPoints;

use super::import::{ImportFormat, ImportRequest, Staged};
use super::interchange::{consent_diagnostic, export_consent_refusal, needs_consent};
use super::*;
use crate::contract::{BondOrderDto, HydrogensDto, RadicalDto};
use crate::smiles::{SmilesCodec, SmilesError, SmilesRead, SmilesStyle, Span};

/// Where new atoms go: a grid, in input order, below whatever the drawing already holds.
const GRID_SPACING: f64 = 30.0;
const GRID_COLUMNS: usize = 10;
const GAP_BELOW_DRAWING: f64 = 60.0;
const EMPTY_DRAWING_ORIGIN: f64 = 40.0;

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SmilesImportParameters {
    document_id: String,
    expected_revision: u64,
    format: String,
    source: String,
    #[serde(default)]
    preview: bool,
    #[serde(default)]
    preview_digest: Option<String>,
    #[serde(default)]
    accept_loss: bool,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SmilesExportParameters {
    format: String,
    document_id: String,
    style: SmilesStyle,
    /// The `loss_digest` of the refusal that showed the assumptions the text makes.
    #[serde(default)]
    accepted_losses: Option<String>,
}

/// A part of the source, in bytes (the API's unit) and in characters (the user's).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct SpanView {
    pub offset: usize,
    pub length: usize,
    pub character_offset: usize,
    pub character_length: usize,
}

/// Converts byte offsets of one source to character offsets by scanning it, without building a
/// table the size of the input.
struct Characters<'a> {
    source: &'a str,
}

impl<'a> Characters<'a> {
    fn new(source: &'a str) -> Self {
        Self { source }
    }

    /// The number of characters that start before `byte`; a position past the end counts all.
    fn before(&self, byte: usize) -> usize {
        self.source
            .char_indices()
            .take_while(|(start, _)| *start < byte)
            .count()
    }

    fn view(&self, span: Span) -> SpanView {
        let first = self.before(span.offset);
        let last = self.before(span.offset.saturating_add(span.length));
        SpanView {
            offset: span.offset,
            length: span.length,
            character_offset: first,
            character_length: last.saturating_sub(first),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SmilesPreviewAtom {
    /// One-based position among the atoms of the string.
    pub index: usize,
    pub element: String,
    pub charge: i8,
    pub isotope: Option<u16>,
    pub hydrogens: HydrogensDto,
    pub radical: RadicalDto,
    pub span: SpanView,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SmilesPreviewBond {
    /// One-based indices of the atoms in the string.
    pub begin: usize,
    pub end: usize,
    pub order: BondOrderDto,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SmilesPreviewResult {
    pub atoms: Vec<SmilesPreviewAtom>,
    pub bonds: Vec<SmilesPreviewBond>,
    /// Non-whitespace text after the first whitespace, which is not read.
    pub ignored_text: Option<SpanView>,
    /// The exact working snapshot captured under the same lock as the candidate.
    pub snapshot: DocumentResult,
    pub document: DocumentView,
    pub preview_digest: String,
    pub valence_model: &'static str,
    pub aromaticity_model: &'static str,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SmilesExportResult {
    pub content: String,
    pub style: SmilesStyle,
    /// The exact working copy the text was written from.
    pub snapshot: DocumentResult,
    /// The document atom written at each position of the text, in order.
    pub atom_order: Vec<String>,
    /// Hydrogen vertices folded into a neighbour's count; they are not in `atom_order`.
    pub folded_hydrogens: Vec<FoldedHydrogen>,
    pub valence_model: &'static str,
    pub aromaticity_model: &'static str,
    /// The chemical identity model a canonical text is bound to.
    pub graph_model: Option<&'static str>,
}

/// A refusal of the reader or writer as a diagnostic, with the offending part of `source` in
/// bytes and in characters.
fn smiles_diagnostic(error: &SmilesError, source: Option<&str>) -> Diagnostic {
    let mut details = Map::new();
    details.insert("class".into(), error.code.class().as_str().into());
    if let Some(reason) = &error.reason {
        details.insert("reason".into(), reason.clone().into());
    }
    if let Some(span) = error.span {
        details.insert("offset".into(), span.offset.into());
        details.insert("length".into(), span.length.into());
        if let Some(source) = source {
            let view = Characters::new(source).view(span);
            details.insert("character_offset".into(), view.character_offset.into());
            details.insert("character_length".into(), view.character_length.into());
        }
    }
    let message = match &error.reason {
        Some(reason) => format!("{} ({reason})", error.code.message()),
        None => error.code.message().into(),
    };
    diagnostic(error.code.as_str(), message, details)
}

struct SmilesImportFormat<'a> {
    codec: &'a dyn SmilesCodec,
    p: &'a SmilesImportParameters,
}

/// Grid positions for new atoms, below `target`'s atoms, or `None` when one is out of range.
fn grid_commands(read: &SmilesRead, target: &DocumentView) -> Result<Vec<Command>, Diagnostic> {
    let (left, bottom) = target
        .atoms
        .iter()
        .fold(None, |extent: Option<(f64, f64)>, atom| {
            Some(match extent {
                Some((left, bottom)) => (left.min(atom.position.x), bottom.max(atom.position.y)),
                None => (atom.position.x, atom.position.y),
            })
        })
        .map_or(
            (EMPTY_DRAWING_ORIGIN, EMPTY_DRAWING_ORIGIN),
            |(left, bottom)| (left, bottom + GAP_BELOW_DRAWING),
        );
    let mut commands = Vec::with_capacity(read.atoms.len() + read.bonds.len());
    for (index, source) in read.atoms.iter().enumerate() {
        let point = Point::new(
            left + (index % GRID_COLUMNS) as f64 * GRID_SPACING,
            bottom + (index / GRID_COLUMNS) as f64 * GRID_SPACING,
        )
        .map_err(|_| {
            diagnostic(
                "coordinate_out_of_range",
                format!(
                    "Atom {} cannot be placed below the existing drawing.",
                    index + 1
                ),
                Map::new(),
            )
        })?;
        commands.push(Command::AddAtom {
            atom: source.atom.clone(),
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

fn generated_layout() -> Loss {
    Loss {
        consent: LossConsent::NotRequired,
        feature: "generated_layout".into(),
        source_path: "source".into(),
        disposition: LossDisposition::Normalized,
        reason: "SMILES has no coordinates and no structure layout is implemented: atoms are placed on a grid in input order below the existing drawing.".into(),
        semantic_impact: SemanticImpact::Geometry,
    }
}

impl ImportFormat for SmilesImportFormat<'_> {
    type Parsed = SmilesRead;

    fn parse(&self) -> Result<SmilesRead, Diagnostic> {
        self.codec
            .read(&self.p.source)
            .map_err(|error| smiles_diagnostic(&error, Some(&self.p.source)))
    }

    fn stage(
        &self,
        read: &SmilesRead,
        target: &DocumentView,
        replayed: bool,
    ) -> Result<Staged, Diagnostic> {
        if target.scene.instances.len() != 1 {
            return Err(diagnostic(
                "unsupported_scene",
                "Import requires exactly one molecule placement.",
                Map::new(),
            ));
        }
        let mut losses = read.losses.clone();
        losses.push(generated_layout());
        // A replayed import has committed already; its placement is not computed again.
        let commands = if replayed {
            Vec::new()
        } else {
            grid_commands(read, target)?
        };
        Ok(Staged {
            commands,
            losses,
            atoms: read.atoms.len(),
            bonds: read.bonds.len(),
        })
    }

    fn digest(
        &self,
        generation: [u8; 16],
        _read: &SmilesRead,
        snapshot: &DocumentView,
        document: &DocumentView,
        losses: &[Loss],
    ) -> String {
        ct_projection::digest(&(
            generation,
            &self.p.format,
            &self.p.source,
            snapshot,
            document,
            losses,
        ))
    }

    fn preview(
        &self,
        read: SmilesRead,
        snapshot: DocumentResponse,
        document: DocumentView,
        preview_digest: String,
    ) -> OperationResult {
        let characters = Characters::new(&self.p.source);
        OperationResult::SmilesPreview(Box::new(SmilesPreviewResult {
            atoms: read
                .atoms
                .iter()
                .enumerate()
                .map(|(index, atom)| SmilesPreviewAtom {
                    index: index + 1,
                    element: atom
                        .atom
                        .kind
                        .element()
                        .map_or_else(String::new, |element| element.symbol().to_owned()),
                    charge: atom.atom.charge,
                    isotope: atom.atom.isotope,
                    hydrogens: atom.atom.hydrogens.into(),
                    radical: atom.atom.radical.into(),
                    span: characters.view(atom.span),
                })
                .collect(),
            bonds: read
                .bonds
                .iter()
                .map(|bond| SmilesPreviewBond {
                    begin: bond.begin + 1,
                    end: bond.end + 1,
                    order: (&bond.order).into(),
                })
                .collect(),
            ignored_text: read.ignored_text.map(|span| characters.view(span)),
            snapshot: snapshot.into(),
            document,
            preview_digest,
            valence_model: VALENCE_MODEL,
            aromaticity_model: chemifree_domain::aromaticity::MODEL_ID,
        }))
    }

    /// Only a loss that changes the chemistry or drops source content needs the caller's
    /// acceptance; the generated layout is reported without it.
    fn needs_accept(&self, losses: &[Loss]) -> bool {
        losses.iter().any(needs_consent)
    }

    fn refusal(&self, code: &'static str, message: &str, losses: &[Loss]) -> Diagnostic {
        if code == "loss_consent_required" {
            let required: Vec<&Loss> = losses.iter().filter(|loss| needs_consent(loss)).collect();
            consent_diagnostic(code, message, &required, None)
        } else {
            diagnostic(code, message, Map::new())
        }
    }

    fn document_error(&self, known: &Known, error: &ApplicationError) -> OperationResponse {
        known.reply(
            Outcome::Error,
            error.current_revision,
            None,
            vec![failure(error)],
        )
    }
}

/// The graph without the attachment points SMILES cannot state, and the consent-required loss
/// naming the atoms that lost them. `None` when the graph holds none.
fn without_attachment_points(graph: &MoleculeGraph) -> Option<(MoleculeGraph, Loss)> {
    let marked: Vec<String> = graph
        .atoms()
        .filter(|(_, atom)| !atom.attachment_points.is_empty())
        .map(|(id, _)| id.to_string())
        .collect();
    if marked.is_empty() {
        return None;
    }
    let stripped = MoleculeGraph::from_parts(
        graph
            .atoms()
            .map(|(id, atom)| {
                let mut atom = atom.clone();
                atom.attachment_points = AttachmentPoints::default();
                (id, atom)
            })
            .collect(),
        graph.bonds().map(|(id, bond)| (id, bond.clone())).collect(),
    );
    let loss = Loss {
        consent: LossConsent::Required,
        feature: "attachment_points".into(),
        source_path: format!("atoms[{}]", marked.join(", ")),
        disposition: LossDisposition::Dropped,
        reason: "Attachment points have no SMILES form; the text is written without them.".into(),
        semantic_impact: SemanticImpact::Chemistry,
    };
    Some((stripped, loss))
}

impl<C: CapabilityCatalog, R: DocumentRepository> Application<C, R> {
    pub(super) fn smiles_import(
        &self,
        known: &Known,
        p: SmilesImportParameters,
    ) -> OperationResponse {
        let Some(codec) = &self.smiles_codec else {
            return known.not_running();
        };
        let request = ImportRequest {
            document_id: &p.document_id,
            expected_revision: p.expected_revision,
            preview: p.preview,
            preview_digest: p.preview_digest.as_deref(),
            accept_loss: p.accept_loss,
            fingerprint: ct_projection::digest(&p),
        };
        let format = SmilesImportFormat {
            codec: codec.as_ref(),
            p: &p,
        };
        let mut response = self.run_import(known, &format, request, self.preview_generation);
        if response.result.is_some() {
            response.model_id = Some(crate::smiles::SMILES_MODEL.into());
        }
        response
    }

    pub(super) fn smiles_export(
        &self,
        known: &Known,
        p: SmilesExportParameters,
    ) -> OperationResponse {
        let Some(codec) = &self.smiles_codec else {
            return known.not_running();
        };
        debug_assert_eq!(p.format, "smiles", "the dispatcher routes only SMILES here");
        let (snapshot, graph) = match self.documents.graph_snapshot(&p.document_id) {
            Ok(captured) => captured,
            Err(error) => {
                return known.reply(
                    Outcome::Error,
                    error.current_revision,
                    None,
                    vec![failure(&error)],
                );
            }
        };
        let mut losses = Vec::new();
        ct_projection::drawing_losses(
            &snapshot.document,
            "document",
            ct_projection::Target::Smiles,
            &mut losses,
        );
        // Attachment points are the one state a SMILES string drops with the caller's consent.
        let stripped = without_attachment_points(&graph);
        let written = match &stripped {
            Some((graph, loss)) => {
                losses.push(loss.clone());
                codec.write(graph, p.style)
            }
            None => codec.write(&graph, p.style),
        };
        let written = match written {
            Ok(written) => written,
            Err(error) => return known.invalid(smiles_diagnostic(&error, None)),
        };
        losses.extend(written.losses);
        if let Some(refusal) = export_consent_refusal(
            &losses,
            written.text.as_bytes(),
            p.accepted_losses.as_deref(),
        ) {
            return known.invalid(refusal);
        }
        let revision = snapshot.document.revision;
        let result = SmilesExportResult {
            content: written.text,
            style: p.style,
            snapshot: snapshot.into(),
            atom_order: written.atom_order.iter().map(ToString::to_string).collect(),
            folded_hydrogens: written
                .folded_hydrogens
                .iter()
                .map(|(hydrogen, parent)| FoldedHydrogen {
                    hydrogen: hydrogen.to_string(),
                    attached_to: parent.to_string(),
                })
                .collect(),
            valence_model: VALENCE_MODEL,
            aromaticity_model: chemifree_domain::aromaticity::MODEL_ID,
            graph_model: (p.style == SmilesStyle::Canonical).then_some(CHEMICAL_MODEL_ID),
        };
        let mut response = converted(
            known,
            Some(revision),
            OperationResult::SmilesExport(Box::new(result)),
            losses,
        );
        response.model_id = Some(p.style.model_id().into());
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn character_offsets_count_characters_not_bytes() {
        let view = Characters::new("C\u{e9}O").view(Span::new(1, 2));
        assert_eq!((view.character_offset, view.character_length), (1, 1));
        let after = Characters::new("C\u{e9}O").view(Span::new(3, 1));
        assert_eq!((after.character_offset, after.character_length), (2, 1));
    }

    #[test]
    fn a_span_past_the_end_neither_underflows_nor_panics() {
        let characters = Characters::new("CCO");
        let past = characters.view(Span::new(10, 5));
        assert_eq!((past.character_offset, past.character_length), (3, 0));
        let straddling = characters.view(Span::new(2, usize::MAX));
        assert_eq!(
            (straddling.character_offset, straddling.character_length),
            (2, 1)
        );
        let empty = Characters::new("").view(Span::new(0, 0));
        assert_eq!((empty.character_offset, empty.character_length), (0, 0));
    }
}
