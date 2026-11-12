//! Reading documents and recovery drafts of every supported version, and the header-only probes.

use chemifree_application::{Draft, DraftInfo};
use chemifree_domain::document::{Document, DocumentParts, MOLECULE_ID, PAGE_ID, PositionKey};
use chemifree_domain::image::PngCache;
use serde::Deserialize;

use super::changes::read_scene_change_set;
use super::checks::{
    check_abbreviation_fields, check_caption_fields, check_graphic_fields, check_settings_fields,
    refuse_earlier_captions,
};
use super::graph::{
    atom_id, bond_id, document_id, group_id, instance_id, point, read_group, read_stereo_state,
    stereo_key,
};
use super::migrate::{migrate, parts};
use super::objects::{read_fragment_display, read_reference, read_scene_state, read_selection};
use super::records::{AtomRecord, AtomState, BondRecord, DraftFile, File, UnmarkedBondState};
use super::scene::{
    SceneDocument, SceneDraftFile, SceneFile, SceneHistory, StoredFragmentDisplayState,
};
use super::settings::to_settings;
use super::{
    DRAFT_FORMAT, DRAFT_FORMAT_VERSION, FORMAT, FORMAT_VERSION, Version, earlier, parse, v1, v2,
    v4, v5, v6, version,
};

#[derive(Deserialize)]
struct RevisionOnly {
    revision: u64,
}

/// Reads only the header and revision; `decode` remains the validating reader.
pub(crate) fn decode_revision(bytes: &[u8]) -> Result<u64, String> {
    #[derive(Deserialize)]
    struct Probe {
        document: RevisionOnly,
    }
    version(bytes, FORMAT, FORMAT_VERSION, "document")?;
    let probe: Probe = parse(bytes, "document")?;
    Ok(probe.document.revision)
}

/// The earlier version of a document file this build reads (1 to 29), which a save
/// replaces only after keeping a copy of it; `None` otherwise.
pub(crate) fn earlier_version(bytes: &[u8]) -> Option<u32> {
    earlier(version(bytes, FORMAT, FORMAT_VERSION, "document"))
}

/// The earlier version of a recovery draft this build reads, if any.
pub(crate) fn earlier_draft_version(bytes: &[u8]) -> Option<u32> {
    earlier(version(bytes, DRAFT_FORMAT, DRAFT_FORMAT_VERSION, "draft"))
}

/// Parses and fully validates a stored document, including its history; a version 1 to 29 file
/// is migrated.
pub(crate) fn decode(bytes: &[u8]) -> Result<Document, String> {
    let format = version(bytes, FORMAT, FORMAT_VERSION, "document")?;
    check_graphic_fields(bytes, format)?;
    check_abbreviation_fields(bytes, format)?;
    check_caption_fields(bytes, format)?;
    check_settings_fields(bytes, format)?;
    let parts = match format {
        Version::Current
        | Version::TwentyNine
        | Version::TwentyEight
        | Version::TwentySeven
        | Version::TwentySix
        | Version::TwentyFive
        | Version::TwentyFour
        | Version::TwentyThree
        | Version::TwentyTwo
        | Version::TwentyOne
        | Version::Twenty
        | Version::Nineteen
        | Version::Eighteen
        | Version::Seventeen
        | Version::Sixteen
        | Version::Fifteen
        | Version::Fourteen
        | Version::Thirteen
        | Version::Twelve
        | Version::Eleven
        | Version::Ten
        | Version::Nine
        | Version::Eight
        | Version::Seven => {
            let file: SceneFile = parse(bytes, "document")?;
            scene_parts(file.document, file.history)?
        }
        Version::Six => {
            let file: v6::File = parse(bytes, "document")?;
            scene_parts(file.document.into(), file.history.into())?
        }
        Version::Five => {
            let file: v5::File = parse(bytes, "document")?;
            scene_parts(file.document.into(), file.history.into())?
        }
        Version::Four => {
            let file: v4::File = parse(bytes, "document")?;
            let (document, history): (v5::Document, v5::History) =
                (file.document.into(), file.history.into());
            scene_parts(document.into(), history.into())?
        }
        Version::Three => {
            let file: File<AtomState, UnmarkedBondState> = parse(bytes, "document")?;
            parts(file.document, file.history)?
        }
        Version::Two => {
            let file: File<v2::AtomState, v2::BondState> = parse(bytes, "document")?;
            parts(file.document, file.history)?
        }
        Version::One => {
            let file: v1::File = parse(bytes, "document")?;
            migrate(file.document, file.history)?
        }
    };
    refuse_earlier_captions(format, &parts)?;
    Document::restore(parts).map_err(|error| error.to_string())
}

