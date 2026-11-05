//! SMILES import and export as `document.import` and `document.export` with `format: "smiles"`.
//!
//! Import is the shared preview, consent and commit flow. Export is read-only: it writes one
//! document's graph as recorded or canonical text and returns the exact snapshot it wrote.

use chemifree_domain::chemistry::VALENCE_MODEL;
use std::collections::HashSet;

use chemifree_domain::document::{AtomRef, Command, NewAtomPositions, Point};
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::identity::CHEMICAL_MODEL_ID;
use chemifree_domain::states::AttachmentPoints;
use chemifree_domain::stereo::{AtomConfiguration, SlotRef};

use super::import::{ImportFormat, ImportRequest, Staged};
use super::interchange::{consent_diagnostic, export_consent_refusal, needs_consent};
use super::*;
use crate::contract::{BondOrderDto, HydrogensDto, RadicalDto, SceneObjectView};
use crate::smiles::{
    ErrorClass, SmilesCode, SmilesCodec, SmilesError, SmilesRead, SmilesSlot, SmilesStyle, Span,
};
use crate::stereo_projection::{self, GroupFault, GroupInput};

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

/// Converts byte offsets of one source to character offsets: by one scan for a single span, or
/// through the table of where each character starts for the many spans of a preview.
struct Characters {
    /// The byte offset at which each character starts, in order.
    starts: Vec<usize>,
}

impl Characters {
    /// The table for a source already known to be within the codec's input limit.
    fn new(source: &str) -> Self {
        Self {
            starts: source.char_indices().map(|(start, _)| start).collect(),
        }
    }

