//! Allocation, insertion, translation, and reference resolution on a transaction's working state.

use std::collections::BTreeSet;

use super::{
    Atom, AtomId, AtomRef, Bond, BondId, BondOrder, BondRef, Command, Created, Draft, EditError,
    GroupId, GroupRef, MAX_ATOMS, MAX_BONDS, NewAtomPositions, ObjectId, Point, PositionKey,
    RingPattern, Vector, unique,
};
use crate::element::Element;
use crate::scene::{
    MAX_OBJECTS, ObjectKind, ObjectRef, SceneObject, Selection, SelectionRequest, SelectionScope,
};
use crate::stereo::{Slot, SlotRef, StereoKey, StereoState};
use crate::stereo_drawing::{DeriveError, Drawing};
use crate::stereo_group::{StereoGroup, StereoGroupError};

impl Draft {
    pub(super) fn allocate_atom(&mut self) -> Result<AtomId, EditError> {
        if self.content.atoms.len() >= MAX_ATOMS {
            return Err(EditError::AtomLimit);
        }
        let id = AtomId(self.next_atom);
        self.next_atom = self
            .next_atom
            .checked_add(1)
            .ok_or(EditError::IdentifierSpaceExhausted)?;
        Ok(id)
    }

    /// Allocates a carbon cycle with explicit orders; callers validate the recipe and positions.
    pub(super) fn insert_carbon_ring(
        &mut self,
        instance: ObjectId,
        positions: &[Point],
        pattern: RingPattern,
    ) -> Result<AtomId, EditError> {
        let mut atoms = Vec::with_capacity(positions.len());
        for point in positions {
            let atom = self.allocate_atom()?;
            self.content.atoms.insert(atom, Atom::from(Element::CARBON));
            self.content
                .positions
                .insert(PositionKey { instance, atom }, *point);
            atoms.push(atom);
        }
        for index in 0..atoms.len() {
            self.insert_bond(
                atoms[index],
                atoms[(index + 1) % atoms.len()],
                if pattern == RingPattern::Kekule && index % 2 == 0 {
                    BondOrder::Double
                } else {
                    BondOrder::Single
                },
            )?;
        }
        Ok(atoms[0])
    }

    pub(super) fn insert_bond(
        &mut self,
        begin: AtomId,
        end: AtomId,
        order: BondOrder,
    ) -> Result<BondId, EditError> {
        self.content.require_atom(begin)?;
        self.content.require_atom(end)?;
        if begin == end {
            return Err(EditError::SelfBond(begin));
        }
        if let Some(existing) = self.content.bond_between(begin, end) {
            return Err(EditError::DuplicateBond { existing });
        }
        if self.content.bonds.len() >= MAX_BONDS {
            return Err(EditError::BondLimit);
        }
        let bond = Bond::new(begin, end, order);
        bond.validate()?;
        let id = BondId(self.next_bond);
        self.next_bond = self
            .next_bond
            .checked_add(1)
            .ok_or(EditError::IdentifierSpaceExhausted)?;
        self.content.bonds.insert(id, bond);
        self.discard_stereo(&BTreeSet::from([begin, end]));
        Ok(id)
    }

    /// Removes the stereo records that a change to the bonded neighbors of the `touched` atoms,
    /// or the deletion of their atom or bond, leaves meaningless and remembers them for the
    /// reply. Every such removal goes through
    /// here, the one place a cleared center is reported.
    pub(super) fn discard_stereo(&mut self, touched: &BTreeSet<AtomId>) {
        let cleared =
            self.content
                .stereo
                .discard_changed(touched, &self.content.atoms, &self.content.bonds);
        self.cascaded.extend(cleared);
    }

    pub(super) fn allocate_object(&mut self) -> Result<ObjectId, EditError> {
        if self.content.scene.objects.len() >= MAX_OBJECTS {
            return Err(EditError::ObjectLimit);
        }
        let id = ObjectId(self.next_object);
        self.next_object = self
            .next_object
            .checked_add(1)
            .ok_or(EditError::IdentifierSpaceExhausted)?;
        Ok(id)
    }

