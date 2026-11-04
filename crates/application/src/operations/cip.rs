//! `chem.cip`: CIP descriptors of the stored configurations.
//!
//! Read-only. The graph and its document snapshot are copied under one lock, so the reply names
//! the exact snapshot the labels are about; the labeler then runs outside the lock. A label is
//! computed on demand from the stored configuration and the current graph. It is never stored,
//! never written back and never read from a mark or a position.

use std::collections::BTreeSet;

use chemifree_domain::budget::Interrupted;
use chemifree_domain::cip::{BondReport, CenterReport, Limits, Reason, State, Stop, Warning};
use chemifree_domain::document::{AtomId, BondId};
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::stereo::StereoRecord;

use super::*;
use crate::cip::{self, CipLabeler, Refusal};
use crate::contract::{
    AtomConfigurationDto, BondConfigurationDto, center_dto, double_bond_dto, slot_name,
};

/// Every reason an item can be `unsupported` or `ambiguous` under, in the order the contract
/// lists them. The model's list is closed; `reason_sentence` has no wildcard, so a reason added
/// to the domain does not compile until it has a sentence.
pub const REASONS: [Reason; 17] = [
    Reason::NotACandidate,
    Reason::CenterStructure,
    Reason::DoubleBondStructure,
    Reason::QueryAtom,
    Reason::BondKind,
    Reason::AttachmentPoint,
    Reason::HydrogensUnknown,
    Reason::AromaticSystem,
    Reason::MancudeRule1b,
    Reason::Rule1bDuplicateDistance,
    Reason::MassNumber,
    Reason::Rule4Or5Needed,
    Reason::StereoCycle,
    Reason::Rule3NestedDescriptor,
    Reason::LigandStereoUnspecified,
    Reason::RankingInconsistent,
    Reason::DigraphLimit,
];

