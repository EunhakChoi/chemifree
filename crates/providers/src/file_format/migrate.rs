//! In-memory migration: version 4 to 6 shapes become the current ones, and version 1 to 3
//! documents and histories become document parts.

use std::collections::BTreeSet;

use chemifree_domain::document::{
    Atom, Change, ChangeSet, DocumentParts, MOLECULE_ID, ObjectId, PAGE_ID, Point, PositionKey,
};
use chemifree_domain::scene::{SceneObject, Selection};
use chemifree_domain::settings::Settings;

use super::changes::to_change_set;
use super::graph::{atom_id, bond_id, document_id, instance_id, parents, point};
use super::records::{
    AtomRecord, BondRecord, Keyed, Stored, StoredDocument, StoredHistory, StoredMolecule,
};
use super::scene::{SceneChangeSet, SceneDocument, SceneHistory};
use super::settings::stored_settings;
use super::{MIGRATED_INSTANCE, v1, v2, v4, v5, v6};

impl From<v6::Document> for SceneDocument {
    fn from(value: v6::Document) -> Self {
        Self {
            fragment_displays: Vec::new(),
            abbreviations: Vec::new(),
            display_labels: Vec::new(),
            id: value.id,
            revision: value.revision,
            title: value.title,
            settings: value.settings,
            molecule: value.molecule,
            pages: value.pages,
            roots: value.roots,
            objects: value.objects,
            references: value.references,
            positions: value.positions,
            stereo: value.stereo,
            stereo_groups: Vec::new(),
            selection: value.selection,
            next_atom: value.next_atom,
            next_bond: value.next_bond,
            next_object: value.next_object,
            next_group: 1,
        }
    }
}

