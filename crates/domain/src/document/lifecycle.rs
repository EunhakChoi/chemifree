//! Creating, copying, and restoring documents, and converting them to stored parts.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::history::{history_size, step_checked};
use super::{
    AtomId, BondId, CAPTION_CONTENT_WEIGHT_LIMIT, CAPTION_HISTORY_WEIGHT_LIMIT, Change, ChangeSet,
    Content, Direction, Document, DocumentId, DocumentParts, EditError, GroupId,
    HISTORY_CHANGE_LIMIT, HISTORY_LIMIT, MAX_REVISION, ObjectId, RestoreError, persistence,
    validated_title,
};
use crate::image::{MAX_CURRENT_IMAGE_PIXELS, MAX_HISTORY_IMAGE_PIXELS};
use crate::scene::{SceneObject, SceneTree, Selection};
use crate::settings::Settings;
use crate::stereo::{StereoTable, read_legacy_reference};
use crate::stereo_group::StereoGroupTable;

impl Document {
    /// Creates an empty document at revision 0 with one scene instance, `s1`, and nothing
    /// selected. Surrounding whitespace is trimmed from the title.
    pub fn new(id: DocumentId, title: &str) -> Result<Self, EditError> {
        Self::new_with_settings(id, title, Settings::default())
    }

    /// Creates a document with its own initial settings, without adding a history entry.
    pub fn new_with_settings(
        id: DocumentId,
        title: &str,
        settings: Settings,
    ) -> Result<Self, EditError> {
        let title = validated_title(title)?;
        settings.validate()?;
        Ok(Self {
            id,
            revision: 0,
            content: Content {
                fragment_displays: BTreeMap::new(),
                abbreviations: BTreeMap::new(),
                display_labels: BTreeMap::new(),
                title: title.to_owned(),
                settings,
                atoms: BTreeMap::new(),
                bonds: BTreeMap::new(),
                scene: SceneTree::with_instance(ObjectId(1)),
                references: BTreeSet::new(),
                positions: BTreeMap::new(),
                stereo: StereoTable::default(),
                stereo_groups: StereoGroupTable::default(),
            },
            selection: Selection::default(),
            next_atom: 1,
            next_bond: 1,
            next_object: 2,
            next_group: 1,
            undo: VecDeque::new(),
            redo: Vec::new(),
        })
    }

    /// An independent document with this one's graph, scene, selection, and identifier
    /// counters under a new identifier and title, at revision 0 with no history. Atom, bond,
    /// and scene object identifiers are kept, so commands that name them mean the same entities
    /// in the copy.
    pub fn copy_as(&self, id: DocumentId, title: &str) -> Result<Self, EditError> {
        let title = validated_title(title)?;
        Ok(Self {
            id,
            revision: 0,
            content: Content {
                title: title.to_owned(),
                ..self.content.clone()
            },
            selection: self.selection.clone(),
            next_atom: self.next_atom,
            next_bond: self.next_bond,
            next_object: self.next_object,
            next_group: self.next_group,
            undo: VecDeque::new(),
            redo: Vec::new(),
        })
    }

    /// This document's current state, at the same identifier and revision, without its undo
    /// and redo history: what a read-only analysis or a copy needs. It is a snapshot, not a
    /// working copy, and is never saved or edited in a session's place.
    #[must_use]
    pub fn without_history(&self) -> Self {
        Self {
            id: self.id.clone(),
            revision: self.revision,
            content: self.content.clone(),
            selection: self.selection.clone(),
            next_atom: self.next_atom,
            next_bond: self.next_bond,
            next_object: self.next_object,
            next_group: self.next_group,
            undo: VecDeque::new(),
            redo: Vec::new(),
        }
    }