/// Parses and fully validates a recovery draft; a version 1 to 29 draft is migrated.
pub(crate) fn decode_draft(bytes: &[u8]) -> Result<Draft, String> {
    let format = version(bytes, DRAFT_FORMAT, DRAFT_FORMAT_VERSION, "draft")?;
    check_graphic_fields(bytes, format)?;
    check_abbreviation_fields(bytes, format)?;
    check_caption_fields(bytes, format)?;
    check_settings_fields(bytes, format)?;
    let (parts, base_revision, saved_at_ms) = match format {
        Version::Current
        | Version::TwentyNine
        | Version::TwentyEight
        | Version::TwentySeven
        | Version::TwentySix
        | Version::TwentyFive
        | Version::TwentyFour
        | Version::TwentyThree
        | Version::TwentyTwo
        | Version::TwentyOne
        | Version::Twenty
        | Version::Nineteen
        | Version::Eighteen
        | Version::Seventeen
        | Version::Sixteen
        | Version::Fifteen
        | Version::Fourteen
        | Version::Thirteen
        | Version::Twelve
        | Version::Eleven
        | Version::Ten
        | Version::Nine
        | Version::Eight
        | Version::Seven => {
            let file: SceneDraftFile = parse(bytes, "draft")?;
            let parts = scene_parts(file.document, file.history)?;
            (parts, file.base_revision, file.saved_at_ms)
        }
        Version::Six => {
            let file: v6::Draft = parse(bytes, "draft")?;
            let parts = scene_parts(file.document.into(), file.history.into())?;
            (parts, file.base_revision, file.saved_at_ms)
        }
        Version::Five => {
            let file: v5::Draft = parse(bytes, "draft")?;
            let parts = scene_parts(file.document.into(), file.history.into())?;
            (parts, file.base_revision, file.saved_at_ms)
        }
        Version::Four => {
            let file: v4::Draft = parse(bytes, "draft")?;
            let (document, history): (v5::Document, v5::History) =
                (file.document.into(), file.history.into());
            let parts = scene_parts(document.into(), history.into())?;
            (parts, file.base_revision, file.saved_at_ms)
        }
        Version::Three => {
            let file: DraftFile<AtomState, UnmarkedBondState> = parse(bytes, "draft")?;
            let parts = parts(file.document, file.history)?;
            (parts, file.base_revision, file.saved_at_ms)
        }
        Version::Two => {
            let file: DraftFile<v2::AtomState, v2::BondState> = parse(bytes, "draft")?;
            let parts = parts(file.document, file.history)?;
            (parts, file.base_revision, file.saved_at_ms)
        }
        Version::One => {
            let file: v1::Draft = parse(bytes, "draft")?;
            let parts = migrate(file.document, file.history)?;
            (parts, file.base_revision, file.saved_at_ms)
        }
    };
    refuse_earlier_captions(format, &parts)?;
    Ok(Draft {
        document: Document::restore(parts).map_err(|error| error.to_string())?,
        base_revision,
        saved_at_ms,
    })
}

