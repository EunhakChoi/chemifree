//! Scene objects on a document's page and the selection that names them.
//!
//! The page holds an ordered containment tree of scene objects: molecule scene instances (each
//! draws the document's chemical graph with one position per atom), groups (which contain other
//! objects), and simple graphics (a line, a rectangle, or an arrow). Every object has exactly one
//! parent, the page or a group, and its place among that parent's children is its drawing order.
//! All kinds share one identifier space (`s1`, `s2`, …) that is never reused.
//!
//! A selection names scene objects, atoms, and bonds. It is document state: each history entry
//! records the selection before and after its change, and undo and redo restore it with the
//! content.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::document::{AtomId, BondId, EditError, ObjectId, Point, Vector};

/// Scene objects a document holds, of every kind together.
pub const MAX_OBJECTS: usize = 1_000;
/// How deeply objects nest: a page root is at depth 1, a group's child one deeper.
pub const MAX_DEPTH: usize = 16;
/// Directed references held separately from the containment tree.
pub const MAX_REFERENCES: usize = 4_096;

/// The target of a scene object's reference. These links carry no chemical interpretation.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ReferenceTarget {
    Object(ObjectId),
    Atom(AtomId),
    Bond(BondId),
}

impl fmt::Display for ReferenceTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Object(id) => write!(formatter, "scene object {id}"),
            Self::Atom(id) => write!(formatter, "atom {id}"),
            Self::Bond(id) => write!(formatter, "bond {id}"),
        }
    }
}

/// A directed link owned by one scene object, independent of containment and drawing order.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SceneReference {
    pub source: ObjectId,
    pub target: ReferenceTarget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphicShape {
    Line,
    /// An axis-aligned rectangle between two opposite corners.
    Rectangle,
    /// A line with its head at the second point.
    Arrow,
}

/// A simple graphic. It records geometry only; it has no chemical meaning.
#[derive(Clone, Debug, PartialEq)]
pub struct Graphic {
    pub shape: GraphicShape,
    pub points: [Point; 2],
}

impl Graphic {
    fn translated(&self, delta: Vector) -> Result<Self, EditError> {
        let [first, second] = self.points;
        Ok(Self {
            shape: self.shape,
            points: [first.translated(delta)?, second.translated(delta)?],
        })
    }
}

/// One scene object's own state. Containment is kept here for groups (their ordered children)
/// and by the tree for page roots.
#[derive(Clone, Debug, PartialEq)]
pub enum SceneObject {
    /// A molecule scene instance; its atom positions are kept apart, per atom.
    Instance,
    Group {
        children: Vec<ObjectId>,
    },
    Graphic(Graphic),
}

impl SceneObject {
    pub const fn kind(&self) -> ObjectKind {
        match self {
            Self::Instance => ObjectKind::Instance,
            Self::Group { .. } => ObjectKind::Group,
            Self::Graphic(_) => ObjectKind::Graphic,
        }
    }

    /// Whether `other` is the same object in another state: no command changes an object's
    /// kind or a graphic's shape.
    pub(crate) fn same_kind(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Graphic(a), Self::Graphic(b)) => a.shape == b.shape,
            _ => self.kind() == other.kind(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectKind {
    Instance,
    Group,
    Graphic,
}

impl fmt::Display for ObjectKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Instance => "a scene instance",
            Self::Group => "a group",
            Self::Graphic => "a graphic",
        })
    }
}

/// What deleting an object does with its contents.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeleteMode {
    /// The object and everything it contains.
    Subtree,
    /// The object alone; its children take its place in its parent, in order.
    ObjectOnly,
}

/// A refused scene command or selection. Each has a stable code, sent as `reason` with
/// `invalid_command`. The list is closed and documented in docs/CONTRACT.md.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SceneError {
    /// A command names a scene object that does not exist.
    ObjectUnknown,
    /// A command names an object of a kind it does not act on.
    ObjectKindMismatch,
    /// Objects to group do not share one parent.
    GroupMixedParents,
    /// Objects would nest deeper than [`MAX_DEPTH`].
    DepthLimit,
    /// The document would hold more than [`MAX_OBJECTS`] scene objects.
    ObjectLimit,
    /// A position among a parent's children that does not exist.
    IndexOutOfRange,
    /// The last molecule scene instance cannot be deleted.
    LastInstance,
    /// `delete_objects` without its required `mode`.
    DeleteModeMissing,
    /// A selection names what does not exist (or, for `$N`, a command that creates no object).
    SelectionUnknown,
    /// A reference names a missing target or a link that is not present.
    ReferenceUnknown,
    /// A target is still referenced by a scene object that would survive the deletion.
    ReferenceInUse,
    /// The document would exceed the reference limit.
    ReferenceLimit,
}