/// The sentence a loss entry gives for a reason; docs/CONTRACT.md carries the same meaning.
pub fn reason_sentence(reason: Reason) -> &'static str {
    match reason {
        Reason::NotACandidate => {
            "The named atom is not a stereocenter candidate (it needs three or four bonded neighbors) or the named bond is not a double bond."
        }
        Reason::CenterStructure => {
            "The stored center does not fit the structure the model reads: an aromatic bond or a radical at the center, or a stored statement the valence model cannot place."
        }
        Reason::DoubleBondStructure => {
            "The double bond does not fit the structure the model reads: a cumulated end, more than two other neighbors at an end, positions the valence model does not fit to two, or a stored reference that names no position of an end."
        }
        Reason::QueryAtom => "A query atom is met while expanding the digraph.",
        Reason::BondKind => "A dative, unknown or query bond is met while expanding the digraph.",
        Reason::AttachmentPoint => {
            "An atom with an attachment point is met while expanding the digraph."
        }
        Reason::HydrogensUnknown => {
            "The valence model cannot give the hydrogens of an atom that must be expanded: unknown hydrogens, a radical, or an element, charge or valence it does not list."
        }
        Reason::AromaticSystem => {
            "An aromatic system other than a neutral all-carbon one must be looked into."
        }
        Reason::MancudeRule1b => {
            "Rule 1b is needed on a digraph with a mancude duplicate, and the rule is not defined for those."
        }
        Reason::Rule1bDuplicateDistance => {
            "Rule 1b compares duplicates that two readings of the distance to the corresponding atom order differently, which happens in small strained rings."
        }
        Reason::MassNumber => {
            "Rule 2 compares an unlabeled atom with a labeled one and the answer depends on how an unlabeled atom is read."
        }
        Reason::Rule4Or5Needed => {
            "The ligands tie under Rules 1a to 3 and Rules 4 and 5, which this build does not apply, might separate them."
        }
        Reason::StereoCycle => "The descriptor of a ligand double bond depends on itself.",
        Reason::Rule3NestedDescriptor => {
            "Rule 3 needs the descriptor of a ligand double bond that reads differently ranked in the whole molecule and ranked in the digraph rooted at the unit."
        }
        Reason::LigandStereoUnspecified => {
            "Rule 3 has to place a ligand double bond whose geometry the document does not state, so the answer depends on information it does not have."
        }
        Reason::RankingInconsistent => {
            "The pairwise comparisons do not form a consistent order; this is not expected and is reported, not guessed."
        }
        Reason::DigraphLimit => {
            "The comparison needs more digraph nodes or spheres than the limits allow."
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CipParameters {
    document_id: String,
    /// Any JSON value is read so that a wrong type is reported against `parameters.scope`.
    #[serde(default)]
    scope: Option<Value>,
    #[serde(default)]
    atoms: Option<Value>,
    #[serde(default)]
    bonds: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CipResult {
    /// The exact snapshot the labels are about; `snapshot.document.stereo` is what is stored.
    pub snapshot: DocumentResult,
    /// The labeler's model identifier.
    pub model: &'static str,
    /// The sequence rules the labeler applied, in order.
    pub rules: Vec<&'static str>,
    /// What the request listed: `stored`, `stereogenic`, or `named` (only the named items).
    pub scope: &'static str,
    /// Centers, ascending by atom.
    pub atoms: Vec<AtomLabel>,
    /// Double bonds, ascending by bond.
    pub bonds: Vec<BondLabel>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AtomLabel {
    pub atom: String,
    pub state: &'static str,
    /// `R` or `S` exactly when the state is `assigned`.
    pub descriptor: Option<&'static str>,
    /// Set exactly when the state is `unsupported` or `ambiguous`.
    pub reason: Option<&'static str>,
    /// The stored configuration as a snapshot shows it; `null` when none is stored.
    pub stored: Option<AtomConfigurationDto>,
    /// The four positions, highest first, with their rank; present when the ranking is complete.
    pub priority: Option<Vec<Priority>>,
    /// The last rule needed to separate the four positions; present with `priority`.
    pub decided_by: Option<&'static str>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Priority {
    /// An atom identifier, `implicit_hydrogen` or `lone_pair`.
    pub neighbor: String,
    pub rank: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BondLabel {
    pub bond: String,
    pub state: &'static str,
    /// `E` or `Z` exactly when the state is `assigned`.
    pub descriptor: Option<&'static str>,
    /// Set exactly when the state is `unsupported` or `ambiguous`.
    pub reason: Option<&'static str>,
    pub stored: Option<BondConfigurationDto>,
    /// The higher substituent at each end, `begin` end first; present when both ends have one.
    pub ends: Option<Vec<EndLabel>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct EndLabel {
    pub atom: String,
    /// The position that ranks higher at this end.
    pub higher: String,
    pub decided_by: &'static str,
}

/// The state's wire spelling, its descriptor, and its reason.
fn split(state: State) -> (&'static str, Option<&'static str>, Option<&'static str>) {
    match state {
        State::Assigned(descriptor) => ("assigned", Some(descriptor.as_str()), None),
        State::None => ("none", None, None),
        State::Unspecified => ("unspecified", None, None),
        State::Unknown => ("unknown", None, None),
        State::Ambiguous(reason) => ("ambiguous", None, Some(reason.code())),
        State::Unsupported(reason) => ("unsupported", None, Some(reason.code())),
    }
}

fn atom_label(report: &CenterReport) -> AtomLabel {
    let (state, descriptor, reason) = split(report.state);
    AtomLabel {
        atom: report.atom.to_string(),
        state,
        descriptor,
        reason,
        stored: report.stored.as_ref().map(center_dto),
        priority: report.priority.as_ref().map(|priority| {
            priority
                .iter()
                .map(|&(slot, rank)| Priority {
                    neighbor: slot_name(slot),
                    rank,
                })
                .collect()
        }),
        decided_by: report.decided_by.map(|rule| rule.as_str()),
    }
}

fn bond_label(report: &BondReport) -> BondLabel {
    let (state, descriptor, reason) = split(report.state);
    BondLabel {
        bond: report.bond.to_string(),
        state,
        descriptor,
        reason,
        stored: report.stored.as_ref().map(double_bond_dto),
        ends: report.ends.as_ref().map(|ends| {
            ends.iter()
                .map(|end| EndLabel {
                    atom: end.atom.to_string(),
                    higher: slot_name(end.higher),
                    decided_by: end.decided_by.as_str(),
                })
                .collect()
        }),
    }
}

/// Which items a request lists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Scope {
    Stored,
    Stereogenic,
    Named,
}

impl Scope {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Stored => "stored",
            Self::Stereogenic => "stereogenic",
            Self::Named => "named",
        }
    }

    fn statement(self) -> &'static str {
        match self {
            Self::Stored => {
                "Listed: every stored configuration (centers and double bonds) and each atom and bond named in the request. An atom or bond that is not listed was not examined; it may or may not be stereogenic."
            }
            Self::Stereogenic => {
                "Listed: every stored configuration, each atom and bond named in the request, and every center or double bond found stereogenic or undecidable that has none stored. A double bond inside a ring system with no stored configuration is not scanned. An atom or bond that is not listed was not examined unless it was found not stereogenic."
            }
            Self::Named => {
                "Listed: only the atoms and bonds named in the request. Nothing else was examined, stored configurations included."
            }
        }
    }
}

const NOT_CHECKED: &str = "Rules 1a, 1b, 2 and 3 only. Rules 4a to 5, pseudoasymmetric r and s, axes and planes, and heteroaromatic rings are not applied: a tie they might break is unsupported, never none. A label describes the stored configuration of this graph, never the drawing, and is not stored.";

fn invalid(path: String, message: String, value: Option<Value>) -> Diagnostic {
    let mut details = Map::new();
    if let Some(value) = value {
        details.insert("value".into(), value);
    }
    Diagnostic {
        code: ErrorCode::InvalidRequest.as_str(),
        severity: Severity::Error,
        message,
        path: Some(path),
        details,
    }
}

/// The scope the request names, or `None` when it names none.
fn parse_scope(value: Option<Value>) -> Result<Option<Scope>, Diagnostic> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) if text == "stored" => Ok(Some(Scope::Stored)),
        Some(Value::String(text)) if text == "stereogenic" => Ok(Some(Scope::Stereogenic)),
        Some(other) => Err(invalid(
            "parameters.scope".into(),
            "scope must be \"stored\" or \"stereogenic\".".into(),
            Some(other),
        )),
    }
}

