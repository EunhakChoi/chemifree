//! Native graphic/group forests. References are data, never a reason to expand containment.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::document::{Command, Document, EditError, MAX_COMMANDS, ObjectId, Vector};
use crate::scene::{
    MAX_DEPTH, MAX_OBJECTS, MAX_REFERENCES, ObjectRef, ReferenceTarget, SceneObject,
    SceneReference, Selection, SelectionRequest, TransactionSelection,
};
use crate::settings::Settings;

/// A graphic or group, named within the copied forest. Parentage lives only in roots/children.
#[derive(Clone, Debug, PartialEq)]
pub struct FragmentObject {
    pub id: ObjectId,
    pub object: SceneObject,
}

/// Data-only graphic forest. Public fields are untrusted until validated.
#[derive(Clone, Debug, PartialEq)]
pub struct Fragment {
    pub settings: Settings,
    pub roots: Vec<ObjectId>,
    pub objects: Vec<FragmentObject>,
    pub references: Vec<SceneReference>,
}

/// One clip-local object and the freshly allocated destination identifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectMapping {
    pub source: ObjectId,
    pub target: ObjectId,
}

/// An atomic plan for one exact destination revision. Mapping follows source drawing order,
/// while allocation first creates graphic leaves and then their groups bottom-up.
#[derive(Clone, Debug, PartialEq)]
pub struct PastePlan {
    pub commands: Vec<Command>,
    pub objects: Vec<ObjectMapping>,
    pub selection: SelectionRequest,
}

/// A refusal leaves source and destination untouched.
#[derive(Clone, Debug, PartialEq)]
pub enum SceneFragmentError {
    GraphSelection,
    EmptySelection,
    UnknownSelection(ObjectId),
    InstanceUnsupported(ObjectId),
    CaptionUnsupported(ObjectId),
    /// Historical v2-only writer refusal, retained for diagnostic compatibility.
    GraphicLineWidthUnsupported(ObjectId),
    GroupTooSmall(ObjectId),
    InvalidTree,
    DepthLimit,
    ObjectLimit,
    ReferenceUnsupported(SceneReference),
    DuplicateReference,
    StyleMismatch,
    CommandLimit,
    IdentifierLimit,
    Invalid(EditError),
}

impl SceneFragmentError {
    /// Stable application-boundary reason, independent of a display message.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::GraphSelection => "clipboard_scene_graph_selection_unsupported",
            Self::EmptySelection => "clipboard_scene_empty_selection",
            Self::UnknownSelection(_) => "clipboard_scene_selection_unknown",
            Self::InstanceUnsupported(_) => "clipboard_scene_instance_unsupported",
            Self::CaptionUnsupported(_) => "clipboard_scene_caption_unsupported",
            Self::GraphicLineWidthUnsupported(_) => {
                "clipboard_scene_graphic_line_width_unsupported"
            }
            Self::GroupTooSmall(_) => "clipboard_scene_group_too_small",
            Self::InvalidTree => "clipboard_scene_invalid_tree",
            Self::DepthLimit => "clipboard_scene_depth_limit",
            Self::ObjectLimit => "clipboard_scene_object_limit",
            Self::ReferenceUnsupported(_) => "clipboard_scene_reference_unsupported",
            Self::DuplicateReference => "clipboard_scene_duplicate_reference",
            Self::StyleMismatch => "clipboard_scene_style_mismatch",
            Self::CommandLimit => "clipboard_scene_command_limit",
            Self::IdentifierLimit => "clipboard_scene_identifier_limit",
            Self::Invalid(_) => "clipboard_scene_fragment_invalid",
        }
    }
}

impl fmt::Display for SceneFragmentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownSelection(id)
            | Self::CaptionUnsupported(id)
            | Self::GraphicLineWidthUnsupported(id)
            | Self::InstanceUnsupported(id)
            | Self::GroupTooSmall(id) => {
                write!(formatter, "{}: {id}", self.as_str())
            }
            Self::ReferenceUnsupported(reference) => {
                write!(formatter, "{}: {reference:?}", self.as_str())
            }
            Self::Invalid(error) => write!(formatter, "{}: {error}", self.as_str()),
            other => formatter.write_str(other.as_str()),
        }
    }
}

impl std::error::Error for SceneFragmentError {}

struct Traversal {
    drawing: Vec<ObjectId>,
    groups: Vec<ObjectId>,
}