    /// The number of characters that start before `byte`; a position past the end counts all.
    fn before(&self, byte: usize) -> usize {
        self.starts.partition_point(|start| *start < byte)
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

/// The same conversion for one span of a source of any length, with one scan and no table:
/// refusals of an oversize source must not allocate in proportion to it.
fn view_once(source: &str, span: Span) -> SpanView {
    let before = |byte: usize| {
        source
            .char_indices()
            .take_while(|(start, _)| *start < byte)
            .count()
    };
    let first = before(span.offset);
    let last = before(span.offset.saturating_add(span.length));
    SpanView {
        offset: span.offset,
        length: span.length,
        character_offset: first,
        character_length: last.saturating_sub(first),
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
    if let Some(family) = error.code.class().family() {
        details.insert("family".into(), family.into());
    }
    if let Some(reason) = &error.reason {
        details.insert("reason".into(), reason.clone().into());
    }
    if let Some(span) = error.span {
        details.insert("offset".into(), span.offset.into());
        details.insert("length".into(), span.length.into());
        if let Some(source) = source {
            let view = view_once(source, span);
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

/// The commands of the whole import: atoms on a grid below `target`'s atoms, the bonds, the
/// configurations the text's tetrahedral marks state (no marks: the atoms have no coordinates
/// that could show them) and the enhanced stereo groups its CXSMILES block lists.
fn import_commands(
    read: &SmilesRead,
    source: &str,
    target: &DocumentView,
    losses: &mut Vec<Loss>,
) -> Result<Vec<Command>, Diagnostic> {
    let mut commands = grid_commands(read, target)?;
    let created = |slot: SmilesSlot| match slot {
        SmilesSlot::Atom(index) => SlotRef::Atom(AtomRef::Created(index)),
        SmilesSlot::ImplicitHydrogen => SlotRef::ImplicitHydrogen,
    };
    for center in &read.stereo.centers {
        commands.push(Command::SetAtomConfiguration {
            atom: AtomRef::Created(center.atom),
            configuration: Some(AtomConfiguration::Tetrahedral {
                neighbors: center.slots.map(created),
                turn: center.turn,
            }),
        });
    }
    let groups = group_inputs(read);
    let centers: HashSet<usize> = read.stereo.centers.iter().map(|c| c.atom).collect();
    let group_commands = stereo_projection::group_commands(
        &groups,
        target,
        Some(&centers),
        |k| group_path(read, k),
        losses,
    )
    .map_err(|fault| group_fault_diagnostic(fault, read, source))?;
    commands.extend(group_commands);
    Ok(commands)
}

/// A group the mapping cannot make, as the diagnostic of its own cause.
fn group_fault_diagnostic(fault: GroupFault, read: &SmilesRead, source: &str) -> Diagnostic {
    match fault {
        GroupFault::NoFreeNumber => {
            let mut details = Map::new();
            details.insert("class".into(), ErrorClass::Limit.as_str().into());
            details.insert("family".into(), "resource_limit".into());
            diagnostic(
                "resource_limit",
                "No positive stereo group number is left to renumber a group that collides with one already in the document.",
                details,
            )
        }
        // The centers handed over are the text's own, so the drawing is always readable.
        GroupFault::DrawingUnreadable => diagnostic(
            "internal_error",
            "The stereo groups of the text could not be mapped.",
            Map::new(),
        ),
        GroupFault::MemberNotStereocenter { group, atom } => {
            let error = SmilesError::at(
                SmilesCode::CxsmilesGroupMemberNotStereocenter,
                read.stereo.groups[group].span,
            );
            let mut found = smiles_diagnostic(&error, Some(source));
            found.details.insert("group".into(), group.into());
            found.details.insert("atom".into(), atom.into());
            found
        }
    }
}

/// The file's groups as the mapping takes them.
fn group_inputs(read: &SmilesRead) -> Vec<GroupInput> {
    read.stereo
        .groups
        .iter()
        .map(|group| GroupInput {
            kind: group.kind,
            number: group.number,
            atoms: group.atoms.clone(),
        })
        .collect()
}

fn group_path(read: &SmilesRead, k: usize) -> String {
    format!("source[{}]", read.stereo.groups[k].span.describe())
}

/// The group normalization losses of an import that has committed, from the document it made.
fn replayed_group_losses(read: &SmilesRead, committed: &DocumentView) -> Vec<Loss> {
    let created: Vec<String> = committed
        .atoms
        .iter()
        .skip(committed.atoms.len().saturating_sub(read.atoms.len()))
        .map(|atom| atom.id.clone())
        .collect();
    stereo_projection::replayed_group_losses(&group_inputs(read), committed, &created, |k| {
        group_path(read, k)
    })
}

/// Grid positions for new atoms, below `target`'s atoms, or `None` when one is out of range.
fn grid_commands(read: &SmilesRead, target: &DocumentView) -> Result<Vec<Command>, Diagnostic> {
    // Everything the drawing holds that has a position: atoms and the points of graphics.
    let graphic_points = target.scene.objects.iter().flat_map(|object| match object {
        SceneObjectView::Graphic { points, .. } => points.iter().collect::<Vec<_>>(),
        SceneObjectView::Instance { .. } | SceneObjectView::Group { .. } => Vec::new(),
    });
    let (left, bottom) = target
        .atoms
        .iter()
        .map(|atom| &atom.position)
        .chain(graphic_points)
        .fold(None, |extent: Option<(f64, f64)>, position| {
            Some(match extent {
                Some((left, bottom)) => (left.min(position.x), bottom.max(position.y)),
                None => (position.x, position.y),
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
            // The document is the one the commit produced: the losses the group mapping
            // reported then are found again from it, so a replay reports what the import did.
            losses.extend(replayed_group_losses(read, target));
            Vec::new()
        } else {
            import_commands(read, &self.p.source, target, &mut losses)?
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
    let mut stripped = graph.clone();
    for (_, atom) in stripped.atoms_mut() {
        atom.attachment_points = AttachmentPoints::default();
    }
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
        let (snapshot, graph, groups) =
            match self.documents.graph_and_groups_snapshot(&p.document_id) {
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
        // Recorded stereo is accounted for by the codec, including partial preservation.
        // Canonical output still omits all document stereo under its existing graph model.
        if p.style == SmilesStyle::Canonical {
            losses.extend(ct_projection::stereo_losses(&snapshot.document));
        }
        // Attachment points have no SMILES form and require the caller's consent.
        let stripped = without_attachment_points(&graph);
        let written = match &stripped {
            Some((graph, loss)) => {
                losses.push(loss.clone());
                codec.write_with_groups(graph, p.style, &groups)
            }
            None => codec.write_with_groups(&graph, p.style, &groups),
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

    fn both(source: &str, span: Span) -> (SpanView, SpanView) {
        (Characters::new(source).view(span), view_once(source, span))
    }

    #[test]
    fn character_offsets_count_characters_not_bytes() {
        let (table, scan) = both("C\u{e9}O", Span::new(1, 2));
        assert_eq!((table.character_offset, table.character_length), (1, 1));
        assert_eq!(table, scan);
        let (table, scan) = both("C\u{e9}O", Span::new(3, 1));
        assert_eq!((table.character_offset, table.character_length), (2, 1));
        assert_eq!(table, scan);
    }

    #[test]
    fn a_span_past_the_end_neither_underflows_nor_panics() {
        for (source, span, expected) in [
            ("CCO", Span::new(10, 5), (3, 0)),
            ("CCO", Span::new(2, usize::MAX), (2, 1)),
            ("", Span::new(0, 0), (0, 0)),
        ] {
            let (table, scan) = both(source, span);
            assert_eq!((table.character_offset, table.character_length), expected);
            assert_eq!(table, scan);
        }
    }

    #[test]
    fn a_table_converts_many_spans_of_a_large_source_in_linear_time() {
        // 8,192 two-byte characters fill the 16 KiB input limit. A scan from the start for
        // each of 200,000 views would take about 8e8 steps (many seconds in a test build);
        // the table takes a binary search each.
        let source = "\u{e9}".repeat(8192);
        let table = Characters::new(&source);
        let started = std::time::Instant::now();
        let mut total = 0;
        for index in 0..200_000_usize {
            total += table
                .view(Span::new((index * 2) % 16_384, 2))
                .character_offset;
        }
        assert!(total > 0);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(table.view(Span::new(16_382, 2)).character_offset, 8191);
    }
}

#[cfg(test)]
mod group_fault_tests {
    use chemifree_domain::stereo_group::GroupKind;

    use super::*;
    use crate::smiles::{SmilesGroup, SmilesStereo};

    fn read_with_group() -> SmilesRead {
        SmilesRead {
            atoms: Vec::new(),
            bonds: Vec::new(),
            ignored_text: None,
            losses: Vec::new(),
            stereo: SmilesStereo {
                centers: Vec::new(),
                groups: vec![SmilesGroup {
                    kind: GroupKind::Or,
                    number: Some(1),
                    atoms: vec![0],
                    span: Span::new(14, 4),
                }],
            },
        }
    }

    #[test]
    fn each_group_fault_has_its_own_diagnostic_with_class_family_and_place() {
        let read = read_with_group();
        let source = "F[C@H](Cl)Br |o1:0|";
        let member = group_fault_diagnostic(
            GroupFault::MemberNotStereocenter { group: 0, atom: 0 },
            &read,
            source,
        );
        assert_eq!(member.code, "cxsmiles_group_member_not_stereocenter");
        assert_eq!(member.details["class"], "chemistry");
        assert_eq!(member.details["family"], "invalid_chemistry");
        assert_eq!(member.details["offset"], 14);
        assert_eq!(member.details["group"], 0);
        assert_eq!(member.details["atom"], 0);
        let exhausted = group_fault_diagnostic(GroupFault::NoFreeNumber, &read, source);
        assert_eq!(exhausted.code, "resource_limit");
        assert_eq!(exhausted.details["family"], "resource_limit");
        let unreadable = group_fault_diagnostic(GroupFault::DrawingUnreadable, &read, source);
        assert_eq!(unreadable.code, "internal_error");
    }
}
