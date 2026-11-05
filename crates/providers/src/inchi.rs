//! Bounded mapping of drawing graph state to the registered official native InChI API.

use std::collections::{BTreeMap, BTreeSet};

use chemifree_application::inchi::{
    InchiError, InchiErrorKind, InchiOutput, InchiProvider, InchiProviderIdentity,
};
use chemifree_application::{Loss, LossConsent, LossDisposition, SemanticImpact};
use chemifree_domain::chemistry::{self, Finding};
use chemifree_domain::document::{AtomId, BondOrder};
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::states::{Hydrogens, Radical};
use chemifree_domain::stereo::{CenterState, DoubleBondState, Relation, Slot, StereoRecord, Turn};
use chemifree_domain::stereo_group::{GroupKind, StereoGroupTable};
use chemifree_inchi_native as native;

pub use native::Registration as InchiRegistration;

/// No automatic discovery: construct this adapter only from a verified explicit registration.
pub struct RegisteredInchiProvider {
    native: native::NativeInchi,
}

impl RegisteredInchiProvider {
    /// Checks the configured digest, release pin and options, then loads the native symbols.
    /// The version is registered metadata; the SDK has no runtime version query.
    ///
    /// # Errors
    /// Refuses unavailable, mismatched or malformed registrations without generating output.
    pub fn load(registration: InchiRegistration) -> Result<Self, InchiError> {
        Ok(Self {
            native: native::NativeInchi::load(registration).map_err(native_error)?,
        })
    }
}

fn native_error(error: native::Error) -> InchiError {
    let (kind, code) = match error.kind {
        native::ErrorKind::Registration => {
            (InchiErrorKind::Unavailable, "inchi_registration_invalid")
        }
        native::ErrorKind::Unavailable => (InchiErrorKind::Unavailable, "inchi_unavailable"),
        native::ErrorKind::InvalidInput => (InchiErrorKind::Invalid, "inchi_invalid_input"),
        native::ErrorKind::Limit => (InchiErrorKind::Limit, "inchi_limit"),
        native::ErrorKind::Native => (InchiErrorKind::Failed, "inchi_native_error"),
        native::ErrorKind::InvalidOutput => (InchiErrorKind::Failed, "inchi_invalid_output"),
    };
    let message = match error.native_code {
        Some(code) => format!("{} (native code {code})", error.message),
        None => error.message,
    };
    InchiError {
        kind,
        code,
        message,
    }
}

fn unsupported(code: &'static str, message: impl Into<String>) -> InchiError {
    InchiError {
        kind: InchiErrorKind::Unsupported,
        code,
        message: message.into(),
    }
}

fn loss(feature: &str, source_path: String, impact: SemanticImpact, reason: &str) -> Loss {
    Loss {
        feature: feature.into(),
        source_path,
        disposition: LossDisposition::Dropped,
        reason: reason.into(),
        semantic_impact: impact,
        consent: LossConsent::Required,
    }
}

struct Prepared {
    molecule: native::Molecule,
    losses: Vec<Loss>,
}

