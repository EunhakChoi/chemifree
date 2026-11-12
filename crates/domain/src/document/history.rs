//! Atomic transactions on a document and stepping its undo and redo history.

use std::collections::BTreeSet;

use super::change_set::replay;
use super::{
    Applied, CAPTION_CONTENT_WEIGHT_LIMIT, CAPTION_HISTORY_WEIGHT_LIMIT, Change, ChangeSet,
    Command, Content, Direction, Document, Draft, EditError, HISTORY_CHANGE_LIMIT, HISTORY_LIMIT,
    HistoryError, MAX_COMMANDS, MAX_EDIT_REVISION, Outcome, TransactionError, persistence,
};
use crate::image::{MAX_CURRENT_IMAGE_PIXELS, MAX_HISTORY_IMAGE_PIXELS};
use crate::scene::{GraphicShape, TransactionSelection};
use crate::stereo::StereoKey;
use crate::stereo_group::meaning_changes;

impl Document {
    /// Applies every command or none, keeping the current selection less what no longer
    /// exists; see [`Self::apply_selecting`].
    pub fn apply(&mut self, commands: &[Command]) -> Result<Outcome, TransactionError> {
        self.apply_selecting(commands, &TransactionSelection::default())
    }

    /// Applies every command or none and records the selection before and after with the
    /// change. `selection.before` is checked against the document before the commands and
    /// `selection.after` against it after them; a refused selection fails the whole
    /// transaction without a command index. A transaction that changes nothing leaves the
    /// revision, identifier counters, history, and selection untouched, whatever selections it
    /// names.
    pub fn apply_selecting(
        &mut self,
        commands: &[Command],
        selection: &TransactionSelection,
    ) -> Result<Outcome, TransactionError> {
        self.apply_reporting(commands, selection)
            .map(|applied| applied.outcome)
    }

    /// Like [`Self::apply_selecting`], and also reports the stereo records the edit removed
    /// because it changed the neighbors they were about (see [`Applied`]).
    pub fn apply_reporting(
        &mut self,
        commands: &[Command],
        selection: &TransactionSelection,
    ) -> Result<Applied, TransactionError> {
        let whole = |error| TransactionError { index: None, error };
        if commands.is_empty() {
            return Err(whole(EditError::EmptyTransaction));
        }
        if commands.len() > MAX_COMMANDS {
            return Err(whole(EditError::TooManyCommands));
        }
        let before = match &selection.before {
            Some(before) => {
                self.content.check_selection(before).map_err(whole)?;
                before.clone()
            }
            None => self.selection.clone(),
        };
        let mut draft = Draft {
            content: self.content.clone(),
            next_atom: self.next_atom,
            next_bond: self.next_bond,
            next_object: self.next_object,
            next_group: self.next_group,
            created: Vec::with_capacity(commands.len()),
            cascaded: BTreeSet::new(),
        };
        for (index, command) in commands.iter().enumerate() {
            draft.apply(command).map_err(|error| TransactionError {
                index: Some(index),
                error,
            })?;
        }
        draft
            .content
            .stereo
            .validate(&draft.content.atoms, &draft.content.bonds)
            .map_err(whole)?;
        // The one place a group loses a member that stopped being a stored tetrahedral center,
        // whichever command (a configuration set or cleared, a neighbor change, a deletion)
        // caused it. Doing it once, here, means a center cleared and stated again within the
        // transaction keeps its group.
        draft.content.stereo_groups.prune(&draft.content.stereo);
        draft
            .content
            .stereo_groups
            .validate(&draft.content.stereo)
            .map_err(whole)?;
        let after = match &selection.after {
            Some(request) => draft.resolve_selection(request).map_err(whole)?,
            None => draft.content.existing(&before),
        };
        let changes = ChangeSet::between(&self.content, &draft.content, Change { before, after });
        if changes.is_empty() {
            return Ok(Applied {
                outcome: Outcome::Unchanged,
                cleared: Vec::new(),
                group_notes: Vec::new(),
            });
        }
        if draft.content.current_image_pixels() > MAX_CURRENT_IMAGE_PIXELS {
            return Err(whole(EditError::ImagePixelLimit));
        }
        if persistence::history_image_pixels([&changes]) > MAX_HISTORY_IMAGE_PIXELS {
            return Err(whole(EditError::ImageHistoryPixelLimit));
        }
        let variable_size_budget = draft.content.has_variable_size()
            || changes.has_variable_size()
            || self
                .undo
                .iter()
                .chain(&self.redo)
                .any(ChangeSet::has_variable_size);
        if variable_size_budget
            && (draft
                .content
                .caption_admission_weight(&changes.selection.after)
                > CAPTION_CONTENT_WEIGHT_LIMIT
                || changes.history_admission_weight() > CAPTION_HISTORY_WEIGHT_LIMIT)
        {
            return Err(whole(if changes.has_images() {
                EditError::ImagePersistenceLimit
            } else if changes.has_graphic_shape(GraphicShape::Polygon) {
                EditError::PolygonPersistenceLimit
            } else if changes.has_graphic_shape(GraphicShape::Spline) {
                EditError::SplinePersistenceLimit
            } else {
                EditError::CaptionPersistenceLimit
            }));
        }
        let cleared: Vec<StereoKey> = draft
            .cascaded
            .iter()
            .copied()
            .filter(|key| {
                self.content.stereo.get(*key).is_some() && draft.content.stereo.get(*key).is_none()
            })
            .collect();
        let group_notes = meaning_changes(
            &self.content.stereo_groups,
            &draft.content.stereo_groups,
            &draft.content.stereo,
        );
        if self.revision >= MAX_EDIT_REVISION {
            return Err(whole(EditError::RevisionLimit));
        }
        self.content = draft.content;
        self.selection.clone_from(&changes.selection.after);
        self.next_atom = draft.next_atom;
        self.next_bond = draft.next_bond;
        self.next_object = draft.next_object;
        self.next_group = draft.next_group;
        self.revision += 1;
        self.undo.push_back(changes);
        self.redo.clear();
        while persistence::history_image_pixels(self.undo.iter()) > MAX_HISTORY_IMAGE_PIXELS
            || self.undo.len() > HISTORY_LIMIT
            || history_size(&self.undo) > HISTORY_CHANGE_LIMIT
            || (variable_size_budget
                && persistence::history_weight(&self.undo) > CAPTION_HISTORY_WEIGHT_LIMIT)
        {
            self.undo.pop_front();
        }
        Ok(Applied {
            outcome: Outcome::Changed,
            cleared,
            group_notes,
        })
    }