    /// Rebuilds a stored document, rejecting any state commands could not have produced,
    /// including history entries that do not replay cleanly from the current state, and a
    /// selection that is not where the history ends: the newest undo entry's `after` (left by
    /// an edit or a redo) or the nearest redo entry's `before` (left by an undo), compared as
    /// sets. With no history, any selection of what exists is accepted.
    pub fn restore(mut parts: DocumentParts) -> Result<Self, RestoreError> {
        let invalid = |reason: &str| RestoreError(reason.to_owned());
        read_legacy_references(&mut parts);
        if parts.revision > MAX_REVISION {
            return Err(invalid("revision exceeds the largest revision"));
        }
        let counts = (
            parts.fragment_displays.len(),
            parts.abbreviations.len(),
            parts.display_labels.len(),
            parts.atoms.len(),
            parts.bonds.len(),
            parts.objects.len(),
            parts.references.len(),
            parts.positions.len(),
            parts.stereo.len(),
            parts.stereo_groups.len(),
        );
        let content = Content {
            fragment_displays: parts.fragment_displays.into_iter().collect(),
            abbreviations: parts.abbreviations.into_iter().collect(),
            display_labels: parts.display_labels.into_iter().collect(),
            title: parts.title,
            settings: parts.settings,
            atoms: parts.atoms.into_iter().collect(),
            bonds: parts.bonds.into_iter().collect(),
            scene: SceneTree {
                roots: parts.roots,
                objects: parts.objects.into_iter().collect(),
            },
            references: parts.references.into_iter().collect(),
            positions: parts.positions.into_iter().collect(),
            stereo: StereoTable::from_records(parts.stereo.into_iter().collect()),
            stereo_groups: StereoGroupTable::from_groups(parts.stereo_groups.into_iter().collect()),
        };
        if counts
            != (
                content.fragment_displays.len(),
                content.abbreviations.len(),
                content.display_labels.len(),
                content.atoms.len(),
                content.bonds.len(),
                content.scene.objects.len(),
                content.references.len(),
                content.positions.len(),
                content.stereo.len(),
                content.stereo_groups.len(),
            )
        {
            return Err(invalid(
                "duplicate atom, bond, scene object, reference, position, stereo record, or stereo group",
            ));
        }
        if content.current_image_pixels() > MAX_CURRENT_IMAGE_PIXELS {
            return Err(invalid("current image pixel limit exceeded"));
        }
        content
            .validate()
            .map_err(|error| RestoreError(format!("invalid document content: {error}")))?;
        content
            .check_selection(&parts.selection)
            .map_err(|error| RestoreError(format!("invalid selection: {error}")))?;
        // History retention is not a migration path for malformed display text. Check
        // every incoming string and local record shape, including entries later trimmed.
        for set in parts.undo.iter().chain(&parts.redo) {
            // Retention cannot erase an invalid style from an older incoming record.
            for (_, change) in &set.objects {
                if let (Some(SceneObject::Image(a)), Some(SceneObject::Image(b))) =
                    (&change.before, &change.after)
                    && a.asset != b.asset
                {
                    return Err(invalid("immutable image asset changed in history"));
                }
                for object in [&change.before, &change.after].into_iter().flatten() {
                    if let SceneObject::Image(image) = object {
                        image
                            .validate()
                            .map_err(|e| RestoreError(format!("invalid image history: {e}")))?;
                    }
                    if let SceneObject::Graphic(graphic) = object {
                        graphic.validate().map_err(|error| {
                            RestoreError(format!("invalid graphic history: {error}"))
                        })?;
                    }
                }
            }
            set.validate_fragment_display_changes().map_err(|error| {
                RestoreError(format!("invalid fragment display history: {error}"))
            })?;
            set.validate_display_label_changes()
                .map_err(|error| RestoreError(format!("invalid display label history: {error}")))?;
        }
        let mut undo: VecDeque<ChangeSet> = parts.undo.into();
        let mut redo = parts.redo;
        let mut selection = parts.selection;
        let history_end = [
            undo.back().map(|set| &set.selection.after),
            redo.last().map(|set| &set.selection.before),
        ];
        if history_end.iter().any(Option::is_some) && !history_end.contains(&Some(&selection)) {
            return Err(invalid(
                "the selection is not where the history ends: it is neither the newest undo \
                 entry's selection after nor the nearest redo entry's selection before",
            ));
        }
        let variable_size_budget = content.has_variable_size()
            || undo.iter().chain(&redo).any(ChangeSet::has_variable_size);
        // Editing keeps undo + redo within the limits (undo and redo only move entries between
        // them). A stored history beyond them, e.g. written under looser limits, is trimmed the
        // way editing trims: oldest undo entries first, then the farthest redo entries.
        while persistence::history_image_pixels(undo.iter().chain(&redo)) > MAX_HISTORY_IMAGE_PIXELS
            || undo.len() + redo.len() > HISTORY_LIMIT
            || history_size(&undo) + history_size(&redo) > HISTORY_CHANGE_LIMIT
            || (variable_size_budget
                && persistence::history_weight(undo.iter().chain(&redo))
                    > CAPTION_HISTORY_WEIGHT_LIMIT)
        {
            if undo.pop_front().is_none() {
                redo.remove(0);
            }
        }
        // With every undo entry gone, the state is where an undo of them all would leave it.
        if undo.is_empty()
            && let Some(next) = redo.last()
        {
            selection.clone_from(&next.selection.before);
        }
        if variable_size_budget
            && content.caption_admission_weight(&selection) > CAPTION_CONTENT_WEIGHT_LIMIT
        {
            return Err(invalid("caption content exceeds the admission budget"));
        }
        let document = Self {
            id: parts.id,
            revision: parts.revision,
            content,
            selection,
            next_atom: parts.next_atom,
            next_bond: parts.next_bond,
            next_object: parts.next_object,
            next_group: parts.next_group,
            undo,
            redo,
        };
        document.check_counters()?;
        if document
            .undo
            .iter()
            .chain(&document.redo)
            .any(|set| !set.is_well_formed())
        {
            return Err(invalid(
                "history contains an empty, repeated, or no-op change, or changes an object's \
                 kind or a graphic's shape",
            ));
        }
        // An identifier can be introduced once, including across the current undo/redo
        // boundary. Replay alone cannot detect deleting an object and later creating a
        // different one with its old ID: both individual changes would still replay.
        let history = || document.undo.iter().chain(document.redo.iter().rev());
        if reuses_identifier(history().map(|set| set.atoms.as_slice()))
            || reuses_identifier(history().map(|set| set.bonds.as_slice()))
            || reuses_identifier(history().map(|set| set.objects.as_slice()))
            || reuses_identifier(history().map(|set| set.stereo_groups.as_slice()))
        {
            return Err(invalid("history reuses a deleted identifier"));
        }
        let mut past = document.content.clone();
        for set in document.undo.iter().rev() {
            step_checked(&mut past, set, Direction::Backward)
                .map_err(|()| invalid("undo history does not match the document"))?;
        }
        let mut future = document.content.clone();
        for set in document.redo.iter().rev() {
            step_checked(&mut future, set, Direction::Forward)
                .map_err(|()| invalid("redo history does not match the document"))?;
        }
        Ok(document)
    }