fn prepare(graph: &MoleculeGraph, groups: &StereoGroupTable) -> Result<Prepared, InchiError> {
    if graph.atoms().len() == 0 {
        return Err(InchiError {
            kind: InchiErrorKind::Invalid,
            code: "inchi_empty_graph",
            message: "An empty drawing has no molecular InChI.".into(),
        });
    }
    if graph.atoms().len() > native::MAX_ATOMS {
        return Err(InchiError {
            kind: InchiErrorKind::Limit,
            code: "inchi_limit",
            message: format!(
                "The native InChI profile accepts at most {} atoms.",
                native::MAX_ATOMS
            ),
        });
    }
    if groups
        .iter()
        .any(|(_, group)| group.kind() != GroupKind::Absolute)
    {
        return Err(unsupported(
            "inchi_enhanced_groups_unsupported",
            "AND and OR group semantics cannot be converted to absolute Standard InChI stereo.",
        ));
    }
    let mut atom_indices = BTreeMap::new();
    for (index, (id, _)) in graph.atoms().enumerate() {
        if atom_indices.insert(id, index).is_some() {
            return Err(unsupported(
                "inchi_invalid_graph",
                "Atom IDs must be distinct.",
            ));
        }
    }
    let index_of = |id: AtomId| {
        atom_indices.get(&id).copied().ok_or_else(|| {
            unsupported(
                "inchi_invalid_graph",
                format!("Atom {id} is absent from the graph."),
            )
        })
    };
    let mut bonds = Vec::with_capacity(graph.bonds().len());
    let mut edges = BTreeSet::new();
    for (_, bond) in graph.bonds() {
        let order = match bond.order {
            BondOrder::Single => native::BondOrder::Single,
            BondOrder::Double => native::BondOrder::Double,
            BondOrder::Triple => native::BondOrder::Triple,
            _ => {
                return Err(unsupported(
                    "inchi_bond_unsupported",
                    "Only ordinary single, double and triple bonds are supported; aromatic, query, dative and unknown bonds are refused.",
                ));
            }
        };
        let begin = index_of(bond.begin)?;
        let end = index_of(bond.end)?;
        if begin == end || !edges.insert((begin.min(end), begin.max(end))) {
            return Err(unsupported(
                "inchi_invalid_graph",
                "Self bonds and duplicate bonds are refused.",
            ));
        }
        bonds.push(native::Bond { begin, end, order });
    }
    let orders =
        chemistry::incident_orders(graph.atoms().map(|(id, _)| id), graph.bonds(), |_, bond| {
            chemistry::bond_order_value(&bond.order)
        });
    let mut atoms = Vec::with_capacity(graph.atoms().len());
    for (id, atom) in graph.atoms() {
        let element = atom.kind.element().ok_or_else(|| {
            unsupported(
                "inchi_query_unsupported",
                format!("Query atom {id} has no molecular InChI in this profile."),
            )
        })?;
        if !atom.attachment_points.is_empty() {
            return Err(unsupported(
                "inchi_attachment_points_unsupported",
                format!("Atom {id} has attachment points that this profile cannot encode."),
            ));
        }
        if atom.hydrogens == Hydrogens::Unknown {
            return Err(unsupported(
                "inchi_hydrogens_unknown",
                format!("Atom {id} has an unknown hydrogen count."),
            ));
        }
        let radical = match atom.radical {
            Radical::None => native::Radical::None,
            Radical::Singlet => native::Radical::Singlet,
            Radical::Doublet => native::Radical::Doublet,
            Radical::Triplet => native::Radical::Triplet,
            Radical::Unknown => {
                return Err(unsupported(
                    "inchi_radical_unknown",
                    format!("Atom {id} has unknown radical state."),
                ));
            }
        };
        let incident = orders
            .get(&id)
            .ok_or_else(|| unsupported("inchi_invalid_graph", "Atom adjacency is missing."))?;
        let assessment = chemistry::assess(id, atom, incident);
        if assessment.finding != Finding::Valid {
            return Err(unsupported(
                "inchi_valence_unsupported",
                format!(
                    "Atom {id} cannot be mapped under {}: {}.",
                    chemistry::VALENCE_MODEL,
                    assessment.reason.unwrap_or("unsupported_valence")
                ),
            ));
        }
        let count = assessment
            .hydrogen_count
            .and_then(|count| u8::try_from(count).ok())
            .ok_or_else(|| {
                unsupported(
                    "inchi_hydrogens_unsupported",
                    format!("Atom {id} has no bounded hydrogen count."),
                )
            })?;
        atoms.push(native::Atom {
            element: element.symbol().into(),
            isotope: atom.isotope,
            charge: atom.charge,
            radical,
            hydrogens: native::Hydrogens::Count(count),
        });
    }
    let records = graph
        .stereo()
        .typed()
        .map_err(|error| unsupported("inchi_stereo_unsupported", error.to_string()))?;
    let mut stereo = Vec::with_capacity(records.len());
    for record in records {
        match record {
            StereoRecord::Center { atom, state } => {
                if matches!(state, CenterState::Unknown { .. }) {
                    return Err(unsupported(
                        "inchi_stereo_unknown",
                        format!(
                            "Center {atom} is explicitly unknown; this profile does not equate it with unspecified stereo."
                        ),
                    ));
                }
                if state.slots().contains(&Slot::LonePair) {
                    return Err(unsupported(
                        "inchi_lone_pair_unsupported",
                        format!("Center {atom} has a lone-pair stereo slot outside this profile."),
                    ));
                }
                let center = index_of(atom)?;
                let mut order = *state.slots();
                if let Some(hydrogen) = order
                    .iter()
                    .position(|slot| *slot == Slot::ImplicitHydrogen)
                {
                    if atoms[center].hydrogens != native::Hydrogens::Count(1) {
                        return Err(unsupported(
                            "inchi_stereo_unsupported",
                            format!("Center {atom} does not have exactly one implicit hydrogen."),
                        ));
                    }
                    order.rotate_right(4 - hydrogen);
                }
                let mut neighbors = [0; 4];
                for (target, slot) in neighbors.iter_mut().zip(order) {
                    *target = match slot {
                        Slot::Atom(neighbor) => index_of(neighbor)?,
                        Slot::ImplicitHydrogen => center,
                        Slot::LonePair => {
                            return Err(unsupported(
                                "inchi_lone_pair_unsupported",
                                "Lone-pair stereo is outside this profile.",
                            ));
                        }
                    };
                }
                let turn = state
                    .turn_for_order(order)
                    .map_err(|error| unsupported("inchi_stereo_unsupported", error.to_string()))?;
                let parity = match turn {
                    Some(Turn::Clockwise) => native::Parity::Even,
                    Some(Turn::Anticlockwise) => native::Parity::Odd,
                    None => {
                        return Err(unsupported(
                            "inchi_stereo_unknown",
                            "Unknown tetrahedral stereo is outside this profile.",
                        ));
                    }
                };
                stereo.push(native::Stereo::Tetrahedral {
                    center,
                    neighbors,
                    parity,
                });
            }
            StereoRecord::DoubleBond { bond: id, state } => {
                let bond = graph
                    .bonds()
                    .find(|(found, _)| *found == id)
                    .map(|(_, bond)| bond)
                    .ok_or_else(|| {
                        unsupported(
                            "inchi_invalid_graph",
                            "A stereo bond is absent from the graph.",
                        )
                    })?;
                let DoubleBondState::Relative {
                    begin_ref: Slot::Atom(begin_ref),
                    end_ref: Slot::Atom(end_ref),
                    relation,
                } = state
                else {
                    return Err(unsupported(
                        "inchi_double_stereo_unsupported",
                        format!(
                            "Bond {id} needs specified relative stereo with explicit reference atoms."
                        ),
                    ));
                };
                if begin_ref == end_ref {
                    return Err(unsupported(
                        "inchi_double_stereo_unsupported",
                        format!(
                            "Bond {id} has a shared ring reference that this InChI profile cannot encode."
                        ),
                    ));
                }
                stereo.push(native::Stereo::DoubleBond {
                    neighbors: [
                        index_of(*begin_ref)?,
                        index_of(bond.begin)?,
                        index_of(bond.end)?,
                        index_of(*end_ref)?,
                    ],
                    parity: match relation {
                        Relation::SameSide => native::Parity::Odd,
                        Relation::OppositeSides => native::Parity::Even,
                    },
                });
            }
        }
    }
    let mut losses = Vec::new();
    let marked: Vec<String> = graph
        .bonds()
        .filter(|(_, bond)| bond.mark.is_some())
        .map(|(id, _)| id.to_string())
        .collect();
    if !marked.is_empty() {
        losses.push(loss("stereo_marks", format!("bonds[{}]", marked.join(", ")), SemanticImpact::Stereo,
            "The identifier uses stored configurations only. Drawn wedge, hash, wavy and crossed marks and their geometry are not encoded or interpreted."));
    }
    if !groups.is_empty() {
        let ids: Vec<String> = groups.iter().map(|(id, _)| id.to_string()).collect();
        losses.push(loss("stereo_group", format!("groups[{}]", ids.join(", ")), SemanticImpact::Stereo,
            "Individual stored configurations are submitted as Standard InChI stereo; explicit absolute group membership and native group IDs are not encoded."));
    }
    Ok(Prepared {
        molecule: native::Molecule {
            atoms,
            bonds,
            stereo,
        },
        losses,
    })
}

