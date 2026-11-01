//! Projection between SMILES notation and the editor's single graph.
//!
//! The text codec handles notation only. Hydrogen counts, radical states, aromaticity and
//! canonical order come from the shared chemistry models, so a SMILES string never carries
//! its own private valence table.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use chemifree_domain::aromaticity::{self, Classification, KekuleOrder};
use chemifree_domain::chemistry::{self, RadicalInference};
use chemifree_domain::document::{
    Atom, AtomId, AtomRef, BondId, BondOrder, Command, NewAtomPositions, Point,
};
use chemifree_domain::element::Element;
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::identity::{self, IdentityError};
use chemifree_domain::states::{AtomKind, Hydrogens, Radical};

use crate::contract::DocumentView;
use crate::smiles::{
    MAX_SMILES_ITEMS, ORGANIC_SUBSET, SmilesAtom, SmilesBond, SmilesBondKind, SmilesCodec,
    SmilesFormatError, SmilesMolecule, SmilesRead, WriteMode,
};
use crate::{Loss, LossDisposition, SemanticImpact};

const GRID_COLUMNS: usize = 12;
const GRID_SPACING: f64 = 40.0;

/// Why an import cannot be projected onto the drawing.
pub(crate) struct ImportRefusal {
    pub code: &'static str,
    pub message: String,
}

fn loss(
    path: String,
    feature: &str,
    disposition: LossDisposition,
    impact: SemanticImpact,
    reason: &str,
) -> Loss {
    Loss {
        feature: feature.into(),
        source_path: path,
        disposition,
        reason: reason.into(),
        semantic_impact: impact,
    }
}

fn order(kind: SmilesBondKind) -> BondOrder {
    match kind {
        SmilesBondKind::Single => BondOrder::Single,
        SmilesBondKind::Double => BondOrder::Double,
        SmilesBondKind::Triple => BondOrder::Triple,
        SmilesBondKind::Aromatic => BondOrder::Aromatic,
    }
}

fn kind_of(order: &BondOrder) -> Option<SmilesBondKind> {
    match order {
        BondOrder::Single => Some(SmilesBondKind::Single),
        BondOrder::Double => Some(SmilesBondKind::Double),
        BondOrder::Triple => Some(SmilesBondKind::Triple),
        BondOrder::Aromatic => Some(SmilesBondKind::Aromatic),
        BondOrder::Dative | BondOrder::Query(_) | BondOrder::Unknown => None,
    }
}

fn integral(kind: SmilesBondKind) -> Option<u16> {
    match kind {
        SmilesBondKind::Single => Some(1),
        SmilesBondKind::Double => Some(2),
        SmilesBondKind::Triple => Some(3),
        SmilesBondKind::Aromatic => None,
    }
}