/// A list of identifier strings, not empty, or `None` when the request has none.
fn parse_list(name: &str, value: Option<Value>) -> Result<Option<Vec<String>>, Diagnostic> {
    let path = format!("parameters.{name}");
    let items = match value {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Array(items)) => items,
        Some(other) => {
            return Err(invalid(
                path,
                format!("{name} must be a list of identifiers."),
                Some(other),
            ));
        }
    };
    if items.is_empty() {
        return Err(invalid(
            path,
            format!("{name} must not be empty; leave it out to name nothing."),
            None,
        ));
    }
    items
        .into_iter()
        .enumerate()
        .map(|(index, item)| match item {
            Value::String(text) => Ok(text),
            other => Err(invalid(
                format!("{path}[{index}]"),
                format!("{name}[{index}] must be a string."),
                Some(other),
            )),
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

fn article(noun: &str) -> &'static str {
    if noun.starts_with('a') { "an" } else { "a" }
}

/// The named identifiers resolved against the document; a repeat merges.
fn resolve<T: Ord + Copy>(
    name: &str,
    noun: &str,
    names: Option<Vec<String>>,
    parse: impl Fn(&str) -> Option<T>,
    wrong_kind: impl Fn(&str) -> bool,
    exists: &BTreeSet<T>,
) -> Result<BTreeSet<T>, Diagnostic> {
    let mut resolved = BTreeSet::new();
    for (index, text) in names.into_iter().flatten().enumerate() {
        let path = format!("parameters.{name}[{index}]");
        let value = Some(Value::String(text.clone()));
        match parse(&text) {
            Some(id) if exists.contains(&id) => {
                resolved.insert(id);
            }
            Some(_) => {
                return Err(invalid(
                    path,
                    format!("{text} is not {} {noun} of this document.", article(noun)),
                    value,
                ));
            }
            None if wrong_kind(&text) => {
                return Err(invalid(
                    path,
                    format!(
                        "{text} is not {} {noun}; {name} takes {noun} identifiers only.",
                        article(noun)
                    ),
                    value,
                ));
            }
            None => {
                return Err(invalid(
                    path,
                    format!("{text:?} is not {} {noun} identifier.", article(noun)),
                    value,
                ));
            }
        }
    }
    Ok(resolved)
}

fn item_details(kind: &str, id: &str) -> Map<String, Value> {
    let mut details = Map::new();
    details.insert("item".into(), kind.into());
    details.insert("id".into(), id.into());
    details
}

fn warning_diagnostic(kind: &str, id: &str, warning: Warning) -> Diagnostic {
    let message = match warning {
        Warning::StoredNotStereogenic => format!(
            "{kind} {id}: a configuration is stored on a unit that is not stereogenic under Rules 1a to 3, so it makes no difference."
        ),
        Warning::SlotUnusual => format!(
            "{kind} {id}: the stored non-atom position differs from the one the valence model names; the stored one was used."
        ),
    };
    Diagnostic {
        code: warning.code(),
        severity: Severity::Warning,
        message,
        path: None,
        details: item_details(&kind.to_lowercase(), id),
    }
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

fn loss(source_path: String, reason: Reason) -> Loss {
    Loss {
        feature: "cip_descriptor".into(),
        source_path,
        disposition: LossDisposition::Unknown,
        reason: format!("{}: {}", reason.code(), reason_sentence(reason)),
        semantic_impact: SemanticImpact::Stereo,
        consent: LossConsent::NotRequired,
    }
}

/// The units of a request, before they are labeled.
struct Units {
    atoms: BTreeSet<AtomId>,
    bonds: BTreeSet<BondId>,
    named_atoms: BTreeSet<AtomId>,
    named_bonds: BTreeSet<BondId>,
}

impl<C: CapabilityCatalog, R: DocumentRepository> Application<C, R> {
    pub(super) fn cip(
        &self,
        known: &Known,
        p: CipParameters,
        cancel: &dyn Interrupt,
    ) -> OperationResponse {
        let labeler = &*self.cip_labeler;
        let model = labeler.model_id();
        let with_model = |mut reply: OperationResponse| {
            reply.model_id = Some(model.into());
            reply
        };
        let scope = match parse_scope(p.scope) {
            Ok(scope) => scope,
            Err(invalid) => return with_model(known.invalid(invalid)),
        };
        let (atom_names, bond_names) =
            match (parse_list("atoms", p.atoms), parse_list("bonds", p.bonds)) {
                (Ok(atoms), Ok(bonds)) => (atoms, bonds),
                (Err(invalid), _) | (_, Err(invalid)) => return with_model(known.invalid(invalid)),
            };
        let (snapshot, graph) = match self.documents.graph_snapshot(&p.document_id) {
            Ok(pair) => pair,
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
        let named = atom_names.is_some() || bond_names.is_some();
        // Naming items without a scope lists only those: it narrows, it never adds the stored
        // records.
        let scope = match (scope, named) {
            (Some(scope), _) => scope,
            (None, false) => Scope::Stored,
            (None, true) => Scope::Named,
        };
        let present_atoms: BTreeSet<AtomId> = graph.atoms().map(|(id, _)| id).collect();
        let present_bonds: BTreeSet<BondId> = graph.bonds().map(|(id, _)| id).collect();
        let named_atoms = resolve(
            "atoms",
            "atom",
            atom_names,
            AtomId::parse,
            |text| BondId::parse(text).is_some(),
            &present_atoms,
        );
        let named_bonds = resolve(
            "bonds",
            "bond",
            bond_names,
            BondId::parse,
            |text| AtomId::parse(text).is_some(),
            &present_bonds,
        );
        let (named_atoms, named_bonds) = match (named_atoms, named_bonds) {
            (Ok(atoms), Ok(bonds)) => (atoms, bonds),
            (Err(invalid), _) | (_, Err(invalid)) => return with_model(known.invalid(invalid)),
        };
        let records = match graph.stereo().typed() {
            Ok(records) => records,
            Err(error) => {
                return with_model(known.reply(
                    Outcome::Error,
                    Some(revision),
                    None,
                    vec![diagnostic(
                        ErrorCode::DocumentUnreadable.as_str(),
                        format!("The document's stereo cannot be read: {error}"),
                        Map::new(),
                    )],
                ));
            }
        };
        let mut units = Units {
            atoms: named_atoms.clone(),
            bonds: named_bonds.clone(),
            named_atoms,
            named_bonds,
        };
        let limits = labeler.limits();
        if scope != Scope::Named {
            for record in &records {
                match record {
                    StereoRecord::Center { atom, .. } => {
                        units.atoms.insert(*atom);
                    }
                    StereoRecord::DoubleBond { bond, .. } => {
                        units.bonds.insert(*bond);
                    }
                }
            }
        }
        if scope == Scope::Stereogenic {
            match cip::scan_candidates(&graph, limits, cancel) {
                Ok(found) => {
                    units
                        .atoms
                        .extend(found.centers.iter().map(|report| report.atom));
                    units
                        .bonds
                        .extend(found.double_bonds.iter().map(|report| report.bond));
                }
                Err(stop) => {
                    return with_model(self.stopped(known, stop, revision, &units, limits));
                }
            }
        }
        let (centers, double_bonds) =
            match self.label_units(&graph, &units, limits, labeler, cancel) {
                Ok(reports) => reports,
                Err(Failure::Stop(stop)) => {
                    return with_model(self.stopped(known, stop, revision, &units, limits));
                }
                Err(Failure::Refused(refusal)) => {
                    return with_model(known.reply(
                        Outcome::Error,
                        Some(revision),
                        None,
                        vec![refusal_diagnostic(&refusal)],
                    ));
                }
            };
        let stored_atoms: BTreeSet<AtomId> = records
            .iter()
            .filter_map(|record| match record {
                StereoRecord::Center { atom, .. } => Some(*atom),
                StereoRecord::DoubleBond { .. } => None,
            })
            .collect();
        let stored_bonds: BTreeSet<BondId> = records
            .iter()
            .filter_map(|record| match record {
                StereoRecord::DoubleBond { bond, .. } => Some(*bond),
                StereoRecord::Center { .. } => None,
            })
            .collect();
        // A unit found only by the scan is listed when it is not stereogenic by the labeler's
        // own answer; a stored or named one is listed whatever it came to.
        let centers: Vec<&CenterReport> = centers
            .iter()
            .filter(|report| {
                report.state != State::None
                    || units.named_atoms.contains(&report.atom)
                    || stored_atoms.contains(&report.atom)
            })
            .collect();
        let double_bonds: Vec<&BondReport> = double_bonds
            .iter()
            .filter(|report| {
                report.state != State::None
                    || units.named_bonds.contains(&report.bond)
                    || stored_bonds.contains(&report.bond)
            })
            .collect();
        let mut diagnostics = Vec::new();
        let mut losses = Vec::new();
        for report in &centers {
            let id = report.atom.to_string();
            diagnostics.extend(
                report
                    .warnings
                    .iter()
                    .map(|&warning| warning_diagnostic("Atom", &id, warning)),
            );
            if let State::Ambiguous(reason) | State::Unsupported(reason) = report.state {
                losses.push(loss(format!("atoms[{id}]"), reason));
            }
        }
        for report in &double_bonds {
            let id = report.bond.to_string();
            diagnostics.extend(
                report
                    .warnings
                    .iter()
                    .map(|&warning| warning_diagnostic("Bond", &id, warning)),
            );
            if let State::Ambiguous(reason) | State::Unsupported(reason) = report.state {
                losses.push(loss(format!("bonds[{id}]"), reason));
            }
        }
        let mut scope_details = Map::new();
        scope_details.insert("scope".into(), scope.as_str().into());
        diagnostics.push(info("cip_scope", scope.statement().into(), scope_details));
        diagnostics.push(info("cip_not_checked", NOT_CHECKED.into(), Map::new()));
        let result = CipResult {
            snapshot: snapshot.into(),
            model,
            rules: labeler.rules().to_vec(),
            scope: scope.as_str(),
            atoms: centers.iter().map(|report| atom_label(report)).collect(),
            bonds: double_bonds
                .iter()
                .map(|report| bond_label(report))
                .collect(),
        };
        let mut reply = converted(
            known,
            Some(revision),
            OperationResult::Cip(Box::new(result)),
            losses,
        );
        reply.diagnostics = diagnostics;
        with_model(reply)
    }

    /// Asks the labeler about the units and checks what it answers. The request limit is shared
    /// by the centers and the double bonds: the bonds get what the centers left.
    fn label_units(
        &self,
        graph: &MoleculeGraph,
        units: &Units,
        limits: Limits,
        labeler: &dyn CipLabeler,
        cancel: &dyn Interrupt,
    ) -> Result<(Vec<CenterReport>, Vec<BondReport>), Failure> {
        let atoms: Vec<AtomId> = units.atoms.iter().copied().collect();
        let bonds: Vec<BondId> = units.bonds.iter().copied().collect();
        let centers = labeler
            .label_centers(graph, &atoms, limits, cancel)
            .map_err(Failure::Stop)?;
        cip::check_centers(graph, &atoms, &centers).map_err(Failure::Refused)?;
        let used = centers
            .iter()
            .fold(0_usize, |sum, report| sum.saturating_add(report.nodes));
        let left = Limits {
            request_nodes: limits.request_nodes.saturating_sub(used),
            ..limits
        };
        let double_bonds = labeler
            .label_double_bonds(graph, &bonds, left, cancel)
            .map_err(|stop| match stop {
                Stop::RequestLimit { .. } => Failure::Stop(Stop::RequestLimit {
                    limit: limits.request_nodes,
                }),
                other => Failure::Stop(other),
            })?;
        cip::check_double_bonds(graph, &bonds, &double_bonds).map_err(Failure::Refused)?;
        Ok((centers, double_bonds))
    }

    /// The reply to a request that produced no labels: over the request limit, or interrupted.
    fn stopped(
        &self,
        known: &Known,
        stop: Stop,
        revision: u64,
        units: &Units,
        limits: Limits,
    ) -> OperationResponse {
        match stop {
            Stop::RequestLimit { limit } => {
                let items = units.atoms.len() + units.bonds.len();
                let mut details = Map::new();
                details.insert("limit".into(), limit.into());
                details.insert("items".into(), items.into());
                details.insert("item_limit".into(), limits.item_nodes.into());
                details.insert("parameter".into(), "atoms".into());
                known.reply(
                    Outcome::Error,
                    Some(revision),
                    None,
                    vec![Diagnostic {
                        code: ErrorCode::ResourceLimit.as_str(),
                        severity: Severity::Error,
                        message: format!(
                            "{items} items need more than {limit} digraph nodes together; name fewer `atoms` and `bonds` to label them in smaller requests."
                        ),
                        path: Some("parameters.atoms".into()),
                        details,
                    }],
                )
            }
            Stop::Interrupted(interrupted) => {
                let (code, message) = match interrupted {
                    Interrupted::Cancelled => (
                        ErrorCode::Cancelled,
                        "The computation was cancelled before it finished; nothing was changed.",
                    ),
                    Interrupted::TimeLimit => (
                        ErrorCode::TimeLimit,
                        "The computation did not finish in time; nothing was changed.",
                    ),
                };
                known.reply(
                    Outcome::Error,
                    Some(revision),
                    None,
                    vec![diagnostic(code.as_str(), message, Map::new())],
                )
            }
        }
    }
}

/// Why labeling the units produced no reports.
enum Failure {
    Stop(Stop),
    Refused(Refusal),
}

/// The diagnostic of a labeler answer the application refused.
fn refusal_diagnostic(refusal: &Refusal) -> Diagnostic {
    let mut details = Map::new();
    details.insert("unit".into(), refusal.unit.clone().into());
    details.insert("problem".into(), refusal.problem.as_str().into());
    diagnostic(
        "cip_labeler_invalid",
        format!("The CIP labeler's answer was refused: {refusal}. Nothing was labeled."),
        details,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_reason_has_a_distinct_code_and_a_sentence_ending_in_a_period() {
        let codes: BTreeSet<&str> = REASONS.iter().map(|reason| reason.code()).collect();
        assert_eq!(codes.len(), REASONS.len(), "codes are distinct");
        for reason in REASONS {
            let sentence = reason_sentence(reason);
            assert!(sentence.ends_with('.'), "{}: {sentence}", reason.code());
            assert!(reason.code().starts_with("cip_"), "{}", reason.code());
        }
    }
}