impl InchiProvider for RegisteredInchiProvider {
    fn identity(&self) -> InchiProviderIdentity {
        let registration = self.native.registration();
        InchiProviderIdentity {
            name: "IUPAC InChI native".into(),
            version: registration.version.clone(),
            build: format!("sha256:{}", registration.sha256),
            options: self.native.options(),
        }
    }

    fn generate(
        &self,
        graph: &MoleculeGraph,
        groups: &StereoGroupTable,
    ) -> Result<InchiOutput, InchiError> {
        let prepared = prepare(graph, groups)?;
        let output = self
            .native
            .generate(&prepared.molecule)
            .map_err(native_error)?;
        Ok(finish(output, prepared.losses))
    }
}

fn finish(output: native::Output, losses: Vec<Loss>) -> InchiOutput {
    let mut losses = losses;
    let mut warnings = Vec::new();
    if output.warning {
        warnings.push(
            output
                .message
                .clone()
                .unwrap_or_else(|| "The native InChI implementation returned a warning.".into()),
        );
        let mut warning = loss(
            "native_warning",
            "provider".into(),
            SemanticImpact::Unknown,
            "The native InChI implementation returned a warning. Its raw code and message are shown before consent; its raw log is diagnostic-only and is not bound to consent. This adapter does not infer which submitted chemistry or stereo the warning affects.",
        );
        warning.disposition = LossDisposition::Unknown;
        losses.push(warning);
    }
    InchiOutput {
        inchi: output.inchi,
        inchikey: output.inchikey,
        aux_info: output.aux_info,
        warnings,
        native_code: output.return_code,
        native_message: output.message,
        native_log: output.log,
        losses,
    }
}

