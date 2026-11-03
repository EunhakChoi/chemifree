//! `chem.stereo`: what the document states about stereo against what each scene instance draws.
//!
//! Read-only and advisory. The document and its view are copied under one lock, so the reply
//! names the exact snapshot the comparison is about; the comparison then runs outside the lock.
//! Nothing is derived into the document, stored or edited, the outcome is `ok`, and a
//! disagreement is a finding about a valid document, not a loss.

use chemifree_domain::chemistry::VALENCE_MODEL;
use chemifree_domain::document::{Document, ObjectId};
use chemifree_domain::stereo_drawing::{MODEL_ID, WEAK_EVIDENCE};
use chemifree_domain::stereo_inspection::{
    self, Agreement, AtomEntry, BondEntry, Inspection, Plan,
};

use super::*;
use crate::contract::{AtomConfigurationDto, BondConfigurationDto, center_dto, double_bond_dto};

/// The most entries (items times inspected instances) one reply holds. A single instance holds
/// at most 6,000 (2,000 atoms and 4,000 bonds), so naming an `instance` always fits.
pub const MAX_ENTRIES: usize = 20_000;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StereoParameters {
    document_id: String,
    /// Any JSON value is read so that a wrong type is reported against `parameters.instance`
    /// like a wrong or unknown identifier.
    #[serde(default)]
    instance: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StereoResult {
    /// The exact snapshot compared; `snapshot.document.stereo` is what is stored.
    pub snapshot: DocumentResult,
    /// The valence model the fourth-position rule of the drawn side reads.
    pub valence_model: &'static str,
    /// Always `false`: stereogenicity is not assessed. An atom or bond that is not listed is
    /// neither stereogenic nor non-stereogenic as far as this operation knows.
    pub stereogenicity_assessed: bool,
    /// The scene instances inspected, in identifier order.
    pub instances: Vec<String>,
    pub atoms: Vec<AtomInspection>,
    pub bonds: Vec<BondInspection>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AtomInspection {
    pub atom: String,
    pub stored: Option<AtomConfigurationDto>,
    /// One entry per inspected instance, in `instances` order.
    pub drawn: Vec<AtomDrawnView>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AtomDrawnView {
    pub instance: String,
    /// The configuration the drawing shows; `null` exactly when `reason` is set.
    pub configuration: Option<AtomConfigurationDto>,
    pub reason: Option<&'static str>,
    pub agreement: &'static str,
    /// Stored and drawn both have a non-atom position and its label (`implicit_hydrogen` or
    /// `lone_pair`) differs. The agreement ignores the label.
    pub label_differs: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BondInspection {
    pub bond: String,
    pub stored: Option<BondConfigurationDto>,
    pub drawn: Vec<BondDrawnView>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BondDrawnView {
    pub instance: String,
    pub configuration: Option<BondConfigurationDto>,
    pub reason: Option<&'static str>,
    pub agreement: &'static str,
}

fn info(code: &'static str, message: String, details: Map<String, Value>) -> Diagnostic {
    Diagnostic {
        code,
        severity: Severity::Info,
        message,
        path: None,
        details,
    }
}

fn warning(code: &'static str, message: String, details: Map<String, Value>) -> Diagnostic {
    Diagnostic {
        severity: Severity::Warning,
        ..info(code, message, details)
    }
}

fn item_details(kind: &str, id: &str, instance: &str) -> Map<String, Value> {
    let mut details = Map::new();
    details.insert("item".into(), kind.into());
    details.insert("id".into(), id.into());
    details.insert("instance".into(), instance.into());
    details
}

fn to_value(value: &impl Serialize) -> Value {
    serde_json::to_value(value).expect("configuration views serialize to JSON")
}

fn atom_view(entry: &AtomEntry) -> (AtomInspection, Vec<Diagnostic>) {
    let atom = entry.atom.to_string();
    let stored = entry.stored.as_ref().map(center_dto);
    let mut diagnostics = Vec::new();
    let mut drawn = Vec::with_capacity(entry.drawn.len());
    for item in &entry.drawn {
        let instance = item.instance.to_string();
        let configuration = item.drawn.as_ref().ok().map(center_dto);
        let reason = item.drawn.as_ref().err().map(|reason| reason.code());
        let mut details = item_details("atom", &atom, &instance);
        match (item.agreement, item.drawn.as_ref().err()) {
            (Agreement::Disagrees, _) => {
                details.insert("stored".into(), to_value(&stored));
                details.insert("drawn".into(), to_value(&configuration));
                diagnostics.push(warning(
                    "drawing_disagrees",
                    format!("Atom {atom}: the drawing in {instance} shows a different configuration from the stored one."),
                    details,
                ));
            }
            (Agreement::OnlyDrawn, _) => diagnostics.push(info(
                "marks_without_configuration",
                format!("Atom {atom}: the marks show a configuration in {instance} that is not stored."),
                details,
            )),
            (Agreement::OnlyStored, Some(why)) if why.is_no_marks() => diagnostics.push(info(
                "configuration_without_marks",
                format!("Atom {atom}: a configuration is stored and no mark bears on this atom in {instance}."),
                details,
            )),
            (_, Some(why)) if !why.is_no_marks() => {
                details.insert("reason".into(), why.code().into());
                diagnostics.push(warning(
                    "drawing_not_derivable",
                    format!("Atom {atom}: a mark bears on this atom but the drawing in {instance} gives no configuration ({}).", why.code()),
                    details,
                ));
            }
            _ => {}
        }
        if let (true, Some(strength)) = (item.is_weak_evidence(), item.strength) {
            let mut details = item_details("atom", &atom, &instance);
            details.insert("strength".into(), strength.into());
            details.insert("limit".into(), WEAK_EVIDENCE.into());
            diagnostics.push(info(
                "drawing_weak_evidence",
                format!("Atom {atom}: the drawing in {instance} is nearly flat, so the derived turn rests on little evidence."),
                details,
            ));
        }
        drawn.push(AtomDrawnView {
            instance,
            configuration,
            reason,
            agreement: item.agreement.as_str(),
            label_differs: item.label_differs,
        });
    }
    (
        AtomInspection {
            atom,
            stored,
            drawn,
        },
        diagnostics,
    )
}

fn bond_view(entry: &BondEntry) -> (BondInspection, Vec<Diagnostic>) {
    let bond = entry.bond.to_string();
    let stored = entry.stored.as_ref().map(double_bond_dto);
    let mut diagnostics = Vec::new();
    let mut drawn = Vec::with_capacity(entry.drawn.len());
    for item in &entry.drawn {
        let instance = item.instance.to_string();
        let configuration = item.drawn.as_ref().ok().map(double_bond_dto);
        let reason = item.drawn.as_ref().err().map(|reason| reason.code());
        let mut details = item_details("bond", &bond, &instance);
        match (item.agreement, item.drawn.as_ref().err()) {
            (Agreement::Disagrees, _) => {
                details.insert("stored".into(), to_value(&stored));
                details.insert("drawn".into(), to_value(&configuration));
                diagnostics.push(warning(
                    "drawing_disagrees",
                    format!("Bond {bond}: the drawing in {instance} shows a different configuration from the stored one."),
                    details,
                ));
            }
            (Agreement::OnlyDrawn, _) => diagnostics.push(info(
                "marks_without_configuration",
                format!("Bond {bond}: the drawing shows a configuration in {instance} that is not stored."),
                details,
            )),
            // A double bond is read from its geometry whether or not it is marked, so every
            // reason counts here, unlike for a center.
            (_, Some(why)) => {
                details.insert("reason".into(), why.code().into());
                diagnostics.push(warning(
                    "drawing_not_derivable",
                    format!("Bond {bond}: the drawing in {instance} gives no configuration ({}).", why.code()),
                    details,
                ));
            }
            _ => {}
        }
        if !item.same_side_ends.is_empty() {
            let mut details = item_details("bond", &bond, &instance);
            details.insert(
                "ends".into(),
                item.same_side_ends
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .into(),
            );
            diagnostics.push(warning(
                "double_bond_substituents_same_side",
                format!("Bond {bond}: an end has two substituents drawn on the same side of the double bond in {instance}; the relation follows the lower-numbered one."),
                details,
            ));
        }
        drawn.push(BondDrawnView {
            instance,
            configuration,
            reason,
            agreement: item.agreement.as_str(),
        });
    }
    (
        BondInspection {
            bond,
            stored,
            drawn,
        },
        diagnostics,
    )
}

/// The findings that do not belong to one instance, then the statement of what is not judged.
fn general_diagnostics(document: &Document, inspection: &Inspection) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for mark in &inspection.misplaced_marks {
        let mut details = Map::new();
        details.insert("bond".into(), mark.bond.to_string().into());
        details.insert("narrow_atom".into(), mark.narrow.to_string().into());
        details.insert("other_atom".into(), mark.other.to_string().into());
        if mark.other_could_be_center {
            details.insert(
                "hint".into(),
                format!(
                    "atom {} could be the center: a mark bears on the atom at its narrow end",
                    mark.other
                )
                .into(),
            );
        }
        diagnostics.push(warning(
            "mark_narrow_end_not_a_center",
            format!("Bond {}: the wedge or hash has its narrow end at atom {}, which cannot be a stereocenter, so it states nothing.", mark.bond, mark.narrow),
            details,
        ));
    }
    for &atom in &inspection.aromatic_centers {
        let mut details = Map::new();
        details.insert("atom".into(), atom.to_string().into());
        diagnostics.push(warning(
            "center_has_aromatic_bond",
            format!("Atom {atom}: a center with an aromatic bond is read as drawn; its configuration is not judged."),
            details,
        ));
    }
    for &bond in &inspection.wavy_between_centers {
        let mut details = Map::new();
        details.insert("bond".into(), bond.to_string().into());
        if let Some(record) = document.bond(bond) {
            details.insert(
                "atoms".into(),
                vec![record.begin.to_string(), record.end.to_string()].into(),
            );
        }
        diagnostics.push(info(
            "wavy_between_possible_centers",
            format!("Bond {bond}: a wavy bond joins two possible centers; an end with its own wedge or hash follows that mark, the other end is unknown."),
            details,
        ));
    }
    diagnostics.push(info(
        "stereo_not_checked",
        "Only the stored configurations are compared with the drawing. No CIP label is computed, no atom is judged stereogenic, ring and strain plausibility are not checked, and chem.identity and chem.substructure do not read stereo.".into(),
        Map::new(),
    ));
    diagnostics
}

impl<C: CapabilityCatalog, R: DocumentRepository> Application<C, R> {
    pub(super) fn stereo(&self, known: &Known, p: StereoParameters) -> OperationResponse {
        let invalid_instance = |message: String| {
            let mut reply = known.invalid(Diagnostic {
                code: ErrorCode::InvalidRequest.as_str(),
                severity: Severity::Error,
                message,
                path: Some("parameters.instance".into()),
                details: Map::new(),
            });
            reply.model_id = Some(MODEL_ID.into());
            reply
        };
        let requested = match p.instance {
            None | Some(Value::Null) => None,
            Some(Value::String(text)) => match ObjectId::parse(&text) {
                Some(id) => Some(id),
                None => {
                    return invalid_instance(format!(
                        "{text:?} is not a scene instance identifier."
                    ));
                }
            },
            Some(_) => {
                return invalid_instance(
                    "instance must be a string naming a scene instance.".into(),
                );
            }
        };
        let (snapshot, document) = match self.documents.analysis_snapshot(&p.document_id) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return known.reply(
                    Outcome::Error,
                    error.current_revision,
                    None,
                    vec![failure(&error)],
                );
            }
        };
        let revision = snapshot.document.revision;
        let all: Vec<ObjectId> = document.instances().collect();
        let instances = match requested {
            None => all,
            Some(id) if all.contains(&id) => vec![id],
            Some(id) => {
                return invalid_instance(format!("{id} is not a scene instance of this document."));
            }
        };
        let unreadable = |error: stereo_inspection::InspectError| {
            let mut reply = known.reply(
                Outcome::Error,
                Some(revision),
                None,
                vec![Diagnostic {
                    code: ErrorCode::DocumentUnreadable.as_str(),
                    severity: Severity::Error,
                    message: format!("The document's stereo cannot be inspected: {error}"),
                    path: None,
                    details: Map::new(),
                }],
            );
            reply.model_id = Some(MODEL_ID.into());
            reply
        };
        let plan = match Plan::of(&document) {
            Ok(plan) => plan,
            Err(error) => return unreadable(error),
        };
        let entries = plan.len().saturating_mul(instances.len());
        if entries > MAX_ENTRIES {
            let mut details = Map::new();
            details.insert("entries".into(), entries.into());
            details.insert("limit".into(), MAX_ENTRIES.into());
            details.insert("items".into(), plan.len().into());
            details.insert("instances".into(), instances.len().into());
            details.insert("parameter".into(), "instance".into());
            let mut reply = known.reply(
                Outcome::Error,
                Some(revision),
                None,
                vec![Diagnostic {
                    code: ErrorCode::ResourceLimit.as_str(),
                    severity: Severity::Error,
                    message: format!(
                        "{} items in {} instances would be {entries} entries, above the limit of {MAX_ENTRIES}; name one `instance` to inspect it alone.",
                        plan.len(),
                        instances.len()
                    ),
                    path: Some("parameters.instance".into()),
                    details,
                }],
            );
            reply.model_id = Some(MODEL_ID.into());
            return reply;
        }
        let inspection = match stereo_inspection::inspect(&document, &plan, &instances) {
            Ok(inspection) => inspection,
            Err(error) => return unreadable(error),
        };
        let mut diagnostics = Vec::new();
        let atoms = inspection
            .atoms
            .iter()
            .map(|entry| {
                let (view, found) = atom_view(entry);
                diagnostics.extend(found);
                view
            })
            .collect();
        let bonds = inspection
            .bonds
            .iter()
            .map(|entry| {
                let (view, found) = bond_view(entry);
                diagnostics.extend(found);
                view
            })
            .collect();
        diagnostics.extend(general_diagnostics(&document, &inspection));
        let result = StereoResult {
            snapshot: snapshot.into(),
            valence_model: VALENCE_MODEL,
            stereogenicity_assessed: false,
            instances: instances.iter().map(ToString::to_string).collect(),
            atoms,
            bonds,
        };
        let mut reply = known.reply(
            Outcome::Ok,
            Some(revision),
            Some(OperationResult::Stereo(Box::new(result))),
            diagnostics,
        );
        reply.model_id = Some(MODEL_ID.into());
        reply
    }
}
