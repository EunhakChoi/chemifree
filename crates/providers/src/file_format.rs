//! Chemifree document file format, version 3 (`*.json`), and the recovery draft format that
//! wraps the same document and history. This is Chemifree's own working format, not an
//! interchange format such as MOL or CDXML.
//!
//! Version 3 stores the chemical graph (`molecule`), the page, and the molecule scene
//! instances with their atom positions apart, and history entries record graph, instance,
//! and position changes apart. Each atom keeps its kind (element or query), charge, isotope,
//! hydrogen state, radical, and attachment points; each bond its order, and a query bond its
//! expression. Earlier versions are still read and migrated in memory, keeping identifiers,
//! revision, title, and undo/redo history:
//!
//! - version 2 (graph and scene apart, atoms with element, charge, and isotope only, bonds
//!   single, double, or triple): every atom gets inferred hydrogens, no radical, and no
//!   attachment points; bonds keep their order;
//! - version 1 (one position on each atom): additionally, the positions move into one page
//!   `p1` and one instance `s1`.
//!
//! Reading never writes; whatever is written next is version 3. Any other version is refused.

use chemifree_application::{Draft, DraftInfo};
use chemifree_domain::document::{
    Atom, AtomId, AtomKind, AtomQuery, AttachmentPoints, Bond, BondId, BondOrder, BondQuery,
    Change, ChangeSet, Document, DocumentId, DocumentParts, Hydrogens, InstanceId, MOLECULE_ID,
    PAGE_ID, Point, PositionKey, QueryBondOrder, Radical, SceneInstance,
};
use chemifree_domain::element::Element;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

const FORMAT: &str = "chemifree.document";
const FORMAT_VERSION: u32 = 3;
const DRAFT_FORMAT: &str = "chemifree.draft";
const DRAFT_FORMAT_VERSION: u32 = 3;
/// The version before graph and scene were separated; read and migrated, never written.
const VERSION_1: u32 = 1;
/// The version before hydrogen, radical, query, and bond-kind states; read and migrated,
/// never written.
const VERSION_2: u32 = 2;
/// The one instance a version 1 document becomes.
const MIGRATED_INSTANCE: u32 = 1;

/// A document file whose atoms and bonds are stored as `A` and `B`: version 3 shapes when
/// written, earlier shapes when an older file is read.
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

/// An atom or bond with its state.
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
    query: Option<StoredAtomQuery>,
    charge: i8,
    isotope: Option<u16>,
    hydrogens: StoredHydrogens,
    radical: StoredRadical,
    /// In increasing order.
    attachment_points: Vec<u64>,
}

/// `elements` and `negated` are present exactly for kind `list`.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredAtomQuery {
    kind: StoredAtomQueryKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    elements: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    negated: Option<bool>,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum StoredAtomQueryKind {
    Any,
    AnyHeavy,
    Heteroatom,
    Halogen,
    List,
}

