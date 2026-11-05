//! Bounded native chemical fragments. This module never interprets chemistry or clipboard text.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::document::{
    Atom, AtomId, AtomRef, Bond, BondId, BondRef, Command, Document, EditError, MAX_ATOMS,
    MAX_BONDS, MAX_COMMANDS, NewAtomPositions, ObjectId, Point, Vector,
};
use crate::scene::{Selection, SelectionRequest};
use crate::settings::Settings;
use crate::stereo::{
    AtomConfiguration, BondConfiguration, CenterState, DoubleBondState, Slot, SlotRef, StereoKey,
    StereoState, StereoTable,
};

/// An atom and its one copied source-placement coordinate. IDs are local to the fragment.
#[derive(Clone, Debug, PartialEq)]
pub struct FragmentAtom {
    pub id: AtomId,
    pub atom: Atom,
    pub position: Point,
}

/// An induced bond, including its recorded kind and drawing mark.
#[derive(Clone, Debug, PartialEq)]
pub struct FragmentBond {
    pub id: BondId,
    pub bond: Bond,
}

/// One stored configuration. It must be canonical and wholly valid against the fragment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FragmentStereo {
    pub key: StereoKey,
    pub state: StereoState,
}

/// A data-only native fragment. Public fields remain untrusted until paste planning validates them.
#[derive(Clone, Debug, PartialEq)]
pub struct Fragment {
    pub settings: Settings,
    pub atoms: Vec<FragmentAtom>,
    pub bonds: Vec<FragmentBond>,
    pub stereo: Vec<FragmentStereo>,
}

/// Exact copied content plus crossing bonds deliberately outside the induced subgraph.
#[derive(Clone, Debug, PartialEq)]
pub struct CopiedFragment {
    pub fragment: Fragment,
    pub boundary_bonds: Vec<BondId>,
}

/// One copied atom's destination identifier, reserved by the checked document numbering.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AtomMapping {
    pub source: AtomId,
    pub target: AtomId,
}

/// One copied bond's destination identifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BondMapping {
    pub source: BondId,
    pub target: BondId,
}

/// Commands preflighted against one exact destination. Apply under the same revision/lock.
#[derive(Clone, Debug, PartialEq)]
pub struct PastePlan {
    pub commands: Vec<Command>,
    pub atoms: Vec<AtomMapping>,
    pub bonds: Vec<BondMapping>,
    pub selection: SelectionRequest,
}

/// A refusal changes neither source nor destination.
#[derive(Clone, Debug, PartialEq)]
pub enum FragmentError {
    SceneSelection,
    EmptySelection,
    UnknownSelection,
    InvalidInstance,
    EnhancedStereo,
    StereoBoundary,
    DuplicateAtom,
    DuplicateBond,
    DuplicateStereo,
    DestinationInstances,
    StyleMismatch,
    CommandLimit,
    IdentifierLimit,
    Invalid(EditError),
}

impl FragmentError {
    /// Stable reason for the application boundary.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::SceneSelection => "clipboard_scene_selection_unsupported",
            Self::EmptySelection => "clipboard_empty_selection",
            Self::UnknownSelection => "clipboard_selection_unknown",
            Self::InvalidInstance => "clipboard_instance_invalid",
            Self::EnhancedStereo => "clipboard_enhanced_stereo_unsupported",
            Self::StereoBoundary => "clipboard_stereo_boundary",
            Self::DuplicateAtom => "clipboard_duplicate_atom",
            Self::DuplicateBond => "clipboard_duplicate_bond",
            Self::DuplicateStereo => "clipboard_duplicate_stereo",
            Self::DestinationInstances => "clipboard_destination_instances",
            Self::StyleMismatch => "clipboard_style_mismatch",
            Self::CommandLimit => "clipboard_command_limit",
            Self::IdentifierLimit => "clipboard_identifier_limit",
            Self::Invalid(_) => "clipboard_fragment_invalid",
        }
    }
}

impl fmt::Display for FragmentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(error) => write!(formatter, "{}: {error}", self.as_str()),
            other => formatter.write_str(other.as_str()),
        }
    }
}

impl std::error::Error for FragmentError {}