fn walk(
    id: ObjectId,
    depth: usize,
    objects: &BTreeMap<ObjectId, &SceneObject>,
    seen: &mut BTreeSet<ObjectId>,
    traversal: &mut Traversal,
) -> Result<(), SceneFragmentError> {
    if depth > MAX_DEPTH {
        return Err(SceneFragmentError::DepthLimit);
    }
    let object = objects.get(&id).ok_or(SceneFragmentError::InvalidTree)?;
    if !seen.insert(id) {
        return Err(SceneFragmentError::InvalidTree);
    }
    traversal.drawing.push(id);
    match object {
        SceneObject::Instance => return Err(SceneFragmentError::InstanceUnsupported(id)),
        SceneObject::Caption(_) => return Err(SceneFragmentError::CaptionUnsupported(id)),
        SceneObject::Graphic(graphic) => graphic.validate().map_err(SceneFragmentError::Invalid)?,
        SceneObject::Group { children } => {
            if children.len() < 2 {
                return Err(SceneFragmentError::GroupTooSmall(id));
            }
            for child in children {
                walk(*child, depth + 1, objects, seen, traversal)?;
            }
            traversal.groups.push(id);
        }
    }
    Ok(())
}

impl Fragment {
    fn traversal(&self) -> Result<Traversal, SceneFragmentError> {
        if self.roots.is_empty() {
            return Err(SceneFragmentError::EmptySelection);
        }
        if self.objects.len() > MAX_OBJECTS {
            return Err(SceneFragmentError::ObjectLimit);
        }
        let mut objects = BTreeMap::new();
        for object in &self.objects {
            if objects.insert(object.id, &object.object).is_some() {
                return Err(SceneFragmentError::InvalidTree);
            }
        }
        let mut seen = BTreeSet::new();
        let mut traversal = Traversal {
            drawing: Vec::new(),
            groups: Vec::new(),
        };
        for root in &self.roots {
            walk(*root, 1, &objects, &mut seen, &mut traversal)?;
        }
        if seen.len() != objects.len() {
            return Err(SceneFragmentError::InvalidTree);
        }
        if self.references.len() > MAX_REFERENCES
            || self.objects.len() + self.references.len() > MAX_COMMANDS
        {
            return Err(SceneFragmentError::CommandLimit);
        }
        let mut references = BTreeSet::new();
        for reference in &self.references {
            if !references.insert(*reference) {
                return Err(SceneFragmentError::DuplicateReference);
            }
            if !seen.contains(&reference.source)
                || !matches!(reference.target, ReferenceTarget::Object(id) if seen.contains(&id))
            {
                return Err(SceneFragmentError::ReferenceUnsupported(*reference));
            }
        }
        self.settings
            .validate()
            .map_err(|error| SceneFragmentError::Invalid(error.into()))?;
        Ok(traversal)
    }

    /// Checks a closed forest, its arity/depth/size and all reference endpoints.
    ///
    /// # Errors
    /// Refuses instances, duplicate/loose/shared/cyclic tree objects, undersized groups,
    /// graph or external references, duplicate references and resource/settings violations.
    pub fn validate(&self) -> Result<(), SceneFragmentError> {
        self.traversal().map(|_| ())
    }

    /// The canonical source drawing order, independent of the objects vector's encoding order.
    ///
    /// # Errors
    /// Returns the same refusals as [`Self::validate`].
    pub fn ordered_ids(&self) -> Result<Vec<ObjectId>, SceneFragmentError> {
        self.traversal().map(|traversal| traversal.drawing)
    }