/// Reads a draft's header without restoring its document; `decode_draft` validates.
pub(crate) fn decode_draft_info(bytes: &[u8]) -> Result<DraftInfo, String> {
    #[derive(Deserialize)]
    struct Probe {
        base_revision: u64,
        saved_at_ms: u64,
        document: RevisionOnly,
    }
    version(bytes, DRAFT_FORMAT, DRAFT_FORMAT_VERSION, "draft")?;
    let probe: Probe = parse(bytes, "draft")?;
    Ok(DraftInfo {
        revision: probe.document.revision,
        base_revision: probe.base_revision,
        saved_at_ms: probe.saved_at_ms,
    })
}

fn scene_parts(stored: SceneDocument, history: SceneHistory) -> Result<DocumentParts, String> {
    if stored.molecule.id != MOLECULE_ID {
        return Err(format!("unknown molecule {:?}", stored.molecule.id));
    }
    if !matches!(stored.pages.as_slice(), [page] if page.id == PAGE_ID) {
        return Err(format!("a document holds exactly one page, {PAGE_ID:?}"));
    }
    let mut image_cache = PngCache::default();
    Ok(DocumentParts {
        fragment_displays: stored
            .fragment_displays
            .into_iter()
            .map(|entry| {
                Ok((
                    PositionKey {
                        instance: instance_id(&entry.instance)?,
                        atom: atom_id(&entry.anchor)?,
                    },
                    read_fragment_display(StoredFragmentDisplayState {
                        members: entry.members,
                        text: entry.text,
                        runs: entry.runs,
                    })?,
                ))
            })
            .collect::<Result<_, String>>()?,
        display_labels: stored
            .display_labels
            .into_iter()
            .map(|entry| {
                Ok((
                    PositionKey {
                        instance: instance_id(&entry.instance)?,
                        atom: atom_id(&entry.anchor)?,
                    },
                    entry.text,
                ))
            })
            .collect::<Result<_, String>>()?,
        abbreviations: stored
            .abbreviations
            .into_iter()
            .map(|entry| {
                Ok((
                    PositionKey {
                        instance: instance_id(&entry.instance)?,
                        atom: atom_id(&entry.anchor)?,
                    },
                    atom_id(&entry.terminal)?,
                ))
            })
            .collect::<Result<_, String>>()?,
        id: document_id(&stored.id)?,
        revision: stored.revision,
        title: stored.title,
        settings: to_settings(&stored.settings),
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
        roots: stored
            .roots
            .iter()
            .map(|id| instance_id(id))
            .collect::<Result<_, _>>()?,
        objects: stored
            .objects
            .into_iter()
            .map(|object| {
                Ok((
                    instance_id(&object.id)?,
                    read_scene_state(object.state, &mut image_cache)?,
                ))
            })
            .collect::<Result<_, String>>()?,
        references: stored
            .references
            .into_iter()
            .map(read_reference)
            .collect::<Result<_, _>>()?,
        positions: stored
            .positions
            .into_iter()
            .map(|position| {
                Ok((
                    PositionKey {
                        instance: instance_id(&position.instance)?,
                        atom: atom_id(&position.atom)?,
                    },
                    point(position.x, position.y)?,
                ))
            })
            .collect::<Result<_, String>>()?,
        stereo: stored
            .stereo
            .into_iter()
            .map(|record| Ok((stereo_key(&record.id)?, read_stereo_state(record.state)?)))
            .collect::<Result<_, String>>()?,
        stereo_groups: stored
            .stereo_groups
            .into_iter()
            .map(|record| Ok((group_id(&record.id)?, read_group(record.state)?)))
            .collect::<Result<_, String>>()?,
        selection: read_selection(stored.selection)?,
        next_atom: stored.next_atom,
        next_bond: stored.next_bond,
        next_object: stored.next_object,
        next_group: stored.next_group,
        undo: history
            .undo
            .into_iter()
            .map(|set| read_scene_change_set(set, &mut image_cache))
            .collect::<Result<_, _>>()?,
        redo: history
            .redo
            .into_iter()
            .map(|set| read_scene_change_set(set, &mut image_cache))
            .collect::<Result<_, _>>()?,
    })
}
