//! Scene objects on a document's page and the selection a history entry records.
//!
//! The page holds an ordered list of root objects. An object is a molecule scene instance
//! (which draws the document's graph; its atom positions live in the document), a group (an
//! ordered list of child objects), or a graphic (a line, rectangle, or arrow given by two
//! points). Every object has exactly one parent, the page or a group: group children are
//! containment, not references, and the only reference an object holds is an instance's
//! reference to the molecule. The tree has no cycles, is at most [`MAX_DEPTH`] objects deep, and
//! holds at most [`MAX_OBJECTS`] objects. Scene objects share one identifier namespace (`s<n>`)
//! and one counter that never goes back.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

use crate::document::{AtomId, BondId, EditError, ObjectId, Point, Vector};

/// Scene objects per document, of every kind together.
pub const MAX_OBJECTS: usize = 1_000;
/// The deepest containment: a root object is at depth 1, a child of a root group at depth 2.
pub const MAX_DEPTH: usize = 16;

/// What a graphic draws between its two points.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Shape {
    Line,
    /// The axis-aligned rectangle with the two points as opposite corners.
    Rectangle,
    /// A line from the first point with an arrowhead at the second.
    Arrow,
}

impl Shape {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Line => "line",
            Self::Rectangle => "rectangle",
            Self::Arrow => "arrow",
        }
    }
}

/// A graphic: a shape given by two points in pt.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Graphic {
    pub shape: Shape,
    pub points: [Point; 2],
}

impl Graphic {
    fn translated(self, delta: Vector) -> Result<Self, EditError> {
        let [first, second] = self.points;
        Ok(Self {
            shape: self.shape,
            points: [first.translated(delta)?, second.translated(delta)?],
        })
    }
}

/// One object of the page's containment tree.
#[derive(Clone, Debug, PartialEq)]
pub enum SceneObject {
    /// A molecule scene instance: one drawing of the document's molecule. Its atom positions
    /// are kept by the document under [`crate::document::PositionKey`]s.
    Molecule,
    /// A group and its ordered children.
    Group(Vec<ObjectId>),
    Graphic(Graphic),
}

impl SceneObject {
    pub const fn kind(&self) -> ObjectKind {
        match self {
            Self::Molecule => ObjectKind::Molecule,
            Self::Group(_) => ObjectKind::Group,
            Self::Graphic(_) => ObjectKind::Graphic,
        }
    }

    fn children(&self) -> &[ObjectId] {
        match self {
            Self::Group(children) => children,
            Self::Molecule | Self::Graphic(_) => &[],
        }
    }

    /// The identifiers this object's state names, counted against the history change limit.
    pub(crate) fn size(&self) -> usize {
        1 + self.children().len()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectKind {
    Molecule,
    Group,
    Graphic,
}

impl ObjectKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Molecule => "molecule",
            Self::Group => "group",
            Self::Graphic => "graphic",
        }
    }
}

/// Where an object is contained.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Parent {
    Page,
    Group(ObjectId),
}

/// What `delete_objects` removes besides the objects it names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeleteMode {
    /// Each named object and everything it contains.
    Subtree,
    /// Each named object alone; a deleted group's children take its place in its parent.
    ObjectOnly,
}

/// What a client had selected: scene objects, atoms, and bonds. Each history entry records the
/// selection before and after it, and undo and redo restore it with the rest of the entry.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Selection {
    pub objects: BTreeSet<ObjectId>,
    pub atoms: BTreeSet<AtomId>,
    pub bonds: BTreeSet<BondId>,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty() && self.atoms.is_empty() && self.bonds.is_empty()
    }

    /// The identifiers it names, counted against the history change limit.
    pub fn len(&self) -> usize {
        self.objects.len() + self.atoms.len() + self.bonds.len()
    }
}

