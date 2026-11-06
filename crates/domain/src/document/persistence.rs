//! Conservative persistence weights, independent of a serializer. Providers verify that
//! their stored document and draft representations fit these reservations. Caption-bearing
//! histories use a byte-oriented budget because one object can contain substantial text.

use super::{Change, ChangeSet, Content, Document};
use crate::scene::{SceneObject, Selection};
use crate::stereo_group::StereoGroup;

// Each reservation includes field/identifier overhead. Identifiers need at most 11 ASCII
// bytes, floats at most 32; ID list entries reserve 16. Control-free text reserves twice its
// UTF-8 length for quotes/backslashes, plus run metadata (including the longest script, weight and style).
// A v16 run has at most 170 compact JSON metadata bytes: 89 for text/script/
// weight/style/font (sans-serif is longest), 29 for size_pt (bounded floats need
// at most 18 characters), 18 for ,"color":"#RRGGBB", and
// 18 for ,"underline":false, and 16 for ,"outline":false.
// v17 adds 15 bytes for ,"shadow":false. The 192-byte encoding reservation covers all run metadata and every finite 4–72 pt override.
// The caption-level 256-byte overhead also covers the bounded v11 alignment enum.
const ATOM: usize = 512;
const BOND: usize = 256;
const POSITION: usize = 192;
const REFERENCE: usize = 128;
const STEREO: usize = 256;
const SETTINGS: usize = 256;
// Three IDs in content; the history form also carries both optional terminal IDs.
const ABBREVIATION: usize = 192;

fn text_weight(text: &str) -> usize {
    128_usize.saturating_add(text.len().saturating_mul(2))
}

fn ids_weight(count: usize) -> usize {
    32_usize.saturating_add(count.saturating_mul(16))
}

fn object_weight(object: &SceneObject) -> usize {
    object_weight_with(object, false)
}

fn admission_object_weight(object: &SceneObject) -> usize {
    object_weight_with(object, true)
}

fn object_weight_with(object: &SceneObject, admission: bool) -> usize {
    match object {
        SceneObject::Instance => 128,
        SceneObject::Graphic(_) => 256,
        SceneObject::Group { children } => 128 + ids_weight(children.len()),
        SceneObject::Caption(caption) => caption.runs.iter().fold(256_usize, |total, run| {
            total
                .saturating_add(if admission {
                    if run.shadow {
                        224
                    } else if run.outline {
                        192
                    } else {
                        160
                    }
                } else {
                    192
                })
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

// Preserve v15/v16 admission weights when shadow=false, including existing histories.
// v17 run metadata is at most 185 bytes (v16's 170 plus ,"shadow":false).
// Reserve 192 bytes physically. History encoding <=192/160=6/5 of admission.
// Current content has <=1,000 objects and <=32 runs each, so the extra physical
// reservation over admission is <=32*32,000 =1,024,000 bytes (regardless of text).
// Whole-file bound: 48 MiB +1,024,000 +8 MiB*6/5 =61,421,977.6 bytes <64 MiB,
// leaving >5 MiB reserve; the 4 KiB envelope already covers document/draft metadata.
pub(super) fn history_weight<'a>(sets: impl IntoIterator<Item = &'a ChangeSet>) -> usize {
    sets.into_iter().fold(0_usize, |total, set| {
        total.saturating_add(set.history_admission_weight())
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
        self.weight(selection, object_weight)
    }

    pub(super) fn caption_admission_weight(&self, selection: &Selection) -> usize {
        self.weight(selection, admission_object_weight)
    }

    fn weight(&self, selection: &Selection, object_weight: fn(&SceneObject) -> usize) -> usize {
        // Format/draft envelopes, counters, document metadata and all fixed list keys.
        4096 + text_weight(&self.title)
            + SETTINGS
            + self.atoms.len() * ATOM
            + self.bonds.len() * BOND
            + self.positions.len() * POSITION
            + self.references.len() * REFERENCE
            + self.stereo.len() * STEREO
            + self.abbreviations.len() * ABBREVIATION
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
        self.weight(object_weight)
    }

    pub(super) fn history_admission_weight(&self) -> usize {
        self.weight(admission_object_weight)
    }

    fn weight(&self, object_weight: fn(&SceneObject) -> usize) -> usize {
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
            .saturating_add(self.abbreviations.len().saturating_mul(ABBREVIATION))
    }
}

impl Document {
    /// Conservative persistence reservation for current content and both history stacks.
    /// This is neither memory usage nor an exact serialized size; providers must verify
    /// that their encoding fits it. Caption-bearing accepted states reserve at most
    /// 61.6 MiB ((48 + 8) × 11/10), including fixed file/draft envelopes.
    #[must_use]
    pub fn persistence_weight(&self) -> usize {
        self.content
            .persistence_weight(&self.selection)
            .saturating_add(
                self.undo
                    .iter()
                    .chain(&self.redo)
                    .fold(0_usize, |total, set| {
                        total.saturating_add(set.persistence_weight())
                    }),
            )
    }
}
