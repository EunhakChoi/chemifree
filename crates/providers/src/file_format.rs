//! Chemifree document file format, version 4 (`*.json`), and the recovery draft format that
//! wraps the same document and history. This is Chemifree's own working format, not an
//! interchange format such as MOL or CDXML.
//!
//! Version 4 stores the chemical graph (`molecule`), the page's ordered scene tree, directed
//! references, positions, and selection separately. History records their changes together.
//! An atom state records its kind (an element or a query),
//! charge, isotope, hydrogen state, radical, and attachment points; a bond state its order and,
//! for a query bond, its query. Files are compact JSON, so the largest document a save allows,
//! full history included, stays within the read limit.
//!
//! Older versions are still read and migrated in memory, history included: version 2 atoms and
//! bonds get the defaults (an element atom, inferred hydrogens, no radical, no attachment
//! points), and version 1 files, which kept one position on each atom, also move those
//! positions into one page `p1` and one instance `s1`. Versions 2 and 3 reconstruct root
//! order and its history from their instances. Reading never writes; the next write is version
//! 4. Other versions are refused.

use std::collections::BTreeSet;

use chemifree_application::{Draft, DraftInfo};
use chemifree_domain::document::{
    Atom, AtomId, Bond, BondId, BondOrder, Change, ChangeSet, Document, DocumentId, DocumentParts,
    MOLECULE_ID, ObjectId, PAGE_ID, Point, PositionKey,
};
use chemifree_domain::element::Element;
use chemifree_domain::scene::{
    Graphic, GraphicShape, ReferenceTarget, SceneObject, SceneReference, Selection,
};
use chemifree_domain::states::{
    AtomKind, AttachmentPoints, BondQuery, Hydrogens, QueryAtom, QueryBondOrder, Radical,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

const FORMAT: &str = "chemifree.document";
const FORMAT_VERSION: u32 = 4;
const DRAFT_FORMAT: &str = "chemifree.draft";
const DRAFT_FORMAT_VERSION: u32 = 4;
/// The version before graph and scene were separated; read and migrated, never written.
const VERSION_1: u32 = 1;
/// The version before recorded hydrogen, radical, query, and attachment states; read and
/// migrated, never written.
const VERSION_2: u32 = 2;
const VERSION_3: u32 = 3;
/// The one instance a version 1 document becomes.
const MIGRATED_INSTANCE: u32 = 1;

/// Version 4 records the tree, positions and selection separately, including in history.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SceneFile {
    format: String,
    format_version: u32,
    document: SceneDocument,
    history: SceneHistory,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SceneDraftFile {
    format: String,
    format_version: u32,
    base_revision: u64,
    saved_at_ms: u64,
    document: SceneDocument,
    history: SceneHistory,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SceneDocument {
    id: String,
    revision: u64,
    title: String,
    molecule: StoredMolecule<AtomState, BondState>,
    pages: Vec<StoredPage>,
    roots: Vec<String>,
    objects: Vec<Stored<SceneState>>,
    references: Vec<StoredReference>,
    positions: Vec<ScenePosition>,
    selection: StoredSelection,
    next_atom: u32,
    next_bond: u32,
    next_object: u32,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum SceneState {
    Instance {
        molecule: String,
    },
    Group {
        children: Vec<String>,
    },
    Graphic {
        shape: StoredShape,
        points: [StoredPoint; 2],
    },
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum StoredShape {
    Line,
    Rectangle,
    Arrow,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ScenePosition {
    instance: String,
    atom: String,
    x: f64,
    y: f64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredSelection {
    objects: Vec<String>,
    atoms: Vec<String>,
    bonds: Vec<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredReference {
    source: String,
    target: StoredReferenceTarget,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum StoredReferenceTarget {
    Object { id: String },
    Atom { id: String },
    Bond { id: String },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredReferenceChange {
    reference: StoredReference,
    before: bool,
    after: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SceneHistory {
    undo: Vec<SceneChangeSet>,
    redo: Vec<SceneChangeSet>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SceneChangeSet {
    title: Option<StoredChange<String>>,
    atoms: Vec<Keyed<AtomState>>,
    bonds: Vec<Keyed<BondState>>,
    roots: Option<StoredChange<Vec<String>>>,
    objects: Vec<Keyed<SceneState>>,
    references: Vec<StoredReferenceChange>,
    positions: Vec<PositionChange>,
    selection: StoredChange<StoredSelection>,
}

/// A document file whose atom and bond states are `A` and `B`; versions 2 and 3 differ only
/// there.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct File<A, B> {
    format: String,
    format_version: u32,
    document: StoredDocument<A, B>,
    history: StoredHistory<A, B>,
}

/// A recovery draft: the working copy with the stored revision it started from.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DraftFile<A, B> {
    format: String,
    format_version: u32,
    base_revision: u64,
    saved_at_ms: u64,
    document: StoredDocument<A, B>,
    history: StoredHistory<A, B>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredDocument<A, B> {
    id: String,
    revision: u64,
    title: String,
    molecule: StoredMolecule<A, B>,
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
struct StoredMolecule<A, B> {
    id: String,
    atoms: Vec<Stored<A>>,
    bonds: Vec<Stored<B>>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Stored<T> {
    id: String,
    state: T,
}

/// A version 3 atom: exactly one of `element` and `query`.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AtomState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    element: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    query: Option<StoredQuery>,
    charge: i8,
    isotope: Option<u16>,
    hydrogens: StoredHydrogens,
    radical: StoredRadical,
    attachment_points: Vec<u8>,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum StoredQuery {
    Any {},
    AnyHeavy {},
    Heteroatom {},
    Halogen {},
    List {
        elements: Vec<String>,
        negated: bool,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
enum StoredHydrogens {
    Inferred {},
    Unknown {},
    Specified { count: u8 },
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum StoredRadical {
    None,
    Singlet,
    Doublet,
    Triplet,
    Unknown,
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

/// A version 3 bond: `query` is present exactly when `order` is `query`.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct BondState {
    begin: String,
    end: String,
    order: StoredOrder,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    query: Option<StoredBondQuery>,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum StoredOrder {
    Single,
    Double,
    Triple,
    Aromatic,
    Dative,
    Query,
    Unknown,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum StoredBondQuery {
    Any {},
    List { orders: Vec<StoredQueryOrder> },
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum StoredQueryOrder {
    Single,
    Double,
    Triple,
    Aromatic,
}

/// `undo` is oldest first; `redo` is a stack whose last entry is redone next.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredHistory<A, B> {
    undo: Vec<StoredChangeSet<A, B>>,
    redo: Vec<StoredChangeSet<A, B>>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredChangeSet<A, B> {
    title: Option<StoredChange<String>>,
    atoms: Vec<Keyed<A>>,
    bonds: Vec<Keyed<B>>,
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

/// A stored atom state of any version, as a domain atom.
trait AtomRecord {
    fn into_atom(self) -> Result<Atom, String>;
}

/// A stored bond state of any version, as a domain bond.
trait BondRecord {
    fn into_bond(self) -> Result<Bond, String>;
}

/// The version 2 atom and bond states, read for migration only: an element atom with its
/// charge and isotope, and a single, double, or triple bond. Everything else takes its
/// default.
mod v2 {
    use serde::Deserialize;

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct AtomState {
        pub element: String,
        pub charge: i8,
        pub isotope: Option<u16>,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct BondState {
        pub begin: String,
        pub end: String,
        pub order: Order,
    }

    #[derive(Clone, Copy, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub(super) enum Order {
        Single,
        Double,
        Triple,
    }
}

/// The version 1 shapes, read for migration only. The header fields are checked by
/// [`version`] before these are parsed.
mod v1 {
    use serde::Deserialize;
    use serde::de::IgnoredAny;

    use super::{Keyed, Stored, StoredChange, StoredPoint, v2};

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
        pub bonds: Vec<Stored<v2::BondState>>,
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
        pub bonds: Vec<Keyed<v2::BondState>>,
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
    Three,
    Two,
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
        VERSION_3 => Ok(Version::Three),
        VERSION_2 => Ok(Version::Two),
        VERSION_1 => Ok(Version::One),
        version => Err(format!(
            "unsupported {kind} format {format:?} version {version}; this build reads versions \
             {VERSION_1}, {VERSION_2}, {VERSION_3}, and {current}"
        )),
    }
}

fn parse<T: DeserializeOwned>(bytes: &[u8], kind: &str) -> Result<T, String> {
    serde_json::from_slice(bytes).map_err(|error| format!("invalid {kind} file: {error}"))
}

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
        id: parts.id.to_string(),
        revision: parts.revision,
        title: parts.title,
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
        selection: stored_selection(&parts.selection),
        next_atom: parts.next_atom,
        next_bond: parts.next_bond,
        next_object: parts.next_object,
    };
    let history = SceneHistory {
        undo: parts.undo.iter().map(stored_scene_change_set).collect(),
        redo: parts.redo.iter().map(stored_scene_change_set).collect(),
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

/// The earlier version of a document file this build reads (1, 2, or 3), which a save
/// replaces only after keeping a copy of it; `None` otherwise.
pub(crate) fn earlier_version(bytes: &[u8]) -> Option<u32> {
    match version(bytes, FORMAT, FORMAT_VERSION, "document") {
        Ok(Version::One) => Some(VERSION_1),
        Ok(Version::Two) => Some(VERSION_2),
        Ok(Version::Three) => Some(VERSION_3),
        Ok(Version::Current) | Err(_) => None,
    }
}

/// Parses and fully validates a stored document, including its history; a version 1, 2, or 3
/// file is migrated.
pub(crate) fn decode(bytes: &[u8]) -> Result<Document, String> {
    let parts = match version(bytes, FORMAT, FORMAT_VERSION, "document")? {
        Version::Current => {
            let file: SceneFile = parse(bytes, "document")?;
            scene_parts(file.document, file.history)?
        }
        Version::Three => {
            let file: File<AtomState, BondState> = parse(bytes, "document")?;
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
    Document::restore(parts).map_err(|error| error.to_string())
}

/// Parses and fully validates a recovery draft; a version 1, 2, or 3 draft is migrated.
pub(crate) fn decode_draft(bytes: &[u8]) -> Result<Draft, String> {
    let (parts, base_revision, saved_at_ms) =
        match version(bytes, DRAFT_FORMAT, DRAFT_FORMAT_VERSION, "draft")? {
            Version::Current => {
                let file: SceneDraftFile = parse(bytes, "draft")?;
                let parts = scene_parts(file.document, file.history)?;
                (parts, file.base_revision, file.saved_at_ms)
            }
            Version::Three => {
                let file: DraftFile<AtomState, BondState> = parse(bytes, "draft")?;
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
    Ok(DocumentParts {
        id: document_id(&stored.id)?,
        revision: stored.revision,
        title: stored.title,
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
            .map(|object| Ok((instance_id(&object.id)?, read_scene_state(object.state)?)))
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
        selection: read_selection(stored.selection)?,
        next_atom: stored.next_atom,
        next_bond: stored.next_bond,
        next_object: stored.next_object,
        undo: history
            .undo
            .into_iter()
            .map(read_scene_change_set)
            .collect::<Result<_, _>>()?,
        redo: history
            .redo
            .into_iter()
            .map(read_scene_change_set)
            .collect::<Result<_, _>>()?,
    })
}

fn stored_scene_state(object: &SceneObject) -> SceneState {
    match object {
        SceneObject::Instance => SceneState::Instance {
            molecule: MOLECULE_ID.to_owned(),
        },
        SceneObject::Group { children } => SceneState::Group {
            children: children.iter().map(ToString::to_string).collect(),
        },
        SceneObject::Graphic(graphic) => SceneState::Graphic {
            shape: match graphic.shape {
                GraphicShape::Line => StoredShape::Line,
                GraphicShape::Rectangle => StoredShape::Rectangle,
                GraphicShape::Arrow => StoredShape::Arrow,
            },
            points: graphic.points.each_ref().map(stored_point),
        },
    }
}

fn stored_reference(reference: &SceneReference) -> StoredReference {
    let target = match reference.target {
        ReferenceTarget::Object(id) => StoredReferenceTarget::Object { id: id.to_string() },
        ReferenceTarget::Atom(id) => StoredReferenceTarget::Atom { id: id.to_string() },
        ReferenceTarget::Bond(id) => StoredReferenceTarget::Bond { id: id.to_string() },
    };
    StoredReference {
        source: reference.source.to_string(),
        target,
    }
}

fn read_reference(reference: StoredReference) -> Result<SceneReference, String> {
    let target = match reference.target {
        StoredReferenceTarget::Object { id } => ReferenceTarget::Object(instance_id(&id)?),
        StoredReferenceTarget::Atom { id } => ReferenceTarget::Atom(atom_id(&id)?),
        StoredReferenceTarget::Bond { id } => ReferenceTarget::Bond(bond_id(&id)?),
    };
    Ok(SceneReference {
        source: instance_id(&reference.source)?,
        target,
    })
}

fn read_scene_state(object: SceneState) -> Result<SceneObject, String> {
    Ok(match object {
        SceneState::Instance { molecule } => {
            parents(PAGE_ID, &molecule)?;
            SceneObject::Instance
        }
        SceneState::Group { children } => SceneObject::Group {
            children: children
                .iter()
                .map(|id| instance_id(id))
                .collect::<Result<_, _>>()?,
        },
        SceneState::Graphic { shape, points } => {
            let [first, second] = points;
            SceneObject::Graphic(Graphic {
                shape: match shape {
                    StoredShape::Line => GraphicShape::Line,
                    StoredShape::Rectangle => GraphicShape::Rectangle,
                    StoredShape::Arrow => GraphicShape::Arrow,
                },
                points: [point(first.x, first.y)?, point(second.x, second.y)?],
            })
        }
    })
}

fn stored_selection(selection: &Selection) -> StoredSelection {
    StoredSelection {
        objects: selection.objects.iter().map(ToString::to_string).collect(),
        atoms: selection.atoms.iter().map(ToString::to_string).collect(),
        bonds: selection.bonds.iter().map(ToString::to_string).collect(),
    }
}

fn read_selection(selection: StoredSelection) -> Result<Selection, String> {
    fn distinct<T: Ord>(
        values: Vec<String>,
        parse: fn(&str) -> Result<T, String>,
    ) -> Result<BTreeSet<T>, String> {
        let mut parsed = BTreeSet::new();
        for value in values {
            if !parsed.insert(parse(&value)?) {
                return Err(format!("selection repeats identifier {value:?}"));
            }
        }
        Ok(parsed)
    }
    Ok(Selection {
        objects: distinct(selection.objects, instance_id)?,
        atoms: distinct(selection.atoms, atom_id)?,
        bonds: distinct(selection.bonds, bond_id)?,
    })
}

fn read_scene_change_set(stored: SceneChangeSet) -> Result<ChangeSet, String> {
    let mut set = to_change_set(StoredChangeSet {
        title: stored.title,
        atoms: stored.atoms,
        bonds: stored.bonds,
        instances: Vec::new(),
        positions: stored.positions,
    })?;
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
                    before: object.before.map(read_scene_state).transpose()?,
                    after: object.after.map(read_scene_state).transpose()?,
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
    set.selection = Change {
        before: read_selection(stored.selection.before)?,
        after: read_selection(stored.selection.after)?,
    };
    Ok(set)
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

fn parts<A: AtomRecord, B: BondRecord>(
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
        id: document_id(&stored.id)?,
        revision: stored.revision,
        title: stored.title,
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
        selection: Selection::default(),
        next_atom: stored.next_atom,
        next_bond: stored.next_bond,
        next_object: stored.next_instance,
        undo,
        redo,
    })
}

/// A version 1 document as current parts: the atoms' positions move into instance `s1`, and
/// each history entry's atom changes split into graph changes and `s1` position changes (an
/// entry that only moved atoms becomes position changes only). `Document::restore` then
/// checks the result as it checks any stored document.
fn migrate(stored: v1::Document, history: v1::History) -> Result<DocumentParts, String> {
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
        id: document_id(&stored.id)?,
        revision: stored.revision,
        title: stored.title,
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
        selection: Selection::default(),
        next_atom: stored.next_atom,
        next_bond: stored.next_bond,
        next_object: MIGRATED_INSTANCE + 1,
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

impl AtomRecord for v2::AtomState {
    /// An element atom with inferred hydrogens, no radical, and no attachment points.
    fn into_atom(self) -> Result<Atom, String> {
        Ok(Atom {
            charge: self.charge,
            isotope: self.isotope,
            ..Atom::from(element(&self.element)?)
        })
    }
}

impl BondRecord for v2::BondState {
    fn into_bond(self) -> Result<Bond, String> {
        Ok(Bond {
            begin: atom_id(&self.begin)?,
            end: atom_id(&self.end)?,
            order: match self.order {
                v2::Order::Single => BondOrder::Single,
                v2::Order::Double => BondOrder::Double,
                v2::Order::Triple => BondOrder::Triple,
            },
        })
    }
}

impl AtomRecord for AtomState {
    fn into_atom(self) -> Result<Atom, String> {
        let kind = match (self.element, self.query) {
            (Some(symbol), None) => AtomKind::Element(element(&symbol)?),
            (None, Some(query)) => AtomKind::Query(query_atom(query)?),
            _ => return Err("an atom has exactly one of element and query".to_owned()),
        };
        Ok(Atom {
            kind,
            charge: self.charge,
            isotope: self.isotope,
            hydrogens: match self.hydrogens {
                StoredHydrogens::Inferred {} => Hydrogens::Inferred,
                StoredHydrogens::Unknown {} => Hydrogens::Unknown,
                StoredHydrogens::Specified { count } => Hydrogens::Specified(count),
            },
            radical: match self.radical {
                StoredRadical::None => Radical::None,
                StoredRadical::Singlet => Radical::Singlet,
                StoredRadical::Doublet => Radical::Doublet,
                StoredRadical::Triplet => Radical::Triplet,
                StoredRadical::Unknown => Radical::Unknown,
            },
            // The same rule as an edit's; the rest is checked by `Document::restore`.
            attachment_points: AttachmentPoints::new(&self.attachment_points)
                .map_err(|error| error.to_string())?,
        })
    }
}

impl BondRecord for BondState {
    fn into_bond(self) -> Result<Bond, String> {
        let order = match (self.order, self.query) {
            (StoredOrder::Query, Some(query)) => BondOrder::Query(match query {
                StoredBondQuery::Any {} => BondQuery::Any,
                StoredBondQuery::List { orders } => BondQuery::OneOf(
                    orders
                        .into_iter()
                        .map(|order| match order {
                            StoredQueryOrder::Single => QueryBondOrder::Single,
                            StoredQueryOrder::Double => QueryBondOrder::Double,
                            StoredQueryOrder::Triple => QueryBondOrder::Triple,
                            StoredQueryOrder::Aromatic => QueryBondOrder::Aromatic,
                        })
                        .collect(),
                ),
            }),
            (StoredOrder::Query, None) => return Err("a query bond needs its query".to_owned()),
            (_, Some(_)) => return Err("only a query bond has a query".to_owned()),
            (StoredOrder::Single, None) => BondOrder::Single,
            (StoredOrder::Double, None) => BondOrder::Double,
            (StoredOrder::Triple, None) => BondOrder::Triple,
            (StoredOrder::Aromatic, None) => BondOrder::Aromatic,
            (StoredOrder::Dative, None) => BondOrder::Dative,
            (StoredOrder::Unknown, None) => BondOrder::Unknown,
        };
        Ok(Bond {
            begin: atom_id(&self.begin)?,
            end: atom_id(&self.end)?,
            order,
        })
    }
}

fn query_atom(query: StoredQuery) -> Result<QueryAtom, String> {
    Ok(match query {
        StoredQuery::Any {} => QueryAtom::Any,
        StoredQuery::AnyHeavy {} => QueryAtom::AnyHeavy,
        StoredQuery::Heteroatom {} => QueryAtom::Heteroatom,
        StoredQuery::Halogen {} => QueryAtom::Halogen,
        StoredQuery::List { elements, negated } => QueryAtom::List {
            elements: elements
                .iter()
                .map(|symbol| element(symbol))
                .collect::<Result<_, _>>()?,
            negated,
        },
    })
}

fn atom_state(atom: &Atom) -> AtomState {
    let (element, query) = match &atom.kind {
        AtomKind::Element(element) => (Some(element.symbol().to_owned()), None),
        AtomKind::Query(query) => (None, Some(stored_query(query))),
    };
    AtomState {
        element,
        query,
        charge: atom.charge,
        isotope: atom.isotope,
        hydrogens: match atom.hydrogens {
            Hydrogens::Inferred => StoredHydrogens::Inferred {},
            Hydrogens::Unknown => StoredHydrogens::Unknown {},
            Hydrogens::Specified(count) => StoredHydrogens::Specified { count },
        },
        radical: match atom.radical {
            Radical::None => StoredRadical::None,
            Radical::Singlet => StoredRadical::Singlet,
            Radical::Doublet => StoredRadical::Doublet,
            Radical::Triplet => StoredRadical::Triplet,
            Radical::Unknown => StoredRadical::Unknown,
        },
        attachment_points: atom.attachment_points.values().collect(),
    }
}

fn stored_query(query: &QueryAtom) -> StoredQuery {
    match query {
        QueryAtom::Any => StoredQuery::Any {},
        QueryAtom::AnyHeavy => StoredQuery::AnyHeavy {},
        QueryAtom::Heteroatom => StoredQuery::Heteroatom {},
        QueryAtom::Halogen => StoredQuery::Halogen {},
        QueryAtom::List { elements, negated } => StoredQuery::List {
            elements: elements
                .iter()
                .map(|element| element.symbol().to_owned())
                .collect(),
            negated: *negated,
        },
    }
}

fn bond_state(bond: &Bond) -> BondState {
    let (order, query) = match &bond.order {
        BondOrder::Single => (StoredOrder::Single, None),
        BondOrder::Double => (StoredOrder::Double, None),
        BondOrder::Triple => (StoredOrder::Triple, None),
        BondOrder::Aromatic => (StoredOrder::Aromatic, None),
        BondOrder::Dative => (StoredOrder::Dative, None),
        BondOrder::Unknown => (StoredOrder::Unknown, None),
        BondOrder::Query(BondQuery::Any) => (StoredOrder::Query, Some(StoredBondQuery::Any {})),
        BondOrder::Query(BondQuery::OneOf(orders)) => (
            StoredOrder::Query,
            Some(StoredBondQuery::List {
                orders: orders
                    .iter()
                    .map(|order| match order {
                        QueryBondOrder::Single => StoredQueryOrder::Single,
                        QueryBondOrder::Double => StoredQueryOrder::Double,
                        QueryBondOrder::Triple => StoredQueryOrder::Triple,
                        QueryBondOrder::Aromatic => StoredQueryOrder::Aromatic,
                    })
                    .collect(),
            }),
        ),
    };
    BondState {
        begin: bond.begin.to_string(),
        end: bond.end.to_string(),
        order,
        query,
    }
}

fn stored_point(point: &Point) -> StoredPoint {
    StoredPoint {
        x: point.x(),
        y: point.y(),
    }
}

fn stored_scene_change_set(set: &ChangeSet) -> SceneChangeSet {
    SceneChangeSet {
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

fn to_change_set<A: AtomRecord, B: BondRecord>(
    stored: StoredChangeSet<A, B>,
) -> Result<ChangeSet, String> {
    let to_instance = |state: InstanceState| {
        parents(&state.page, &state.molecule).map(|()| SceneObject::Instance)
    };
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

fn element(symbol: &str) -> Result<Element, String> {
    Element::from_symbol(symbol).ok_or_else(|| format!("unknown element {symbol:?}"))
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

fn instance_id(value: &str) -> Result<ObjectId, String> {
    ObjectId::parse(value).ok_or_else(|| format!("invalid scene object identifier {value:?}"))
}
