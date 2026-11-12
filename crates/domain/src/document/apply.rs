//! The effect of each command on a transaction's working state.

use std::collections::BTreeSet;

use super::{
    Bond, BondOrder, Command, Created, Draft, EditError, FragmentDisplay, FragmentDisplayRun,
    MAX_INSTANCES, ObjectId, Point, PositionKey, RingPattern, RingSize, abbreviation,
    fragment_display, unique, validated_title,
};
use crate::scene;
use crate::scene::{DeleteMode, Graphic, GraphicShape, MAX_REFERENCES, ObjectKind, SceneObject};
use crate::states::AtomKind;
use crate::stereo::{
    AtomConfiguration, BondConfiguration, CenterState, DoubleBondState, Graph, StereoError,
    StereoKey, StereoState, check_center, check_double_bond, relative_double_bond,
};

// Fixed recipes start at the rightmost vertex, clockwise in y-down coordinates.
// Keep the original six-ring arithmetic unchanged for compatibility.
fn ring_offsets(length: f64, size: RingSize) -> Vec<(f64, f64)> {
    match size {
        RingSize::Six => {
            let height = length * (3.0_f64.sqrt() / 2.0);
            vec![
                (length, 0.0),
                (length / 2.0, height),
                (-length / 2.0, height),
                (-length, 0.0),
                (-length / 2.0, -height),
                (length / 2.0, -height),
            ]
        }
        RingSize::Five => {
            // sin(36°), cos(72°), cos(36°), sin(72°), from the golden ratio.
            let root_five = 5.0_f64.sqrt();
            let radius = 2.0 * length / (10.0 - 2.0 * root_five).sqrt();
            let near_x = radius * (root_five - 1.0) / 4.0;
            let far_x = -radius * (root_five + 1.0) / 4.0;
            let high_y = radius * (10.0 + 2.0 * root_five).sqrt() / 4.0;
            vec![
                (radius, 0.0),
                (near_x, high_y),
                (far_x, length / 2.0),
                (far_x, -length / 2.0),
                (near_x, -high_y),
            ]
        }
    }
}

