//! Chemifree document file format, version 4 (`*.json`), and the recovery draft format that
//! wraps the same document and history. This is Chemifree's own working format, not an
//! interchange format such as MOL or CDXML.
//!
//! Version 4 stores the chemical graph (`molecule`), the page with its ordered root objects, the
//! scene objects (molecule scene instances with their atom positions, groups with their
//! ordered children, and graphics with their two points), and the selection; history entries
//! record graph, object, root order, and position changes apart, with the selection before and
//! after. An atom state records its kind (an element or a query), charge, isotope, hydrogen
//! state, radical, and attachment points; a bond state its order and, for a query bond, its
//! query. Files are compact JSON, so the largest document a save allows, full history
//! included, stays within the read limit.
//!
//! Older versions are still read and migrated in memory, history included: version 3 instances
//! become the page's root objects in order, with an empty selection, and history entries that
//! created or deleted instances also change the root order; version 2 atoms and bonds also get
//! the defaults (an element atom, inferred hydrogens, no radical, no attachment points); and
//! version 1 files, which kept one position on each atom, also move those positions into one
//! page `p1` and one instance `s1`. Reading never writes; whatever is written next is version
//! 4. Any other version is refused.

use chemifree_application::{Draft, DraftInfo};
use std::collections::BTreeSet;
use std::fmt::Display;

use chemifree_domain::document::{
    Atom, AtomId, Bond, BondId, BondOrder, Change, ChangeSet, Document, DocumentId, DocumentParts,
    MOLECULE_ID, ObjectId, PAGE_ID, Point, PositionKey,
};
use chemifree_domain::element::Element;
use chemifree_domain::scene::{Graphic, SceneObject, Selection, Shape};
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
/// The version before groups, graphics, and the selection; read and migrated, never written.
const VERSION_3: u32 = 3;
/// The one instance a version 1 document becomes.
const MIGRATED_INSTANCE: u32 = 1;

/// A version 4 document file.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct File {
    format: String,
    format_version: u32,
    document: StoredDocument,
    history: StoredHistory,
}

/// A version 4 recovery draft: the working copy with the stored revision it started from.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DraftFile {
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
    molecule: StoredMolecule<AtomState, BondState>,
    pages: Vec<StoredPage>,
    /// Every scene object, in identifier order; containment is in `pages[].roots` and the
    /// groups' `children`.
    objects: Vec<StoredObject>,
    selection: StoredSelection,
    next_atom: u32,
    next_bond: u32,
    next_object: u32,
}

/// The page and its root objects in order.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredPage {
    id: String,
    roots: Vec<String>,
}

/// A scene object of the document.
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum StoredObject {
    /// A molecule scene instance with one position per atom of its molecule.
    Molecule {
        id: String,
        molecule: String,
        positions: Vec<StoredPosition>,
    },
    Group {
        id: String,
        children: Vec<String>,
    },
    Graphic {
        id: String,
        shape: StoredShape,
        points: [StoredPoint; 2],
    },
}

