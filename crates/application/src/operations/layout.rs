//! `layout.structure` in preview mode: the coordinates a chain layout would give the selected
//! atoms, with the sets, boundary and components they were derived from.
//!
//! Read-only. The document is copied under one lock, so the reply names the exact snapshot the
//! preview is about. The preparation runs outside the lock and is not interruptible; the placement
//! runs under the request's time limit and the host cancel, and takes no compute slot. Nothing is
//! changed: the document keeps its revision, history, selection and dirty state. A stop replies
//! `error` with `time_limit` or `cancelled`, the snapshot's revision and no result. Ring layout,
//! templates, scoring, collision resolution and applying a layout are not implemented.

use chemifree_domain::document::{ObjectId, Point};
use chemifree_domain::layout::{
    self, LayoutError, MODEL_ID, OVERLAP_FACTOR, PlacedComponent, Preparation, Preview, Scope,
};

use super::*;
use crate::contract::PositionView;

/// The only mode. A preview states that nothing is applied.
const PREVIEW: &str = "preview";
/// The most overlapping pairs one reply lists. The count it reports is the whole count.
const MAX_OVERLAP_PAIRS: usize = 100;

/// The reasons of a `layout_unsupported` refusal, as its `reason` detail names them. Only the
/// unit test reads this list; it checks the list against `refusal_reply`.
#[cfg(test)]
pub const LAYOUT_REASONS: [&str; 7] = [
    "object_selection",
    "ring_in_layout_set",
    "stereo_in_layout",
    "display_in_layout_set",
    "multiple_attachments",
    "coincident_fixed_atoms",
    "coordinate_out_of_range",
];

/// The parameters of `layout.structure`. The handler checks the mode and the scope, so that a
/// refusal names the parameter it concerns.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LayoutParameters {
    document_id: String,
    /// Must be `"preview"`, the only mode.
    mode: String,
    selection: SelectionDto,
    /// `"components"` (the default) or `"selection"`.
    #[serde(default)]
    scope: Option<String>,
    /// A scene instance. It may be omitted only while the document has exactly one.
    #[serde(default)]
    instance: Option<String>,
    /// The time limit this request asks for, in milliseconds: any value up to the server's maximum
    /// (so it may be above the server's default); above the maximum it is refused.
    #[serde(default)]
    time_limit_ms: Option<u64>,
}

/// The atoms and bonds that the layout moves, ascending.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LayoutSetView {
    pub atoms: Vec<String>,
    pub bonds: Vec<String>,
}

/// A bond between a layout atom and a fixed atom. The fixed atom stays where it is.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BoundaryView {
    pub atom: String,
    pub fixed_atom: String,
    pub bond: String,
}

/// The boundary bond that anchors a layout component to a fixed atom.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AnchorView {
    pub fixed_atom: String,
    pub bond: String,
}

/// A run of atoms placed one after another, in placement order.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ChainView {
    pub atoms: Vec<String>,
    /// The fixed anchor, the parent atom, or `null` for the first chain of a free component.
    pub branch_from: Option<String>,
}

/// A connected component of the layout set, in placement order.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LayoutComponentView {
    pub atoms: Vec<String>,
    pub bonds: Vec<String>,
    /// The boundary bond that anchors the component, or `null` for a free component.
    pub anchor: Option<AnchorView>,
    /// The atom that placement started from.
    pub root: String,
    pub chains: Vec<ChainView>,
}

/// Where one layout atom stands before and after placement, in pt with y growing down.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LayoutPositionView {
    pub atom: String,
    pub before: PositionView,
    pub after: PositionView,
}

/// The read-only preview: the exact snapshot it is about, the sets and boundary it was derived
/// from, and the proposed coordinates. `positions` holds exactly the layout atoms.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LayoutResult {
    pub snapshot: DocumentResult,
    /// Always `"preview"`.
    pub mode: &'static str,
    pub instance: String,
    /// `"components"` or `"selection"`.
    pub scope: &'static str,
    pub bond_length_pt: f64,
    pub layout: LayoutSetView,
    pub fixed_atom_count: usize,
    pub fixed_ring_systems: usize,
    pub boundary: Vec<BoundaryView>,
    pub components: Vec<LayoutComponentView>,
    pub positions: Vec<LayoutPositionView>,
}