/// `count` is present exactly for mode `specified`.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredHydrogens {
    mode: StoredHydrogenMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    count: Option<u8>,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum StoredHydrogenMode {
    Inferred,
    Unknown,
    Specified,
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

/// A version 3 bond: `query` is present exactly for order `query`.
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

/// `orders` is present exactly for kind `list`.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredBondQuery {
    kind: StoredBondQueryKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    orders: Option<Vec<StoredQueryOrder>>,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum StoredBondQueryKind {
    Any,
    List,
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

/// A stored atom or bond state, read into the domain.
trait StoredState: Sized {
    type Value;

    fn into_value(self) -> Result<Self::Value, String>;
}

/// The version 2 atom and bond shapes, read for migration only; version 1 bonds have the
/// same shape. The rest of a version 2 file has the version 3 shape.
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
        pub atoms: Vec<Stored<AtomState>>,
        pub bonds: Vec<Stored<v2::BondState>>,
        pub next_atom: u32,
        pub next_bond: u32,
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
        VERSION_2 => Ok(Version::Two),
        VERSION_1 => Ok(Version::One),
        version => Err(format!(
            "unsupported {kind} format {format:?} version {version}; this build reads versions \
             {VERSION_1}, {VERSION_2}, and {current}"
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

/// Compact JSON: version 3 atom states are long enough that indented history at the entity
/// and history limits would outgrow the read limit (`MAX_DOCUMENT_BYTES`).
fn to_bytes(value: &impl Serialize) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(value).expect("document values are serializable");
    bytes.push(b'\n');
    bytes
}

type CurrentDocument = StoredDocument<AtomState, BondState>;
type CurrentHistory = StoredHistory<AtomState, BondState>;

fn stored(document: &Document) -> (CurrentDocument, CurrentHistory) {
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

/// The version of `bytes` when they are a document file of an earlier version that this
/// build reads (1 or 2), which a save replaces only after keeping a copy of them.
pub(crate) fn earlier_version(bytes: &[u8]) -> Option<u32> {
    match version(bytes, FORMAT, FORMAT_VERSION, "document") {
        Ok(Version::One) => Some(VERSION_1),
        Ok(Version::Two) => Some(VERSION_2),
        Ok(Version::Current) | Err(_) => None,
    }
}

/// Parses and fully validates a stored document, including its history; a version 1 or 2
/// file is migrated.
pub(crate) fn decode(bytes: &[u8]) -> Result<Document, String> {
    let parts = match version(bytes, FORMAT, FORMAT_VERSION, "document")? {
        Version::Current => {
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

/// Parses and fully validates a recovery draft; a version 1 or 2 draft is migrated.
pub(crate) fn decode_draft(bytes: &[u8]) -> Result<Draft, String> {
    let (parts, base_revision, saved_at_ms) =
        match version(bytes, DRAFT_FORMAT, DRAFT_FORMAT_VERSION, "draft")? {
            Version::Current => {
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

fn parts<A, B>(
    stored: StoredDocument<A, B>,
    history: StoredHistory<A, B>,
) -> Result<DocumentParts, String>
where
    A: StoredState<Value = Atom>,
    B: StoredState<Value = Bond>,
{
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
            .map(|atom| Ok((atom_id(&atom.id)?, atom.state.into_value()?)))
            .collect::<Result<_, String>>()?,
        bonds: stored
            .molecule
            .bonds
            .into_iter()
            .map(|bond| Ok((bond_id(&bond.id)?, bond.state.into_value()?)))
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

/// A version 1 document as current parts: the atoms' positions move into instance `s1`, and
/// each history entry's atom changes split into graph changes and `s1` position changes (an
/// entry that only moved atoms becomes position changes only). Atoms get the version 2
/// defaults. `Document::restore` then checks the result as it checks any stored document.
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
            .map(|bond| Ok((bond_id(&bond.id)?, bond.state.into_value()?)))
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
        bonds: keyed_changes(set.bonds, bond_id)?,
        instances: Vec::new(),
        positions,
    })
}

/// A version 1 atom state as the graph atom and its position.
fn split_atom(state: v1::AtomState) -> Result<(Atom, Point), String> {
    let position = point(state.position.x, state.position.y)?;
    let atom = v2::AtomState {
        element: state.element,
        charge: state.charge,
        isotope: state.isotope,
    }
    .into_value()?;
    Ok((atom, position))
}

impl StoredState for v2::AtomState {
    type Value = Atom;

    /// A version 2 atom with what version 2 could not hold: inferred hydrogens, no radical,
    /// and no attachment points.
    fn into_value(self) -> Result<Atom, String> {
        Ok(Atom {
            charge: self.charge,
            isotope: self.isotope,
            ..Atom::from(element(&self.element)?)
        })
    }
}

impl StoredState for v2::BondState {
    type Value = Bond;

    fn into_value(self) -> Result<Bond, String> {
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

impl StoredState for AtomState {
    type Value = Atom;

    /// Ranges, lists, and the query-atom rules are checked by `Document::restore`.
    fn into_value(self) -> Result<Atom, String> {
        let kind = match (self.element, self.query) {
            (Some(symbol), None) => AtomKind::Element(element(&symbol)?),
            (None, Some(query)) => AtomKind::Query(atom_query(query)?),
            _ => return Err("an atom has exactly one of element and query".to_owned()),
        };
        let hydrogens = match (self.hydrogens.mode, self.hydrogens.count) {
            (StoredHydrogenMode::Inferred, None) => Hydrogens::Inferred,
            (StoredHydrogenMode::Unknown, None) => Hydrogens::Unknown,
            (StoredHydrogenMode::Specified, Some(count)) => Hydrogens::Specified(count),
            _ => return Err("only specified hydrogens have a count, and they need one".to_owned()),
        };
        if !self.attachment_points.is_sorted() {
            return Err("attachment points are stored in increasing order".to_owned());
        }
        Ok(Atom {
            kind,
            charge: self.charge,
            isotope: self.isotope,
            hydrogens,
            radical: match self.radical {
                StoredRadical::None => Radical::None,
                StoredRadical::Singlet => Radical::Singlet,
                StoredRadical::Doublet => Radical::Doublet,
                StoredRadical::Triplet => Radical::Triplet,
                StoredRadical::Unknown => Radical::Unknown,
            },
            attachment_points: AttachmentPoints::try_new(&self.attachment_points)
                .map_err(|error| error.to_string())?,
        })
    }
}

fn atom_query(query: StoredAtomQuery) -> Result<AtomQuery, String> {
    Ok(match (query.kind, query.elements, query.negated) {
        (StoredAtomQueryKind::List, Some(elements), Some(negated)) => AtomQuery::List {
            elements: elements
                .iter()
                .map(|symbol| element(symbol))
                .collect::<Result<_, _>>()?,
            negated,
        },
        (StoredAtomQueryKind::Any, None, None) => AtomQuery::Any,
        (StoredAtomQueryKind::AnyHeavy, None, None) => AtomQuery::AnyHeavy,
        (StoredAtomQueryKind::Heteroatom, None, None) => AtomQuery::Heteroatom,
        (StoredAtomQueryKind::Halogen, None, None) => AtomQuery::Halogen,
        _ => {
            return Err(
                "a query atom has elements and negated exactly when its kind is list".to_owned(),
            );
        }
    })
}

impl StoredState for BondState {
    type Value = Bond;

    fn into_value(self) -> Result<Bond, String> {
        let order = match (self.order, self.query) {
            (StoredOrder::Single, None) => BondOrder::Single,
            (StoredOrder::Double, None) => BondOrder::Double,
            (StoredOrder::Triple, None) => BondOrder::Triple,
            (StoredOrder::Aromatic, None) => BondOrder::Aromatic,
            (StoredOrder::Dative, None) => BondOrder::Dative,
            (StoredOrder::Unknown, None) => BondOrder::Unknown,
            (StoredOrder::Query, Some(query)) => {
                BondOrder::Query(match (query.kind, query.orders) {
                    (StoredBondQueryKind::Any, None) => BondQuery::Any,
                    (StoredBondQueryKind::List, Some(orders)) => BondQuery::List(
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
                    _ => {
                        return Err(
                            "a query bond has orders exactly when its kind is list".to_owned()
                        );
                    }
                })
            }
            _ => return Err("a bond has a query exactly when its order is query".to_owned()),
        };
        Ok(Bond {
            begin: atom_id(&self.begin)?,
            end: atom_id(&self.end)?,
            order,
        })
    }
}

fn atom_state(atom: &Atom) -> AtomState {
    let (element, query) = match &atom.kind {
        AtomKind::Element(element) => (Some(element.symbol().to_owned()), None),
        AtomKind::Query(query) => (None, Some(stored_atom_query(query))),
    };
    let (mode, count) = match atom.hydrogens {
        Hydrogens::Inferred => (StoredHydrogenMode::Inferred, None),
        Hydrogens::Unknown => (StoredHydrogenMode::Unknown, None),
        Hydrogens::Specified(count) => (StoredHydrogenMode::Specified, Some(count)),
    };
    AtomState {
        element,
        query,
        charge: atom.charge,
        isotope: atom.isotope,
        hydrogens: StoredHydrogens { mode, count },
        radical: match atom.radical {
            Radical::None => StoredRadical::None,
            Radical::Singlet => StoredRadical::Singlet,
            Radical::Doublet => StoredRadical::Doublet,
            Radical::Triplet => StoredRadical::Triplet,
            Radical::Unknown => StoredRadical::Unknown,
        },
        attachment_points: atom.attachment_points.iter().map(u64::from).collect(),
    }
}

fn stored_atom_query(query: &AtomQuery) -> StoredAtomQuery {
    let kind = match query {
        AtomQuery::Any => StoredAtomQueryKind::Any,
        AtomQuery::AnyHeavy => StoredAtomQueryKind::AnyHeavy,
        AtomQuery::Heteroatom => StoredAtomQueryKind::Heteroatom,
        AtomQuery::Halogen => StoredAtomQueryKind::Halogen,
        AtomQuery::List { elements, negated } => {
            return StoredAtomQuery {
                kind: StoredAtomQueryKind::List,
                elements: Some(
                    elements
                        .iter()
                        .map(|element| element.symbol().to_owned())
                        .collect(),
                ),
                negated: Some(*negated),
            };
        }
    };
    StoredAtomQuery {
        kind,
        elements: None,
        negated: None,
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
        BondOrder::Query(BondQuery::Any) => (
            StoredOrder::Query,
            Some(StoredBondQuery {
                kind: StoredBondQueryKind::Any,
                orders: None,
            }),
        ),
        BondOrder::Query(BondQuery::List(orders)) => (
            StoredOrder::Query,
            Some(StoredBondQuery {
                kind: StoredBondQueryKind::List,
                orders: Some(
                    orders
                        .iter()
                        .map(|order| match order {
                            QueryBondOrder::Single => StoredQueryOrder::Single,
                            QueryBondOrder::Double => StoredQueryOrder::Double,
                            QueryBondOrder::Triple => StoredQueryOrder::Triple,
                            QueryBondOrder::Aromatic => StoredQueryOrder::Aromatic,
                        })
                        .collect(),
                ),
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

fn stored_change_set(set: &ChangeSet) -> StoredChangeSet<AtomState, BondState> {
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

/// Changes of one kind of entity, by identifier, as a change set holds them.
type Changes<K, V> = Vec<(K, Change<Option<V>>)>;

/// Changes of one kind of entity, keyed by `id`, with their states read into the domain.
fn keyed_changes<K, S: StoredState>(
    changes: Vec<Keyed<S>>,
    id: impl Fn(&str) -> Result<K, String>,
) -> Result<Changes<K, S::Value>, String> {
    changes
        .into_iter()
        .map(|keyed| {
            Ok((
                id(&keyed.id)?,
                Change {
                    before: keyed.before.map(S::into_value).transpose()?,
                    after: keyed.after.map(S::into_value).transpose()?,
                },
            ))
        })
        .collect()
}

fn to_change_set<A, B>(stored: StoredChangeSet<A, B>) -> Result<ChangeSet, String>
where
    A: StoredState<Value = Atom>,
    B: StoredState<Value = Bond>,
{
    let to_instance =
        |state: InstanceState| parents(&state.page, &state.molecule).map(|()| SceneInstance);
    let to_point = |stored: StoredPoint| point(stored.x, stored.y);
    Ok(ChangeSet {
        title: stored.title.map(|change| Change {
            before: change.before,
            after: change.after,
        }),
        atoms: keyed_changes(stored.atoms, atom_id)?,
        bonds: keyed_changes(stored.bonds, bond_id)?,
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

fn instance_id(value: &str) -> Result<InstanceId, String> {
    InstanceId::parse(value).ok_or_else(|| format!("invalid scene instance identifier {value:?}"))
}