impl SceneError {
    /// Every code, in documentation order.
    pub const ALL: [Self; 12] = [
        Self::ObjectUnknown,
        Self::ObjectKindMismatch,
        Self::GroupMixedParents,
        Self::DepthLimit,
        Self::ObjectLimit,
        Self::IndexOutOfRange,
        Self::LastInstance,
        Self::DeleteModeMissing,
        Self::SelectionUnknown,
        Self::ReferenceUnknown,
        Self::ReferenceInUse,
        Self::ReferenceLimit,
    ];

    /// The stable machine-readable code.
    pub const fn code(self) -> &'static str {
        match self {
            Self::ObjectUnknown => "object_unknown",
            Self::ObjectKindMismatch => "object_kind_mismatch",
            Self::GroupMixedParents => "group_mixed_parents",
            Self::DepthLimit => "depth_limit",
            Self::ObjectLimit => "object_limit",
            Self::IndexOutOfRange => "index_out_of_range",
            Self::LastInstance => "last_instance",
            Self::DeleteModeMissing => "delete_mode_missing",
            Self::SelectionUnknown => "selection_unknown",
            Self::ReferenceUnknown => "reference_unknown",
            Self::ReferenceInUse => "reference_in_use",
            Self::ReferenceLimit => "reference_limit",
        }
    }
}

/// What is selected: scene objects, atoms, and bonds, each a set.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Selection {
    pub objects: BTreeSet<ObjectId>,
    pub atoms: BTreeSet<AtomId>,
    pub bonds: BTreeSet<BondId>,
}

/// A scene object named in a selection: an existing one, or one created earlier in the same
/// transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectRef {
    Id(ObjectId),
    /// The object created by the command at this index (`add_instance`, `add_graphic`, or
    /// `group`).
    Created(usize),
}

/// The selection a transaction leaves, as the client names it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SelectionRequest {
    pub objects: Vec<ObjectRef>,
    pub atoms: Vec<AtomId>,
    pub bonds: Vec<BondId>,
}

/// The selections a transaction records. `before` is the client's selection when it made the
/// edit (the document's current selection when `None`); `after` is the selection the edit
/// leaves (the selection before, less what no longer exists, when `None`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TransactionSelection {
    pub before: Option<Selection>,
    pub after: Option<SelectionRequest>,
}

/// Where an object sits: `None` for the page, or the group that contains it.
pub type Parent = Option<ObjectId>;

/// The page's containment tree: its ordered root objects and every object's state.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SceneTree {
    pub(crate) roots: Vec<ObjectId>,
    pub(crate) objects: BTreeMap<ObjectId, SceneObject>,
}

impl SceneTree {
    /// A page holding one molecule scene instance.
    pub(crate) fn with_instance(instance: ObjectId) -> Self {
        Self {
            roots: vec![instance],
            objects: BTreeMap::from([(instance, SceneObject::Instance)]),
        }
    }

    pub(crate) fn get(&self, id: ObjectId) -> Result<&SceneObject, EditError> {
        self.objects.get(&id).ok_or(EditError::UnknownObject(id))
    }

    /// Fails unless `id` is an object of `kind`.
    pub(crate) fn require(&self, id: ObjectId, kind: ObjectKind) -> Result<(), EditError> {
        if self.get(id)?.kind() == kind {
            Ok(())
        } else {
            Err(EditError::ObjectKindMismatch {
                object: id,
                expected: kind,
            })
        }
    }