/// A refused scene command, with its stable code. The codes are documented in docs/CONTRACT.md.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SceneError {
    /// The command names a scene object that does not exist.
    UnknownObject(ObjectId),
    /// The object exists but is not the kind the command needs.
    KindMismatch {
        object: ObjectId,
        expected: ObjectKind,
    },
    /// `group` named objects with different parents.
    MixedParents,
    /// The command would contain an object deeper than [`MAX_DEPTH`].
    DepthLimit,
    /// The command would make more than [`MAX_OBJECTS`] scene objects.
    ObjectLimit,
    /// `reorder` named an index outside the parent's children.
    IndexOutOfRange { index: usize, len: usize },
    /// The selection names an object, atom, or bond the edited document does not have.
    UnknownSelection(String),
}

impl SceneError {
    /// Every code, in documentation order.
    pub const CODES: [&'static str; 7] = [
        "object_unknown",
        "object_kind_mismatch",
        "group_mixed_parents",
        "scene_depth_limit",
        "scene_object_limit",
        "index_out_of_range",
        "selection_unknown",
    ];

    /// The stable machine-readable code.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::UnknownObject(_) => "object_unknown",
            Self::KindMismatch { .. } => "object_kind_mismatch",
            Self::MixedParents => "group_mixed_parents",
            Self::DepthLimit => "scene_depth_limit",
            Self::ObjectLimit => "scene_object_limit",
            Self::IndexOutOfRange { .. } => "index_out_of_range",
            Self::UnknownSelection(_) => "selection_unknown",
        }
    }
}

impl fmt::Display for SceneError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownObject(id) => write!(formatter, "scene object {id} does not exist"),
            Self::KindMismatch { object, expected } => {
                write!(
                    formatter,
                    "scene object {object} is not a {}",
                    expected.as_str()
                )
            }
            Self::MixedParents => {
                write!(formatter, "objects grouped together must share a parent")
            }
            Self::DepthLimit => write!(
                formatter,
                "scene objects are contained at most {MAX_DEPTH} deep"
            ),
            Self::ObjectLimit => write!(
                formatter,
                "a document holds at most {MAX_OBJECTS} scene objects"
            ),
            Self::IndexOutOfRange { index, len } => write!(
                formatter,
                "index {index} is outside the {len} children of the object's parent"
            ),
            Self::UnknownSelection(id) => {
                write!(formatter, "the selection names {id}, which does not exist")
            }
        }
    }
}

impl From<SceneError> for EditError {
    fn from(error: SceneError) -> Self {
        Self::Scene(error)
    }
}

/// The page's containment tree: the objects by identifier and the ordered root list.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Tree {
    pub objects: BTreeMap<ObjectId, SceneObject>,
    pub roots: Vec<ObjectId>,
}

impl Tree {
    pub fn get(&self, id: ObjectId) -> Result<&SceneObject, SceneError> {
        self.objects.get(&id).ok_or(SceneError::UnknownObject(id))
    }

    /// Each contained object's parent and its index there.
    pub fn parents(&self) -> HashMap<ObjectId, (Parent, usize)> {
        let mut parents = HashMap::with_capacity(self.objects.len());
        for (index, id) in self.roots.iter().enumerate() {
            parents.insert(*id, (Parent::Page, index));
        }
        for (id, object) in &self.objects {
            for (index, child) in object.children().iter().enumerate() {
                parents.insert(*child, (Parent::Group(*id), index));
            }
        }
        parents
    }

    pub fn children(&self, parent: Parent) -> &[ObjectId] {
        match parent {
            Parent::Page => &self.roots,
            Parent::Group(id) => self.objects.get(&id).map_or(&[], SceneObject::children),
        }
    }

    /// The children of `parent`, which must be the page or a group.
    pub fn children_mut(&mut self, parent: Parent) -> &mut Vec<ObjectId> {
        match parent {
            Parent::Page => &mut self.roots,
            Parent::Group(id) => match self.objects.get_mut(&id) {
                Some(SceneObject::Group(children)) => children,
                _ => unreachable!("a parent is the page or an existing group"),
            },
        }
    }