impl From<v6::History> for SceneHistory {
    fn from(value: v6::History) -> Self {
        Self {
            undo: value.undo.into_iter().map(Into::into).collect(),
            redo: value.redo.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<v6::ChangeSet> for SceneChangeSet {
    fn from(value: v6::ChangeSet) -> Self {
        Self {
            fragment_displays: Vec::new(),
            abbreviations: Vec::new(),
            display_labels: Vec::new(),
            title: value.title,
            settings: value.settings,
            atoms: value.atoms,
            bonds: value.bonds,
            roots: value.roots,
            objects: value.objects,
            references: value.references,
            positions: value.positions,
            stereo: value.stereo,
            stereo_groups: Vec::new(),
            selection: value.selection,
        }
    }
}

impl From<v4::Document> for v5::Document {
    fn from(value: v4::Document) -> Self {
        Self {
            id: value.id,
            revision: value.revision,
            title: value.title,
            settings: stored_settings(&Settings::default()),
            molecule: value.molecule,
            pages: value.pages,
            roots: value.roots,
            objects: value.objects,
            references: value.references,
            positions: value.positions,
            selection: value.selection,
            next_atom: value.next_atom,
            next_bond: value.next_bond,
            next_object: value.next_object,
        }
    }
}

impl From<v4::History> for v5::History {
    fn from(value: v4::History) -> Self {
        Self {
            undo: value.undo.into_iter().map(Into::into).collect(),
            redo: value.redo.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<v4::ChangeSet> for v5::ChangeSet {
    fn from(value: v4::ChangeSet) -> Self {
        Self {
            title: value.title,
            settings: None,
            atoms: value.atoms,
            bonds: value.bonds,
            roots: value.roots,
            objects: value.objects,
            references: value.references,
            positions: value.positions,
            selection: value.selection,
        }
    }
}

/// Versions 1–5 state no stereo: no bond has a mark and no record exists.
impl From<v5::Document> for SceneDocument {
    fn from(value: v5::Document) -> Self {
        Self {
            fragment_displays: Vec::new(),
            abbreviations: Vec::new(),
            display_labels: Vec::new(),
            id: value.id,
            revision: value.revision,
            title: value.title,
            settings: value.settings,
            molecule: StoredMolecule {
                id: value.molecule.id,
                atoms: value.molecule.atoms,
                bonds: value
                    .molecule
                    .bonds
                    .into_iter()
                    .map(|bond| Stored {
                        id: bond.id,
                        state: bond.state.into(),
                    })
                    .collect(),
            },
            pages: value.pages,
            roots: value.roots,
            objects: value.objects,
            references: value.references,
            positions: value.positions,
            stereo: Vec::new(),
            stereo_groups: Vec::new(),
            selection: value.selection,
            next_atom: value.next_atom,
            next_bond: value.next_bond,
            next_object: value.next_object,
            next_group: 1,
        }
    }
}

impl From<v5::History> for SceneHistory {
    fn from(value: v5::History) -> Self {
        Self {
            undo: value.undo.into_iter().map(Into::into).collect(),
            redo: value.redo.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<v5::ChangeSet> for SceneChangeSet {
    fn from(value: v5::ChangeSet) -> Self {
        Self {
            fragment_displays: Vec::new(),
            abbreviations: Vec::new(),
            display_labels: Vec::new(),
            title: value.title,
            settings: value.settings,
            atoms: value.atoms,
            bonds: value
                .bonds
                .into_iter()
                .map(|keyed| Keyed {
                    id: keyed.id,
                    before: keyed.before.map(Into::into),
                    after: keyed.after.map(Into::into),
                })
                .collect(),
            roots: value.roots,
            objects: value.objects,
            references: value.references,
            positions: value.positions,
            stereo: Vec::new(),
            stereo_groups: Vec::new(),
            selection: value.selection,
        }
    }
}

/// Versions 2 and 3 kept instances in ID order without an explicit page-root history.
/// Reconstruct each boundary from the instance changes; domain restore still checks that
/// the resulting changes actually replay from the stored content.
fn migrate_history_roots(roots: &[ObjectId], undo: &mut [ChangeSet], redo: &mut [ChangeSet]) {
    let initial: BTreeSet<ObjectId> = roots.iter().copied().collect();
    let mut current = initial.clone();
    for set in undo.iter_mut().rev() {
        let after: Vec<_> = current.iter().copied().collect();
        for (id, change) in &set.objects {
            if change.before.is_some() {
                current.insert(*id);
            } else {
                current.remove(id);
            }
        }
        let before: Vec<_> = current.iter().copied().collect();
        if before != after {
            set.roots = Some(Change { before, after });
        }
    }
    current = initial;
    for set in redo.iter_mut().rev() {
        let before: Vec<_> = current.iter().copied().collect();
        for (id, change) in &set.objects {
            if change.after.is_some() {
                current.insert(*id);
            } else {
                current.remove(id);
            }
        }
        let after: Vec<_> = current.iter().copied().collect();
        if before != after {
            set.roots = Some(Change { before, after });
        }
    }
}

pub(super) fn parts<A: AtomRecord, B: BondRecord>(
    stored: StoredDocument<A, B>,
    history: StoredHistory<A, B>,
) -> Result<DocumentParts, String> {
    if stored.molecule.id != MOLECULE_ID {
        return Err(format!(
            "molecule {:?} is not this document's molecule {MOLECULE_ID:?}",
            stored.molecule.id
        ));
    }
    if !matches!(stored.pages.as_slice(), [page] if page.id == PAGE_ID) {
        return Err(format!("a document holds exactly one page, {PAGE_ID:?}"));
    }
    let mut instances = Vec::with_capacity(stored.instances.len());
    let mut positions = Vec::new();
    for instance in stored.instances {
        parents(&instance.page, &instance.molecule)?;
        let id = instance_id(&instance.id)?;
        instances.push(id);
        for position in instance.positions {
            let key = PositionKey {
                instance: id,
                atom: atom_id(&position.atom)?,
            };
            positions.push((key, point(position.x, position.y)?));
        }
    }
    // Older formats ordered instances by ID. Preserve that order when constructing page roots.
    let mut roots = instances.clone();
    roots.sort();
    let mut undo: Vec<ChangeSet> = history
        .undo
        .into_iter()
        .map(to_change_set)
        .collect::<Result<_, _>>()?;
    let mut redo: Vec<ChangeSet> = history
        .redo
        .into_iter()
        .map(to_change_set)
        .collect::<Result<_, _>>()?;
    migrate_history_roots(&roots, &mut undo, &mut redo);
    Ok(DocumentParts {
        fragment_displays: Vec::new(),
        abbreviations: Vec::new(),
        display_labels: Vec::new(),
        id: document_id(&stored.id)?,
        revision: stored.revision,
        title: stored.title,
        settings: Settings::default(),
        atoms: stored
            .molecule
            .atoms
            .into_iter()
            .map(|atom| Ok((atom_id(&atom.id)?, atom.state.into_atom()?)))
            .collect::<Result<_, String>>()?,
        bonds: stored
            .molecule
            .bonds
            .into_iter()
            .map(|bond| Ok((bond_id(&bond.id)?, bond.state.into_bond()?)))
            .collect::<Result<_, String>>()?,
        roots,
        objects: instances
            .into_iter()
            .map(|id| (id, SceneObject::Instance))
            .collect(),
        references: Vec::new(),
        positions,
        stereo: Vec::new(),
        stereo_groups: Vec::new(),
        selection: Selection::default(),
        next_atom: stored.next_atom,
        next_bond: stored.next_bond,
        next_object: stored.next_instance,
        next_group: 1,
        undo,
        redo,
    })
}

/// A version 1 document as current parts: the atoms' positions move into instance `s1`, and
/// each history entry's atom changes split into graph changes and `s1` position changes (an
/// entry that only moved atoms becomes position changes only). `Document::restore` then
/// checks the result as it checks any stored document.
pub(super) fn migrate(stored: v1::Document, history: v1::History) -> Result<DocumentParts, String> {
    let instance = ObjectId::from_serial(MIGRATED_INSTANCE).expect("serial 1 is an identifier");
    let mut atoms = Vec::with_capacity(stored.atoms.len());
    let mut positions = Vec::with_capacity(stored.atoms.len());
    for atom in stored.atoms {
        let id = atom_id(&atom.id)?;
        let (state, position) = split_atom(atom.state)?;
        atoms.push((id, state));
        positions.push((PositionKey { instance, atom: id }, position));
    }
    let change_sets = |sets: Vec<v1::ChangeSet>| -> Result<Vec<ChangeSet>, String> {
        sets.into_iter()
            .map(|set| migrate_change_set(set, instance))
            .collect()
    };
    Ok(DocumentParts {
        fragment_displays: Vec::new(),
        abbreviations: Vec::new(),
        display_labels: Vec::new(),
        id: document_id(&stored.id)?,
        revision: stored.revision,
        title: stored.title,
        settings: Settings::default(),
        atoms,
        bonds: stored
            .bonds
            .into_iter()
            .map(|bond| Ok((bond_id(&bond.id)?, bond.state.into_bond()?)))
            .collect::<Result<_, String>>()?,
        roots: vec![instance],
        objects: vec![(instance, SceneObject::Instance)],
        references: Vec::new(),
        positions,
        stereo: Vec::new(),
        stereo_groups: Vec::new(),
        selection: Selection::default(),
        next_atom: stored.next_atom,
        next_bond: stored.next_bond,
        next_object: MIGRATED_INSTANCE + 1,
        next_group: 1,
        undo: change_sets(history.undo)?,
        redo: change_sets(history.redo)?,
    })
}

fn migrate_change_set(set: v1::ChangeSet, instance: ObjectId) -> Result<ChangeSet, String> {
    let mut atoms = Vec::new();
    let mut positions = Vec::new();
    for keyed in set.atoms {
        let id = atom_id(&keyed.id)?;
        let (before, before_position) = keyed.before.map(split_atom).transpose()?.unzip();
        let (after, after_position) = keyed.after.map(split_atom).transpose()?.unzip();
        if before != after {
            atoms.push((id, Change { before, after }));
        }
        if before_position != after_position {
            let change = Change {
                before: before_position,
                after: after_position,
            };
            positions.push((PositionKey { instance, atom: id }, change));
        }
    }
    Ok(ChangeSet {
        fragment_displays: Vec::new(),
        abbreviations: Vec::new(),
        display_labels: Vec::new(),
        title: set.title.map(|change| Change {
            before: change.before,
            after: change.after,
        }),
        settings: None,
        atoms,
        bonds: set
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
        objects: Vec::new(),
        references: Vec::new(),
        positions,
        stereo: Vec::new(),
        stereo_groups: Vec::new(),
        selection: Change::default(),
    })
}

/// A version 1 atom state as the graph atom, with the version 2 defaults, and its position.
fn split_atom(state: v1::AtomState) -> Result<(Atom, Point), String> {
    let position = point(state.position.x, state.position.y)?;
    let atom = v2::AtomState {
        element: state.element,
        charge: state.charge,
        isotope: state.isotope,
    }
    .into_atom()?;
    Ok((atom, position))
}
