//! The recorded difference of one transaction and how it is replayed in either direction.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    Atom, AtomId, Bond, BondId, Content, Direction, FragmentDisplay, GroupId, ObjectId, Point,
    PositionKey,
};
use crate::scene::{SceneObject, SceneReference, Selection};
use crate::settings::Settings;
use crate::stereo::{StereoKey, StereoState};
use crate::stereo_group::StereoGroup;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Change<T> {
    pub before: T,
    pub after: T,
}

/// The exact difference made by one transaction; applied backward to undo and forward to redo.
/// Graph changes (atoms, bonds), scene changes (the page's root order, scene objects, and
/// positions), the title, style settings, and selection are recorded apart, and undo or redo
/// replays all of them as one step.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChangeSet {
    /// Independent graph-retaining fragment display changes.
    pub fragment_displays: Vec<(PositionKey, Change<Option<FragmentDisplay>>)>,
    /// Per-placement Et display changes; the graph is retained.
    pub abbreviations: Vec<(PositionKey, Change<Option<AtomId>>)>,
    /// Independent cosmetic text changes, never chemical graph definitions.
    pub display_labels: Vec<(PositionKey, Change<Option<String>>)>,
    pub title: Option<Change<String>>,
    pub settings: Option<Change<Settings>>,
    pub atoms: Vec<(AtomId, Change<Option<Atom>>)>,
    pub bonds: Vec<(BondId, Change<Option<Bond>>)>,
    /// The page's root objects in order, when that changed.
    pub roots: Option<Change<Vec<ObjectId>>>,
    /// Scene objects; a group's state includes its ordered children.
    pub objects: Vec<(ObjectId, Change<Option<SceneObject>>)>,
    pub references: Vec<(SceneReference, Change<bool>)>,
    pub positions: Vec<(PositionKey, Change<Option<Point>>)>,
    /// Stated stereo configurations. Marks are part of the bond states.
    pub stereo: Vec<(StereoKey, Change<Option<StereoState>>)>,
    /// Enhanced-stereo groups, whole before and after.
    pub stereo_groups: Vec<(GroupId, Change<Option<StereoGroup>>)>,
    /// The selection before and after the change. It is recorded with every entry and never
    /// makes one: a change set whose content is unchanged is empty.
    pub selection: Change<Selection>,
}

impl ChangeSet {
    pub(super) fn between(before: &Content, after: &Content, selection: Change<Selection>) -> Self {
        Self {
            fragment_displays: diff_maps(&before.fragment_displays, &after.fragment_displays),
            abbreviations: diff_maps(&before.abbreviations, &after.abbreviations),
            display_labels: diff_maps(&before.display_labels, &after.display_labels),
            title: (before.title != after.title).then(|| Change {
                before: before.title.clone(),
                after: after.title.clone(),
            }),
            settings: (before.settings != after.settings).then(|| Change {
                before: before.settings.clone(),
                after: after.settings.clone(),
            }),
            atoms: diff_maps(&before.atoms, &after.atoms),
            bonds: diff_maps(&before.bonds, &after.bonds),
            roots: (before.scene.roots != after.scene.roots).then(|| Change {
                before: before.scene.roots.clone(),
                after: after.scene.roots.clone(),
            }),
            objects: diff_maps(&before.scene.objects, &after.scene.objects),
            references: diff_sets(&before.references, &after.references),
            positions: diff_maps(&before.positions, &after.positions),
            stereo: diff_maps(before.stereo.records(), after.stereo.records()),
            stereo_groups: diff_maps(before.stereo_groups.groups(), after.stereo_groups.groups()),
            selection,
        }
    }

    /// Whether the content is unchanged; the selection does not count.
    pub fn is_empty(&self) -> bool {
        self.fragment_displays.is_empty()
            && self.abbreviations.is_empty()
            && self.display_labels.is_empty()
            && self.title.is_none()
            && self.settings.is_none()
            && self.atoms.is_empty()
            && self.bonds.is_empty()
            && self.roots.is_none()
            && self.objects.is_empty()
            && self.references.is_empty()
            && self.positions.is_empty()
            && self.stereo.is_empty()
            && self.stereo_groups.is_empty()
    }

