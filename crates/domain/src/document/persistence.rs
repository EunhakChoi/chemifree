//! Conservative persistence weights, independent of a serializer. Providers verify that
//! their stored document and draft representations fit these reservations. Variable-sized
//! histories use a byte-oriented budget because text and multi-point geometry vary in size.

use super::{Change, ChangeSet, Content, Document, FragmentDisplay};
use crate::scene::{GraphicShape, SceneObject, Selection};
use crate::stereo_group::StereoGroup;
use std::collections::HashSet;

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
// v22 adds another required settings boolean. Keep the v16–21 admission budget
// unchanged; reserve extra physical bytes for current and both history sides.
const SETTINGS_ADMISSION: usize = 256;
const SETTINGS: usize = 320;
// Three IDs in content; the history form also carries both optional terminal IDs.
const ABBREVIATION: usize = 192;

fn text_weight(text: &str) -> usize {
    128_usize.saturating_add(text.len().saturating_mul(2))
}

fn fragment_display_weight(display: &FragmentDisplay) -> usize {
    // One baseline run fits v24's fixed reservation. Extra runs reserve their full
    // text/script JSON metadata while preserving every legacy admission weight.
    let bytes: usize = display.runs.iter().map(|run| run.text.len()).sum();
    128 + ids_weight(display.members.len())
        + 128
        + bytes * 2
        + display.runs.len().saturating_sub(1) * 64
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
        SceneObject::Image(image) => 512 + image.asset.data_base64().len() * 2,
        // Each polygon or cubic defining point reserves two finite coordinate lexemes and JSON field overhead.
        SceneObject::Graphic(graphic)
            if matches!(graphic.shape, GraphicShape::Polygon | GraphicShape::Spline) =>
        {
            256 + 96 * graphic.points.len()
        }
        // Native v26's width field still fits this reservation: at most 244 JSON
        // bytes for id/state, the longest two-point shape and five <=25-byte f64 lexemes.
        // Keeping it unchanged preserves every old current/history admission limit.
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
// v22 settings add <=64*(1+2*100)=12,864 physical bytes, for <=61,434,842.
// v23 display-label strings reserve 192 + 2*UTF8 bytes each in content and
// 64 + both optional sides in history, at least their complete JSON representation.
// They activate the same byte budgets without changing any old caption weight.
// The extra empty display_labels list key fits each existing 256-byte history
// envelope and 4096-byte document/draft envelope, so the physical bound is unchanged.
// v24 fragment displays reserve 128 bytes plus the full member-ID list and escaped text.
// Both history sides use the same reservation, and any current/history fragment display
// activates the existing byte budgets. Empty field keys fit the existing envelopes, so
// old documents retain their admission weights and the physical 64 MiB bound is unchanged.
// v25's first text/script run fits that existing fixed fragment reservation; each
// additional nonempty run reserves 64 metadata bytes beyond its escaped UTF-8 text.
// Admission and physical weights use the same run reservation, so the bound remains.
// v29 cubic splines reserve 640 bytes (256 + 96 * 4) and activate these same budgets.
// v28 polygons reserve 256 + 96 bytes per vertex for admission and physical encoding.
// Current or history-only polygons activate these budgets; original two-point graphics
// retain their exact 256-byte weight. Polygon reservations do not increase the bound.
// This leaves >5 MiB reserve; the 4 KiB envelope already covers document/draft metadata.
pub(super) fn history_weight<'a>(sets: impl IntoIterator<Item = &'a ChangeSet>) -> usize {
    sets.into_iter().fold(0_usize, |total, set| {
        total.saturating_add(set.history_admission_weight())
    })
}

fn variable_size_object(object: &SceneObject) -> bool {
    matches!(object, SceneObject::Caption(_) | SceneObject::Image(_))
        || matches!(object, SceneObject::Graphic(graphic) if matches!(graphic.shape, GraphicShape::Polygon | GraphicShape::Spline))
}

pub(super) fn history_image_pixels<'a>(sets: impl IntoIterator<Item = &'a ChangeSet>) -> u64 {
    let mut seen = HashSet::new();
    let mut pixels = 0;
    for set in sets {
        for (_, change) in &set.objects {
            for object in [&change.before, &change.after].into_iter().flatten() {
                if let SceneObject::Image(image) = object
                    && seen.insert(image.asset.data_base64())
                {
                    pixels += image.asset.pixels();
                }
            }
        }
    }
    pixels
}
impl Content {
    pub(super) fn current_image_pixels(&self) -> u64 {
        self.scene
            .objects
            .values()
            .filter_map(|object| {
                if let SceneObject::Image(image) = object {
                    Some(image.asset.pixels())
                } else {
                    None
                }
            })
            .sum()
    }

