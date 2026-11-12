//! Lookups, selection planning, and invariant checks over a document's current content.

use std::collections::{BTreeSet, HashSet};

use super::{
    AtomId, BondId, Content, EditError, MAX_ATOMS, MAX_BONDS, MAX_INSTANCES, MAX_TITLE_CHARS,
    ObjectId, Point, PositionKey, Vector,
};
use crate::image::MAX_CURRENT_IMAGE_PIXELS;
use crate::scene;
use crate::scene::{
    ArrangementPlan, ArrangementUnit, Bounds, Graphic, GraphicShape, MAX_REFERENCES,
    ObjectArrangement, ObjectKind, ReferenceTarget, SceneObject, SceneReference, Selection,
    SelectionScope, SelectionTransform, TransformPlan, TransformedGraphic, TransformedPosition,
};

impl Content {
    pub(super) fn target_exists(&self, target: ReferenceTarget) -> bool {
        match target {
            ReferenceTarget::Object(id) => self.scene.objects.contains_key(&id),
            ReferenceTarget::Atom(id) => self.atoms.contains_key(&id),
            ReferenceTarget::Bond(id) => self.bonds.contains_key(&id),
        }
    }

    fn invalid_reference(&self) -> Option<SceneReference> {
        self.references.iter().copied().find(|reference| {
            !self.scene.objects.contains_key(&reference.source)
                || !self.target_exists(reference.target)
        })
    }

    /// Called after a deletion has removed the references owned by deleted objects.
    pub(super) fn refuse_dangling_references(&self) -> Result<(), EditError> {
        match self.invalid_reference() {
            Some(reference) => Err(EditError::ReferenceInUse(reference)),
            None => Ok(()),
        }
    }

    pub(super) fn bond_between(&self, a: AtomId, b: AtomId) -> Option<BondId> {
        self.bonds
            .iter()
            .find(|(_, bond)| bond.connects(a, b))
            .map(|(id, _)| *id)
    }

    pub(super) fn require_atom(&self, id: AtomId) -> Result<(), EditError> {
        if self.atoms.contains_key(&id) {
            Ok(())
        } else {
            Err(EditError::UnknownAtom(id))
        }
    }

    pub(super) fn require_instance(&self, id: ObjectId) -> Result<ObjectId, EditError> {
        self.scene.require(id, ObjectKind::Instance).map(|()| id)
    }

    pub(super) fn instance_count(&self) -> usize {
        self.scene.instances().count()
    }

    pub(super) fn first_instance(&self) -> ObjectId {
        self.scene
            .instances()
            .next()
            .expect("validated content keeps at least one scene instance")
    }

    /// The instance a command acts in: the named one, or the only one.
    pub(super) fn target_instance(&self, named: Option<ObjectId>) -> Result<ObjectId, EditError> {
        match named {
            Some(id) => self.require_instance(id),
            None => match self.instance_count() {
                1 => Ok(self.first_instance()),
                instances => Err(EditError::InstanceRequired { instances }),
            },
        }
    }

