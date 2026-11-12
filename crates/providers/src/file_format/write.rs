//! Writing documents and recovery drafts in the current version as compact JSON.

use chemifree_application::Draft;
use chemifree_domain::document::{Document, MOLECULE_ID, PAGE_ID};
use serde::Serialize;

use super::changes::stored_scene_change_set;
use super::graph::{atom_state, bond_state, stereo_key_id, stored_group, stored_stereo_state};
use super::objects::{
    stored_fragment_display, stored_reference, stored_scene_state, stored_selection,
};
use super::records::{Stored, StoredMolecule, StoredPage};
use super::scene::{
    SceneDocument, SceneDraftFile, SceneFile, SceneHistory, ScenePosition, StoredAbbreviation,
    StoredDisplayLabel, StoredFragmentDisplay,
};
use super::settings::stored_settings;
use super::{DRAFT_FORMAT, DRAFT_FORMAT_VERSION, FORMAT, FORMAT_VERSION};

pub(crate) fn encode(document: &Document) -> Vec<u8> {
    let (document, history) = stored(document);
    to_bytes(&SceneFile {
        format: FORMAT.to_owned(),
        format_version: FORMAT_VERSION,
        document,
        history,
    })
}

pub(crate) fn encode_draft(draft: &Draft) -> Vec<u8> {
    let (document, history) = stored(&draft.document);
    to_bytes(&SceneDraftFile {
        format: DRAFT_FORMAT.to_owned(),
        format_version: DRAFT_FORMAT_VERSION,
        base_revision: draft.base_revision,
        saved_at_ms: draft.saved_at_ms,
        document,
        history,
    })
}

/// Compact JSON and a final newline. Indentation would multiply the size of a full history
/// beyond the read limit.
fn to_bytes(value: &impl Serialize) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(value).expect("document values are serializable");
    bytes.push(b'\n');
    bytes
}

type Current = (SceneDocument, SceneHistory);

fn stored(document: &Document) -> Current {
    let parts = document.to_parts();
    let stored = SceneDocument {
        fragment_displays: parts
            .fragment_displays
            .iter()
            .map(|(key, display)| StoredFragmentDisplay {
                instance: key.instance.to_string(),
                anchor: key.atom.to_string(),
                members: display.members.iter().map(ToString::to_string).collect(),
                text: None,
                runs: stored_fragment_display(display).runs,
            })
            .collect(),
        display_labels: parts
            .display_labels
            .iter()
            .map(|(key, text)| StoredDisplayLabel {
                instance: key.instance.to_string(),
                anchor: key.atom.to_string(),
                text: text.clone(),
            })
            .collect(),
        abbreviations: parts
            .abbreviations
            .iter()
            .map(|(key, terminal)| StoredAbbreviation {
                instance: key.instance.to_string(),
                anchor: key.atom.to_string(),
                terminal: terminal.to_string(),
            })
            .collect(),
        id: parts.id.to_string(),
        revision: parts.revision,
        title: parts.title,
        settings: stored_settings(&parts.settings),
        molecule: StoredMolecule {
            id: MOLECULE_ID.to_owned(),
            atoms: parts
                .atoms
                .iter()
                .map(|(id, atom)| Stored {
                    id: id.to_string(),
                    state: atom_state(atom),
                })
                .collect(),
            bonds: parts
                .bonds
                .iter()
                .map(|(id, bond)| Stored {
                    id: id.to_string(),
                    state: bond_state(bond),
                })
                .collect(),
        },
        pages: vec![StoredPage {
            id: PAGE_ID.to_owned(),
        }],
        roots: parts.roots.iter().map(ToString::to_string).collect(),
        objects: parts
            .objects
            .iter()
            .map(|(id, object)| Stored {
                id: id.to_string(),
                state: stored_scene_state(object),
            })
            .collect(),
        references: parts.references.iter().map(stored_reference).collect(),
        positions: parts
            .positions
            .iter()
            .map(|(key, point)| ScenePosition {
                instance: key.instance.to_string(),
                atom: key.atom.to_string(),
                x: point.x(),
                y: point.y(),
            })
            .collect(),
        stereo: parts
            .stereo
            .iter()
            .map(|(key, state)| Stored {
                id: stereo_key_id(*key),
                state: stored_stereo_state(state),
            })
            .collect(),
        stereo_groups: parts
            .stereo_groups
            .iter()
            .map(|(id, group)| Stored {
                id: id.to_string(),
                state: stored_group(group),
            })
            .collect(),
        selection: stored_selection(&parts.selection),
        next_atom: parts.next_atom,
        next_bond: parts.next_bond,
        next_object: parts.next_object,
        next_group: parts.next_group,
    };
    let history = SceneHistory {
        undo: parts.undo.iter().map(stored_scene_change_set).collect(),
        redo: parts.redo.iter().map(stored_scene_change_set).collect(),
    };
    (stored, history)
}
