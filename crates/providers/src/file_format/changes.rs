//! Change-set codecs: version 30 change sets in both directions, and version 2 and 3 change sets
//! read into domain change sets.

use chemifree_domain::document::{Change, ChangeSet, PositionKey};
use chemifree_domain::image::PngCache;
use chemifree_domain::scene::SceneObject;

use super::graph::{
    atom_id, atom_state, bond_id, bond_state, group_id, instance_id, parents, point, read_group,
    read_stereo_state, stereo_key, stereo_key_id, stored_group, stored_point, stored_stereo_state,
};
use super::objects::{
    read_fragment_display, read_reference, read_scene_state, read_selection,
    stored_fragment_display, stored_reference, stored_scene_state, stored_selection,
};
use super::records::{
    AtomRecord, BondRecord, InstanceState, Keyed, PositionChange, StoredChange, StoredChangeSet,
    StoredPoint,
};
use super::scene::{
    SceneChangeSet, StoredAbbreviationChange, StoredDisplayLabelChange,
    StoredFragmentDisplayChange, StoredReferenceChange,
};
use super::settings::{stored_settings, to_settings};

pub(super) fn read_scene_change_set(
    stored: SceneChangeSet,
    image_cache: &mut PngCache,
) -> Result<ChangeSet, String> {
    let mut set = to_change_set(StoredChangeSet {
        title: stored.title,
        atoms: stored.atoms,
        bonds: stored.bonds,
        instances: Vec::new(),
        positions: stored.positions,
    })?;
    set.fragment_displays = stored
        .fragment_displays
        .into_iter()
        .map(|entry| {
            Ok((
                PositionKey {
                    instance: instance_id(&entry.instance)?,
                    atom: atom_id(&entry.anchor)?,
                },
                Change {
                    before: entry.before.map(read_fragment_display).transpose()?,
                    after: entry.after.map(read_fragment_display).transpose()?,
                },
            ))
        })
        .collect::<Result<_, String>>()?;
    set.display_labels = stored
        .display_labels
        .into_iter()
        .map(|entry| {
            Ok((
                PositionKey {
                    instance: instance_id(&entry.instance)?,
                    atom: atom_id(&entry.anchor)?,
                },
                Change {
                    before: entry.before,
                    after: entry.after,
                },
            ))
        })
        .collect::<Result<_, String>>()?;
    set.abbreviations = stored
        .abbreviations
        .into_iter()
        .map(|entry| {
            Ok((
                PositionKey {
                    instance: instance_id(&entry.instance)?,
                    atom: atom_id(&entry.anchor)?,
                },
                Change {
                    before: entry.before.map(|id| atom_id(&id)).transpose()?,
                    after: entry.after.map(|id| atom_id(&id)).transpose()?,
                },
            ))
        })
        .collect::<Result<_, String>>()?;
    set.settings = stored.settings.map(|change| Change {
        before: to_settings(&change.before),
        after: to_settings(&change.after),
    });
    set.roots = stored
        .roots
        .map(|change| {
            Ok::<_, String>(Change {
                before: change
                    .before
                    .iter()
                    .map(|id| instance_id(id))
                    .collect::<Result<_, _>>()?,
                after: change
                    .after
                    .iter()
                    .map(|id| instance_id(id))
                    .collect::<Result<_, _>>()?,
            })
        })
        .transpose()?;
    set.objects = stored
        .objects
        .into_iter()
        .map(|object| {
            Ok((
                instance_id(&object.id)?,
                Change {
                    before: object
                        .before
                        .map(|state| read_scene_state(state, image_cache))
                        .transpose()?,
                    after: object
                        .after
                        .map(|state| read_scene_state(state, image_cache))
                        .transpose()?,
                },
            ))
        })
        .collect::<Result<_, String>>()?;
    set.references = stored
        .references
        .into_iter()
        .map(|change| {
            Ok((
                read_reference(change.reference)?,
                Change {
                    before: change.before,
                    after: change.after,
                },
            ))
        })
        .collect::<Result<_, String>>()?;
    set.stereo = stored
        .stereo
        .into_iter()
        .map(|keyed| {
            Ok((
                stereo_key(&keyed.id)?,
                Change {
                    before: keyed.before.map(read_stereo_state).transpose()?,
                    after: keyed.after.map(read_stereo_state).transpose()?,
                },
            ))
        })
        .collect::<Result<_, String>>()?;
    set.stereo_groups = stored
        .stereo_groups
        .into_iter()
        .map(|keyed| {
            Ok((
                group_id(&keyed.id)?,
                Change {
                    before: keyed.before.map(read_group).transpose()?,
                    after: keyed.after.map(read_group).transpose()?,
                },
            ))
        })
        .collect::<Result<_, String>>()?;
    set.selection = Change {
        before: read_selection(stored.selection.before)?,
        after: read_selection(stored.selection.after)?,
    };
    Ok(set)
}