#[cfg(test)]
mod tests {
    use chemifree_domain::document::{
        Atom, AtomRef, Bond, BondId, BondRef, Command, Document, DocumentId, NewAtomPositions,
        Point,
    };
    use chemifree_domain::element::Element;
    use chemifree_domain::states::{AtomKind, AttachmentPoints, QueryAtom};
    use chemifree_domain::stereo::{AtomConfiguration, BondConfiguration, SlotRef, StereoMark};

    use super::*;

    fn atom(symbol: &str) -> Atom {
        Atom::from(Element::from_symbol(symbol).unwrap())
    }

    fn methane() -> MoleculeGraph {
        MoleculeGraph::from_parts(
            vec![(AtomId::from_serial(1).unwrap(), atom("C"))],
            Vec::new(),
        )
    }

    fn tetrahedral(turn: Turn, implicit_hydrogen: bool) -> Document {
        let symbols = if implicit_hydrogen {
            vec!["C", "F", "Cl", "Br"]
        } else {
            vec!["C", "F", "Cl", "Br", "I"]
        };
        let mut commands = Vec::new();
        for symbol in &symbols {
            commands.push(Command::AddAtom {
                atom: atom(symbol),
                positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
                bond_to: None,
            });
        }
        for index in 1..symbols.len() {
            commands.push(Command::AddBond {
                begin: AtomRef::Created(0),
                end: AtomRef::Created(index),
                order: BondOrder::Single,
            });
        }
        commands.push(Command::SetAtomConfiguration {
            atom: AtomRef::Created(0),
            configuration: Some(AtomConfiguration::Tetrahedral {
                neighbors: [
                    SlotRef::Atom(AtomRef::Created(1)),
                    SlotRef::Atom(AtomRef::Created(2)),
                    SlotRef::Atom(AtomRef::Created(3)),
                    if implicit_hydrogen {
                        SlotRef::ImplicitHydrogen
                    } else {
                        SlotRef::Atom(AtomRef::Created(4))
                    },
                ],
                turn,
            }),
        });
        let mut document =
            Document::new(DocumentId::parse("inchi-tetra").unwrap(), "Stereo").unwrap();
        document.apply(&commands).unwrap();
        document
    }

    #[test]
    fn inferred_hydrogens_are_resolved_by_the_existing_valence_model() {
        let prepared = prepare(&methane(), &StereoGroupTable::default()).unwrap();
        assert_eq!(
            prepared.molecule.atoms,
            vec![native::Atom {
                element: "C".into(),
                isotope: None,
                charge: 0,
                radical: native::Radical::None,
                hydrogens: native::Hydrogens::Count(4)
            }]
        );
        assert_eq!(prepared.molecule.bonds, Vec::new());
        assert_eq!(prepared.molecule.stereo, Vec::new());
        assert_eq!(prepared.losses, Vec::new());
    }

