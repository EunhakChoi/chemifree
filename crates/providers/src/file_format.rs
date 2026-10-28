//! Chemifree document file format, version 2 (`*.json`), and the recovery draft format that
//! wraps the same document and history. This is Chemifree's own working format, not an
//! interchange format such as MOL or CDXML.
//!
//! Version 2 stores the chemical graph (`molecule`), the page, and the molecule scene
//! instances with their atom positions apart, and history entries record graph, instance,
//! and position changes apart. Version 1 files and drafts, which kept one position on each
//! atom, are still read: they are migrated in memory into one page `p1` and one instance `s1`
//! holding the old positions, keeping identifiers, revision, title, and undo/redo history.
//! Reading never writes; whatever is written next is version 2. Any other version is refused.

use chemifree_application::{Draft, DraftInfo};
use chemifree_domain::document::{
    Atom, AtomId, Bond, BondId, BondOrder, Change, ChangeSet, Document, DocumentId, DocumentParts,
    InstanceId, MOLECULE_ID, PAGE_ID, Point, PositionKey, SceneInstance,
};
use chemifree_domain::element::Element;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

const FORMAT: &str = "chemifree.document";
const FORMAT_VERSION: u32 = 2;
const DRAFT_FORMAT: &str = "chemifree.draft";
const DRAFT_FORMAT_VERSION: u32 = 2;
/// The version before graph and scene were separated; read and migrated, never written.
const VERSION_1: u32 = 1;
/// The one instance a version 1 document becomes.
const MIGRATED_INSTANCE: u32 = 1;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FileV2 {
    format: String,
    format_version: u32,
    document: StoredDocument,
    history: StoredHistory,
}

/// A recovery draft: the working copy with the stored revision it started from.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DraftV2 {
    format: String,
    format_version: u32,
    base_revision: u64,
    saved_at_ms: u64,
    document: StoredDocument,
    history: StoredHistory,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredDocument {
    id: String,
    revision: u64,
    title: String,
    molecule: StoredMolecule,
    pages: Vec<StoredPage>,
    /// In order; the v1 view reports the first one's positions as `atoms[].position`.
    instances: Vec<StoredInstance>,
    next_atom: u32,
    next_bond: u32,
    next_instance: u32,
}

