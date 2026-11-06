//! Conservative persistence weights, independent of a serializer. Providers verify that
//! their stored document and draft representations fit these reservations. Caption-bearing
//! histories use a byte-oriented budget because one object can contain substantial text.

use super::{Change, ChangeSet, Content, Document};
use crate::scene::{SceneObject, Selection};
use crate::stereo_group::StereoGroup;

// Each reservation includes field/identifier overhead. Identifiers need at most 11 ASCII
// bytes, floats at most 32; ID list entries reserve 16. Control-free text reserves twice its
// UTF-8 length for quotes/backslashes, plus run metadata (including the longest script).
const ATOM: usize = 512;
const BOND: usize = 256;
const POSITION: usize = 192;
const REFERENCE: usize = 128;
const STEREO: usize = 256;
const SETTINGS: usize = 256;

fn text_weight(text: &str) -> usize {
    128_usize.saturating_add(text.len().saturating_mul(2))
}

fn ids_weight(count: usize) -> usize {
    32_usize.saturating_add(count.saturating_mul(16))
}

fn object_weight(object: &SceneObject) -> usize {
    match object {
        SceneObject::Instance => 128,
        SceneObject::Graphic(_) => 256,
        SceneObject::Group { children } => 128 + ids_weight(children.len()),
        SceneObject::Caption(caption) => caption.runs.iter().fold(256_usize, |total, run| {
            total
                .saturating_add(96)
                .saturating_add(run.text.len().saturating_mul(2))
        }),
    }
}

fn group_weight(group: &StereoGroup) -> usize {
    128 + ids_weight(group.members().len())
}

fn selection_weight(selection: &Selection) -> usize {
    128 + ids_weight(selection.objects.len() + selection.atoms.len() + selection.bonds.len())
}

fn changed_weight<K, T>(changes: &[(K, Change<Option<T>>)], weight: impl Fn(&T) -> usize) -> usize {
    changes.iter().fold(0_usize, |total, (_, change)| {
        total
            .saturating_add(64)
            .saturating_add(change.before.as_ref().map_or(0, &weight))
            .saturating_add(change.after.as_ref().map_or(0, &weight))
    })
}

pub(super) fn history_weight<'a>(sets: impl IntoIterator<Item = &'a ChangeSet>) -> usize {
    sets.into_iter().fold(0_usize, |total, set| {
        total.saturating_add(set.persistence_weight())
    })
}

impl Content {
    pub(super) fn has_captions(&self) -> bool {
        self.scene
            .objects
            .values()
            .any(|object| matches!(object, SceneObject::Caption(_)))
    }

    pub(super) fn persistence_weight(&self, selection: &Selection) -> usize {
        // Format/draft envelopes, counters, document metadata and all fixed list keys.
        4096 + text_weight(&self.title)
            + SETTINGS
            + self.atoms.len() * ATOM
            + self.bonds.len() * BOND
            + self.positions.len() * POSITION
            + self.references.len() * REFERENCE
            + self.stereo.len() * STEREO
            + ids_weight(self.scene.roots.len())
            + self
                .scene
                .objects
                .values()
                .map(object_weight)
                .sum::<usize>()
            + self
                .stereo_groups
                .iter()
                .map(|(_, group)| group_weight(group))
                .sum::<usize>()
            + selection_weight(selection)
    }
}

impl ChangeSet {
    pub(super) fn has_captions(&self) -> bool {
        self.objects.iter().any(|(_, change)| {
            [&change.before, &change.after]
                .into_iter()
                .flatten()
                .any(|object| matches!(object, SceneObject::Caption(_)))
        })
    }

    pub(super) fn persistence_weight(&self) -> usize {
        let mut total = 256
            + selection_weight(&self.selection.before)
            + selection_weight(&self.selection.after);
        if let Some(title) = &self.title {
            total += text_weight(&title.before) + text_weight(&title.after);
        }
        if self.settings.is_some() {
            total += 2 * SETTINGS;
        }
        if let Some(roots) = &self.roots {
            total += ids_weight(roots.before.len()) + ids_weight(roots.after.len());
        }
        total
            .saturating_add(changed_weight(&self.atoms, |_| ATOM))
            .saturating_add(changed_weight(&self.bonds, |_| BOND))
            .saturating_add(changed_weight(&self.positions, |_| POSITION))
            .saturating_add(changed_weight(&self.stereo, |_| STEREO))
            .saturating_add(changed_weight(&self.stereo_groups, group_weight))
            .saturating_add(changed_weight(&self.objects, object_weight))
            .saturating_add(self.references.len().saturating_mul(REFERENCE))
    }
}

impl Document {
    /// Conservative persistence reservation for current content and both history stacks.
    /// This is neither memory usage nor an exact serialized size; providers must verify
    /// that their encoding fits it. Caption-bearing accepted states reserve at most 56 MiB.
    #[must_use]
    pub fn persistence_weight(&self) -> usize {
        self.content
            .persistence_weight(&self.selection)
            .saturating_add(history_weight(&self.undo))
            .saturating_add(history_weight(&self.redo))
    }
}