    /// Refuses counters of zero (serial 0 is not an identifier) and counters that would hand
    /// out an identifier the content or history already uses.
    fn check_counters(&self) -> Result<(), RestoreError> {
        let history = || self.undo.iter().chain(&self.redo);
        let atom_ids = self
            .content
            .atoms
            .keys()
            .copied()
            .chain(history().flat_map(|set| set.atoms.iter().map(|(id, _)| *id)));
        let bond_ids = self
            .content
            .bonds
            .keys()
            .copied()
            .chain(history().flat_map(|set| set.bonds.iter().map(|(id, _)| *id)));
        // Positions in history name instances too, and must not name a future one.
        let object_ids = self
            .content
            .scene
            .objects
            .keys()
            .copied()
            .chain(history().flat_map(|set| {
                set.objects
                    .iter()
                    .map(|(id, _)| *id)
                    .chain(set.positions.iter().map(|(key, _)| key.instance))
            }));
        let group_ids = self
            .content
            .stereo_groups
            .iter()
            .map(|(id, _)| id)
            .chain(history().flat_map(|set| set.stereo_groups.iter().map(|(id, _)| *id)));
        if self.next_group == 0
            || group_ids
                .map(GroupId::serial)
                .any(|serial| serial >= self.next_group)
            || self.next_atom == 0
            || self.next_bond == 0
            || self.next_object == 0
            || atom_ids
                .map(AtomId::serial)
                .any(|serial| serial >= self.next_atom)
            || bond_ids
                .map(BondId::serial)
                .any(|serial| serial >= self.next_bond)
            || object_ids
                .map(ObjectId::serial)
                .any(|serial| serial >= self.next_object)
        {
            return Err(RestoreError(
                "identifier counter would reuse an existing identifier".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn to_parts(&self) -> DocumentParts {
        DocumentParts {
            fragment_displays: self
                .fragment_displays()
                .map(|(key, display)| (key, display.clone()))
                .collect(),
            abbreviations: self.abbreviations().collect(),
            display_labels: self
                .content
                .display_labels
                .iter()
                .map(|(key, text)| (*key, text.clone()))
                .collect(),
            id: self.id.clone(),
            revision: self.revision,
            title: self.content.title.clone(),
            settings: self.content.settings.clone(),
            atoms: self.atoms().map(|(id, atom)| (id, atom.clone())).collect(),
            bonds: self.bonds().map(|(id, bond)| (id, bond.clone())).collect(),
            roots: self.content.scene.roots.clone(),
            objects: self
                .content
                .scene
                .objects
                .iter()
                .map(|(id, object)| (*id, object.clone()))
                .collect(),
            references: self.content.references.iter().copied().collect(),
            positions: self
                .content
                .positions
                .iter()
                .map(|(key, point)| (*key, *point))
                .collect(),
            stereo: self
                .content
                .stereo
                .iter()
                .map(|(key, state)| (key, *state))
                .collect(),
            stereo_groups: self
                .content
                .stereo_groups
                .iter()
                .map(|(id, group)| (id, group.clone()))
                .collect(),
            selection: self.selection.clone(),
            next_atom: self.next_atom,
            next_bond: self.next_bond,
            next_object: self.next_object,
            next_group: self.next_group,
            undo: self.undo.iter().cloned().collect(),
            redo: self.redo.clone(),
        }
    }
}

/// Normalizes the stereo of stored parts before any check: the document's records and both
/// sides of every history entry. See [`read_legacy_reference`].
fn read_legacy_references(parts: &mut DocumentParts) {
    for (_, state) in &mut parts.stereo {
        read_legacy_reference(state);
    }
    for set in parts.undo.iter_mut().chain(&mut parts.redo) {
        for (_, change) in &mut set.stereo {
            for state in [&mut change.before, &mut change.after]
                .into_iter()
                .flatten()
            {
                read_legacy_reference(state);
            }
        }
    }
}

/// History is in forward time order. A `before` value also records an identifier whose
/// original creation may have been trimmed from the oldest end of the history.
fn reuses_identifier<'a, K: Copy + Ord + 'a, V: 'a>(
    entries: impl IntoIterator<Item = &'a [(K, Change<Option<V>>)]>,
) -> bool {
    let mut seen = BTreeSet::new();
    for changes in entries {
        for (id, change) in changes {
            if change.before.is_none() && !seen.insert(*id) {
                return true;
            }
            if change.before.is_some() {
                seen.insert(*id);
            }
        }
    }
    false
}