pub(super) fn stored_scene_change_set(set: &ChangeSet) -> SceneChangeSet {
    SceneChangeSet {
        fragment_displays: set
            .fragment_displays
            .iter()
            .map(|(key, change)| StoredFragmentDisplayChange {
                instance: key.instance.to_string(),
                anchor: key.atom.to_string(),
                before: change.before.as_ref().map(stored_fragment_display),
                after: change.after.as_ref().map(stored_fragment_display),
            })
            .collect(),
        display_labels: set
            .display_labels
            .iter()
            .map(|(key, change)| StoredDisplayLabelChange {
                instance: key.instance.to_string(),
                anchor: key.atom.to_string(),
                before: change.before.clone(),
                after: change.after.clone(),
            })
            .collect(),
        abbreviations: set
            .abbreviations
            .iter()
            .map(|(key, change)| StoredAbbreviationChange {
                instance: key.instance.to_string(),
                anchor: key.atom.to_string(),
                before: change.before.map(|atom| atom.to_string()),
                after: change.after.map(|atom| atom.to_string()),
            })
            .collect(),
        title: set.title.as_ref().map(|change| StoredChange {
            before: change.before.clone(),
            after: change.after.clone(),
        }),
        settings: set.settings.as_ref().map(|change| StoredChange {
            before: stored_settings(&change.before),
            after: stored_settings(&change.after),
        }),
        atoms: set
            .atoms
            .iter()
            .map(|(id, change)| Keyed {
                id: id.to_string(),
                before: change.before.as_ref().map(atom_state),
                after: change.after.as_ref().map(atom_state),
            })
            .collect(),
        bonds: set
            .bonds
            .iter()
            .map(|(id, change)| Keyed {
                id: id.to_string(),
                before: change.before.as_ref().map(bond_state),
                after: change.after.as_ref().map(bond_state),
            })
            .collect(),
        roots: set.roots.as_ref().map(|change| StoredChange {
            before: change.before.iter().map(ToString::to_string).collect(),
            after: change.after.iter().map(ToString::to_string).collect(),
        }),
        objects: set
            .objects
            .iter()
            .map(|(id, change)| Keyed {
                id: id.to_string(),
                before: change.before.as_ref().map(stored_scene_state),
                after: change.after.as_ref().map(stored_scene_state),
            })
            .collect(),
        references: set
            .references
            .iter()
            .map(|(reference, change)| StoredReferenceChange {
                reference: stored_reference(reference),
                before: change.before,
                after: change.after,
            })
            .collect(),
        stereo: set
            .stereo
            .iter()
            .map(|(key, change)| Keyed {
                id: stereo_key_id(*key),
                before: change.before.as_ref().map(stored_stereo_state),
                after: change.after.as_ref().map(stored_stereo_state),
            })
            .collect(),
        stereo_groups: set
            .stereo_groups
            .iter()
            .map(|(id, change)| Keyed {
                id: id.to_string(),
                before: change.before.as_ref().map(stored_group),
                after: change.after.as_ref().map(stored_group),
            })
            .collect(),
        selection: StoredChange {
            before: stored_selection(&set.selection.before),
            after: stored_selection(&set.selection.after),
        },
        positions: set
            .positions
            .iter()
            .map(|(key, change)| PositionChange {
                instance: key.instance.to_string(),
                atom: key.atom.to_string(),
                before: change.before.as_ref().map(stored_point),
                after: change.after.as_ref().map(stored_point),
            })
            .collect(),
    }
}

pub(super) fn to_change_set<A: AtomRecord, B: BondRecord>(
    stored: StoredChangeSet<A, B>,
) -> Result<ChangeSet, String> {
    let to_instance = |state: InstanceState| {
        parents(&state.page, &state.molecule).map(|()| SceneObject::Instance)
    };
    let to_point = |stored: StoredPoint| point(stored.x, stored.y);
    Ok(ChangeSet {
        fragment_displays: Vec::new(),
        abbreviations: Vec::new(),
        display_labels: Vec::new(),
        title: stored.title.map(|change| Change {
            before: change.before,
            after: change.after,
        }),
        settings: None,
        atoms: stored
            .atoms
            .into_iter()
            .map(|keyed| {
                Ok((
                    atom_id(&keyed.id)?,
                    Change {
                        before: keyed.before.map(AtomRecord::into_atom).transpose()?,
                        after: keyed.after.map(AtomRecord::into_atom).transpose()?,
                    },
                ))
            })
            .collect::<Result<_, String>>()?,
        bonds: stored
            .bonds
            .into_iter()
            .map(|keyed| {
                Ok((
                    bond_id(&keyed.id)?,
                    Change {
                        before: keyed.before.map(BondRecord::into_bond).transpose()?,
                        after: keyed.after.map(BondRecord::into_bond).transpose()?,
                    },
                ))
            })
            .collect::<Result<_, String>>()?,
        roots: None,
        objects: stored
            .instances
            .into_iter()
            .map(|keyed| {
                Ok((
                    instance_id(&keyed.id)?,
                    Change {
                        before: keyed.before.map(to_instance).transpose()?,
                        after: keyed.after.map(to_instance).transpose()?,
                    },
                ))
            })
            .collect::<Result<_, String>>()?,
        references: Vec::new(),
        stereo: Vec::new(),
        stereo_groups: Vec::new(),
        selection: Change::default(),
        positions: stored
            .positions
            .into_iter()
            .map(|change| {
                let key = PositionKey {
                    instance: instance_id(&change.instance)?,
                    atom: atom_id(&change.atom)?,
                };
                Ok((
                    key,
                    Change {
                        before: change.before.map(to_point).transpose()?,
                        after: change.after.map(to_point).transpose()?,
                    },
                ))
            })
            .collect::<Result<_, String>>()?,
    })
}