/// Domain edits for one read molecule, with every loss the projection implies.
pub(crate) fn import_commands(
    read: &SmilesRead,
    doc: &DocumentView,
    source: &str,
    losses: &mut Vec<Loss>,
) -> Result<Vec<Command>, ImportRefusal> {
    if doc.scene.instances.len() != 1 {
        return Err(ImportRefusal {
            code: "unsupported_scene",
            message: "Import requires exactly one molecule placement.".into(),
        });
    }
    let molecule = &read.molecule;
    if let Some((start, end)) = read.ignored {
        losses.push(loss(
            format!("source[{start}..{end}]"),
            "ignored_text",
            LossDisposition::Dropped,
            SemanticImpact::Document,
            &format!(
                "Text after the first whitespace is not part of the SMILES (often a name) and was ignored: {:?}",
                source.get(start..end).unwrap_or_default().chars().take(60).collect::<String>()
            ),
        ));
    }
    let mut sum = vec![Some(0_u16); molecule.atoms.len()];
    let mut aromatic = vec![false; molecule.atoms.len()];
    for bond in &molecule.bonds {
        for end in [bond.begin, bond.end] {
            sum[end] = sum[end].zip(integral(bond.kind)).map(|(a, b)| a + b);
            aromatic[end] |= bond.kind == SmilesBondKind::Aromatic;
        }
    }
    let left = doc
        .atoms
        .iter()
        .map(|a| a.position.x)
        .fold(f64::INFINITY, f64::min);
    let top = doc
        .atoms
        .iter()
        .map(|a| a.position.y)
        .fold(f64::NEG_INFINITY, f64::max);
    let (x0, y0) = if doc.atoms.is_empty() {
        (0.0, 0.0)
    } else {
        (left, top + 2.0 * GRID_SPACING)
    };
    let mut commands = Vec::with_capacity(molecule.atoms.len() + molecule.bonds.len());
    for (index, source_atom) in molecule.atoms.iter().enumerate() {
        let mut atom = Atom::new(AtomKind::Element(source_atom.element));
        atom.charge = source_atom.charge;
        atom.isotope = source_atom.isotope;
        if let Some(count) = source_atom.hydrogens {
            atom.hydrogens = Hydrogens::Specified(count);
            // An aromatic bracket atom's bond orders are not known until a Kekulé form is
            // assigned, so its radical state is left closed-shell.
            if !aromatic[index] {
                match chemistry::infer_radical(&atom, sum[index]) {
                    RadicalInference::Determined(radical) => atom.radical = radical,
                    RadicalInference::Undetermined(reason) => {
                        atom.radical = Radical::Unknown;
                        let (start, end) = source_atom.span;
                        losses.push(loss(
                            format!("source[{start}..{end}]"),
                            "radical_state",
                            LossDisposition::Unknown,
                            SemanticImpact::Chemistry,
                            match reason {
                                "singlet_or_triplet" => {
                                    "This bracket atom has two fewer bonds than its closed shell; SMILES does not distinguish a singlet from a triplet, so the radical state is recorded as undetermined."
                                }
                                _ => {
                                    "This atom is outside the valence model, so its radical state is recorded as undetermined; this is not a finding that the shell is incomplete."
                                }
                            },
                        ));
                    }
                }
            }
        }
        let column = index % GRID_COLUMNS;
        let row = index / GRID_COLUMNS;
        let point = Point::new(
            x0 + column as f64 * GRID_SPACING,
            y0 + row as f64 * GRID_SPACING,
        )
        .map_err(|_| ImportRefusal {
            code: "coordinate_out_of_range",
            message: "The generated layout does not fit in the editor.".into(),
        })?;
        commands.push(Command::AddAtom {
            atom,
            positions: NewAtomPositions::Single(point),
            bond_to: None,
        });
    }
    for bond in &molecule.bonds {
        commands.push(Command::AddBond {
            begin: AtomRef::Created(bond.begin),
            end: AtomRef::Created(bond.end),
            order: order(bond.kind),
        });
    }
    if !molecule.atoms.is_empty() {
        losses.push(loss(
            "atoms".into(),
            "generated_coordinates",
            LossDisposition::Normalized,
            SemanticImpact::Geometry,
            "SMILES carries no coordinates; atoms are placed on a placeholder grid because no structure layout is implemented.",
        ));
    }
    if molecule
        .bonds
        .iter()
        .any(|b| b.kind == SmilesBondKind::Aromatic)
    {
        losses.push(loss(
            "bonds".into(),
            "aromatic_bonds_unchecked",
            LossDisposition::Unknown,
            SemanticImpact::Chemistry,
            "Aromatic bonds are recorded as written. Whether the rings are aromatic under the bounded aromaticity model is a separate inspection.",
        ));
    }
    Ok(commands)
}

/// Why a drawing was not written, with its cause.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WriteRefusal {
    pub reason: &'static str,
    /// A size or search limit, as opposed to something the notation cannot express.
    pub limit: bool,
    pub atom: Option<AtomId>,
}

impl WriteRefusal {
    fn unsupported(reason: &'static str, atom: Option<AtomId>) -> Self {
        Self {
            reason,
            limit: false,
            atom,
        }
    }

    fn limit(reason: &'static str) -> Self {
        Self {
            reason,
            limit: true,
            atom: None,
        }
    }
}

pub(crate) struct Written {
    pub text: String,
    /// The drawing's atoms in the order they appear in the written molecule.
    pub order: Vec<AtomId>,
    /// Canonical mode: explicit terminal hydrogens folded into counts, as (hydrogen, heavy atom).
    pub folded: Vec<(AtomId, AtomId)>,
    pub losses: Vec<Loss>,
}