    /// A new atom's position in each current scene instance.
    pub(super) fn new_atom_positions(
        &self,
        positions: &NewAtomPositions,
    ) -> Result<Vec<(ObjectId, Point)>, EditError> {
        match positions {
            NewAtomPositions::Single(point) => match self.content.instance_count() {
                1 => Ok(vec![(self.content.first_instance(), *point)]),
                instances => Err(EditError::PositionsRequired { instances }),
            },
            NewAtomPositions::PerInstance(points) => {
                for id in points.keys() {
                    self.content.require_instance(*id)?;
                }
                self.content
                    .scene
                    .instances()
                    .map(|id| {
                        points
                            .get(&id)
                            .map(|point| (id, *point))
                            .ok_or(EditError::MissingPosition(id))
                    })
                    .collect()
            }
        }
    }

    pub(super) fn translate_scope(
        &mut self,
        scope: &SelectionScope,
        delta: Vector,
    ) -> Result<(), EditError> {
        if !delta.dx.is_finite() || !delta.dy.is_finite() {
            return Err(EditError::CoordinateOutOfRange);
        }
        for id in &scope.objects {
            if let Some(SceneObject::Image(image)) = self.content.scene.objects.get_mut(id) {
                *image = image.translated(delta)?;
            }
            if let Some(SceneObject::Caption(caption)) = self.content.scene.objects.get_mut(id) {
                caption.anchor = caption.anchor.translated(delta)?;
            }
        }
        for key in &scope.positions {
            self.translate(key.instance, [key.atom], delta)?;
        }
        for id in &scope.graphics {
            self.content.scene.require(*id, ObjectKind::Graphic)?;
            if let Some(SceneObject::Graphic(graphic)) = self.content.scene.objects.get_mut(id) {
                *graphic = graphic.translated(delta)?;
            }
        }
        Ok(())
    }

    /// Translates every listed atom's position in `instance` once.
    pub(super) fn translate(
        &mut self,
        instance: ObjectId,
        atoms: impl IntoIterator<Item = AtomId>,
        delta: Vector,
    ) -> Result<(), EditError> {
        for atom in atoms {
            let point = self
                .content
                .positions
                .get_mut(&PositionKey { instance, atom })
                .ok_or(EditError::UnknownAtom(atom))?;
            *point = point.translated(delta)?;
        }
        Ok(())
    }

    pub(super) fn resolve(&self, atom: AtomRef) -> Result<AtomId, EditError> {
        match atom {
            AtomRef::Id(id) => Ok(id),
            AtomRef::Created(index) => match self.created.get(index) {
                Some(Created::Atom(id) | Created::AtomAndBond(id, _)) => Ok(*id),
                _ => Err(EditError::UnresolvedReference(index)),
            },
        }
    }

    fn resolve_group(&self, group: GroupRef) -> Result<GroupId, EditError> {
        match group {
            GroupRef::Id(id) => Ok(id),
            GroupRef::Created(index) => match self.created.get(index) {
                Some(Created::Group(id)) => Ok(*id),
                _ => Err(EditError::UnresolvedGroupReference(index)),
            },
        }
    }

    /// The atoms a group command names, as a set; at least one.
    fn resolve_group_atoms(&self, atoms: &[AtomRef]) -> Result<BTreeSet<AtomId>, EditError> {
        if atoms.is_empty() {
            return Err(StereoGroupError::Empty.into());
        }
        atoms.iter().map(|atom| self.resolve(*atom)).collect()
    }

    fn allocate_group(&mut self) -> Result<GroupId, EditError> {
        let id = GroupId(self.next_group);
        self.next_group = self
            .next_group
            .checked_add(1)
            .ok_or(EditError::IdentifierSpaceExhausted)?;
        Ok(id)
    }