/// Copies selected atoms and bond endpoints, and every bond internal to those atoms.
///
/// # Errors
/// Refuses scene-object targets, absent targets/placement, empty selections, selected enhanced
/// group members, non-self-contained stereo and fragments exceeding the transaction budget.
pub fn copy_fragment(
    document: &Document,
    selection: &Selection,
    instance: ObjectId,
) -> Result<CopiedFragment, FragmentError> {
    if !selection.objects.is_empty() {
        return Err(FragmentError::SceneSelection);
    }
    if !document.instances().any(|id| id == instance) {
        return Err(FragmentError::InvalidInstance);
    }
    let existing: BTreeMap<AtomId, &Atom> = document.atoms().collect();
    let mut atoms = selection.atoms.clone();
    if atoms.iter().any(|id| !existing.contains_key(id)) {
        return Err(FragmentError::UnknownSelection);
    }
    for id in &selection.bonds {
        let bond = document.bond(*id).ok_or(FragmentError::UnknownSelection)?;
        atoms.insert(bond.begin);
        atoms.insert(bond.end);
    }
    if atoms.is_empty() {
        return Err(FragmentError::EmptySelection);
    }
    if document
        .stereo_groups()
        .iter()
        .any(|(_, group)| group.members().iter().any(|id| atoms.contains(id)))
    {
        return Err(FragmentError::EnhancedStereo);
    }
    let mut fragment = Fragment {
        settings: document.settings().clone(),
        atoms: Vec::with_capacity(atoms.len()),
        bonds: Vec::new(),
        stereo: Vec::new(),
    };
    for id in &atoms {
        let atom = existing.get(id).ok_or(FragmentError::UnknownSelection)?;
        let position = document
            .position(instance, *id)
            .ok_or(FragmentError::InvalidInstance)?;
        fragment.atoms.push(FragmentAtom {
            id: *id,
            atom: (**atom).clone(),
            position,
        });
    }
    let mut boundary_bonds = Vec::new();
    let mut copied_bonds = BTreeSet::new();
    for (id, bond) in document.bonds() {
        match (atoms.contains(&bond.begin), atoms.contains(&bond.end)) {
            (true, true) => {
                copied_bonds.insert(id);
                fragment.bonds.push(FragmentBond {
                    id,
                    bond: bond.clone(),
                });
            }
            (true, false) | (false, true) => boundary_bonds.push(id),
            (false, false) => {}
        }
    }
    for (key, state) in document.stereo().iter() {
        let included = match key {
            StereoKey::Center(id) => atoms.contains(&id),
            StereoKey::DoubleBond(id) => copied_bonds.contains(&id),
        };
        if included {
            fragment.stereo.push(FragmentStereo { key, state: *state });
        }
    }
    fragment.validate_stereo().map_err(|error| match error {
        FragmentError::Invalid(_) => FragmentError::StereoBoundary,
        other => other,
    })?;
    fragment.check_budget()?;
    Ok(CopiedFragment {
        fragment,
        boundary_bonds,
    })
}

impl Fragment {
    fn check_budget(&self) -> Result<(), FragmentError> {
        if self.atoms.is_empty() {
            return Err(FragmentError::EmptySelection);
        }
        let count = self.atoms.len()
            + self.bonds.len()
            + self
                .bonds
                .iter()
                .filter(|item| item.bond.mark.is_some())
                .count()
            + self.stereo.len();
        if count > MAX_COMMANDS {
            return Err(FragmentError::CommandLimit);
        }
        Ok(())
    }

    fn validate_stereo(&self) -> Result<(), FragmentError> {
        let mut atoms = BTreeMap::new();
        for item in &self.atoms {
            if atoms.insert(item.id, item.atom.clone()).is_some() {
                return Err(FragmentError::DuplicateAtom);
            }
        }
        let mut bonds = BTreeMap::new();
        for item in &self.bonds {
            if bonds.insert(item.id, item.bond.clone()).is_some() {
                return Err(FragmentError::DuplicateBond);
            }
            if !atoms.contains_key(&item.bond.begin) {
                return Err(FragmentError::Invalid(EditError::UnknownAtom(
                    item.bond.begin,
                )));
            }
            if !atoms.contains_key(&item.bond.end) {
                return Err(FragmentError::Invalid(EditError::UnknownAtom(
                    item.bond.end,
                )));
            }
        }
        let mut records = BTreeMap::new();
        for item in &self.stereo {
            if records.insert(item.key, item.state).is_some() {
                return Err(FragmentError::DuplicateStereo);
            }
        }
        StereoTable::from_records(records)
            .validate(&atoms, &bonds)
            .map_err(FragmentError::Invalid)
    }

