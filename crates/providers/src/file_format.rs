//! Chemifree document file format, version 1 (`*.json`), and the recovery draft format that
//! wraps the same document and history. This is Chemifree's own working format, not an
//! interchange format such as MOL or CDXML.

use chemifree_application::{Draft, DraftInfo};
use chemifree_domain::document::{
    Atom, AtomId, Bond, BondId, BondOrder, Change, ChangeSet, Document, DocumentId, DocumentParts,
    Point,
};
use chemifree_domain::element::Element;
use serde::{Deserialize, Serialize};

const FORMAT: &str = "chemifree.document";
const FORMAT_VERSION: u32 = 1;
const DRAFT_FORMAT: &str = "chemifree.draft";
const DRAFT_FORMAT_VERSION: u32 = 1;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FileV1 {
    format: String,
    format_version: u32,
    document: StoredDocument,
    history: StoredHistory,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredDocument {
    id: String,
    revision: u64,
    title: String,
    atoms: Vec<StoredAtom>,
    bonds: Vec<StoredBond>,
    next_atom: u32,
    next_bond: u32,
}

/// A recovery draft: the working copy with the stored revision it started from.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DraftV1 {
    format: String,
    format_version: u32,
    base_revision: u64,
    saved_at_ms: u64,
    document: StoredDocument,
    history: StoredHistory,
}

/// Top-level atoms and bonds are `{"id": ..., "state": {...}}`, matching history entries.
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
    position: StoredPoint,
}

#[derive(Deserialize, Serialize)]
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

pub(crate) fn encode(document: &Document) -> Vec<u8> {
    let (document, history) = stored(document);
    to_bytes(&FileV1 {
        format: FORMAT.to_owned(),
        format_version: FORMAT_VERSION,
        document,
        history,
    })
}

pub(crate) fn encode_draft(draft: &Draft) -> Vec<u8> {
    let (document, history) = stored(&draft.document);
    to_bytes(&DraftV1 {
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
    let stored = StoredDocument {
        id: parts.id.to_string(),
        revision: parts.revision,
        title: parts.title,
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
        next_atom: parts.next_atom,
        next_bond: parts.next_bond,
    };
    let history = StoredHistory {
        undo: parts.undo.iter().map(stored_change_set).collect(),
        redo: parts.redo.iter().map(stored_change_set).collect(),
    };
    (stored, history)
}

/// Reads only the header and revision; `decode` remains the validating reader.
pub(crate) fn decode_revision(bytes: &[u8]) -> Result<u64, String> {
    #[derive(Deserialize)]
    struct Probe {
        format: String,
        format_version: u32,
        document: RevisionOnly,
    }
    #[derive(Deserialize)]
    struct RevisionOnly {
        revision: u64,
    }
    let probe: Probe =
        serde_json::from_slice(bytes).map_err(|error| format!("invalid document file: {error}"))?;
    if probe.format != FORMAT || probe.format_version != FORMAT_VERSION {
        return Err(format!(
            "unsupported format {:?} version {}",
            probe.format, probe.format_version
        ));
    }
    Ok(probe.document.revision)
}

/// Parses and fully validates a stored document, including its history.
pub(crate) fn decode(bytes: &[u8]) -> Result<Document, String> {
    let file: FileV1 =
        serde_json::from_slice(bytes).map_err(|error| format!("invalid document file: {error}"))?;
    if file.format != FORMAT || file.format_version != FORMAT_VERSION {
        return Err(format!(
            "unsupported format {:?} version {}",
            file.format, file.format_version
        ));
    }
    restore(file.document, file.history)
}

/// Parses and fully validates a recovery draft.
pub(crate) fn decode_draft(bytes: &[u8]) -> Result<Draft, String> {
    let file: DraftV1 =
        serde_json::from_slice(bytes).map_err(|error| format!("invalid draft file: {error}"))?;
    if file.format != DRAFT_FORMAT || file.format_version != DRAFT_FORMAT_VERSION {
        return Err(format!(
            "unsupported draft format {:?} version {}",
            file.format, file.format_version
        ));
    }
    Ok(Draft {
        document: restore(file.document, file.history)?,
        base_revision: file.base_revision,
        saved_at_ms: file.saved_at_ms,
    })
}

/// Reads a draft's header without restoring its document; `decode_draft` validates.
pub(crate) fn decode_draft_info(bytes: &[u8]) -> Result<DraftInfo, String> {
    #[derive(Deserialize)]
    struct Probe {
        format: String,
        format_version: u32,
        base_revision: u64,
        saved_at_ms: u64,
        document: RevisionOnly,
    }
    #[derive(Deserialize)]
    struct RevisionOnly {
        revision: u64,
    }
    let probe: Probe =
        serde_json::from_slice(bytes).map_err(|error| format!("invalid draft file: {error}"))?;
    if probe.format != DRAFT_FORMAT || probe.format_version != DRAFT_FORMAT_VERSION {
        return Err(format!(
            "unsupported draft format {:?} version {}",
            probe.format, probe.format_version
        ));
    }
    Ok(DraftInfo {
        revision: probe.document.revision,
        base_revision: probe.base_revision,
        saved_at_ms: probe.saved_at_ms,
    })
}

fn restore(stored: StoredDocument, history: StoredHistory) -> Result<Document, String> {
    let parts = DocumentParts {
        id: DocumentId::parse(&stored.id).ok_or("invalid document identifier")?,
        revision: stored.revision,
        title: stored.title,
        atoms: stored
            .atoms
            .into_iter()
            .map(|atom| Ok((atom_id(&atom.id)?, to_atom(atom.state)?)))
            .collect::<Result<_, String>>()?,
        bonds: stored
            .bonds
            .into_iter()
            .map(|bond| Ok((bond_id(&bond.id)?, to_bond(bond.state)?)))
            .collect::<Result<_, String>>()?,
        next_atom: stored.next_atom,
        next_bond: stored.next_bond,
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
    };
    Document::restore(parts).map_err(|error| error.to_string())
}

fn atom_state(atom: &Atom) -> AtomState {
    AtomState {
        element: atom.element.symbol().to_owned(),
        charge: atom.charge,
        isotope: atom.isotope,
        position: StoredPoint {
            x: atom.position.x(),
            y: atom.position.y(),
        },
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
    }
}

fn to_change_set(stored: StoredChangeSet) -> Result<ChangeSet, String> {
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
    })
}

fn to_atom(state: AtomState) -> Result<Atom, String> {
    Ok(Atom {
        element: Element::from_symbol(&state.element)
            .ok_or_else(|| format!("unknown element {:?}", state.element))?,
        charge: state.charge,
        isotope: state.isotope,
        position: Point::new(state.position.x, state.position.y).map_err(|e| e.to_string())?,
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

fn atom_id(value: &str) -> Result<AtomId, String> {
    AtomId::parse(value).ok_or_else(|| format!("invalid atom identifier {value:?}"))
}

fn bond_id(value: &str) -> Result<BondId, String> {
    BondId::parse(value).ok_or_else(|| format!("invalid bond identifier {value:?}"))
}