    /// How deep `id` is contained: 1 for a root object.
    pub fn depth(&self, parents: &HashMap<ObjectId, (Parent, usize)>, id: ObjectId) -> usize {
        let mut depth = 1;
        let mut current = id;
        while let Some((Parent::Group(group), _)) = parents.get(&current) {
            depth += 1;
            current = *group;
        }
        depth
    }

    /// How many levels lie below `id`: 0 for anything but a non-empty group.
    pub fn height(&self, id: ObjectId) -> usize {
        let mut height = 0;
        let mut stack = vec![(id, 0)];
        while let Some((current, level)) = stack.pop() {
            height = height.max(level);
            if let Some(object) = self.objects.get(&current) {
                stack.extend(object.children().iter().map(|child| (*child, level + 1)));
            }
        }
        height
    }

    /// `id` and everything it contains.
    pub fn subtree(&self, id: ObjectId, into: &mut BTreeSet<ObjectId>) {
        let mut stack = vec![id];
        while let Some(current) = stack.pop() {
            if into.insert(current)
                && let Some(object) = self.objects.get(&current)
            {
                stack.extend(object.children());
            }
        }
    }

    /// Checks containment: every object is contained exactly once, by the page or an existing
    /// group, every listed child exists (so nothing forms a cycle apart from the page), and
    /// the limits hold.
    pub fn validate(&self) -> Result<(), EditError> {
        if self.objects.len() > MAX_OBJECTS {
            return Err(SceneError::ObjectLimit.into());
        }
        let mut seen = BTreeSet::new();
        let mut stack: Vec<(ObjectId, usize)> =
            self.roots.iter().rev().map(|id| (*id, 1)).collect();
        while let Some((id, depth)) = stack.pop() {
            let object = self
                .objects
                .get(&id)
                .ok_or(EditError::BrokenContainment(id))?;
            if !seen.insert(id) {
                return Err(EditError::BrokenContainment(id));
            }
            if depth > MAX_DEPTH {
                return Err(SceneError::DepthLimit.into());
            }
            stack.extend(
                object
                    .children()
                    .iter()
                    .rev()
                    .map(|child| (*child, depth + 1)),
            );
        }
        // Whatever the page does not reach is detached or in a cycle of groups.
        match self.objects.keys().find(|id| !seen.contains(id)) {
            Some(id) => Err(EditError::BrokenContainment(*id)),
            None => Ok(()),
        }
    }

    /// The objects in depth-first order from the page's roots, with their parents.
    pub fn in_order(&self) -> Vec<(ObjectId, Parent)> {
        let mut ordered = Vec::with_capacity(self.objects.len());
        let mut stack: Vec<(ObjectId, Parent)> = self
            .roots
            .iter()
            .rev()
            .map(|id| (*id, Parent::Page))
            .collect();
        while let Some((id, parent)) = stack.pop() {
            ordered.push((id, parent));
            if let Some(object) = self.objects.get(&id) {
                stack.extend(
                    object
                        .children()
                        .iter()
                        .rev()
                        .map(|child| (*child, Parent::Group(id))),
                );
            }
        }
        ordered
    }

    /// `list` with every object in `deleted` replaced by its own children, recursively, so a
    /// deleted group's surviving descendants take its place.
    pub fn without(&self, list: &[ObjectId], deleted: &BTreeSet<ObjectId>) -> Vec<ObjectId> {
        let mut kept = Vec::with_capacity(list.len());
        let mut stack: Vec<ObjectId> = list.iter().rev().copied().collect();
        while let Some(id) = stack.pop() {
            if deleted.contains(&id) {
                if let Some(object) = self.objects.get(&id) {
                    stack.extend(object.children().iter().rev());
                }
            } else {
                kept.push(id);
            }
        }
        kept
    }

    /// Translates a graphic's points; other objects are left to the caller.
    pub fn translate_graphic(&mut self, id: ObjectId, delta: Vector) -> Result<(), EditError> {
        if let Some(SceneObject::Graphic(graphic)) = self.objects.get_mut(&id) {
            *graphic = graphic.translated(delta)?;
        }
        Ok(())
    }
}