    #[test]
    fn unknown_or_unrepresentable_atom_state_and_aromatic_bonds_are_refused() {
        let mut graph = methane();
        graph.atoms_mut().next().unwrap().1.hydrogens = Hydrogens::Unknown;
        assert_eq!(
            prepare(&graph, &StereoGroupTable::default())
                .err()
                .unwrap()
                .code,
            "inchi_hydrogens_unknown"
        );
        graph.atoms_mut().next().unwrap().1.hydrogens = Hydrogens::Inferred;
        graph.atoms_mut().next().unwrap().1.kind = AtomKind::Query(QueryAtom::Any);
        assert_eq!(
            prepare(&graph, &StereoGroupTable::default())
                .err()
                .unwrap()
                .code,
            "inchi_query_unsupported"
        );
        let mut graph = methane();
        graph.atoms_mut().next().unwrap().1.attachment_points =
            AttachmentPoints::new(&[1]).unwrap();
        assert_eq!(
            prepare(&graph, &StereoGroupTable::default())
                .err()
                .unwrap()
                .code,
            "inchi_attachment_points_unsupported"
        );
        let ids = [
            AtomId::from_serial(1).unwrap(),
            AtomId::from_serial(2).unwrap(),
        ];
        let graph = MoleculeGraph::from_parts(
            vec![(ids[0], atom("C")), (ids[1], atom("C"))],
            vec![(
                BondId::from_serial(1).unwrap(),
                Bond::new(ids[0], ids[1], BondOrder::Aromatic),
            )],
        );
        assert_eq!(
            prepare(&graph, &StereoGroupTable::default())
                .err()
                .unwrap()
                .code,
            "inchi_bond_unsupported"
        );
    }

    #[test]
    fn tetrahedral_order_moves_implicit_hydrogen_first_and_adjusts_parity() {
        for (implicit_hydrogen, turn, neighbors, parity) in [
            (true, Turn::Clockwise, [0, 1, 2, 3], native::Parity::Odd),
            (
                true,
                Turn::Anticlockwise,
                [0, 1, 2, 3],
                native::Parity::Even,
            ),
            (false, Turn::Clockwise, [1, 2, 3, 4], native::Parity::Even),
            (
                false,
                Turn::Anticlockwise,
                [1, 2, 3, 4],
                native::Parity::Odd,
            ),
        ] {
            let document = tetrahedral(turn, implicit_hydrogen);
            let prepared = prepare(
                &MoleculeGraph::from_document(&document),
                document.stereo_groups(),
            )
            .unwrap();
            assert_eq!(
                prepared.molecule.stereo,
                vec![native::Stereo::Tetrahedral {
                    center: 0,
                    neighbors,
                    parity
                }]
            );
        }
    }

    #[test]
    fn relative_double_bond_stereo_uses_the_explicit_path_and_reference_relation() {
        for (relation, parity) in [
            (Relation::SameSide, native::Parity::Odd),
            (Relation::OppositeSides, native::Parity::Even),
        ] {
            let mut document =
                Document::new(DocumentId::parse("inchi-alkene").unwrap(), "Alkene").unwrap();
            let mut commands = Vec::new();
            for symbol in ["F", "C", "C", "F"] {
                commands.push(Command::AddAtom {
                    atom: atom(symbol),
                    positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
                    bond_to: None,
                });
            }
            for (begin, end, order) in [
                (0, 1, BondOrder::Single),
                (1, 2, BondOrder::Double),
                (2, 3, BondOrder::Single),
            ] {
                commands.push(Command::AddBond {
                    begin: AtomRef::Created(begin),
                    end: AtomRef::Created(end),
                    order,
                });
            }
            commands.push(Command::SetBondConfiguration {
                bond: BondRef::Created(5),
                configuration: Some(BondConfiguration::Relative {
                    begin_neighbor: SlotRef::Atom(AtomRef::Created(0)),
                    end_neighbor: SlotRef::Atom(AtomRef::Created(3)),
                    relation,
                }),
            });
            document.apply(&commands).unwrap();
            let prepared = prepare(
                &MoleculeGraph::from_document(&document),
                document.stereo_groups(),
            )
            .unwrap();
            assert_eq!(
                prepared.molecule.stereo,
                vec![native::Stereo::DoubleBond {
                    neighbors: [0, 1, 2, 3],
                    parity
                }]
            );
        }
    }