impl<C: CapabilityCatalog, R: DocumentRepository> Application<C, R> {
    /// Previews the layout that `p` asks for. `Ok` carries the result, the snapshot's revision and
    /// the diagnostics. `Err` is the complete reply of a refusal or a stop, with `model_id` set.
    pub(super) fn layout_preview(
        &self,
        known: &Known,
        p: LayoutParameters,
        cancel: &dyn Interrupt,
    ) -> Result<(LayoutResult, u64, Vec<Diagnostic>), Box<OperationResponse>> {
        // The limit is read first, so a limit above the maximum is refused before anything else.
        let (budget, limit_ms) =
            self.start_time_limit_or_refuse(known, p.time_limit_ms, cancel, MODEL_ID)?;
        if p.mode != PREVIEW {
            let message = format!(
                "mode {:?} is not supported: the only mode is \"preview\", and applying a layout is not implemented.",
                p.mode
            );
            let details = fields([("supported", Value::from(vec![PREVIEW]))]);
            let diagnostic = invalid_request("parameters.mode", message, details);
            return Err(model_reply(known, Outcome::Error, None, diagnostic));
        }
        let scope = match p.scope.as_deref() {
            None | Some("components") => Scope::Components,
            Some("selection") => Scope::Selection,
            Some(other) => {
                let message = format!(
                    "scope {other:?} is not supported: use \"components\" or \"selection\"."
                );
                let diagnostic = invalid_request("parameters.scope", message, Map::new());
                return Err(model_reply(known, Outcome::Error, None, diagnostic));
            }
        };
        let requested = match p
            .instance
            .as_deref()
            .map(crate::contract::instance_id)
            .transpose()
        {
            Ok(requested) => requested,
            Err(message) => {
                let diagnostic = invalid_request("parameters.instance", message, Map::new());
                return Err(model_reply(known, Outcome::Error, None, diagnostic));
            }
        };
        let selection = match crate::contract::exact_selection(&p.selection) {
            Ok(selection) => selection,
            Err(message) => {
                let details = fields([("reason", "selection_unknown".into())]);
                let diagnostic = invalid_request("parameters.selection", message, details);
                return Err(model_reply(known, Outcome::Error, None, diagnostic));
            }
        };
        let (snapshot, document) = match self.documents.analysis_snapshot(&p.document_id) {
            Ok(captured) => captured,
            Err(error) => {
                let diagnostic = failure(&error);
                return Err(model_reply(
                    known,
                    Outcome::Error,
                    error.current_revision,
                    diagnostic,
                ));
            }
        };
        let revision = snapshot.document.revision;
        let instances: Vec<ObjectId> = document.instances().collect();
        let instance = match (requested, instances.as_slice()) {
            (Some(id), _) if instances.contains(&id) => id,
            (Some(id), _) => {
                let message = format!("{id} is not a scene instance of this document.");
                let diagnostic = invalid_request("parameters.instance", message, Map::new());
                return Err(model_reply(known, Outcome::Error, None, diagnostic));
            }
            (None, [only]) => *only,
            (None, _) => {
                let message = format!(
                    "instance is required: the document has {} scene instances.",
                    instances.len()
                );
                let diagnostic = invalid_request("parameters.instance", message, Map::new());
                return Err(model_reply(known, Outcome::Error, None, diagnostic));
            }
        };
        let prep = layout::prepare(&document, instance, &selection, scope)
            .map_err(|error| refusal_reply(known, revision, &error))?;
        let placed =
            self.compute_read_only(&budget, |interrupt| layout::place_with(&prep, interrupt));
        let preview = match placed {
            Ok(Ok(preview)) => preview,
            Ok(Err(error)) => return Err(refusal_reply(known, revision, &error)),
            Err(interrupted) => {
                let reply = stopped_read_only(known, revision, interrupted, limit_ms, MODEL_ID);
                return Err(Box::new(reply));
            }
        };
        let diagnostics = preview_diagnostics(&prep, &preview);
        let result = LayoutResult {
            snapshot: snapshot.into(),
            mode: PREVIEW,
            instance: instance.to_string(),
            scope: scope_name(scope),
            bond_length_pt: prep.bond_length_pt,
            layout: LayoutSetView {
                atoms: ids(&prep.layout_atoms),
                bonds: ids(&prep.layout_bonds),
            },
            fixed_atom_count: prep.fixed_atom_count,
            fixed_ring_systems: prep.fixed_ring_systems,
            boundary: prep
                .boundary
                .iter()
                .map(|entry| BoundaryView {
                    atom: entry.atom.to_string(),
                    fixed_atom: entry.fixed_atom.to_string(),
                    bond: entry.bond.to_string(),
                })
                .collect(),
            components: preview.components.iter().map(component_view).collect(),
            positions: preview
                .positions
                .iter()
                .map(|placement| LayoutPositionView {
                    atom: placement.atom.to_string(),
                    before: position(placement.before),
                    after: position(placement.after),
                })
                .collect(),
        };
        Ok((result, revision, diagnostics))
    }
}