    pub(super) fn has_variable_size(&self) -> bool {
        !self.fragment_displays.is_empty()
            || !self.display_labels.is_empty()
            || self.scene.objects.values().any(variable_size_object)
    }

    pub(super) fn persistence_weight(&self, selection: &Selection) -> usize {
        self.weight(selection, object_weight, SETTINGS)
    }

    pub(super) fn caption_admission_weight(&self, selection: &Selection) -> usize {
        self.weight(selection, admission_object_weight, SETTINGS_ADMISSION)
    }

    fn weight(
        &self,
        selection: &Selection,
        object_weight: fn(&SceneObject) -> usize,
        settings_weight: usize,
    ) -> usize {
        // Format/draft envelopes, counters, document metadata and all fixed list keys.
        4096 + text_weight(&self.title)
            + settings_weight
            + self.atoms.len() * ATOM
            + self.bonds.len() * BOND
            + self.positions.len() * POSITION
            + self.references.len() * REFERENCE
            + self.stereo.len() * STEREO
            + self
                .fragment_displays
                .values()
                .map(fragment_display_weight)
                .sum::<usize>()
            + self.abbreviations.len() * ABBREVIATION
            + self
                .display_labels
                .values()
                .map(|text| text_weight(text) + 64)
                .sum::<usize>()
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
    pub(super) fn has_images(&self) -> bool {
        self.objects.iter().any(|(_, change)| {
            [&change.before, &change.after]
                .into_iter()
                .flatten()
                .any(|object| matches!(object, SceneObject::Image(_)))
        })
    }

    pub(super) fn has_graphic_shape(&self, shape: GraphicShape) -> bool {
        self.objects.iter().any(|(_, change)| {
            [&change.before, &change.after].into_iter().flatten().any(
                |object| matches!(object, SceneObject::Graphic(graphic) if graphic.shape == shape),
            )
        })
    }

    pub(super) fn has_variable_size(&self) -> bool {
        !self.fragment_displays.is_empty()
            || !self.display_labels.is_empty()
            || self.objects.iter().any(|(_, change)| {
                [&change.before, &change.after]
                    .into_iter()
                    .flatten()
                    .any(variable_size_object)
            })
    }

    pub(super) fn persistence_weight(&self) -> usize {
        self.weight(object_weight, SETTINGS)
    }

    pub(super) fn history_admission_weight(&self) -> usize {
        self.weight(admission_object_weight, SETTINGS_ADMISSION)
    }

    fn weight(&self, object_weight: fn(&SceneObject) -> usize, settings_weight: usize) -> usize {
        let mut total = 256
            + selection_weight(&self.selection.before)
            + selection_weight(&self.selection.after);
        if let Some(title) = &self.title {
            total += text_weight(&title.before) + text_weight(&title.after);
        }
        if self.settings.is_some() {
            total += 2 * settings_weight;
        }
        if let Some(roots) = &self.roots {
            total += ids_weight(roots.before.len()) + ids_weight(roots.after.len());
        }
        total
            .saturating_add(changed_weight(
                &self.fragment_displays,
                fragment_display_weight,
            ))
            .saturating_add(changed_weight(&self.atoms, |_| ATOM))
            .saturating_add(changed_weight(&self.bonds, |_| BOND))
            .saturating_add(changed_weight(&self.positions, |_| POSITION))
            .saturating_add(changed_weight(&self.stereo, |_| STEREO))
            .saturating_add(changed_weight(&self.stereo_groups, group_weight))
            .saturating_add(changed_weight(&self.objects, object_weight))
            .saturating_add(self.references.len().saturating_mul(REFERENCE))
            .saturating_add(self.abbreviations.len().saturating_mul(ABBREVIATION))
            .saturating_add(changed_weight(&self.display_labels, |text| {
                text_weight(text) + 64
            }))
    }
}

impl Document {
    /// Conservative persistence reservation for current content and both history stacks.
    /// This is neither memory usage nor an exact serialized size; providers must verify
    /// that their encoding fits it. States with variable-sized text, polygons or splines reserve at most
    /// 61,434,842 bytes (48 MiB + 1,024,000 + 8 MiB × 6/5 + 12,864), below the 64 MiB
    /// native limit, including fixed file/draft envelopes.
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