    /// The positions of one instance, by atom.
    pub(super) fn instance_positions(
        &self,
        instance: ObjectId,
    ) -> impl Iterator<Item = (AtomId, Point)> + '_ {
        let first = PositionKey {
            instance,
            atom: AtomId(0),
        };
        let last = PositionKey {
            instance,
            atom: AtomId(u32::MAX),
        };
        self.positions
            .range(first..=last)
            .map(|(key, point)| (key.atom, *point))
    }

    /// Fails unless everything `selection` names exists.
    pub(super) fn check_selection(&self, selection: &Selection) -> Result<(), EditError> {
        let unknown = |what: String| Err(EditError::SelectionUnknown(what));
        if let Some(id) = selection
            .objects
            .iter()
            .find(|id| !self.scene.objects.contains_key(id))
        {
            return unknown(format!("scene object {id}, which does not exist"));
        }
        if let Some(id) = selection
            .atoms
            .iter()
            .find(|id| !self.atoms.contains_key(id))
        {
            return unknown(format!("atom {id}, which does not exist"));
        }
        if let Some(id) = selection
            .bonds
            .iter()
            .find(|id| !self.bonds.contains_key(id))
        {
            return unknown(format!("bond {id}, which does not exist"));
        }
        Ok(())
    }

    pub(super) fn selection_scope(
        &self,
        selection: &Selection,
        instance: Option<ObjectId>,
    ) -> Result<SelectionScope, EditError> {
        self.check_selection(selection)?;
        // Even a scene-only or empty selection must not silently accept a bad placement.
        if let Some(instance) = instance {
            self.require_instance(instance)?;
        }
        let mut scope = SelectionScope {
            objects: self.scene.subtrees(&selection.objects),
            ..SelectionScope::default()
        };
        for object in &scope.objects {
            match self.scene.get(*object)? {
                SceneObject::Instance => {
                    scope
                        .positions
                        .extend(
                            self.instance_positions(*object)
                                .map(|(atom, _)| PositionKey {
                                    instance: *object,
                                    atom,
                                }),
                        );
                }
                SceneObject::Graphic(_) => {
                    scope.graphics.insert(*object);
                }
                SceneObject::Caption(_) | SceneObject::Image(_) | SceneObject::Group { .. } => {}
            }
        }
        let mut atoms = selection.atoms.clone();
        for id in &selection.bonds {
            let bond = self.bonds.get(id).ok_or(EditError::UnknownBond(*id))?;
            atoms.extend([bond.begin, bond.end]);
        }
        if !atoms.is_empty() {
            let instance = self.target_instance(instance)?;
            scope
                .positions
                .extend(atoms.into_iter().map(|atom| PositionKey { instance, atom }));
        }
        Ok(scope)
    }

    pub(super) fn object_arrangement(
        &self,
        objects: &BTreeSet<ObjectId>,
        arrangement: ObjectArrangement,
    ) -> Result<ArrangementPlan, EditError> {
        let roots = self.scene.outermost(objects)?;
        let mut units = Vec::with_capacity(roots.len());
        for object in roots {
            let scope = self.selection_scope(
                &Selection {
                    objects: BTreeSet::from([object]),
                    ..Selection::default()
                },
                None,
            )?;
            for id in &scope.objects {
                if matches!(self.scene.get(*id)?, SceneObject::Caption(_)) {
                    return Err(EditError::CaptionUnsupported(*id));
                }
            }
            let mut points = Vec::new();
            for key in &scope.positions {
                points.push(
                    *self
                        .positions
                        .get(key)
                        .ok_or(EditError::MissingPosition(key.instance))?,
                );
            }
            for graphic in &scope.graphics {
                if let SceneObject::Graphic(graphic) = self.scene.get(*graphic)? {
                    points.extend(graphic.points.iter().copied());
                }
            }
            for id in &scope.objects {
                if let SceneObject::Image(image) = self.scene.get(*id)? {
                    points.extend([image.origin, image.end()?]);
                }
            }
            let bounds =
                Bounds::from_points(points).ok_or(EditError::ArrangementEmptyGeometry(object))?;
            units.push(ArrangementUnit {
                object,
                bounds,
                delta: Vector { dx: 0.0, dy: 0.0 },
                translation: None,
                scope,
            });
        }
        let plan = scene::plan_arrangement(units, arrangement)?;
        // A preview and the mutation share the same preflight, including bounds on each
        // actual geometry point rather than just an approximate rendered rectangle.
        for unit in &plan.units {
            for key in &unit.scope.positions {
                unit.translated(
                    *self
                        .positions
                        .get(key)
                        .ok_or(EditError::MissingPosition(key.instance))?,
                )?;
            }
            for id in &unit.scope.objects {
                if let SceneObject::Image(image) = self.scene.get(*id)? {
                    unit.translated(image.origin)?;
                    unit.translated(image.end()?)?;
                }
            }
            for graphic in &unit.scope.graphics {
                if let SceneObject::Graphic(graphic) = self.scene.get(*graphic)? {
                    for point in &graphic.points {
                        unit.translated(*point)?;
                    }
                }
            }
        }
        Ok(plan)
    }

    pub(super) fn selection_transform(
        &self,
        selection: &Selection,
        instance: Option<ObjectId>,
        pivot: Point,
        transform: SelectionTransform,
    ) -> Result<TransformPlan, EditError> {
        let scope = self.selection_scope(selection, instance)?;
        for id in &scope.objects {
            if matches!(self.scene.get(*id)?, SceneObject::Caption(_)) {
                return Err(EditError::CaptionUnsupported(*id));
            }
            if matches!(self.scene.get(*id)?, SceneObject::Image(_)) {
                return Err(EditError::ImageUnsupported(*id));
            }
        }
        let prepared = scene::PreparedTransform::new(transform)?;
        let mut positions = Vec::with_capacity(scope.positions.len());
        let mut graphics = Vec::with_capacity(scope.graphics.len());
        for key in &scope.positions {
            let before = *self
                .positions
                .get(key)
                .ok_or(EditError::MissingPosition(key.instance))?;
            positions.push(TransformedPosition {
                key: *key,
                before,
                after: prepared.point(before, pivot)?,
            });
        }
        for object in &scope.graphics {
            if let SceneObject::Graphic(before) = self.scene.get(*object)? {
                if !prepared.supports_axis_aligned() {
                    match before.shape {
                        GraphicShape::Rectangle => {
                            return Err(EditError::TransformRectangleUnsupported(*object));
                        }
                        GraphicShape::Ellipse => {
                            return Err(EditError::TransformEllipseUnsupported(*object));
                        }
                        _ => {}
                    }
                }
                let after = Graphic {
                    line_width_pt: before.line_width_pt,
                    shape: before.shape,
                    points: before
                        .points
                        .iter()
                        .map(|point| prepared.point(*point, pivot))
                        .collect::<Result<_, _>>()?,
                };
                graphics.push(TransformedGraphic {
                    object: *object,
                    before: before.clone(),
                    after,
                });
            }
        }
        let drawing_stereo_may_change = positions
            .iter()
            .any(|position| position.before != position.after);
        Ok(TransformPlan {
            scope,
            positions,
            graphics,
            drawing_stereo_may_change,
        })
    }

    /// `selection` less what does not exist here.
    pub(super) fn existing(&self, selection: &Selection) -> Selection {
        Selection {
            objects: selection
                .objects
                .iter()
                .copied()
                .filter(|id| self.scene.objects.contains_key(id))
                .collect(),
            atoms: selection
                .atoms
                .iter()
                .copied()
                .filter(|id| self.atoms.contains_key(id))
                .collect(),
            bonds: selection
                .bonds
                .iter()
                .copied()
                .filter(|id| self.bonds.contains_key(id))
                .collect(),
        }
    }

    /// Checks every structural invariant; used for restored and replayed states.
    pub(super) fn validate(&self) -> Result<(), EditError> {
        // Restored history is untrusted: every replayed state must fit the same
        // current raster budget as the document that is initially opened.
        if self.current_image_pixels() > MAX_CURRENT_IMAGE_PIXELS {
            return Err(EditError::ImagePixelLimit);
        }
        self.validate_abbreviations()?;
        self.validate_fragment_displays()?;
        validate_title(&self.title)?;
        self.settings.validate()?;
        if self.atoms.len() > MAX_ATOMS {
            return Err(EditError::AtomLimit);
        }
        if self.bonds.len() > MAX_BONDS {
            return Err(EditError::BondLimit);
        }
        for atom in self.atoms.values() {
            atom.validate()?;
        }
        let mut pairs = HashSet::with_capacity(self.bonds.len());
        for (id, bond) in &self.bonds {
            bond.validate()?;
            self.require_atom(bond.begin)?;
            self.require_atom(bond.end)?;
            if bond.begin == bond.end {
                return Err(EditError::SelfBond(bond.begin));
            }
            let pair = (bond.begin.min(bond.end), bond.begin.max(bond.end));
            if !pairs.insert(pair) {
                return Err(EditError::DuplicateBond { existing: *id });
            }
        }
        self.stereo.validate(&self.atoms, &self.bonds)?;
        self.stereo_groups.validate(&self.stereo)?;
        self.scene.validate()?;
        if self.references.len() > MAX_REFERENCES {
            return Err(EditError::ReferenceLimit);
        }
        if let Some(reference) = self.invalid_reference() {
            return Err(EditError::ReferenceUnknown(reference));
        }
        let instances = self.instance_count();
        if instances == 0 {
            return Err(EditError::LastInstance);
        }
        if instances > MAX_INSTANCES {
            return Err(EditError::InstanceLimit);
        }
        for key in self.positions.keys() {
            self.require_instance(key.instance)?;
            self.require_atom(key.atom)?;
        }
        // Keys are unique and each names an existing instance and atom, so the count is
        // right exactly when every atom has one position in every instance.
        if self.positions.len() != instances * self.atoms.len() {
            for instance in self.scene.instances() {
                for atom in self.atoms.keys() {
                    let key = PositionKey {
                        instance,
                        atom: *atom,
                    };
                    if !self.positions.contains_key(&key) {
                        return Err(EditError::MissingPosition(instance));
                    }
                }
            }
        }
        Ok(())
    }
}

/// Trims surrounding whitespace and validates a title for creation, rename, or copying.
///
/// # Errors
///
/// Returns [`EditError::InvalidTitle`] when the trimmed title is empty, too long, or contains
/// control characters.
pub fn validated_title(title: &str) -> Result<&str, EditError> {
    let title = title.trim();
    validate_title(title)?;
    Ok(title)
}

fn validate_title(title: &str) -> Result<(), EditError> {
    let length = title.chars().count();
    if title.trim() != title
        || length == 0
        || length > MAX_TITLE_CHARS
        || title.chars().any(char::is_control)
    {
        Err(EditError::InvalidTitle)
    } else {
        Ok(())
    }
}