    /// Applies one stereo group command. Members are checked against the stored configurations
    /// as they are at this point of the transaction.
    pub(super) fn apply_group_command(&mut self, command: &Command) -> Result<Created, EditError> {
        let mut created = Created::Nothing;
        match command {
            Command::CreateStereoGroup {
                kind,
                number,
                atoms,
            } => {
                let members = self.resolve_group_atoms(atoms)?;
                let id = self.allocate_group()?;
                let group = StereoGroup::new(*kind, *number, members);
                self.content
                    .stereo_groups
                    .create(id, group, &self.content.stereo)?;
                created = Created::Group(id);
            }
            Command::DeleteStereoGroup { group } => {
                let id = self.resolve_group(*group)?;
                self.content.stereo_groups.delete(id)?;
            }
            Command::SetStereoGroupKind { group, kind } => {
                let id = self.resolve_group(*group)?;
                self.content.stereo_groups.set_kind(id, *kind)?;
            }
            Command::SetStereoGroupNumber { group, number } => {
                let id = self.resolve_group(*group)?;
                self.content.stereo_groups.set_number(id, *number)?;
            }
            Command::AddStereoGroupMembers { group, atoms } => {
                let id = self.resolve_group(*group)?;
                let members = self.resolve_group_atoms(atoms)?;
                self.content
                    .stereo_groups
                    .add_members(id, &members, &self.content.stereo)?;
            }
            Command::RemoveStereoGroupMembers { group, atoms } => {
                let id = self.resolve_group(*group)?;
                let members = self.resolve_group_atoms(atoms)?;
                self.content.stereo_groups.remove_members(id, &members)?;
            }
            Command::MergeStereoGroups { into, from } => {
                let (into, from) = (self.resolve_group(*into)?, self.resolve_group(*from)?);
                self.content.stereo_groups.merge(into, from)?;
            }
            Command::SplitStereoGroup {
                group,
                atoms,
                number,
            } => {
                let id = self.resolve_group(*group)?;
                let members = self.resolve_group_atoms(atoms)?;
                let new_group = self.allocate_group()?;
                self.content
                    .stereo_groups
                    .split(id, new_group, &members, *number)?;
                created = Created::Group(new_group);
            }
            _ => unreachable!("only stereo group commands are routed here"),
        }
        Ok(created)
    }

    pub(super) fn resolve_bond(&self, bond: BondRef) -> Result<BondId, EditError> {
        match bond {
            BondRef::Id(id) => Ok(id),
            BondRef::Created(index) => match self.created.get(index) {
                Some(Created::Bond(id) | Created::AtomAndBond(_, id)) => Ok(*id),
                _ => Err(EditError::UnresolvedBondReference(index)),
            },
        }
    }