    /// Reverts the latest transaction as a new revision and restores the selection it was
    /// made with. Identifier counters are not rewound, so a later addition never reuses an
    /// undone entity's identifier.
    pub fn undo(&mut self) -> Result<(), HistoryError> {
        if self.revision >= MAX_EDIT_REVISION {
            return Err(HistoryError::RevisionLimit);
        }
        let set = self.undo.pop_back().ok_or(HistoryError::NothingToUndo)?;
        step(&mut self.content, &set, Direction::Backward)
            .expect("undo entries are validated against the current state");
        self.selection.clone_from(&set.selection.before);
        self.revision += 1;
        self.redo.push(set);
        Ok(())
    }

    /// Reapplies the latest undone transaction as a new revision with the selection it left.
    pub fn redo(&mut self) -> Result<(), HistoryError> {
        if self.revision >= MAX_EDIT_REVISION {
            return Err(HistoryError::RevisionLimit);
        }
        let set = self.redo.pop().ok_or(HistoryError::NothingToRedo)?;
        step(&mut self.content, &set, Direction::Forward)
            .expect("redo entries are validated against the current state");
        self.selection.clone_from(&set.selection.after);
        self.revision += 1;
        self.undo.push_back(set);
        Ok(())
    }
}

pub(super) fn history_size<'a>(sets: impl IntoIterator<Item = &'a ChangeSet>) -> usize {
    sets.into_iter().map(ChangeSet::size).sum()
}

/// Replays one restored change set like [`step`], and checks that its selections name only
/// what exists on their sides of it.
pub(super) fn step_checked(
    content: &mut Content,
    set: &ChangeSet,
    direction: Direction,
) -> Result<(), ()> {
    let (from, to) = set.selection.sides(direction);
    content.check_selection(from).map_err(|_| ())?;
    step(content, set, direction)?;
    content.check_selection(to).map_err(|_| ())
}

/// Replays one change set's content on a copy and commits it only if it matches and stays
/// valid. The selection is not content; callers set it.
fn step(content: &mut Content, set: &ChangeSet, direction: Direction) -> Result<(), ()> {
    let mut next = content.clone();
    if let Some(change) = &set.title {
        let (from, to) = change.sides(direction);
        if next.title != *from {
            return Err(());
        }
        next.title.clone_from(to);
    }
    if let Some(change) = &set.settings {
        let (from, to) = change.sides(direction);
        if next.settings != *from {
            return Err(());
        }
        next.settings.clone_from(to);
    }
    if let Some(change) = &set.roots {
        let (from, to) = change.sides(direction);
        if next.scene.roots != *from {
            return Err(());
        }
        next.scene.roots.clone_from(to);
    }
    replay(
        &mut next.fragment_displays,
        &set.fragment_displays,
        direction,
    )?;
    replay(&mut next.abbreviations, &set.abbreviations, direction)?;
    replay(&mut next.display_labels, &set.display_labels, direction)?;
    replay(&mut next.atoms, &set.atoms, direction)?;
    replay(&mut next.bonds, &set.bonds, direction)?;
    replay(&mut next.scene.objects, &set.objects, direction)?;
    for (reference, change) in &set.references {
        let (from, to) = change.sides(direction);
        if next.references.contains(reference) != *from {
            return Err(());
        }
        if *to {
            next.references.insert(*reference);
        } else {
            next.references.remove(reference);
        }
    }
    replay(&mut next.positions, &set.positions, direction)?;
    replay(next.stereo.records_mut(), &set.stereo, direction)?;
    replay(
        next.stereo_groups.groups_mut(),
        &set.stereo_groups,
        direction,
    )?;
    next.validate().map_err(|_| ())?;
    *content = next;
    Ok(())
}