    /// Entity changes counted against [`HISTORY_CHANGE_LIMIT`](super::HISTORY_CHANGE_LIMIT).
    pub(super) fn size(&self) -> usize {
        self.fragment_displays.len()
            + self.abbreviations.len()
            + self.display_labels.len()
            + usize::from(self.title.is_some())
            + usize::from(self.settings.is_some())
            + self.atoms.len()
            + self.bonds.len()
            + usize::from(self.roots.is_some())
            + self.objects.len()
            + self.references.len()
            + self.positions.len()
            + self.stereo.len()
            + self.stereo_groups.len()
    }

    /// The shape `between` produces: non-empty, each entity at most once, every entry an
    /// actual change, and no object changing its kind or a graphic its shape. Only such a set
    /// replays identically backward and forward, so restored history that passes the backward
    /// check can always be redone.
    pub(super) fn is_well_formed(&self) -> bool {
        fn entries_valid<K: Ord, V: PartialEq>(entries: &[(K, Change<Option<V>>)]) -> bool {
            let mut seen = BTreeSet::new();
            entries
                .iter()
                .all(|(id, change)| change.before != change.after && seen.insert(id))
        }
        let kinds_kept =
            self.objects
                .iter()
                .all(|(_, change)| match (&change.before, &change.after) {
                    (Some(before), Some(after)) => before.same_kind(after),
                    _ => true,
                });
        let mut references_seen = BTreeSet::new();
        let references_valid = self.references.iter().all(|(reference, change)| {
            change.before != change.after && references_seen.insert(reference)
        });
        !self.is_empty()
            && self
                .title
                .as_ref()
                .is_none_or(|change| change.before != change.after)
            && self
                .settings
                .as_ref()
                .is_none_or(|change| change.before != change.after)
            && self
                .roots
                .as_ref()
                .is_none_or(|change| change.before != change.after)
            && entries_valid(&self.fragment_displays)
            && entries_valid(&self.abbreviations)
            && entries_valid(&self.display_labels)
            && entries_valid(&self.atoms)
            && entries_valid(&self.bonds)
            && entries_valid(&self.objects)
            && references_valid
            && entries_valid(&self.positions)
            && entries_valid(&self.stereo)
            && entries_valid(&self.stereo_groups)
            && kinds_kept
    }
}

fn diff_maps<K: Copy + Ord, V: Clone + PartialEq>(
    before: &BTreeMap<K, V>,
    after: &BTreeMap<K, V>,
) -> Vec<(K, Change<Option<V>>)> {
    let keys: BTreeSet<K> = before.keys().chain(after.keys()).copied().collect();
    keys.into_iter()
        .filter_map(|key| {
            let (old, new) = (before.get(&key), after.get(&key));
            (old != new).then(|| {
                (
                    key,
                    Change {
                        before: old.cloned(),
                        after: new.cloned(),
                    },
                )
            })
        })
        .collect()
}

fn diff_sets<T: Copy + Ord>(before: &BTreeSet<T>, after: &BTreeSet<T>) -> Vec<(T, Change<bool>)> {
    before
        .symmetric_difference(after)
        .map(|value| {
            (
                *value,
                Change {
                    before: before.contains(value),
                    after: after.contains(value),
                },
            )
        })
        .collect()
}

impl<T> Change<T> {
    /// The state replaying in `direction` starts from, and the one it leaves.
    pub(super) fn sides(&self, direction: Direction) -> (&T, &T) {
        match direction {
            Direction::Backward => (&self.after, &self.before),
            Direction::Forward => (&self.before, &self.after),
        }
    }
}

pub(super) fn replay<K: Copy + Ord, V: Clone + PartialEq>(
    map: &mut BTreeMap<K, V>,
    changes: &[(K, Change<Option<V>>)],
    direction: Direction,
) -> Result<(), ()> {
    for (key, change) in changes {
        let (from, to) = change.sides(direction);
        if map.get(key) != from.as_ref() {
            return Err(());
        }
        match to {
            Some(value) => map.insert(*key, value.clone()),
            None => map.remove(key),
        };
    }
    Ok(())
}