    #[test]
    fn shared_ring_reference_is_reported_as_unsupported_before_native_conversion() {
        let mut document = Document::new(DocumentId::parse("inchi-ring").unwrap(), "Ring").unwrap();
        let mut commands = Vec::new();
        for _ in 0..3 {
            commands.push(Command::AddAtom {
                atom: atom("C"),
                positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
                bond_to: None,
            });
        }
        for (begin, end, order) in [
            (0, 1, BondOrder::Double),
            (1, 2, BondOrder::Single),
            (2, 0, BondOrder::Single),
        ] {
            commands.push(Command::AddBond {
                begin: AtomRef::Created(begin),
                end: AtomRef::Created(end),
                order,
            });
        }
        commands.push(Command::SetBondConfiguration {
            bond: BondRef::Created(3),
            configuration: Some(BondConfiguration::Relative {
                begin_neighbor: SlotRef::Atom(AtomRef::Created(2)),
                end_neighbor: SlotRef::Atom(AtomRef::Created(2)),
                relation: Relation::SameSide,
            }),
        });
        document.apply(&commands).unwrap();
        let failure = prepare(
            &MoleculeGraph::from_document(&document),
            document.stereo_groups(),
        )
        .err()
        .expect("a shared ring reference is not supported by the native profile");
        assert_eq!(failure.kind, InchiErrorKind::Unsupported);
        assert_eq!(failure.code, "inchi_double_stereo_unsupported");
    }

    #[test]
    fn drawn_marks_and_absolute_group_omissions_require_separate_explicit_consent() {
        let mut document = tetrahedral(Turn::Clockwise, true);
        document
            .apply(&[
                Command::SetBondMark {
                    bond: BondRef::Id(BondId::from_serial(1).unwrap()),
                    mark: Some(StereoMark::Wavy),
                },
                Command::CreateStereoGroup {
                    kind: GroupKind::Absolute,
                    number: None,
                    atoms: vec![AtomRef::Id(AtomId::from_serial(1).unwrap())],
                },
            ])
            .unwrap();
        let prepared = prepare(
            &MoleculeGraph::from_document(&document),
            document.stereo_groups(),
        )
        .unwrap();
        assert_eq!(
            prepared
                .losses
                .iter()
                .map(|loss| (loss.feature.as_str(), loss.consent))
                .collect::<Vec<_>>(),
            vec![
                ("stereo_marks", LossConsent::Required),
                ("stereo_group", LossConsent::Required)
            ]
        );
        assert_eq!(
            prepared.molecule.stereo.len(),
            1,
            "Stored stereo is submitted despite the unrelated drawn mark."
        );
    }

    #[test]
    fn native_warning_preserves_raw_output_and_requires_consent_without_interpreting_english() {
        let native = native::Output {
            inchi: "InChI=1S/CH4/h1H4".into(),
            inchikey: "VNWKTOKETHGBQD-UHFFFAOYSA-N".into(),
            aux_info: None,
            return_code: 1,
            warning: true,
            message: Some("opaque native message".into()),
            log: Some("raw log\\nline 2".into()),
        };
        let output = finish(native, Vec::new());
        assert_eq!(output.native_code, 1);
        assert_eq!(
            output.native_message.as_deref(),
            Some("opaque native message")
        );
        assert_eq!(output.native_log.as_deref(), Some("raw log\\nline 2"));
        assert_eq!(output.warnings, vec!["opaque native message"]);
        assert_eq!(output.losses.len(), 1);
        assert_eq!(output.losses[0].feature, "native_warning");
        assert_eq!(output.losses[0].consent, LossConsent::Required);
        assert_eq!(output.losses[0].disposition, LossDisposition::Unknown);
        assert_eq!(output.losses[0].semantic_impact, SemanticImpact::Unknown);
    }
}