    /// The molecule scene instances in identifier order.
    pub(crate) fn instances(&self) -> impl Iterator<Item = ObjectId> + '_ {
        self.objects
            .iter()
            .filter(|(_, object)| matches!(object, SceneObject::Instance))
            .map(|(id, _)| *id)
    }

    fn children(&self, parent: Parent) -> &[ObjectId] {
        match parent {
            None => &self.roots,
            Some(group) => match self.objects.get(&group) {
                Some(SceneObject::Group { children }) => children,
                _ => &[],
            },
        }
    }

    fn children_mut(&mut self, parent: Parent) -> &mut Vec<ObjectId> {
        match parent {
            None => &mut self.roots,
            Some(group) => match self.objects.get_mut(&group) {
                Some(SceneObject::Group { children }) => children,
                _ => unreachable!("a parent is the page or an existing group"),
            },
        }
    }

    /// Every object's parent.
    fn parents(&self) -> BTreeMap<ObjectId, Parent> {
        let mut parents: BTreeMap<ObjectId, Parent> =
            self.roots.iter().map(|id| (*id, None)).collect();
        for (id, object) in &self.objects {
            if let SceneObject::Group { children } = object {
                parents.extend(children.iter().map(|child| (*child, Some(*id))));
            }
        }
        parents
    }

    pub(crate) fn parent(&self, id: ObjectId) -> Result<Parent, EditError> {
        self.get(id)?;
        Ok(self.parents().get(&id).copied().flatten())
    }

    /// The depth of a child of `parent`: 1 under the page.
    fn child_depth(&self, parent: Parent) -> usize {
        let parents = self.parents();
        let mut depth = 1;
        let mut current = parent;
        while let Some(group) = current {
            depth += 1;
            current = parents.get(&group).copied().flatten();
        }
        depth
    }

    /// Levels an object occupies: 1 for anything but a non-empty group.
    fn height(&self, id: ObjectId) -> usize {
        match self.objects.get(&id) {
            Some(SceneObject::Group { children }) => {
                1 + children
                    .iter()
                    .map(|child| self.height(*child))
                    .max()
                    .unwrap_or(0)
            }
            _ => 1,
        }
    }

    /// `ids` and everything they contain, each once.
    pub(crate) fn subtrees(&self, ids: &BTreeSet<ObjectId>) -> BTreeSet<ObjectId> {
        let mut found = BTreeSet::new();
        let mut pending: Vec<ObjectId> = ids.iter().copied().collect();
        while let Some(id) = pending.pop() {
            if found.insert(id)
                && let Some(SceneObject::Group { children }) = self.objects.get(&id)
            {
                pending.extend(children);
            }
        }
        found
    }

    /// Objects in drawing order: depth first, each parent before its children, siblings in
    /// order, with each object's parent.
    pub(crate) fn depth_first(&self) -> Vec<(ObjectId, Parent)> {
        let mut order = Vec::with_capacity(self.objects.len());
        let mut pending: Vec<(ObjectId, Parent)> =
            self.roots.iter().rev().map(|id| (*id, None)).collect();
        while let Some((id, parent)) = pending.pop() {
            order.push((id, parent));
            if let Some(SceneObject::Group { children }) = self.objects.get(&id) {
                pending.extend(children.iter().rev().map(|child| (*child, Some(id))));
            }
        }
        order
    }

    /// Checks that every object is contained exactly once, under the page, within the depth
    /// and object limits, and that every contained identifier exists.
    pub(crate) fn validate(&self) -> Result<(), EditError> {
        if self.objects.len() > MAX_OBJECTS {
            return Err(EditError::ObjectLimit);
        }
        let mut seen = BTreeSet::new();
        let mut pending: Vec<(ObjectId, usize)> = self.roots.iter().map(|id| (*id, 1)).collect();
        while let Some((id, depth)) = pending.pop() {
            let object = self.get(id)?;
            if !seen.insert(id) {
                return Err(EditError::SceneTreeInvalid(format!(
                    "scene object {id} is contained more than once"
                )));
            }
            if depth > MAX_DEPTH {
                return Err(EditError::DepthLimit);
            }
            if let SceneObject::Group { children } = object {
                pending.extend(children.iter().map(|child| (*child, depth + 1)));
            }
        }
        if let Some(loose) = self.objects.keys().find(|id| !seen.contains(id)) {
            return Err(EditError::SceneTreeInvalid(format!(
                "scene object {loose} is not on the page"
            )));
        }
        Ok(())
    }

    /// Inserts a new object under `parent` at `index` (the end when `None`).
    pub(crate) fn insert(
        &mut self,
        id: ObjectId,
        object: SceneObject,
        parent: Parent,
        index: Option<usize>,
    ) -> Result<(), EditError> {
        if let Some(group) = parent {
            self.require(group, ObjectKind::Group)?;
        }
        let siblings = self.children(parent).len();
        let index = index.unwrap_or(siblings);
        if index > siblings {
            return Err(EditError::IndexOutOfRange {
                index,
                len: siblings,
            });
        }
        if self.child_depth(parent) > MAX_DEPTH {
            return Err(EditError::DepthLimit);
        }
        self.objects.insert(id, object);
        self.children_mut(parent).insert(index, id);
        Ok(())
    }

    /// Puts `members` (at least two, sharing one parent) into the new group `id`, which takes
    /// the place of the earliest of them; they keep their order.
    pub(crate) fn group(&mut self, id: ObjectId, members: &[ObjectId]) -> Result<(), EditError> {
        let members: BTreeSet<ObjectId> = members.iter().copied().collect();
        for member in &members {
            self.get(*member)?;
        }
        if members.len() < 2 {
            return Err(EditError::GroupTooSmall);
        }
        let parents = self.parents();
        let mut shared = members
            .iter()
            .map(|member| parents.get(member).copied().flatten());
        let parent = shared.next().flatten();
        if shared.any(|other| other != parent) {
            return Err(EditError::GroupMixedParents);
        }
        let height = members
            .iter()
            .map(|member| self.height(*member))
            .max()
            .unwrap_or(1);
        // Members move one level down, below the new group.
        if self.child_depth(parent) + height > MAX_DEPTH {
            return Err(EditError::DepthLimit);
        }
        let siblings = self.children_mut(parent);
        let place = siblings
            .iter()
            .position(|sibling| members.contains(sibling))
            .expect("members are children of their shared parent");
        let children: Vec<ObjectId> = siblings
            .iter()
            .copied()
            .filter(|sibling| members.contains(sibling))
            .collect();
        siblings.retain(|sibling| !members.contains(sibling));
        siblings.insert(place, id);
        self.objects.insert(id, SceneObject::Group { children });
        Ok(())
    }

    /// Removes the group `id`; its children take its place.
    pub(crate) fn ungroup(&mut self, id: ObjectId) -> Result<(), EditError> {
        self.require(id, ObjectKind::Group)?;
        let parent = self.parent(id)?;
        let Some(SceneObject::Group { children }) = self.objects.remove(&id) else {
            unreachable!("checked to be a group");
        };
        let siblings = self.children_mut(parent);
        let place = siblings
            .iter()
            .position(|sibling| *sibling == id)
            .expect("an object is among its parent's children");
        siblings.splice(place..=place, children);
        Ok(())
    }

    /// Moves `id` to `index` among its parent's children.
    pub(crate) fn reorder(&mut self, id: ObjectId, index: usize) -> Result<(), EditError> {
        let parent = self.parent(id)?;
        let siblings = self.children_mut(parent);
        if index >= siblings.len() {
            return Err(EditError::IndexOutOfRange {
                index,
                len: siblings.len(),
            });
        }
        siblings.retain(|sibling| *sibling != id);
        siblings.insert(index, id);
        Ok(())
    }

    /// Deletes `ids` as `mode` says and returns every object removed. Each must exist.
    pub(crate) fn delete(
        &mut self,
        ids: &BTreeSet<ObjectId>,
        mode: DeleteMode,
    ) -> Result<BTreeSet<ObjectId>, EditError> {
        for id in ids {
            self.get(*id)?;
        }
        let removed = match mode {
            DeleteMode::Subtree => self.subtrees(ids),
            DeleteMode::ObjectOnly => ids.clone(),
        };
        let mut rebuilt = BTreeMap::new();
        let roots = self.kept(&self.roots, &removed, &mut rebuilt);
        self.roots = roots;
        for (group, children) in rebuilt {
            if let Some(SceneObject::Group { children: kept }) = self.objects.get_mut(&group) {
                *kept = children;
            }
        }
        self.objects.retain(|id, _| !removed.contains(id));
        if self.instances().next().is_none() {
            return Err(EditError::LastInstance);
        }
        Ok(removed)
    }

    /// `list` without the removed objects: a removed object's surviving children take its place.
    /// Collects every surviving group's new children into `rebuilt`.
    fn kept(
        &self,
        list: &[ObjectId],
        removed: &BTreeSet<ObjectId>,
        rebuilt: &mut BTreeMap<ObjectId, Vec<ObjectId>>,
    ) -> Vec<ObjectId> {
        let mut kept = Vec::with_capacity(list.len());
        for id in list {
            let children = match self.objects.get(id) {
                Some(SceneObject::Group { children }) => self.kept(children, removed, rebuilt),
                _ => Vec::new(),
            };
            if removed.contains(id) {
                kept.extend(children);
            } else {
                if matches!(self.objects.get(id), Some(SceneObject::Group { .. })) {
                    rebuilt.insert(*id, children);
                }
                kept.push(*id);
            }
        }
        kept
    }

    /// Translates every graphic in `ids` and what they contain once; returns the instances
    /// among them, whose positions the caller moves.
    pub(crate) fn translate(
        &mut self,
        ids: &BTreeSet<ObjectId>,
        delta: Vector,
    ) -> Result<Vec<ObjectId>, EditError> {
        for id in ids {
            self.get(*id)?;
        }
        let mut instances = Vec::new();
        for id in self.subtrees(ids) {
            match self.objects.get_mut(&id) {
                Some(SceneObject::Instance) => instances.push(id),
                Some(SceneObject::Graphic(graphic)) => *graphic = graphic.translated(delta)?,
                Some(SceneObject::Group { .. }) | None => {}
            }
        }
        Ok(instances)
    }
}