/// Checks the graph against what the notation and the shared models can express, and returns
/// the Kekulé orders of its aromatic bonds. Aromaticity is checked before any atom is formatted
/// so that a refusal names the real cause.
fn prepare(graph: &MoleculeGraph) -> Result<BTreeMap<BondId, KekuleOrder>, WriteRefusal> {
    for (id, atom) in graph.atoms() {
        if atom.kind.element().is_none() {
            return Err(WriteRefusal::unsupported("query_atom", Some(id)));
        }
        if !atom.attachment_points.is_empty() {
            return Err(WriteRefusal::unsupported("attachment_point", Some(id)));
        }
        if atom.hydrogens == Hydrogens::Unknown {
            return Err(WriteRefusal::unsupported("unknown_hydrogens", Some(id)));
        }
    }
    if graph
        .bonds()
        .any(|(_, bond)| kind_of(&bond.order).is_none())
    {
        return Err(WriteRefusal::unsupported("unsupported_bond", None));
    }
    if !graph
        .bonds()
        .any(|(_, bond)| bond.order == BondOrder::Aromatic)
    {
        return Ok(BTreeMap::new());
    }
    let interpretation = aromaticity::perceive(graph);
    let aromatic: BTreeSet<_> = graph
        .bonds()
        .filter_map(|(id, bond)| (bond.order == BondOrder::Aromatic).then_some(id))
        .collect();
    for system in &interpretation.systems {
        if system.classification != Classification::Aromatic
            && system.bonds.iter().any(|id| aromatic.contains(id))
        {
            return Err(WriteRefusal::unsupported(
                system.reason.unwrap_or("aromaticity_unsupported"),
                None,
            ));
        }
    }
    interpretation
        .kekulize()
        .map(|orders| orders.into_iter().collect())
        .map_err(|reason| WriteRefusal::unsupported(reason, None))
}

/// One atom as the notation will state it.
struct AtomPlan<'a> {
    id: AtomId,
    atom: &'a Atom,
    aromatic: bool,
    /// Hydrogens the shared model determines (stated, or inferred from the bonds).
    hydrogens: Option<u16>,
    /// The hydrogens a reader infers for the bare atom from the bonds written.
    written_sum: Option<u16>,
    /// Writes brackets even where the atom would otherwise be bare.
    force_bracket: bool,
}

fn smiles_atom(plan: &AtomPlan<'_>, losses: &mut Vec<Loss>) -> Result<SmilesAtom, WriteRefusal> {
    let atom = plan.atom;
    let element: Element = atom.kind.element().expect("queries were refused");
    let symbol = element.symbol();
    let bare_shape = ORGANIC_SUBSET.contains(&symbol)
        && atom.charge == 0
        && atom.isotope.is_none()
        && atom.radical == Radical::None
        && !plan.force_bracket;
    let default = plan
        .written_sum
        .and_then(|sum| chemistry::inferred_hydrogens(element, sum));
    // A bare aromatic nitrogen is read as pyridine-type, so a nitrogen that carries hydrogen
    // is always bracketed.
    let pyrrole_like = plan.aromatic && symbol == "N" && plan.hydrogens != Some(0);
    let bare =
        bare_shape && !pyrrole_like && (plan.hydrogens.is_none() || plan.hydrogens == default);
    let hydrogens = if bare {
        None
    } else {
        let Some(count) = plan.hydrogens else {
            return Err(WriteRefusal::unsupported(
                "hydrogens_undetermined",
                Some(plan.id),
            ));
        };
        let count =
            u8::try_from(count)
                .ok()
                .filter(|c| *c <= 9)
                .ok_or(WriteRefusal::unsupported(
                    "hydrogen_count_unwritable",
                    Some(plan.id),
                ))?;
        // Check what a reader will conclude about the radical from the text.
        if plan.aromatic {
            if atom.radical != Radical::None {
                return Err(WriteRefusal::unsupported(
                    "radical_not_representable",
                    Some(plan.id),
                ));
            }
        } else {
            let mut probe = atom.clone();
            probe.hydrogens = Hydrogens::Specified(count);
            match (
                chemistry::infer_radical(&probe, plan.written_sum),
                atom.radical,
            ) {
                (RadicalInference::Determined(read), recorded) if read == recorded => {}
                (RadicalInference::Undetermined(_), Radical::Unknown) => {}
                (RadicalInference::Undetermined(_), Radical::None) => {
                    losses.push(loss(
                        format!("atoms[{}]", plan.id),
                        "radical_state",
                        LossDisposition::Unknown,
                        SemanticImpact::Chemistry,
                        "Reading this text records an undetermined radical state for the atom, which the shared valence model cannot settle.",
                    ));
                }
                _ => {
                    return Err(WriteRefusal::unsupported(
                        "radical_not_representable",
                        Some(plan.id),
                    ));
                }
            }
        }
        Some(count)
    };
    Ok(SmilesAtom {
        element,
        aromatic: plan.aromatic,
        isotope: atom.isotope,
        charge: atom.charge,
        hydrogens,
        span: (0, 0),
    })
}