impl Draft {
    /// Applies one command and records what, if anything, it created.
    pub(super) fn apply(&mut self, command: &Command) -> Result<(), EditError> {
        let display_before = (!self.content.abbreviations.is_empty()
            || !self.content.fragment_displays.is_empty())
        .then(|| self.content.clone());
        let mut created = Created::Nothing;
        match command {
            Command::ContractFragment {
                instance,
                anchor,
                members,
                text,
            } => {
                let key = PositionKey {
                    instance: *instance,
                    atom: *anchor,
                };
                let display = FragmentDisplay::plain(members.clone(), text.clone());
                self.content.check_fragment_display(key, &display)?;
                let mut display = display;
                display.members.sort_unstable();
                self.content.check_fragment_overlap(key, &display)?;
                self.content.fragment_displays.insert(key, display);
            }
            Command::SetFragmentDisplayText {
                instance,
                anchor,
                text,
            } => {
                abbreviation::validate_display_label(text)?;
                let key = PositionKey {
                    instance: *instance,
                    atom: *anchor,
                };
                let display = self
                    .content
                    .fragment_displays
                    .get_mut(&key)
                    .ok_or(EditError::FragmentDisplayInvalid)?;
                display.runs = vec![FragmentDisplayRun {
                    text: text.clone(),
                    script: scene::CaptionScript::Baseline,
                }];
            }
            Command::SetFragmentDisplayRuns {
                instance,
                anchor,
                runs,
            } => {
                fragment_display::validate_runs(runs)?;
                let key = PositionKey {
                    instance: *instance,
                    atom: *anchor,
                };
                let display = self
                    .content
                    .fragment_displays
                    .get_mut(&key)
                    .ok_or(EditError::FragmentDisplayInvalid)?;
                display.runs.clone_from(runs);
            }
            Command::ExpandFragment { instance, anchor } => {
                self.content.require_instance(*instance)?;
                self.content.require_atom(*anchor)?;
                self.content.fragment_displays.remove(&PositionKey {
                    instance: *instance,
                    atom: *anchor,
                });
            }
            Command::CollapseEthyl {
                instance,
                anchor,
                terminal,
            } => {
                let key = PositionKey {
                    instance: *instance,
                    atom: *anchor,
                };
                self.content.check_ethyl(key, *terminal)?;
                if self.content.fragment_displays.values().any(|display| {
                    display
                        .members
                        .iter()
                        .any(|id| [*anchor, *terminal].contains(id))
                }) {
                    return Err(EditError::FragmentDisplayInvalid);
                }
                if self.content.abbreviations.iter().any(|(other, end)| {
                    *other != key
                        && [other.atom, *end]
                            .iter()
                            .any(|id| [*anchor, *terminal].contains(id))
                }) {
                    return Err(EditError::AbbreviationInvalid);
                }
                self.content.abbreviations.insert(key, *terminal);
            }
            Command::ExpandEthyl { instance, anchor } => {
                self.content.require_instance(*instance)?;
                self.content.require_atom(*anchor)?;
                let key = PositionKey {
                    instance: *instance,
                    atom: *anchor,
                };
                self.content.abbreviations.remove(&key);
                self.content.display_labels.remove(&key);
            }
            Command::UpdateAbbreviationLabel {
                instance,
                anchor,
                text,
            } => {
                let key = PositionKey {
                    instance: *instance,
                    atom: *anchor,
                };
                let terminal = self
                    .content
                    .abbreviations
                    .get(&key)
                    .ok_or(EditError::AbbreviationInvalid)?;
                self.content.check_ethyl(key, *terminal)?;
                if let Some(text) = text {
                    abbreviation::validate_display_label(text)?;
                    // Canonical default: explicit Et and reset produce the same state.
                    if text == "Et" {
                        self.content.display_labels.remove(&key);
                    } else {
                        self.content.display_labels.insert(key, text.clone());
                    }
                } else {
                    self.content.display_labels.remove(&key);
                }
            }
            Command::InsertRing {
                instance,
                center,
                size,
                pattern,
            } => {
                if *pattern == RingPattern::Kekule && *size != RingSize::Six {
                    return Err(EditError::RingPatternInvalid);
                }
                self.content.require_instance(*instance)?;
                let instances = self.content.instance_count();
                if instances != 1 {
                    return Err(EditError::PositionsRequired { instances });
                }
                let length = self.content.settings.bond_length_pt;
                let positions = ring_offsets(length, *size)
                    .into_iter()
                    .map(|(x, y)| Point::new(center.x() + x, center.y() + y))
                    .collect::<Result<Vec<_>, _>>()?;
                self.insert_carbon_ring(*instance, &positions, *pattern)?;
            }
            Command::AttachRing {
                instance,
                atom,
                direction,
                size,
                pattern,
            } => {
                if *pattern == RingPattern::Kekule && *size != RingSize::Six {
                    return Err(EditError::RingPatternInvalid);
                }
                self.content.require_instance(*instance)?;
                let instances = self.content.instance_count();
                if instances != 1 {
                    return Err(EditError::PositionsRequired { instances });
                }
                self.content.require_atom(*atom)?;
                let scale = direction.dx.abs().max(direction.dy.abs());
                if !direction.dx.is_finite() || !direction.dy.is_finite() || scale == 0.0 {
                    return Err(EditError::RingDirectionInvalid);
                }
                // Scale before hypot so even huge and subnormal finite directions normalize.
                let dx = direction.dx / scale;
                let dy = direction.dy / scale;
                let norm = dx.hypot(dy);
                let (ux, uy) = (dx / norm, dy / norm);
                let origin = self.content.positions[&PositionKey {
                    instance: *instance,
                    atom: *atom,
                }];
                let length = self.content.settings.bond_length_pt;
                let offsets = ring_offsets(length, *size);
                let radius = offsets[0].0;
                let positions = offsets
                    .into_iter()
                    .map(|(x, y)| {
                        let axial = length + (radius - x);
                        Point::new(
                            origin.x() + axial * ux + y * uy,
                            origin.y() + axial * uy - y * ux,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let first = self.insert_carbon_ring(*instance, &positions, *pattern)?;
                self.insert_bond(*atom, first, BondOrder::Single)?;
            }
            Command::AddAtom {
                atom,
                positions,
                bond_to,
            } => {
                atom.validate()?;
                let target = bond_to
                    .as_ref()
                    .map(|target| {
                        Ok::<_, EditError>((self.resolve(target.atom)?, target.order.clone()))
                    })
                    .transpose()?;
                if let Some((atom, _)) = &target {
                    self.content.require_atom(*atom)?;
                }
                let points = self.new_atom_positions(positions)?;
                let id = self.allocate_atom()?;
                created = Created::Atom(id);
                self.content.atoms.insert(id, atom.clone());
                for (instance, point) in points {
                    self.content
                        .positions
                        .insert(PositionKey { instance, atom: id }, point);
                }
                if let Some((atom, order)) = target {
                    created = Created::AtomAndBond(id, self.insert_bond(atom, id, order)?);
                }
            }
            Command::AddBond { begin, end, order } => {
                let (begin, end) = (self.resolve(*begin)?, self.resolve(*end)?);
                created = Created::Bond(self.insert_bond(begin, end, order.clone())?);
            }
            Command::UpdateAtom { atom, edit } => {
                let target = self
                    .content
                    .atoms
                    .get_mut(atom)
                    .ok_or(EditError::UnknownAtom(*atom))?;
                let mut updated = target.clone();
                if let Some(kind) = &edit.kind {
                    updated.kind = kind.clone();
                }
                if let Some(charge) = edit.charge {
                    updated.charge = charge;
                }
                if let Some(isotope) = edit.isotope {
                    updated.isotope = isotope;
                }
                if let Some(hydrogens) = edit.hydrogens {
                    updated.hydrogens = hydrogens;
                }
                if let Some(radical) = edit.radical {
                    updated.radical = radical;
                }
                if let Some(points) = edit.attachment_points {
                    updated.attachment_points = points;
                }
                updated.validate()?;
                if matches!(updated.kind, AtomKind::Query(_))
                    && self.content.stereo.get(StereoKey::Center(*atom)).is_some()
                {
                    return Err(StereoError::CenterUnsupported.into());
                }
                *target = updated;
            }
            Command::MoveAtoms {
                atoms,
                delta,
                instance,
            } => {
                let atoms = unique(atoms)?;
                let instance = self.content.target_instance(*instance)?;
                self.translate(instance, atoms, *delta)?;
            }
            Command::UpdateBond { bond, order } => {
                let target = self
                    .content
                    .bonds
                    .get_mut(bond)
                    .ok_or(EditError::UnknownBond(*bond))?;
                let updated = Bond {
                    order: order.clone(),
                    ..target.clone()
                };
                updated.validate()?;
                *target = updated;
                // A stored configuration must stay valid; the order change that would break it
                // is refused with the rule it breaks rather than erasing the configuration.
                self.content
                    .stereo
                    .validate(&self.content.atoms, &self.content.bonds)?;
            }
            Command::DeleteAtoms { atoms } => {
                let targets = unique(atoms)?;
                for id in &targets {
                    self.content.require_atom(*id)?;
                }
                let mut touched = targets.clone();
                for bond in self.content.bonds.values() {
                    if targets.contains(&bond.begin) || targets.contains(&bond.end) {
                        touched.extend([bond.begin, bond.end]);
                    }
                }
                for id in &targets {
                    self.content.atoms.remove(id);
                }
                self.content.bonds.retain(|_, bond| {
                    !targets.contains(&bond.begin) && !targets.contains(&bond.end)
                });
                self.content
                    .positions
                    .retain(|key, _| !targets.contains(&key.atom));
                self.discard_stereo(&touched);
                self.content.refuse_dangling_references()?;
            }
            Command::DeleteBonds { bonds } => {
                let mut touched = BTreeSet::new();
                for id in unique(bonds)? {
                    let removed = self
                        .content
                        .bonds
                        .remove(&id)
                        .ok_or(EditError::UnknownBond(id))?;
                    touched.extend([removed.begin, removed.end]);
                }
                self.discard_stereo(&touched);
                self.content.refuse_dangling_references()?;
            }
            Command::Rename { title } => {
                let title = validated_title(title)?;
                title.clone_into(&mut self.content.title);
            }
            Command::AddInstance { source, offset } => {
                let source = match source {
                    Some(id) => self.content.require_instance(*id)?,
                    None => self.content.first_instance(),
                };
                let copied = self
                    .content
                    .instance_positions(source)
                    .map(|(atom, point)| Ok((atom, point.translated(*offset)?)))
                    .collect::<Result<Vec<_>, EditError>>()?;
                if self.content.instance_count() >= MAX_INSTANCES {
                    return Err(EditError::InstanceLimit);
                }
                let id = self.allocate_object()?;
                self.content
                    .scene
                    .insert(id, SceneObject::Instance, None, None)?;
                for (atom, point) in copied {
                    self.content
                        .positions
                        .insert(PositionKey { instance: id, atom }, point);
                }
                created = Created::Object(id);
            }
            Command::MoveInstance { instance, offset } => {
                let instance = self.content.require_instance(*instance)?;
                self.translate_instances(&[instance], *offset)?;
            }
            Command::DeleteInstance { instance } => {
                let instance = self.content.require_instance(*instance)?;
                self.delete_objects(&BTreeSet::from([instance]), DeleteMode::ObjectOnly)?;
            }
            Command::AddCaption {
                caption,
                parent,
                index,
            } => {
                caption.validate()?;
                let id = self.allocate_object()?;
                self.content.scene.insert(
                    id,
                    SceneObject::Caption(caption.clone()),
                    *parent,
                    *index,
                )?;
                created = Created::Object(id);
            }
            Command::UpdateCaption { object, caption } => {
                caption.validate()?;
                self.content.scene.require(*object, ObjectKind::Caption)?;
                self.content
                    .scene
                    .objects
                    .insert(*object, SceneObject::Caption(caption.clone()));
            }
            Command::AddImage {
                image,
                parent,
                index,
            } => {
                image.validate()?;
                let id = self.allocate_object()?;
                self.content.scene.insert(
                    id,
                    SceneObject::Image(image.clone()),
                    *parent,
                    *index,
                )?;
                created = Created::Object(id);
            }
            Command::UpdateImageGeometry {
                object,
                origin,
                size_pt,
            } => {
                self.content.scene.require(*object, ObjectKind::Image)?;
                if let Some(SceneObject::Image(image)) = self.content.scene.objects.get_mut(object)
                {
                    image.origin = *origin;
                    image.size_pt = *size_pt;
                    image.validate()?;
                }
            }
            Command::AddGraphic {
                graphic,
                parent,
                index,
            } => {
                if matches!(graphic.shape, GraphicShape::Polygon | GraphicShape::Spline) {
                    return Err(EditError::GraphicCommandMismatch);
                }
                graphic.validate()?;
                let id = self.allocate_object()?;
                self.content.scene.insert(
                    id,
                    SceneObject::Graphic(graphic.clone()),
                    *parent,
                    *index,
                )?;
                created = Created::Object(id);
            }
            Command::AddSpline {
                points,
                line_width_pt,
                parent,
                index,
            } => {
                let graphic = Graphic {
                    shape: GraphicShape::Spline,
                    points: points.to_vec(),
                    line_width_pt: *line_width_pt,
                };
                graphic.validate()?;
                let id = self.allocate_object()?;
                self.content
                    .scene
                    .insert(id, SceneObject::Graphic(graphic), *parent, *index)?;
                created = Created::Object(id);
            }
            Command::UpdateSplinePoints { object, points } => {
                self.content.scene.require(*object, ObjectKind::Graphic)?;
                if let Some(SceneObject::Graphic(graphic)) =
                    self.content.scene.objects.get_mut(object)
                {
                    if graphic.shape != GraphicShape::Spline {
                        return Err(EditError::GraphicCommandMismatch);
                    }
                    graphic.points = points.to_vec();
                }
            }
            Command::AddPolygon {
                vertices,
                line_width_pt,
                parent,
                index,
            } => {
                let graphic = Graphic {
                    shape: GraphicShape::Polygon,
                    points: vertices.clone(),
                    line_width_pt: *line_width_pt,
                };
                graphic.validate()?;
                let id = self.allocate_object()?;
                self.content
                    .scene
                    .insert(id, SceneObject::Graphic(graphic), *parent, *index)?;
                created = Created::Object(id);
            }
            Command::UpdatePolygonVertices { object, vertices } => {
                self.content.scene.require(*object, ObjectKind::Graphic)?;
                if let Some(SceneObject::Graphic(graphic)) =
                    self.content.scene.objects.get_mut(object)
                {
                    if graphic.shape != GraphicShape::Polygon {
                        return Err(EditError::GraphicCommandMismatch);
                    }
                    graphic.points = vertices.clone();
                    graphic.validate()?;
                }
            }
            Command::SetGraphicLineWidth {
                object,
                line_width_pt,
            } => {
                self.content.scene.require(*object, ObjectKind::Graphic)?;
                if let Some(SceneObject::Graphic(graphic)) =
                    self.content.scene.objects.get_mut(object)
                {
                    graphic.line_width_pt = *line_width_pt;
                    graphic.validate()?;
                }
            }
            Command::UpdateGraphicPoints { object, points } => {
                self.content.scene.require(*object, ObjectKind::Graphic)?;
                if let Some(SceneObject::Graphic(graphic)) =
                    self.content.scene.objects.get_mut(object)
                {
                    if matches!(graphic.shape, GraphicShape::Polygon | GraphicShape::Spline) {
                        return Err(EditError::GraphicCommandMismatch);
                    }
                    graphic.points = points.to_vec();
                }
            }
            Command::Group { objects } => {
                if objects.is_empty() {
                    return Err(EditError::EmptyTargetList);
                }
                let id = self.allocate_object()?;
                self.content.scene.group(id, objects)?;
                created = Created::Object(id);
            }
            Command::Ungroup { group } => {
                self.content.scene.ungroup(*group)?;
                self.content
                    .references
                    .retain(|reference| reference.source != *group);
                self.content.refuse_dangling_references()?;
            }
            Command::Reorder { object, index } => self.content.scene.reorder(*object, *index)?,
            Command::DeleteObjects { objects, mode } => {
                self.delete_objects(&unique(objects)?, *mode)?;
            }
            Command::MoveObjects { objects, delta } => {
                let instances = self.content.scene.translate(&unique(objects)?, *delta)?;
                self.translate_instances(&instances, *delta)?;
            }
            Command::MoveSelection {
                selection,
                instance,
                delta,
            } => {
                let scope = self.content.selection_scope(selection, *instance)?;
                self.translate_scope(&scope, *delta)?;
            }
            Command::ArrangeObjects {
                objects,
                arrangement,
            } => {
                let objects: BTreeSet<ObjectId> = objects.iter().copied().collect();
                let plan = self.content.object_arrangement(&objects, *arrangement)?;
                for unit in plan.units {
                    for key in &unit.scope.positions {
                        let point = self
                            .content
                            .positions
                            .get_mut(key)
                            .ok_or(EditError::MissingPosition(key.instance))?;
                        *point = unit.translated(*point)?;
                    }
                    for id in &unit.scope.graphics {
                        if let Some(SceneObject::Graphic(graphic)) =
                            self.content.scene.objects.get_mut(id)
                        {
                            for point in &mut graphic.points {
                                *point = unit.translated(*point)?;
                            }
                        }
                    }
                }
            }
            Command::TransformSelection {
                selection,
                instance,
                pivot,
                transform,
            } => {
                let plan = self
                    .content
                    .selection_transform(selection, *instance, *pivot, *transform)?;
                for position in plan.positions {
                    self.content.positions.insert(position.key, position.after);
                }
                for graphic in plan.graphics {
                    self.content
                        .scene
                        .objects
                        .insert(graphic.object, SceneObject::Graphic(graphic.after));
                }
            }
            Command::AddReference { reference } => {
                if !self.content.scene.objects.contains_key(&reference.source)
                    || !self.content.target_exists(reference.target)
                {
                    return Err(EditError::ReferenceUnknown(*reference));
                }
                if self.content.references.len() >= MAX_REFERENCES
                    && !self.content.references.contains(reference)
                {
                    return Err(EditError::ReferenceLimit);
                }
                self.content.references.insert(*reference);
            }
            Command::RemoveReference { reference } => {
                if !self.content.references.remove(reference) {
                    return Err(EditError::ReferenceUnknown(*reference));
                }
            }
            Command::UpdateSettings(edit) => {
                self.content.settings = self.content.settings.edited(edit)?;
            }
            Command::SetBondMark { bond, mark } => {
                let id = self.resolve_bond(*bond)?;
                let target = self
                    .content
                    .bonds
                    .get_mut(&id)
                    .ok_or(EditError::UnknownBond(id))?;
                let updated = Bond {
                    mark: *mark,
                    ..target.clone()
                };
                updated.validate()?;
                *target = updated;
            }
            Command::SetAtomConfiguration {
                atom,
                configuration,
            } => {
                let atom = self.resolve(*atom)?;
                self.content.require_atom(atom)?;
                let state = match configuration {
                    None => None,
                    Some(configuration) => {
                        let state = match configuration {
                            AtomConfiguration::Tetrahedral { neighbors, turn } => {
                                CenterState::tetrahedral(self.resolve_slots(*neighbors)?, *turn)?
                            }
                            AtomConfiguration::Unknown { neighbors } => {
                                CenterState::unknown(self.resolve_slots(*neighbors)?)?
                            }
                        };
                        let graph = Graph::new(&self.content.atoms, &self.content.bonds);
                        check_center(atom, &state, &graph)?;
                        Some(StereoState::Center(state))
                    }
                };
                self.content.stereo.set(StereoKey::Center(atom), state);
            }
            Command::SetBondConfiguration {
                bond,
                configuration,
            } => {
                let id = self.resolve_bond(*bond)?;
                if !self.content.bonds.contains_key(&id) {
                    return Err(EditError::UnknownBond(id));
                }
                let state = match configuration {
                    None => None,
                    Some(configuration) => {
                        let graph = Graph::new(&self.content.atoms, &self.content.bonds);
                        let state = match configuration {
                            BondConfiguration::Relative {
                                begin_neighbor,
                                end_neighbor,
                                relation,
                            } => relative_double_bond(
                                id,
                                self.resolve_slot(*begin_neighbor)?,
                                self.resolve_slot(*end_neighbor)?,
                                *relation,
                                &graph,
                            )?,
                            BondConfiguration::Unknown => DoubleBondState::Unknown,
                        };
                        check_double_bond(id, &state, &graph)?;
                        Some(StereoState::DoubleBond(state))
                    }
                };
                self.content.stereo.set(StereoKey::DoubleBond(id), state);
            }
            Command::SetConfigurationFromDrawing {
                atoms,
                bonds,
                instance,
            } => self.derive_from_drawing(atoms.as_deref(), bonds.as_deref(), *instance)?,
            Command::CreateStereoGroup { .. }
            | Command::DeleteStereoGroup { .. }
            | Command::SetStereoGroupKind { .. }
            | Command::SetStereoGroupNumber { .. }
            | Command::AddStereoGroupMembers { .. }
            | Command::RemoveStereoGroupMembers { .. }
            | Command::MergeStereoGroups { .. }
            | Command::SplitStereoGroup { .. } => created = self.apply_group_command(command)?,
        }
        if let Some(before) = display_before {
            self.content.reconcile_abbreviations(&before, command);
            self.content.reconcile_fragment_displays(&before, command);
        }
        self.created.push(created);
        Ok(())
    }

    fn delete_objects(
        &mut self,
        objects: &BTreeSet<ObjectId>,
        mode: DeleteMode,
    ) -> Result<(), EditError> {
        let removed = self.content.scene.delete(objects, mode)?;
        self.content
            .references
            .retain(|reference| !removed.contains(&reference.source));
        self.content
            .positions
            .retain(|key, _| !removed.contains(&key.instance));
        self.content.refuse_dangling_references()?;
        Ok(())
    }
}