/// A scene object's state in a history entry. An instance's positions change through
/// `positions` entries.
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ObjectState {
    Molecule {
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

/// Selected scene objects, atoms, and bonds, each without repeats.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredSelection {
    objects: Vec<String>,
    atoms: Vec<String>,
    bonds: Vec<String>,
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
    objects: Vec<Keyed<ObjectState>>,
    roots: Option<StoredChange<Vec<String>>>,
    positions: Vec<PositionChange>,
    selection: StoredChange<StoredSelection>,
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

/// The version 2 and 3 document layout, read for migration only (the header fields are checked
/// by [`version`] before these are parsed): atoms and bonds with states
/// `A` and `B` (the only difference between the two versions), and molecule scene instances as
/// the page's only objects.
mod v3 {
    use serde::Deserialize;
    use serde::de::IgnoredAny;

    use super::{Keyed, PositionChange, StoredChange, StoredMolecule, StoredPosition};

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct File<A, B> {
        #[serde(rename = "format")]
        pub _format: IgnoredAny,
        #[serde(rename = "format_version")]
        pub _format_version: IgnoredAny,
        pub document: Document<A, B>,
        pub history: History<A, B>,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct DraftFile<A, B> {
        #[serde(rename = "format")]
        pub _format: IgnoredAny,
        #[serde(rename = "format_version")]
        pub _format_version: IgnoredAny,
        pub base_revision: u64,
        pub saved_at_ms: u64,
        pub document: Document<A, B>,
        pub history: History<A, B>,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct Document<A, B> {
        pub id: String,
        pub revision: u64,
        pub title: String,
        pub molecule: StoredMolecule<A, B>,
        pub pages: Vec<Page>,
        pub instances: Vec<Instance>,
        pub next_atom: u32,
        pub next_bond: u32,
        pub next_instance: u32,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct Page {
        pub id: String,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct Instance {
        pub id: String,
        pub page: String,
        pub molecule: String,
        pub positions: Vec<StoredPosition>,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct History<A, B> {
        pub undo: Vec<ChangeSet<A, B>>,
        pub redo: Vec<ChangeSet<A, B>>,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct ChangeSet<A, B> {
        pub title: Option<StoredChange<String>>,
        pub atoms: Vec<Keyed<A>>,
        pub bonds: Vec<Keyed<B>>,
        pub instances: Vec<Keyed<InstanceState>>,
        pub positions: Vec<PositionChange>,
    }

    /// An instance's parents. Its positions change through `positions` entries.
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct InstanceState {
        pub page: String,
        pub molecule: String,
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
    to_bytes(&File {
        format: FORMAT.to_owned(),
        format_version: FORMAT_VERSION,
        document,
        history,
    })
}

pub(crate) fn encode_draft(draft: &Draft) -> Vec<u8> {
    let (document, history) = stored(&draft.document);
    to_bytes(&DraftFile {
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

fn stored(document: &Document) -> (StoredDocument, StoredHistory) {
    let parts = document.to_parts();
    let objects = parts
        .objects
        .iter()
        .map(|(id, object)| match object {
            SceneObject::Molecule => StoredObject::Molecule {
                id: id.to_string(),
                molecule: MOLECULE_ID.to_owned(),
                positions: document
                    .positions(*id)
                    .map(|(atom, point)| StoredPosition {
                        atom: atom.to_string(),
                        x: point.x(),
                        y: point.y(),
                    })
                    .collect(),
            },
            SceneObject::Group(children) => StoredObject::Group {
                id: id.to_string(),
                children: names(children),
            },
            SceneObject::Graphic(graphic) => StoredObject::Graphic {
                id: id.to_string(),
                shape: stored_shape(graphic.shape),
                points: graphic.points.map(|point| stored_point(&point)),
            },
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
            roots: names(&parts.roots),
        }],
        objects,
        selection: stored_selection(&parts.selection),
        next_atom: parts.next_atom,
        next_bond: parts.next_bond,
        next_object: parts.next_object,
    };
    let history = StoredHistory {
        undo: parts.undo.iter().map(stored_change_set).collect(),
        redo: parts.redo.iter().map(stored_change_set).collect(),
    };
    (stored, history)
}

/// Entity changes of one history entry, as `ChangeSet` holds them.
type Changes<K, V> = Vec<(K, Change<Option<V>>)>;

/// A graph's atoms and bonds, as `DocumentParts` holds them.
type Graph = (Vec<(AtomId, Atom)>, Vec<(BondId, Bond)>);

fn names<T: Display>(ids: &[T]) -> Vec<String> {
    ids.iter().map(ToString::to_string).collect()
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

/// The version of `bytes` when they are a document file of an earlier version this build reads
/// (1, 2, or 3), which a save replaces only after keeping a copy of them; `None` otherwise.
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
            let file: File = parse(bytes, "document")?;
            parts(file.document, file.history)?
        }
        Version::Three => {
            let file: v3::File<AtomState, BondState> = parse(bytes, "document")?;
            migrate_v3(file.document, file.history)?
        }
        Version::Two => {
            let file: v3::File<v2::AtomState, v2::BondState> = parse(bytes, "document")?;
            migrate_v3(file.document, file.history)?
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
                let file: DraftFile = parse(bytes, "draft")?;
                let parts = parts(file.document, file.history)?;
                (parts, file.base_revision, file.saved_at_ms)
            }
            Version::Three => {
                let file: v3::DraftFile<AtomState, BondState> = parse(bytes, "draft")?;
                let parts = migrate_v3(file.document, file.history)?;
                (parts, file.base_revision, file.saved_at_ms)
            }
            Version::Two => {
                let file: v3::DraftFile<v2::AtomState, v2::BondState> = parse(bytes, "draft")?;
                let parts = migrate_v3(file.document, file.history)?;
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

/// The one page every version holds.
fn page(id: &str) -> Result<(), String> {
    if id == PAGE_ID {
        Ok(())
    } else {
        Err(format!("a document holds exactly one page, {PAGE_ID:?}"))
    }
}

fn molecule(id: &str) -> Result<(), String> {
    if id == MOLECULE_ID {
        Ok(())
    } else {
        Err(format!(
            "molecule {id:?} is not this document's molecule {MOLECULE_ID:?}"
        ))
    }
}

/// The graph of any version from 2 on, as parts' atoms and bonds.
fn graph<A: AtomRecord, B: BondRecord>(stored: StoredMolecule<A, B>) -> Result<Graph, String> {
    molecule(&stored.id)?;
    let atoms = stored
        .atoms
        .into_iter()
        .map(|atom| Ok((atom_id(&atom.id)?, atom.state.into_atom()?)))
        .collect::<Result<_, String>>()?;
    let bonds = stored
        .bonds
        .into_iter()
        .map(|bond| Ok((bond_id(&bond.id)?, bond.state.into_bond()?)))
        .collect::<Result<_, String>>()?;
    Ok((atoms, bonds))
}

fn parts(stored: StoredDocument, history: StoredHistory) -> Result<DocumentParts, String> {
    let [stored_page] = <[StoredPage; 1]>::try_from(stored.pages)
        .map_err(|_| format!("a document holds exactly one page, {PAGE_ID:?}"))?;
    page(&stored_page.id)?;
    let mut objects = Vec::with_capacity(stored.objects.len());
    let mut positions = Vec::new();
    for object in stored.objects {
        match object {
            StoredObject::Molecule {
                id,
                molecule: name,
                positions: points,
            } => {
                molecule(&name)?;
                let id = object_id(&id)?;
                objects.push((id, SceneObject::Molecule));
                for position in points {
                    let key = PositionKey {
                        instance: id,
                        atom: atom_id(&position.atom)?,
                    };
                    positions.push((key, point(position.x, position.y)?));
                }
            }
            StoredObject::Group { id, children } => {
                objects.push((object_id(&id)?, SceneObject::Group(object_ids(&children)?)));
            }
            StoredObject::Graphic { id, shape, points } => {
                objects.push((
                    object_id(&id)?,
                    SceneObject::Graphic(graphic(shape, points)?),
                ));
            }
        }
    }
    let (atoms, bonds) = graph(stored.molecule)?;
    Ok(DocumentParts {
        id: document_id(&stored.id)?,
        revision: stored.revision,
        title: stored.title,
        atoms,
        bonds,
        objects,
        roots: object_ids(&stored_page.roots)?,
        positions,
        selection: selection(stored.selection)?,
        next_atom: stored.next_atom,
        next_bond: stored.next_bond,
        next_object: stored.next_object,
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

/// A version 2 or 3 document as current parts: its instances become the page's root objects in
/// the order stored (identifier order), nothing is selected, and each history entry that
/// created or deleted instances also changes the root order, which is worked out by replaying
/// the instance changes from the stored state. `Document::restore` then checks the result as it
/// checks any stored document.
fn migrate_v3<A: AtomRecord, B: BondRecord>(
    stored: v3::Document<A, B>,
    history: v3::History<A, B>,
) -> Result<DocumentParts, String> {
    let [stored_page] = <[v3::Page; 1]>::try_from(stored.pages)
        .map_err(|_| format!("a document holds exactly one page, {PAGE_ID:?}"))?;
    page(&stored_page.id)?;
    let mut objects = Vec::with_capacity(stored.instances.len());
    let mut positions = Vec::new();
    for instance in stored.instances {
        instance_parents(&instance.page, &instance.molecule)?;
        let id = object_id(&instance.id)?;
        objects.push((id, SceneObject::Molecule));
        for position in instance.positions {
            let key = PositionKey {
                instance: id,
                atom: atom_id(&position.atom)?,
            };
            positions.push((key, point(position.x, position.y)?));
        }
    }
    let roots: Vec<ObjectId> = objects.iter().map(|(id, _)| *id).collect();
    let mut undo = history
        .undo
        .into_iter()
        .map(v3_change_set)
        .collect::<Result<Vec<_>, _>>()?;
    let mut redo = history
        .redo
        .into_iter()
        .map(v3_change_set)
        .collect::<Result<Vec<_>, _>>()?;
    // Undo entries lead back from the stored roots, newest first; redo entries lead forward
    // from them, the last one first.
    let mut current: BTreeSet<ObjectId> = roots.iter().copied().collect();
    for set in undo.iter_mut().rev() {
        current = root_change(set, current, true);
    }
    let mut current: BTreeSet<ObjectId> = roots.iter().copied().collect();
    for set in redo.iter_mut().rev() {
        current = root_change(set, current, false);
    }
    let (atoms, bonds) = graph(stored.molecule)?;
    Ok(DocumentParts {
        id: document_id(&stored.id)?,
        revision: stored.revision,
        title: stored.title,
        atoms,
        bonds,
        objects,
        roots,
        positions,
        selection: Selection::default(),
        next_atom: stored.next_atom,
        next_bond: stored.next_bond,
        next_object: stored.next_instance,
        undo,
        redo,
    })
}

/// Records on `set` how a version 3 entry changed the page's roots, which were exactly its
/// instances in identifier order, and returns the roots on the entry's other side. `current` is
/// the side reached so far: after the entry when walking `backward` through undo entries,
/// before it when walking forward through redo entries.
fn root_change(
    set: &mut ChangeSet,
    current: BTreeSet<ObjectId>,
    backward: bool,
) -> BTreeSet<ObjectId> {
    let mut other = current.clone();
    for (id, change) in &set.objects {
        let (from, to) = if backward {
            (&change.after, &change.before)
        } else {
            (&change.before, &change.after)
        };
        match (from, to) {
            (Some(_), None) => other.remove(id),
            (None, Some(_)) => other.insert(*id),
            _ => false,
        };
    }
    if other != current {
        let (before, after) = if backward {
            (&other, &current)
        } else {
            (&current, &other)
        };
        set.roots = Some(Change {
            before: before.iter().copied().collect(),
            after: after.iter().copied().collect(),
        });
    }
    other
}

/// A version 2 or 3 history entry: its instance changes become molecule object changes, and
/// the selection stays empty on both sides. `migrate_v3` adds the root order changes.
fn v3_change_set<A: AtomRecord, B: BondRecord>(
    stored: v3::ChangeSet<A, B>,
) -> Result<ChangeSet, String> {
    let to_instance = |state: v3::InstanceState| {
        instance_parents(&state.page, &state.molecule).map(|()| SceneObject::Molecule)
    };
    Ok(ChangeSet {
        title: stored.title.map(|change| Change {
            before: change.before,
            after: change.after,
        }),
        atoms: atom_changes(stored.atoms)?,
        bonds: bond_changes(stored.bonds)?,
        objects: stored
            .instances
            .into_iter()
            .map(|keyed| {
                Ok((
                    object_id(&keyed.id)?,
                    Change {
                        before: keyed.before.map(to_instance).transpose()?,
                        after: keyed.after.map(to_instance).transpose()?,
                    },
                ))
            })
            .collect::<Result<_, String>>()?,
        roots: None,
        positions: position_changes(stored.positions)?,
        selection: Change::default(),
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
        objects: vec![(instance, SceneObject::Molecule)],
        roots: vec![instance],
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
        objects: Vec::new(),
        roots: None,
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

fn stored_shape(shape: Shape) -> StoredShape {
    match shape {
        Shape::Line => StoredShape::Line,
        Shape::Rectangle => StoredShape::Rectangle,
        Shape::Arrow => StoredShape::Arrow,
    }
}

fn stored_selection(selection: &Selection) -> StoredSelection {
    StoredSelection {
        objects: selection.objects.iter().map(ToString::to_string).collect(),
        atoms: selection.atoms.iter().map(ToString::to_string).collect(),
        bonds: selection.bonds.iter().map(ToString::to_string).collect(),
    }
}

fn object_state(object: &SceneObject) -> ObjectState {
    match object {
        SceneObject::Molecule => ObjectState::Molecule {
            molecule: MOLECULE_ID.to_owned(),
        },
        SceneObject::Group(children) => ObjectState::Group {
            children: names(children),
        },
        SceneObject::Graphic(graphic) => ObjectState::Graphic {
            shape: stored_shape(graphic.shape),
            points: graphic.points.map(|point| stored_point(&point)),
        },
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
        objects: set
            .objects
            .iter()
            .map(|(id, change)| Keyed {
                id: id.to_string(),
                before: change.before.as_ref().map(object_state),
                after: change.after.as_ref().map(object_state),
            })
            .collect(),
        roots: set.roots.as_ref().map(|change| StoredChange {
            before: names(&change.before),
            after: names(&change.after),
        }),
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
        selection: StoredChange {
            before: stored_selection(&set.selection.before),
            after: stored_selection(&set.selection.after),
        },
    }
}

fn atom_changes<A: AtomRecord>(stored: Vec<Keyed<A>>) -> Result<Changes<AtomId, Atom>, String> {
    stored
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
        .collect()
}

fn bond_changes<B: BondRecord>(stored: Vec<Keyed<B>>) -> Result<Changes<BondId, Bond>, String> {
    stored
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
        .collect()
}

fn position_changes(stored: Vec<PositionChange>) -> Result<Changes<PositionKey, Point>, String> {
    let to_point = |stored: StoredPoint| point(stored.x, stored.y);
    stored
        .into_iter()
        .map(|change| {
            let key = PositionKey {
                instance: object_id(&change.instance)?,
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
        .collect()
}

fn to_object(state: ObjectState) -> Result<SceneObject, String> {
    Ok(match state {
        ObjectState::Molecule { molecule: name } => {
            molecule(&name)?;
            SceneObject::Molecule
        }
        ObjectState::Group { children } => SceneObject::Group(object_ids(&children)?),
        ObjectState::Graphic { shape, points } => SceneObject::Graphic(graphic(shape, points)?),
    })
}

fn to_change_set(stored: StoredChangeSet) -> Result<ChangeSet, String> {
    Ok(ChangeSet {
        title: stored.title.map(|change| Change {
            before: change.before,
            after: change.after,
        }),
        atoms: atom_changes(stored.atoms)?,
        bonds: bond_changes(stored.bonds)?,
        objects: stored
            .objects
            .into_iter()
            .map(|keyed| {
                Ok((
                    object_id(&keyed.id)?,
                    Change {
                        before: keyed.before.map(to_object).transpose()?,
                        after: keyed.after.map(to_object).transpose()?,
                    },
                ))
            })
            .collect::<Result<_, String>>()?,
        roots: stored
            .roots
            .map(|change| {
                Ok::<_, String>(Change {
                    before: object_ids(&change.before)?,
                    after: object_ids(&change.after)?,
                })
            })
            .transpose()?,
        positions: position_changes(stored.positions)?,
        selection: Change {
            before: selection(stored.selection.before)?,
            after: selection(stored.selection.after)?,
        },
    })
}

fn graphic(shape: StoredShape, points: [StoredPoint; 2]) -> Result<Graphic, String> {
    let [first, second] = points;
    Ok(Graphic {
        shape: match shape {
            StoredShape::Line => Shape::Line,
            StoredShape::Rectangle => Shape::Rectangle,
            StoredShape::Arrow => Shape::Arrow,
        },
        points: [point(first.x, first.y)?, point(second.x, second.y)?],
    })
}

/// A stored selection; an identifier listed twice is refused.
fn selection(stored: StoredSelection) -> Result<Selection, String> {
    fn distinct<T: Ord>(
        values: &[String],
        parse: impl Fn(&str) -> Result<T, String>,
    ) -> Result<BTreeSet<T>, String> {
        let set = values
            .iter()
            .map(|value| parse(value))
            .collect::<Result<BTreeSet<T>, String>>()?;
        if set.len() == values.len() {
            Ok(set)
        } else {
            Err("a selection lists an identifier twice".to_owned())
        }
    }
    Ok(Selection {
        objects: distinct(&stored.objects, object_id)?,
        atoms: distinct(&stored.atoms, atom_id)?,
        bonds: distinct(&stored.bonds, bond_id)?,
    })
}

/// Checks that a version 2 or 3 instance is on the document's page and draws its molecule.
fn instance_parents(page: &str, molecule: &str) -> Result<(), String> {
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

fn object_id(value: &str) -> Result<ObjectId, String> {
    ObjectId::parse(value).ok_or_else(|| format!("invalid scene object identifier {value:?}"))
}

fn object_ids(values: &[String]) -> Result<Vec<ObjectId>, String> {
    values.iter().map(|value| object_id(value)).collect()
}