/// A reply that names the layout model. Every refusal and stop of this operation is one. It is
/// boxed, as the refusal of `start_time_limit_or_refuse` is, because the `Err` side of the handler
/// would otherwise be larger than clippy's `result_large_err` allows.
fn model_reply(
    known: &Known,
    outcome: Outcome,
    revision: Option<u64>,
    diagnostic: Diagnostic,
) -> Box<OperationResponse> {
    let mut reply = known.reply(outcome, revision, None, vec![diagnostic]);
    reply.model_id = Some(MODEL_ID.into());
    Box::new(reply)
}

/// A request the layout cannot read, at `path` in the request.
fn invalid_request(path: &str, message: String, details: Map<String, Value>) -> Diagnostic {
    Diagnostic {
        code: ErrorCode::InvalidRequest.as_str(),
        severity: Severity::Error,
        message,
        path: Some(path.into()),
        details,
    }
}

/// The reply to a refused layout, at the snapshot's `revision`. A selection that is missing or
/// empty is `invalid_request`, a layout set over the size limit is `resource_limit`, an atom
/// without a position is `document_unreadable`, and every other refusal is `layout_unsupported`.
fn refusal_reply(known: &Known, revision: u64, error: &LayoutError) -> Box<OperationResponse> {
    let (outcome, code, path) = match error {
        LayoutError::UnknownSelection | LayoutError::EmptySelection => (
            Outcome::Error,
            ErrorCode::InvalidRequest.as_str(),
            Some("parameters.selection"),
        ),
        LayoutError::TooLarge { .. } => (
            Outcome::Error,
            ErrorCode::ResourceLimit.as_str(),
            Some("parameters.selection"),
        ),
        LayoutError::MissingPosition { .. } => {
            (Outcome::Error, ErrorCode::DocumentUnreadable.as_str(), None)
        }
        LayoutError::ObjectSelection
        | LayoutError::RingInLayoutSet { .. }
        | LayoutError::StereoInLayout { .. }
        | LayoutError::DisplayInLayoutSet { .. }
        | LayoutError::MultipleAttachments { .. }
        | LayoutError::CoincidentFixedAtoms { .. }
        | LayoutError::CoordinateOutOfRange { .. } => {
            (Outcome::Unsupported, "layout_unsupported", None)
        }
    };
    let diagnostic = Diagnostic {
        code,
        severity: Severity::Error,
        message: error.to_string(),
        path: path.map(str::to_owned),
        details: refusal_details(error),
    };
    model_reply(known, outcome, Some(revision), diagnostic)
}