    /// The configurations the drawing of `instance` shows for the items a
    /// [`Command::SetConfigurationFromDrawing`] names (see there), stored only when every item
    /// derived, so a refusal leaves the table as it was.
    pub(super) fn derive_from_drawing(
        &mut self,
        atoms: Option<&[AtomRef]>,
        bonds: Option<&[BondRef]>,
        instance: Option<ObjectId>,
    ) -> Result<(), EditError> {
        let instance = self.content.target_instance(instance)?;
        let atoms = atoms
            .map(|refs| {
                let ids = refs
                    .iter()
                    .map(|atom| self.resolve(*atom))
                    .collect::<Result<Vec<_>, _>>()?;
                let ids = unique(&ids)?;
                ids.iter()
                    .try_for_each(|&id| self.content.require_atom(id))?;
                Ok::<_, EditError>(ids)
            })
            .transpose()?;
        let bonds = bonds
            .map(|refs| {
                let ids = refs
                    .iter()
                    .map(|bond| self.resolve_bond(*bond))
                    .collect::<Result<Vec<_>, _>>()?;
                let ids = unique(&ids)?;
                match ids.iter().find(|id| !self.content.bonds.contains_key(id)) {
                    Some(&missing) => Err(EditError::UnknownBond(missing)),
                    None => Ok(ids),
                }
            })
            .transpose()?;

        let drawing = Drawing::new(
            &self.content.atoms,
            &self.content.bonds,
            self.content.instance_positions(instance),
        );
        let listed = atoms.is_some() || bonds.is_some();
        let centers: Vec<AtomId> = match atoms {
            Some(named) => named.into_iter().collect(),
            None if listed => Vec::new(),
            None => drawing.marked_centers(),
        };
        let double_bonds: Vec<BondId> = match bonds {
            Some(named) => named.into_iter().collect(),
            None if listed => Vec::new(),
            None => drawing.crossed_double_bonds(),
        };
        // Without a list, any structurally impossible item (in this scope only a crossed double
        // bond with `GeometryUnsupported` or `ReferenceInvalid`) is left out without a note;
        // with one, the caller asked for it, so it is refused with its structural reason.
        let skippable = |cause: &DeriveError| !listed && matches!(cause, DeriveError::Structure(_));
        let mut derived = Vec::with_capacity(centers.len() + double_bonds.len());
        for atom in centers {
            match drawing.derive_center(atom) {
                Ok(state) => derived.push((StereoKey::Center(atom), StereoState::Center(state))),
                Err(cause) if skippable(&cause) => {}
                Err(cause) => {
                    return Err(EditError::FromDrawing {
                        item: StereoKey::Center(atom),
                        cause,
                    });
                }
            }
        }
        for bond in double_bonds {
            match drawing.derive_double_bond(bond) {
                Ok(state) => {
                    derived.push((StereoKey::DoubleBond(bond), StereoState::DoubleBond(state)))
                }
                Err(cause) if skippable(&cause) => {}
                Err(cause) => {
                    return Err(EditError::FromDrawing {
                        item: StereoKey::DoubleBond(bond),
                        cause,
                    });
                }
            }
        }
        drop(drawing);
        for (key, state) in derived {
            self.content.stereo.set(key, Some(state));
        }
        Ok(())
    }

    pub(super) fn resolve_slot(&self, slot: SlotRef) -> Result<Slot, EditError> {
        Ok(match slot {
            SlotRef::Atom(atom) => {
                let id = self.resolve(atom)?;
                self.content.require_atom(id)?;
                Slot::Atom(id)
            }
            SlotRef::ImplicitHydrogen => Slot::ImplicitHydrogen,
            SlotRef::LonePair => Slot::LonePair,
        })
    }

    pub(super) fn resolve_slots(&self, slots: [SlotRef; 4]) -> Result<[Slot; 4], EditError> {
        let mut resolved = [Slot::ImplicitHydrogen; 4];
        for (target, slot) in resolved.iter_mut().zip(slots) {
            *target = self.resolve_slot(slot)?;
        }
        Ok(resolved)
    }

    /// The selection a transaction leaves, as the client named it: every `$N` must name a
    /// command that created a scene object, and everything named must exist after the edit.
    pub(super) fn resolve_selection(
        &self,
        request: &SelectionRequest,
    ) -> Result<Selection, EditError> {
        let mut objects = BTreeSet::new();
        for object in &request.objects {
            let id = match object {
                ObjectRef::Id(id) => *id,
                ObjectRef::Created(index) => match self.created.get(*index) {
                    Some(Created::Object(id)) => *id,
                    _ => {
                        return Err(EditError::SelectionUnknown(format!(
                            "${index}, which is not a command that creates a scene object"
                        )));
                    }
                },
            };
            objects.insert(id);
        }
        let selection = Selection {
            objects,
            atoms: request.atoms.iter().copied().collect(),
            bonds: request.bonds.iter().copied().collect(),
        };
        self.content.check_selection(&selection)?;
        Ok(selection)
    }

    /// Moves every position of `instances` once.
    pub(super) fn translate_instances(
        &mut self,
        instances: &[ObjectId],
        delta: Vector,
    ) -> Result<(), EditError> {
        let atoms: Vec<AtomId> = self.content.atoms.keys().copied().collect();
        for instance in instances {
            self.translate(*instance, atoms.iter().copied(), delta)?;
        }
        Ok(())
    }
}