fn representation_change(path: String, feature: &str, reason: &str) -> Loss {
    loss(
        path,
        feature,
        LossDisposition::Normalized,
        SemanticImpact::Chemistry,
        reason,
    )
}

/// What a SMILES text does not carry of the drawing, in the vocabulary other exports use.
pub(crate) fn export_losses(doc: &DocumentView, losses: &mut Vec<Loss>) {
    if !doc.atoms.is_empty() {
        losses.push(loss(
            "document.atoms".into(),
            "coordinates",
            LossDisposition::Dropped,
            SemanticImpact::Geometry,
            "SMILES carries no atom coordinates.",
        ));
    }
    losses.push(loss(
        "document.title".into(),
        "document_title",
        LossDisposition::Dropped,
        SemanticImpact::Document,
        "The drawing title is not written.",
    ));
    crate::ct_projection::drawing_losses(doc, "document", "SMILES", "text positions", losses);
}

fn format_failure(error: SmilesFormatError) -> WriteRefusal {
    match error {
        SmilesFormatError::SizeLimit => WriteRefusal::limit("size_limit"),
        SmilesFormatError::OutputLimit => WriteRefusal::limit("output_limit"),
        SmilesFormatError::BranchDepthLimit => WriteRefusal::limit("branch_depth_limit"),
        SmilesFormatError::RingLabelLimit => WriteRefusal::limit("ring_label_limit"),
        SmilesFormatError::NotWritable => WriteRefusal::unsupported("not_writable", None),
    }
}