/// The chemical graph: atoms and bonds without positions. Atoms and bonds are
/// `{"id": ..., "state": {...}}`, matching history entries.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredMolecule {
    id: String,
    atoms: Vec<StoredAtom>,
    bonds: Vec<StoredBond>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredAtom {
    id: String,
    state: AtomState,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AtomState {
    element: String,
    charge: i8,
    isotope: Option<u16>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredPage {
    id: String,
}

/// A molecule scene instance with one position per atom of its molecule.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredInstance {
    id: String,
    page: String,
    molecule: String,
    positions: Vec<StoredPosition>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredPosition {
    atom: String,
    x: f64,
    y: f64,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredPoint {
    x: f64,
    y: f64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredBond {
    id: String,
    state: BondState,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct BondState {
    begin: String,
    end: String,
    order: StoredOrder,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum StoredOrder {
    Single,
    Double,
    Triple,
}

/// `undo` is oldest first; `redo` is a stack whose last entry is redone next.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredHistory {
    undo: Vec<StoredChangeSet>,
    redo: Vec<StoredChangeSet>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredChangeSet {
    title: Option<StoredChange<String>>,
    atoms: Vec<Keyed<AtomState>>,
    bonds: Vec<Keyed<BondState>>,
    instances: Vec<Keyed<InstanceState>>,
    positions: Vec<PositionChange>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredChange<T> {
    before: T,
    after: T,
}

/// One entity's change; `null` means the entity did not exist on that side.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Keyed<T> {
    id: String,
    before: Option<T>,
    after: Option<T>,
}

/// An instance's parents. Its positions change through `positions` entries.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct InstanceState {
    page: String,
    molecule: String,
}

/// One atom's position in one instance; `null` where it had none.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PositionChange {
    instance: String,
    atom: String,
    before: Option<StoredPoint>,
    after: Option<StoredPoint>,
}

/// The version 1 shapes, read for migration only. The header fields are checked by
/// [`version`] before these are parsed.
mod v1 {
    use serde::Deserialize;
    use serde::de::IgnoredAny;

    use super::{BondState, Keyed, StoredBond, StoredChange, StoredPoint};

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct File {
        #[serde(rename = "format")]
        pub _format: IgnoredAny,
        #[serde(rename = "format_version")]
        pub _format_version: IgnoredAny,
        pub document: Document,
        pub history: History,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct Draft {
        #[serde(rename = "format")]
        pub _format: IgnoredAny,
        #[serde(rename = "format_version")]
        pub _format_version: IgnoredAny,
        pub base_revision: u64,
        pub saved_at_ms: u64,
        pub document: Document,
        pub history: History,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct Document {
        pub id: String,
        pub revision: u64,
        pub title: String,
        pub atoms: Vec<Atom>,
        pub bonds: Vec<StoredBond>,
        pub next_atom: u32,
        pub next_bond: u32,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct Atom {
        pub id: String,
        pub state: AtomState,
    }

    /// An atom's chemistry and its one position, as version 1 kept them together.
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct AtomState {
        pub element: String,
        pub charge: i8,
        pub isotope: Option<u16>,
        pub position: StoredPoint,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct History {
        pub undo: Vec<ChangeSet>,
        pub redo: Vec<ChangeSet>,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct ChangeSet {
        pub title: Option<StoredChange<String>>,
        pub atoms: Vec<Keyed<AtomState>>,
        pub bonds: Vec<Keyed<BondState>>,
    }
}

/// The two fields every version has, read before choosing a reader.
#[derive(Deserialize)]
struct Header {
    format: String,
    format_version: u32,
}

/// Which reader a file's header calls for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Version {
    Current,
    One,
}

fn version(bytes: &[u8], format: &str, current: u32, kind: &str) -> Result<Version, String> {
    let header: Header = parse(bytes, kind)?;
    if header.format != format {
        return Err(format!(
            "unsupported {kind} format {:?} version {}",
            header.format, header.format_version
        ));
    }
    match header.format_version {
        version if version == current => Ok(Version::Current),
        VERSION_1 => Ok(Version::One),
        version => Err(format!(
            "unsupported {kind} format {format:?} version {version}; this build reads versions \
             {VERSION_1} and {current}"
        )),
    }
}

fn parse<T: DeserializeOwned>(bytes: &[u8], kind: &str) -> Result<T, String> {
    serde_json::from_slice(bytes).map_err(|error| format!("invalid {kind} file: {error}"))
}

pub(crate) fn encode(document: &Document) -> Vec<u8> {
    let (document, history) = stored(document);
    to_bytes(&FileV2 {
        format: FORMAT.to_owned(),
        format_version: FORMAT_VERSION,
        document,
        history,
    })
}

pub(crate) fn encode_draft(draft: &Draft) -> Vec<u8> {
    let (document, history) = stored(&draft.document);
    to_bytes(&DraftV2 {
        format: DRAFT_FORMAT.to_owned(),
        format_version: DRAFT_FORMAT_VERSION,
        base_revision: draft.base_revision,
        saved_at_ms: draft.saved_at_ms,
        document,
        history,
    })
}

fn to_bytes(value: &impl Serialize) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(value).expect("document values are serializable");
    bytes.push(b'\n');
    bytes
}

fn stored(document: &Document) -> (StoredDocument, StoredHistory) {
    let parts = document.to_parts();
    let instances = parts
        .instances
        .iter()
        .map(|instance| StoredInstance {
            id: instance.to_string(),
            page: PAGE_ID.to_owned(),
            molecule: MOLECULE_ID.to_owned(),
            positions: document
                .positions(*instance)
                .map(|(atom, point)| StoredPosition {
                    atom: atom.to_string(),
                    x: point.x(),
                    y: point.y(),
                })
                .collect(),
        })
        .collect();
    let stored = StoredDocument {
        id: parts.id.to_string(),
        revision: parts.revision,
        title: parts.title,
        molecule: StoredMolecule {
            id: MOLECULE_ID.to_owned(),
            atoms: parts
                .atoms
                .iter()
                .map(|(id, atom)| StoredAtom {
                    id: id.to_string(),
                    state: atom_state(atom),
                })
                .collect(),
            bonds: parts
                .bonds
                .iter()
                .map(|(id, bond)| StoredBond {
                    id: id.to_string(),
                    state: bond_state(bond),
                })
                .collect(),
        },
        pages: vec![StoredPage {
            id: PAGE_ID.to_owned(),
        }],
        instances,
        next_atom: parts.next_atom,
        next_bond: parts.next_bond,
        next_instance: parts.next_instance,
    };
    let history = StoredHistory {
        undo: parts.undo.iter().map(stored_change_set).collect(),
        redo: parts.redo.iter().map(stored_change_set).collect(),
    };
    (stored, history)
}

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

/// True when `bytes` are a document file of version 1, which a save replaces only after
/// keeping a copy of them.
pub(crate) fn is_version_1(bytes: &[u8]) -> bool {
    version(bytes, FORMAT, FORMAT_VERSION, "document") == Ok(Version::One)
}

/// Parses and fully validates a stored document, including its history; a version 1 file
/// is migrated.
pub(crate) fn decode(bytes: &[u8]) -> Result<Document, String> {
    let parts = match version(bytes, FORMAT, FORMAT_VERSION, "document")? {
        Version::Current => {
            let file: FileV2 = parse(bytes, "document")?;
            parts(file.document, file.history)?
        }
        Version::One => {
            let file: v1::File = parse(bytes, "document")?;
            migrate(file.document, file.history)?
        }
    };
    Document::restore(parts).map_err(|error| error.to_string())
}

/// Parses and fully validates a recovery draft; a version 1 draft is migrated.
pub(crate) fn decode_draft(bytes: &[u8]) -> Result<Draft, String> {
    let (parts, base_revision, saved_at_ms) =
        match version(bytes, DRAFT_FORMAT, DRAFT_FORMAT_VERSION, "draft")? {
            Version::Current => {
                let file: DraftV2 = parse(bytes, "draft")?;
                let parts = parts(file.document, file.history)?;
                (parts, file.base_revision, file.saved_at_ms)
            }
            Version::One => {
                let file: v1::Draft = parse(bytes, "draft")?;
                let parts = migrate(file.document, file.history)?;
                (parts, file.base_revision, file.saved_at_ms)
            }
        };
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

fn parts(stored: StoredDocument, history: StoredHistory) -> Result<DocumentParts, String> {
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
    Ok(DocumentParts {
        id: document_id(&stored.id)?,
        revision: stored.revision,
        title: stored.title,
        atoms: stored
            .molecule
            .atoms
            .into_iter()
            .map(|atom| Ok((atom_id(&atom.id)?, to_atom(atom.state)?)))
            .collect::<Result<_, String>>()?,
        bonds: stored
            .molecule
            .bonds
            .into_iter()
            .map(|bond| Ok((bond_id(&bond.id)?, to_bond(bond.state)?)))
            .collect::<Result<_, String>>()?,
        instances,
        positions,
        next_atom: stored.next_atom,
        next_bond: stored.next_bond,
        next_instance: stored.next_instance,
        undo: history
            .undo
            .into_iter()
            .map(to_change_set)
            .collect::<Result<_, _>>()?,
        redo: history
            .redo
            .into_iter()
            .map(to_change_set)
            .collect::<Result<_, _>>()?,
    })
}

/// A version 1 document as version 2 parts: the atoms' positions move into instance `s1`, and
/// each history entry's atom changes split into graph changes and `s1` position changes (an
/// entry that only moved atoms becomes position changes only). `Document::restore` then
/// checks the result as it checks any stored document.
fn migrate(stored: v1::Document, history: v1::History) -> Result<DocumentParts, String> {
    let instance = InstanceId::from_serial(MIGRATED_INSTANCE).expect("serial 1 is an identifier");
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
        id: document_id(&stored.id)?,
        revision: stored.revision,
        title: stored.title,
        atoms,
        bonds: stored
            .bonds
            .into_iter()
            .map(|bond| Ok((bond_id(&bond.id)?, to_bond(bond.state)?)))
            .collect::<Result<_, String>>()?,
        instances: vec![instance],
        positions,
        next_atom: stored.next_atom,
        next_bond: stored.next_bond,
        next_instance: MIGRATED_INSTANCE + 1,
        undo: change_sets(history.undo)?,
        redo: change_sets(history.redo)?,
    })
}

fn migrate_change_set(set: v1::ChangeSet, instance: InstanceId) -> Result<ChangeSet, String> {
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
        title: set.title.map(|change| Change {
            before: change.before,
            after: change.after,
        }),
        atoms,
        bonds: set
            .bonds
            .into_iter()
            .map(|keyed| {
                Ok((
                    bond_id(&keyed.id)?,
                    Change {
                        before: keyed.before.map(to_bond).transpose()?,
                        after: keyed.after.map(to_bond).transpose()?,
                    },
                ))
            })
            .collect::<Result<_, String>>()?,
        instances: Vec::new(),
        positions,
    })
}

/// A version 1 atom state as the graph atom and its position.
fn split_atom(state: v1::AtomState) -> Result<(Atom, Point), String> {
    let position = point(state.position.x, state.position.y)?;
    let atom = to_atom(AtomState {
        element: state.element,
        charge: state.charge,
        isotope: state.isotope,
    })?;
    Ok((atom, position))
}

fn atom_state(atom: &Atom) -> AtomState {
    AtomState {
        element: atom.element.symbol().to_owned(),
        charge: atom.charge,
        isotope: atom.isotope,
    }
}

fn bond_state(bond: &Bond) -> BondState {
    BondState {
        begin: bond.begin.to_string(),
        end: bond.end.to_string(),
        order: match bond.order {
            BondOrder::Single => StoredOrder::Single,
            BondOrder::Double => StoredOrder::Double,
            BondOrder::Triple => StoredOrder::Triple,
        },
    }
}

/// Every instance is on the document's page and draws its molecule.
fn instance_state(_: &SceneInstance) -> InstanceState {
    InstanceState {
        page: PAGE_ID.to_owned(),
        molecule: MOLECULE_ID.to_owned(),
    }
}

fn stored_point(point: &Point) -> StoredPoint {
    StoredPoint {
        x: point.x(),
        y: point.y(),
    }
}

fn stored_change_set(set: &ChangeSet) -> StoredChangeSet {
    StoredChangeSet {
        title: set.title.as_ref().map(|change| StoredChange {
            before: change.before.clone(),
            after: change.after.clone(),
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
        instances: set
            .instances
            .iter()
            .map(|(id, change)| Keyed {
                id: id.to_string(),
                before: change.before.as_ref().map(instance_state),
                after: change.after.as_ref().map(instance_state),
            })
            .collect(),
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

fn to_change_set(stored: StoredChangeSet) -> Result<ChangeSet, String> {
    let to_instance =
        |state: InstanceState| parents(&state.page, &state.molecule).map(|()| SceneInstance);
    let to_point = |stored: StoredPoint| point(stored.x, stored.y);
    Ok(ChangeSet {
        title: stored.title.map(|change| Change {
            before: change.before,
            after: change.after,
        }),
        atoms: stored
            .atoms
            .into_iter()
            .map(|keyed| {
                Ok((
                    atom_id(&keyed.id)?,
                    Change {
                        before: keyed.before.map(to_atom).transpose()?,
                        after: keyed.after.map(to_atom).transpose()?,
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
                        before: keyed.before.map(to_bond).transpose()?,
                        after: keyed.after.map(to_bond).transpose()?,
                    },
                ))
            })
            .collect::<Result<_, String>>()?,
        instances: stored
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

/// Checks that an instance is on the document's page and draws its molecule.
fn parents(page: &str, molecule: &str) -> Result<(), String> {
    if page == PAGE_ID && molecule == MOLECULE_ID {
        Ok(())
    } else {
        Err(format!(
            "a scene instance names page {page:?} and molecule {molecule:?}; a document holds \
             page {PAGE_ID:?} and molecule {MOLECULE_ID:?}"
        ))
    }
}

fn to_atom(state: AtomState) -> Result<Atom, String> {
    Ok(Atom {
        element: Element::from_symbol(&state.element)
            .ok_or_else(|| format!("unknown element {:?}", state.element))?,
        charge: state.charge,
        isotope: state.isotope,
    })
}

fn to_bond(state: BondState) -> Result<Bond, String> {
    Ok(Bond {
        begin: atom_id(&state.begin)?,
        end: atom_id(&state.end)?,
        order: match state.order {
            StoredOrder::Single => BondOrder::Single,
            StoredOrder::Double => BondOrder::Double,
            StoredOrder::Triple => BondOrder::Triple,
        },
    })
}

fn point(x: f64, y: f64) -> Result<Point, String> {
    Point::new(x, y).map_err(|error| error.to_string())
}

fn document_id(value: &str) -> Result<DocumentId, String> {
    DocumentId::parse(value).ok_or_else(|| "invalid document identifier".to_owned())
}

fn atom_id(value: &str) -> Result<AtomId, String> {
    AtomId::parse(value).ok_or_else(|| format!("invalid atom identifier {value:?}"))
}

fn bond_id(value: &str) -> Result<BondId, String> {
    BondId::parse(value).ok_or_else(|| format!("invalid bond identifier {value:?}"))
}

fn instance_id(value: &str) -> Result<InstanceId, String> {
    InstanceId::parse(value).ok_or_else(|| format!("invalid scene instance identifier {value:?}"))
}