/// The facts a refusal names. A `reason` is given except for a layout set over the size limit,
/// which names its `atoms` and `limit`, and an atom without a position, which names its `atom`.
/// The ids are listed in ascending order.
fn refusal_details(error: &LayoutError) -> Map<String, Value> {
    let reason: Value = error.reason().into();
    match error {
        LayoutError::ObjectSelection
        | LayoutError::UnknownSelection
        | LayoutError::EmptySelection => fields([("reason", reason)]),
        LayoutError::TooLarge { atoms, limit } => {
            fields([("atoms", (*atoms).into()), ("limit", (*limit).into())])
        }
        LayoutError::MissingPosition { atom } => fields([("atom", atom.to_string().into())]),
        LayoutError::RingInLayoutSet { atoms } | LayoutError::DisplayInLayoutSet { atoms } => {
            fields([("reason", reason), ("atoms", ids(atoms).into())])
        }
        LayoutError::StereoInLayout { atoms, bonds }
        | LayoutError::MultipleAttachments { atoms, bonds } => fields([
            ("reason", reason),
            ("atoms", ids(atoms).into()),
            ("bonds", ids(bonds).into()),
        ]),
        LayoutError::CoincidentFixedAtoms { atom, other } => fields([
            ("reason", reason),
            ("atom", atom.to_string().into()),
            ("other", other.to_string().into()),
        ]),
        LayoutError::CoordinateOutOfRange { atom } => {
            fields([("reason", reason), ("atom", atom.to_string().into())])
        }
    }
}

/// A details map from its entries, in the order given.
fn fields<const N: usize>(entries: [(&str, Value); N]) -> Map<String, Value> {
    entries
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect()
}

/// The identifiers as strings, in the order given.
fn ids<'a, T: ToString + 'a>(values: impl IntoIterator<Item = &'a T>) -> Vec<String> {
    values.into_iter().map(ToString::to_string).collect()
}

fn position(point: Point) -> PositionView {
    PositionView {
        x: point.x(),
        y: point.y(),
    }
}

fn scope_name(scope: Scope) -> &'static str {
    match scope {
        Scope::Components => "components",
        Scope::Selection => "selection",
    }
}

fn component_view(component: &PlacedComponent) -> LayoutComponentView {
    LayoutComponentView {
        atoms: ids(&component.atoms),
        bonds: ids(&component.bonds),
        anchor: component.anchor.map(|anchor| AnchorView {
            fixed_atom: anchor.fixed_atom.to_string(),
            bond: anchor.bond.to_string(),
        }),
        root: component.root.to_string(),
        chains: component
            .chains
            .iter()
            .map(|chain| ChainView {
                atoms: ids(&chain.atoms),
                branch_from: chain.branch_from.map(|atom| atom.to_string()),
            })
            .collect(),
    }
}

fn advice(
    severity: Severity,
    code: &'static str,
    message: impl Into<String>,
    details: Map<String, Value>,
) -> Diagnostic {
    Diagnostic {
        code,
        severity,
        message: message.into(),
        path: None,
        details,
    }
}