/// Write the graph in recorded order or in Chemifree's canonical order.
pub(crate) fn write_graph(
    codec: &dyn SmilesCodec,
    graph: &MoleculeGraph,
    mode: WriteMode,
) -> Result<Written, WriteRefusal> {
    if graph.atoms().len() + graph.bonds().len() > MAX_SMILES_ITEMS {
        return Err(WriteRefusal::limit("size_limit"));
    }
    if graph.atoms().len() == 0 {
        return Err(WriteRefusal::unsupported("empty_graph", None));
    }
    // What the notation can state is checked first, so both modes name the same cause and atom.
    let kekule = prepare(graph)?;
    let canonical =
        match mode {
            WriteMode::Recorded => None,
            WriteMode::Canonical => Some(identity::canonicalize_chemical(graph).map_err(
                |error| match error {
                    IdentityError::AtomLimit => WriteRefusal::limit("atom_limit"),
                    IdentityError::SearchLimit => WriteRefusal::limit("search_limit"),
                    IdentityError::Unsupported(reason) => WriteRefusal::unsupported(reason, None),
                },
            )?),
        };
    let assessed = chemistry::assess_graph(graph, &kekule);
    let atoms: BTreeMap<_, _> = graph.atoms().collect();
    let mut losses = Vec::new();
    let mut molecule = SmilesMolecule::default();
    let mut order = Vec::new();
    let mut index_of: HashMap<AtomId, usize> = HashMap::new();
    match &canonical {
        None => {
            let aromatic_atoms: BTreeSet<AtomId> = graph
                .bonds()
                .filter(|(_, bond)| bond.order == BondOrder::Aromatic)
                .flat_map(|(_, bond)| [bond.begin, bond.end])
                .collect();
            for (id, atom) in graph.atoms() {
                let state = &assessed[&id];
                let stated = matches!(atom.hydrogens, Hydrogens::Specified(_));
                let plan = AtomPlan {
                    id,
                    atom,
                    aromatic: aromatic_atoms.contains(&id),
                    hydrogens: match atom.hydrogens {
                        Hydrogens::Specified(count) => Some(u16::from(count)),
                        _ => (state.finding == chemistry::Finding::Valid)
                            .then_some(state.hydrogen_count)
                            .flatten(),
                    },
                    written_sum: state.bond_order_sum,
                    force_bracket: stated,
                };
                index_of.insert(id, molecule.atoms.len());
                molecule.atoms.push(smiles_atom(&plan, &mut losses)?);
                order.push(id);
            }
            for (_, bond) in graph.bonds() {
                molecule.bonds.push(SmilesBond {
                    begin: index_of[&bond.begin],
                    end: index_of[&bond.end],
                    kind: kind_of(&bond.order).expect("bond kinds were checked"),
                });
            }
        }
        Some(canonical) => {
            let detail = canonical
                .chemical
                .as_ref()
                .expect("the chemical profile always reports its interpretation");
            let mut folded: HashMap<AtomId, u16> = HashMap::new();
            for (hydrogen, parent) in &canonical.folded_hydrogens {
                *folded.entry(*parent).or_default() += 1;
                losses.push(representation_change(
                    format!("atoms[{hydrogen}]"),
                    "folded_hydrogens",
                    "This terminal hydrogen vertex is written as a hydrogen count on its heavy atom, so it has no atom of its own in the text.",
                ));
            }
            let aromatic_atoms: BTreeSet<AtomId> = graph
                .bonds()
                .filter(|(id, _)| detail.aromatic_bonds.contains(id))
                .flat_map(|(_, bond)| [bond.begin, bond.end])
                .collect();
            for (rank, &id) in canonical.numbering.iter().enumerate() {
                let state = &assessed[&id];
                let hidden = folded.get(&id).copied().unwrap_or(0);
                let plan = AtomPlan {
                    id,
                    atom: atoms[&id],
                    aromatic: aromatic_atoms.contains(&id),
                    hydrogens: Some(detail.hydrogens[rank]),
                    written_sum: state.bond_order_sum.map(|sum| sum.saturating_sub(hidden)),
                    force_bracket: false,
                };
                index_of.insert(id, rank);
                let written = smiles_atom(&plan, &mut losses)?;
                if matches!(plan.atom.hydrogens, Hydrogens::Specified(_))
                    && written.hydrogens.is_none()
                {
                    losses.push(representation_change(
                        format!("atoms[{id}]"),
                        "hydrogen_state",
                        "The stated hydrogen count is written as an inferred one because it equals the default for the atom's bonds.",
                    ));
                }
                molecule.atoms.push(written);
                order.push(id);
            }
            for (id, bond) in graph.bonds() {
                let (Some(&begin), Some(&end)) =
                    (index_of.get(&bond.begin), index_of.get(&bond.end))
                else {
                    continue;
                };
                let kind = if detail.aromatic_bonds.contains(&id) {
                    if bond.order != BondOrder::Aromatic {
                        losses.push(representation_change(
                            format!("bonds[{id}]"),
                            "aromatic_bonds_rewritten",
                            "A single or double bond of a ring the aromaticity model reads as aromatic is written as an aromatic bond.",
                        ));
                    }
                    SmilesBondKind::Aromatic
                } else {
                    kind_of(&bond.order).expect("bond kinds were checked")
                };
                molecule.bonds.push(SmilesBond {
                    begin: begin.min(end),
                    end: begin.max(end),
                    kind,
                });
            }
            molecule.bonds.sort_by_key(|bond| (bond.begin, bond.end));
        }
    }
    let written = codec.write(&molecule).map_err(format_failure)?;
    // The text names atoms in its own emission order, not in the order they were supplied.
    let order = written
        .atom_order
        .iter()
        .map(|&index| order[index])
        .collect();
    let folded = canonical
        .map(|canonical| canonical.folded_hydrogens)
        .unwrap_or_default();
    Ok(Written {
        text: written.text,
        order,
        folded,
        losses,
    })
}
