//! Read access to a document's content, scene, selection and counters.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    Atom, AtomId, Bond, BondId, Document, DocumentId, EditError, FragmentDisplay, MAX_REVISION,
    ObjectId, Point, PositionKey, RevisionsExhausted,
};
use crate::scene::{
    ArrangementPlan, ObjectArrangement, SceneObject, SceneReference, Selection, SelectionScope,
    SelectionTransform, TransformPlan,
};
use crate::settings::Settings;
use crate::stereo::StereoTable;
use crate::stereo_group::StereoGroupTable;

/// How far a document's revision and identifier counters have advanced.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Numbering {
    pub revision: u64,
    pub next_atom: u32,
    pub next_bond: u32,
    pub next_object: u32,
    pub next_group: u32,
}

impl Numbering {
    /// The later of two numberings, field by field.
    #[must_use]
    pub fn max(self, other: Self) -> Self {
        Self {
            revision: self.revision.max(other.revision),
            next_atom: self.next_atom.max(other.next_atom),
            next_bond: self.next_bond.max(other.next_bond),
            next_object: self.next_object.max(other.next_object),
            next_group: self.next_group.max(other.next_group),
        }
    }
}

impl Document {
    /// Fragment display text never replaces or defines retained atoms, bonds, or coordinates.
    pub fn fragment_displays(&self) -> impl Iterator<Item = (PositionKey, &FragmentDisplay)> + '_ {
        self.content
            .fragment_displays
            .iter()
            .map(|(key, display)| (*key, display))
    }

    /// The display map never replaces atoms, bonds, or coordinates.
    pub fn abbreviations(&self) -> impl Iterator<Item = (PositionKey, AtomId)> + '_ {
        self.content
            .abbreviations
            .iter()
            .map(|(key, terminal)| (*key, *terminal))
    }

    /// Cosmetic text never defines the retained ethyl chemistry.
    pub fn abbreviation_label(&self, key: PositionKey) -> &str {
        self.content
            .display_labels
            .get(&key)
            .map_or("Et", String::as_str)
    }

    pub fn id(&self) -> &DocumentId {
        &self.id
    }

    /// Increases by exactly one for each effective transaction, undo, or redo.
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    pub fn title(&self) -> &str {
        &self.content.title
    }

    /// The document's own settings.
    pub fn settings(&self) -> &Settings {
        &self.content.settings
    }

    pub fn atoms(&self) -> impl ExactSizeIterator<Item = (AtomId, &Atom)> {
        self.content.atoms.iter().map(|(id, atom)| (*id, atom))
    }

    pub fn bonds(&self) -> impl ExactSizeIterator<Item = (BondId, &Bond)> {
        self.content.bonds.iter().map(|(id, bond)| (*id, bond))
    }

    /// The graph's atoms and bonds as the maps the stereo checks read.
    pub(crate) const fn graph_maps(&self) -> (&BTreeMap<AtomId, Atom>, &BTreeMap<BondId, Bond>) {
        (&self.content.atoms, &self.content.bonds)
    }

    /// The bond with this identifier, if the document has it.
    pub fn bond(&self, id: BondId) -> Option<&Bond> {
        self.content.bonds.get(&id)
    }

    /// The molecule's scene instances in identifier (creation) order, wherever they are in the
    /// containment tree; there is always at least one.
    pub fn instances(&self) -> impl Iterator<Item = ObjectId> + '_ {
        self.content.scene.instances()
    }

    /// The first scene instance by identifier, whose positions the v1 view reports as
    /// `atoms[].position`.
    pub fn first_instance(&self) -> ObjectId {
        self.content.first_instance()
    }

    /// The page's root objects in drawing order.
    pub fn roots(&self) -> &[ObjectId] {
        &self.content.scene.roots
    }

    pub fn object(&self, id: ObjectId) -> Option<&SceneObject> {
        self.content.scene.objects.get(&id)
    }

    /// The stated stereo configurations (centers and double bonds). Marks are on [`Bond`]s.
    pub const fn stereo(&self) -> &StereoTable {
        &self.content.stereo
    }

    /// The enhanced-stereo groups (see [`crate::stereo_group`]).
    pub const fn stereo_groups(&self) -> &StereoGroupTable {
        &self.content.stereo_groups
    }

    /// Directed references in deterministic order, separate from scene containment.
    pub fn references(&self) -> impl ExactSizeIterator<Item = SceneReference> + '_ {
        self.content.references.iter().copied()
    }

    /// Every scene object in drawing order (depth first, a group before its children) with
    /// its parent (`None` for the page).
    pub fn scene_objects(&self) -> Vec<(ObjectId, Option<ObjectId>, &SceneObject)> {
        self.content
            .scene
            .depth_first()
            .into_iter()
            .map(|(id, parent)| (id, parent, &self.content.scene.objects[&id]))
            .collect()
    }

    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    /// Resolves the exact unique geometry targets of a selection without changing it.
    /// Selected scene subtrees may span placements; explicit atoms and selected-bond
    /// endpoints use only `instance` (or the sole instance when omitted). Graph targets
    /// do not implicitly select their containing scene groups or other placements.
    ///
    /// # Errors
    /// Refuses unknown selected objects, atoms or bonds, a missing/incorrect explicit
    /// instance, or an omitted instance for graph targets when several placements exist.
    pub fn selection_scope(
        &self,
        selection: &Selection,
        instance: Option<ObjectId>,
    ) -> Result<SelectionScope, EditError> {
        self.content.selection_scope(selection, instance)
    }

    /// Plans rigid geometry-only alignment or equal-gap distribution without changing state.
    /// Selected descendants of another selected object collapse into that ancestor's unit.
    /// Alignment uses the original union bounds; distribution keeps first/last units fixed,
    /// ordered by leading bound then object ID. No labels, fonts or stroke extents are used.
    ///
    /// # Errors
    /// Refuses unknown objects, geometry-empty units, too few outermost units, negative
    /// distribution gaps or any translated point outside the document coordinate bounds.
    pub fn object_arrangement(
        &self,
        objects: &BTreeSet<ObjectId>,
        arrangement: ObjectArrangement,
    ) -> Result<ArrangementPlan, EditError> {
        self.content.object_arrangement(objects, arrangement)
    }

    /// Checks a geometry-only transform of the authoritative selected union without editing.
    /// Positive angles rotate clockwise in page coordinates; horizontal reflection mirrors
    /// about y=pivot.y, vertical reflection about x=pivot.x. Quarter turns are exact affine
    /// mappings. Arbitrary rotations are supported for atom positions, lines, arrows, polygons and cubic splines;
    /// any selected rectangle or ellipse, including descendants and degenerate geometry, requires
    /// an exact quarter turn. Diagonal/angled reflection is outside this bounded profile.
    /// Stored stereo remains unchanged, and may disagree with the transformed picture.
    ///
    /// # Errors
    /// Refuses invalid targets/placement, nonfinite angles, unsupported rectangle rotations
    /// or any final geometry point outside the finite document coordinate limits.
    pub fn selection_transform(
        &self,
        selection: &Selection,
        instance: Option<ObjectId>,
        pivot: Point,
        transform: SelectionTransform,
    ) -> Result<TransformPlan, EditError> {
        self.content
            .selection_transform(selection, instance, pivot, transform)
    }

    /// Every atom's position in `instance`, by atom; empty for an unknown instance.
    pub fn positions(&self, instance: ObjectId) -> impl Iterator<Item = (AtomId, Point)> + '_ {
        self.content.instance_positions(instance)
    }

    pub fn position(&self, instance: ObjectId, atom: AtomId) -> Option<Point> {
        self.content
            .positions
            .get(&PositionKey { instance, atom })
            .copied()
    }

    /// The revision and identifier counters this document has reached.
    pub const fn numbering(&self) -> Numbering {
        Numbering {
            revision: self.revision,
            next_atom: self.next_atom,
            next_bond: self.next_bond,
            next_object: self.next_object,
            next_group: self.next_group,
        }
    }

    /// Moves the revision above `reached.revision` and the identifier counters up to
    /// `reached`'s, leaving content and history unchanged. Used when this copy replaces one
    /// that handed out revisions or identifiers for states that were never stored, so those
    /// numbers are not handed out again for different states. Never moves anything back, and
    /// changes nothing when no revision above `reached.revision` remains.
    pub fn continue_after(&mut self, reached: Numbering) -> Result<(), RevisionsExhausted> {
        if self.revision <= reached.revision {
            if reached.revision >= MAX_REVISION {
                return Err(RevisionsExhausted);
            }
            self.revision = reached.revision + 1;
        }
        self.next_atom = self.next_atom.max(reached.next_atom);
        self.next_bond = self.next_bond.max(reached.next_bond);
        self.next_object = self.next_object.max(reached.next_object);
        self.next_group = self.next_group.max(reached.next_group);
        Ok(())
    }

    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    pub fn redo_depth(&self) -> usize {
        self.redo.len()
    }
}