/// What a successful preview says beyond its result, in the order it is reported. Every preview
/// says that nothing was applied and what is not implemented; an overlap and a layout set with no
/// bonds are reported when they hold.
fn preview_diagnostics(prep: &Preparation, preview: &Preview) -> Vec<Diagnostic> {
    let mut diagnostics = vec![advice(
        Severity::Info,
        "layout_preview_only",
        "Nothing was changed; applying a layout is not implemented.",
        Map::new(),
    )];
    if !preview.overlaps.is_empty() {
        let pairs: Vec<Value> = preview
            .overlaps
            .iter()
            .take(MAX_OVERLAP_PAIRS)
            .map(|(first, second)| Value::from(vec![first.to_string(), second.to_string()]))
            .collect();
        let mut details = Map::new();
        details.insert("pairs".into(), Value::Array(pairs));
        details.insert("count".into(), preview.overlaps.len().into());
        diagnostics.push(advice(
            Severity::Warning,
            "layout_overlap",
            format!(
                "{} pairs of atoms that are not bonded lie closer than {OVERLAP_FACTOR} bond lengths after placement; collisions are not resolved.",
                preview.overlaps.len()
            ),
            details,
        ));
    }
    if prep.layout_bonds.is_empty() {
        diagnostics.push(advice(
            Severity::Info,
            "layout_no_bonds",
            "The layout set has no bonds: a free atom keeps its position, and an atom bonded to a fixed atom is placed one bond length from it.",
            Map::new(),
        ));
    }
    diagnostics.push(advice(
        Severity::Info,
        "layout_not_implemented",
        "Ring layout, templates, scoring and search, and collision resolution are not implemented.",
        Map::new(),
    ));
    diagnostics
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use chemifree_domain::document::{AtomId, BondId};

    use super::*;

    fn atom(serial: u32) -> AtomId {
        AtomId::from_serial(serial).unwrap()
    }

    fn bond(serial: u32) -> BondId {
        BondId::from_serial(serial).unwrap()
    }

    /// The reason of each variant, listed explicitly. The match has no wildcard, so a variant
    /// added to `LayoutError` does not compile until it has a reason here.
    fn listed_reason(error: &LayoutError) -> &'static str {
        match error {
            LayoutError::ObjectSelection => "object_selection",
            LayoutError::UnknownSelection => "selection_unknown",
            LayoutError::EmptySelection => "layout_selection_empty",
            LayoutError::TooLarge { .. } => "too_large",
            LayoutError::RingInLayoutSet { .. } => "ring_in_layout_set",
            LayoutError::StereoInLayout { .. } => "stereo_in_layout",
            LayoutError::DisplayInLayoutSet { .. } => "display_in_layout_set",
            LayoutError::MultipleAttachments { .. } => "multiple_attachments",
            LayoutError::CoincidentFixedAtoms { .. } => "coincident_fixed_atoms",
            LayoutError::CoordinateOutOfRange { .. } => "coordinate_out_of_range",
            LayoutError::MissingPosition { .. } => "missing_position",
        }
    }

    /// One refusal of every variant, with the field values it needs.
    fn one_refusal_per_variant() -> Vec<LayoutError> {
        vec![
            LayoutError::ObjectSelection,
            LayoutError::UnknownSelection,
            LayoutError::EmptySelection,
            LayoutError::TooLarge { atoms: 3, limit: 2 },
            LayoutError::RingInLayoutSet {
                atoms: vec![atom(1)],
            },
            LayoutError::StereoInLayout {
                atoms: vec![atom(1)],
                bonds: vec![bond(1)],
            },
            LayoutError::DisplayInLayoutSet {
                atoms: vec![atom(1)],
            },
            LayoutError::MultipleAttachments {
                atoms: vec![atom(1)],
                bonds: vec![bond(1), bond(2)],
            },
            LayoutError::CoincidentFixedAtoms {
                atom: atom(1),
                other: atom(2),
            },
            LayoutError::CoordinateOutOfRange { atom: atom(1) },
            LayoutError::MissingPosition { atom: atom(1) },
        ]
    }

    #[test]
    fn every_variant_has_its_listed_reason() {
        for error in one_refusal_per_variant() {
            assert_eq!(error.reason(), listed_reason(&error), "{error:?}");
        }
    }

    #[test]
    fn reason_codes_are_distinct() {
        let samples = one_refusal_per_variant();
        let codes: BTreeSet<&str> = samples.iter().map(listed_reason).collect();
        assert_eq!(codes.len(), samples.len(), "each variant has its own code");
        let layout: BTreeSet<&str> = LAYOUT_REASONS.into_iter().collect();
        assert_eq!(
            layout.len(),
            LAYOUT_REASONS.len(),
            "LAYOUT_REASONS codes are distinct"
        );
    }

    #[test]
    fn layout_reasons_name_the_layout_unsupported_refusals() {
        let known = Known {
            request_id: "layout-test".into(),
            operation: OperationId::LayoutStructure,
            profile: CompatibilityProfile::PreserveSemantics,
        };
        let mut unsupported = BTreeSet::new();
        for error in one_refusal_per_variant() {
            let reply = refusal_reply(&known, 1, &error);
            if reply.outcome == Outcome::Unsupported {
                let diagnostic = &reply.diagnostics[0];
                assert_eq!(diagnostic.code, "layout_unsupported", "{error:?}");
                assert_eq!(diagnostic.details["reason"], error.reason(), "{error:?}");
                unsupported.insert(error.reason());
            }
        }
        let listed: BTreeSet<&str> = LAYOUT_REASONS.into_iter().collect();
        assert_eq!(
            listed, unsupported,
            "LAYOUT_REASONS names the layout_unsupported refusals"
        );
    }
}