    /// Preflights fresh-ID creation, hierarchy, geometry, references and root-only selection.
    /// No destination graph or existing object is modified while planning.
    ///
    /// # Errors
    /// Refuses invalid fragments, mismatched styles, nonfinite/overflowing coordinates,
    /// exhausted identifiers and any refusal from the ordinary atomic transaction boundary.
    pub fn paste_commands(
        &self,
        destination: &Document,
        offset: Vector,
    ) -> Result<PastePlan, SceneFragmentError> {
        let traversal = self.traversal()?;
        if self.settings != *destination.settings() {
            return Err(SceneFragmentError::StyleMismatch);
        }
        if !offset.dx.is_finite() || !offset.dy.is_finite() {
            return Err(SceneFragmentError::Invalid(EditError::CoordinateOutOfRange));
        }
        let objects: BTreeMap<ObjectId, &SceneObject> = self
            .objects
            .iter()
            .map(|entry| (entry.id, &entry.object))
            .collect();
        let mut targets = BTreeMap::new();
        let mut commands = Vec::with_capacity(self.objects.len() + self.references.len());
        let mut next = destination.numbering().next_object;
        for id in &traversal.drawing {
            if let Some(SceneObject::Graphic(graphic)) = objects.get(id) {
                let target = allocate(&mut next)?;
                targets.insert(*id, target);
                commands.push(Command::AddGraphic {
                    graphic: graphic
                        .translated(offset)
                        .map_err(SceneFragmentError::Invalid)?,
                    parent: None,
                    index: None,
                });
            }
        }
        for id in &traversal.groups {
            if let Some(SceneObject::Group { children }) = objects.get(id) {
                let target = allocate(&mut next)?;
                let members = children
                    .iter()
                    .map(|child| target_of(&targets, *child))
                    .collect::<Result<Vec<_>, _>>()?;
                targets.insert(*id, target);
                commands.push(Command::Group { objects: members });
            }
        }
        for reference in &self.references {
            let ReferenceTarget::Object(id) = reference.target else {
                return Err(SceneFragmentError::ReferenceUnsupported(*reference));
            };
            commands.push(Command::AddReference {
                reference: SceneReference {
                    source: target_of(&targets, reference.source)?,
                    target: ReferenceTarget::Object(target_of(&targets, id)?),
                },
            });
        }
        let selection = SelectionRequest {
            objects: self
                .roots
                .iter()
                .map(|id| target_of(&targets, *id).map(ObjectRef::Id))
                .collect::<Result<Vec<_>, _>>()?,
            atoms: Vec::new(),
            bonds: Vec::new(),
        };
        let mappings = traversal
            .drawing
            .iter()
            .map(|id| {
                Ok(ObjectMapping {
                    source: *id,
                    target: target_of(&targets, *id)?,
                })
            })
            .collect::<Result<Vec<_>, SceneFragmentError>>()?;
        // The existing transaction validates destination capacity, coordinate bounds, final
        // references and selection, and preserves full rollback on every command refusal.
        let mut candidate = destination.clone();
        candidate
            .apply_selecting(
                &commands,
                &TransactionSelection {
                    before: None,
                    after: Some(selection.clone()),
                },
            )
            .map_err(|error| SceneFragmentError::Invalid(error.error))?;
        Ok(PastePlan {
            commands,
            objects: mappings,
            selection,
        })
    }
}

fn allocate(next: &mut u32) -> Result<ObjectId, SceneFragmentError> {
    let id = ObjectId::from_serial(*next).ok_or(SceneFragmentError::IdentifierLimit)?;
    *next = next
        .checked_add(1)
        .ok_or(SceneFragmentError::IdentifierLimit)?;
    Ok(id)
}

fn target_of(
    targets: &BTreeMap<ObjectId, ObjectId>,
    source: ObjectId,
) -> Result<ObjectId, SceneFragmentError> {
    targets
        .get(&source)
        .copied()
        .ok_or(SceneFragmentError::InvalidTree)
}

/// Copies outermost selected graphic/group subtrees in source painting order.
/// Outgoing references must stay wholly inside the copied forest; incoming owners are excluded.
///
/// # Errors
/// Refuses graph/mixed/empty/unknown selections, any contained instance, undersized groups and
/// outgoing graph/external references, plus the validation/size limits of [`Fragment::validate`].
pub fn copy_fragment(
    document: &Document,
    selection: &Selection,
) -> Result<Fragment, SceneFragmentError> {
    if !selection.atoms.is_empty() || !selection.bonds.is_empty() {
        return Err(SceneFragmentError::GraphSelection);
    }
    if selection.objects.is_empty() {
        return Err(SceneFragmentError::EmptySelection);
    }
    for id in &selection.objects {
        if document.object(*id).is_none() {
            return Err(SceneFragmentError::UnknownSelection(*id));
        }
    }
    let drawing = document.scene_objects();
    let mut included = BTreeSet::new();
    let mut roots = Vec::new();
    let mut objects = Vec::new();
    for (id, parent, object) in drawing {
        let inherited = parent.is_some_and(|parent| included.contains(&parent));
        if selection.objects.contains(&id) || inherited {
            included.insert(id);
            if !inherited {
                roots.push(id);
            }
            objects.push(FragmentObject {
                id,
                object: object.clone(),
            });
        }
    }
    let mut references = Vec::new();
    for reference in document.references() {
        if included.contains(&reference.source) {
            if !matches!(reference.target, ReferenceTarget::Object(id) if included.contains(&id)) {
                return Err(SceneFragmentError::ReferenceUnsupported(reference));
            }
            references.push(reference);
        }
    }
    let fragment = Fragment {
        settings: document.settings().clone(),
        roots,
        objects,
        references,
    };
    fragment.validate()?;
    Ok(fragment)
}