    /// Validates and builds one atomic paste, allocating fresh entities rather than addressing
    /// any destination atom or bond by an identifier carried in the fragment.
    ///
    /// # Errors
    /// Refuses malformed data, style mismatch, multiple destination placements, coordinate or
    /// entity limits, exhausted identifiers and more than the ordinary 500 transaction commands.
    pub fn paste_commands(
        &self,
        destination: &Document,
        offset: Vector,
    ) -> Result<PastePlan, FragmentError> {
        if destination.instances().count() != 1 {
            return Err(FragmentError::DestinationInstances);
        }
        if self.settings != *destination.settings() {
            return Err(FragmentError::StyleMismatch);
        }
        self.settings
            .validate()
            .map_err(|error| FragmentError::Invalid(error.into()))?;
        self.check_budget()?;
        if self.atoms.len() > MAX_ATOMS || self.bonds.len() > MAX_BONDS {
            return Err(FragmentError::CommandLimit);
        }
        self.validate_stereo()?;
        if !offset.dx.is_finite() || !offset.dy.is_finite() {
            return Err(FragmentError::Invalid(EditError::CoordinateOutOfRange));
        }
        let mut commands = Vec::new();
        let mut atom_commands = BTreeMap::new();
        let mut bond_commands = BTreeMap::new();
        let mut atom_mapping = Vec::new();
        let mut bond_mapping = Vec::new();
        let numbering = destination.numbering();
        for (index, item) in self.atoms.iter().enumerate() {
            let serial = numbering
                .next_atom
                .checked_add(index as u32)
                .and_then(AtomId::from_serial)
                .ok_or(FragmentError::IdentifierLimit)?;
            atom_mapping.push(AtomMapping {
                source: item.id,
                target: serial,
            });
            atom_commands.insert(item.id, commands.len());
            commands.push(Command::AddAtom {
                atom: item.atom.clone(),
                positions: NewAtomPositions::Single(
                    item.position
                        .translated(offset)
                        .map_err(FragmentError::Invalid)?,
                ),
                bond_to: None,
            });
        }
        for (index, item) in self.bonds.iter().enumerate() {
            let serial = numbering
                .next_bond
                .checked_add(index as u32)
                .and_then(BondId::from_serial)
                .ok_or(FragmentError::IdentifierLimit)?;
            bond_mapping.push(BondMapping {
                source: item.id,
                target: serial,
            });
            bond_commands.insert(item.id, commands.len());
            let begin = created_atom(&atom_commands, item.bond.begin)?;
            let end = created_atom(&atom_commands, item.bond.end)?;
            commands.push(Command::AddBond {
                begin,
                end,
                order: item.bond.order.clone(),
            });
            if let Some(mark) = item.bond.mark {
                commands.push(Command::SetBondMark {
                    bond: BondRef::Created(commands.len() - 1),
                    mark: Some(mark),
                });
            }
        }
        for item in &self.stereo {
            match (item.key, item.state) {
                (StereoKey::Center(id), StereoState::Center(state)) => {
                    let configuration = match state {
                        CenterState::Tetrahedral { slots, turn } => {
                            AtomConfiguration::Tetrahedral {
                                neighbors: remap_slots(&atom_commands, slots)?,
                                turn,
                            }
                        }
                        CenterState::Unknown { slots } => AtomConfiguration::Unknown {
                            neighbors: remap_slots(&atom_commands, slots)?,
                        },
                    };
                    commands.push(Command::SetAtomConfiguration {
                        atom: created_atom(&atom_commands, id)?,
                        configuration: Some(configuration),
                    });
                }
                (StereoKey::DoubleBond(id), StereoState::DoubleBond(state)) => {
                    let index = bond_commands
                        .get(&id)
                        .ok_or(FragmentError::Invalid(EditError::UnknownBond(id)))?;
                    let configuration = match state {
                        DoubleBondState::Relative {
                            begin_ref,
                            end_ref,
                            relation,
                        } => BondConfiguration::Relative {
                            begin_neighbor: remap_slot(&atom_commands, begin_ref)?,
                            end_neighbor: remap_slot(&atom_commands, end_ref)?,
                            relation,
                        },
                        DoubleBondState::Unknown => BondConfiguration::Unknown,
                    };
                    commands.push(Command::SetBondConfiguration {
                        bond: BondRef::Created(*index),
                        configuration: Some(configuration),
                    });
                }
                _ => return Err(FragmentError::StereoBoundary),
            }
        }
        let selection = SelectionRequest {
            objects: Vec::new(),
            atoms: atom_mapping.iter().map(|item| item.target).collect(),
            bonds: bond_mapping.iter().map(|item| item.target).collect(),
        };
        // The existing transaction boundary checks every recorded atom/bond state, graph
        // invariant, coordinate, capacity and identifier counter before callers can commit.
        let mut candidate = destination.clone();
        candidate
            .apply(&commands)
            .map_err(|error| FragmentError::Invalid(error.error))?;
        Ok(PastePlan {
            commands,
            atoms: atom_mapping,
            bonds: bond_mapping,
            selection,
        })
    }
}

fn created_atom(commands: &BTreeMap<AtomId, usize>, id: AtomId) -> Result<AtomRef, FragmentError> {
    commands
        .get(&id)
        .copied()
        .map(AtomRef::Created)
        .ok_or(FragmentError::Invalid(EditError::UnknownAtom(id)))
}

fn remap_slot(commands: &BTreeMap<AtomId, usize>, slot: Slot) -> Result<SlotRef, FragmentError> {
    Ok(match slot {
        Slot::Atom(id) => SlotRef::Atom(created_atom(commands, id)?),
        Slot::ImplicitHydrogen => SlotRef::ImplicitHydrogen,
        Slot::LonePair => SlotRef::LonePair,
    })
}

fn remap_slots(
    commands: &BTreeMap<AtomId, usize>,
    slots: [Slot; 4],
) -> Result<[SlotRef; 4], FragmentError> {
    Ok([
        remap_slot(commands, slots[0])?,
        remap_slot(commands, slots[1])?,
        remap_slot(commands, slots[2])?,
        remap_slot(commands, slots[3])?,
    ])
}
