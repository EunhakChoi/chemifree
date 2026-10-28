//! v1 document request and response shapes shared by HTTP and the CLI.
//! The browser mirror lives in `apps/web/src/lib/document-contract.ts`.

use chemifree_domain::document::{
    AtomId, AtomRef, BondId, BondOrder, BondTo, Command, Document, MAX_CHARGE, MIN_CHARGE, Point,
    Vector,
};
use chemifree_domain::element::Element;
use serde::{Deserialize, Deserializer, Serialize};

use crate::API_VERSION;
use crate::error::{ApplicationError, ErrorCode};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateDocumentRequest {
    pub request_id: String,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransactionRequest {
    pub request_id: String,
    pub expected_revision: u64,
    pub commands: Vec<CommandDto>,
}

/// Body of undo, redo, and save requests.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevisionRequest {
    pub request_id: String,
    pub expected_revision: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloseRequest {
    pub request_id: String,
    #[serde(default)]
    pub discard_unsaved: bool,
    /// Required with `discard_unsaved`: the revision whose changes the client agreed to lose.
    #[serde(default)]
    pub expected_revision: Option<u64>,
}

/// Body of autosave, recover, and discard-draft requests.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftRequest {
    pub request_id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenameRequest {
    pub request_id: String,
    pub expected_revision: u64,
    pub title: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreRequest {
    pub request_id: String,
}

/// How the document list is narrowed and ordered: `?query=…&sort=title|recent`.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ListQuery {
    /// Text the title must contain, ignoring case; empty lists everything.
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub sort: ListSort,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ListSort {
    /// By title, then identifier.
    #[default]
    Title,
    /// Most recently saved first, then by title.
    Recent,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveAsRequest {
    pub request_id: String,
    pub expected_revision: u64,
    /// Defaults to the source title followed by " (copy)".
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoverAsCopyRequest {
    pub request_id: String,
    /// Defaults to the draft's title followed by " (recovered)".
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PointDto {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BondOrderDto {
    Single,
    Double,
    Triple,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BondToDto {
    pub atom: String,
    pub order: BondOrderDto,
}

/// One command in a transaction, tagged by `op`. Identifiers use the `a1`/`b1` form.
/// Where a new bond names an atom (`bond_to.atom`, `begin`, `end`), `"$N"` also refers to
/// the atom added by the `add_atom` command at index `N` earlier in the same transaction.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum CommandDto {
    AddAtom {
        element: String,
        position: PointDto,
        #[serde(default)]
        bond_to: Option<BondToDto>,
    },
    AddBond {
        begin: String,
        end: String,
        order: BondOrderDto,
    },
    /// Omitted fields are unchanged; `"isotope": null` restores natural abundance.
    UpdateAtom {
        atom: String,
        #[serde(default)]
        element: Option<String>,
        #[serde(default)]
        charge: Option<i64>,
        #[serde(default, deserialize_with = "present")]
        isotope: Option<Option<u64>>,
    },
    MoveAtoms {
        atoms: Vec<String>,
        delta: PointDto,
    },
    UpdateBond {
        bond: String,
        order: BondOrderDto,
    },
    DeleteAtoms {
        atoms: Vec<String>,
    },
    DeleteBonds {
        bonds: Vec<String>,
    },
    Rename {
        title: String,
    },
}

/// Distinguishes an explicit `null` (`Some(None)`) from an absent field (`None`).
fn present<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Option<u64>>, D::Error> {
    Option::<u64>::deserialize(deserializer).map(Some)
}

impl From<BondOrderDto> for BondOrder {
    fn from(order: BondOrderDto) -> Self {
        match order {
            BondOrderDto::Single => Self::Single,
            BondOrderDto::Double => Self::Double,
            BondOrderDto::Triple => Self::Triple,
        }
    }
}

impl From<BondOrder> for BondOrderDto {
    fn from(order: BondOrder) -> Self {
        match order {
            BondOrder::Single => Self::Single,
            BondOrder::Double => Self::Double,
            BondOrder::Triple => Self::Triple,
        }
    }
}

/// Converts wire commands to domain commands, reporting the first malformed field.
pub(crate) fn to_commands(commands: &[CommandDto]) -> Result<Vec<Command>, ApplicationError> {
    commands
        .iter()
        .enumerate()
        .map(|(index, command)| {
            to_command(command).map_err(|message| ApplicationError {
                command_index: Some(index),
                ..ApplicationError::new(
                    ErrorCode::InvalidCommand,
                    format!("command {index}: {message}"),
                )
            })
        })
        .collect()
}

fn to_command(command: &CommandDto) -> Result<Command, String> {
    Ok(match command {
        CommandDto::AddAtom {
            element: symbol,
            position,
            bond_to,
        } => Command::AddAtom {
            element: element(symbol)?,
            position: point(*position)?,
            bond_to: bond_to
                .as_ref()
                .map(|target| {
                    Ok::<_, String>(BondTo {
                        atom: atom_ref(&target.atom)?,
                        order: target.order.into(),
                    })
                })
                .transpose()?,
        },
        CommandDto::AddBond { begin, end, order } => Command::AddBond {
            begin: atom_ref(begin)?,
            end: atom_ref(end)?,
            order: (*order).into(),
        },
        CommandDto::UpdateAtom {
            atom,
            element: symbol,
            charge,
            isotope,
        } => Command::UpdateAtom {
            atom: atom_id(atom)?,
            element: symbol.as_deref().map(element).transpose()?,
            // The domain checks the charge range; this only narrows the wire integer.
            charge: charge
                .map(|charge| {
                    i8::try_from(charge).map_err(|_| {
                        format!("charge {charge} is outside {MIN_CHARGE}..={MAX_CHARGE}")
                    })
                })
                .transpose()?,
            isotope: isotope
                .map(|isotope| {
                    isotope
                        .map(|mass| {
                            u16::try_from(mass)
                                .map_err(|_| format!("isotope mass number {mass} is too large"))
                        })
                        .transpose()
                })
                .transpose()?,
        },
        CommandDto::MoveAtoms { atoms, delta } => Command::MoveAtoms {
            atoms: atoms
                .iter()
                .map(|id| atom_id(id))
                .collect::<Result<_, _>>()?,
            delta: Vector {
                dx: delta.x,
                dy: delta.y,
            },
        },
        CommandDto::UpdateBond { bond, order } => Command::UpdateBond {
            bond: bond_id(bond)?,
            order: (*order).into(),
        },
        CommandDto::DeleteAtoms { atoms } => Command::DeleteAtoms {
            atoms: atoms
                .iter()
                .map(|id| atom_id(id))
                .collect::<Result<_, _>>()?,
        },
        CommandDto::DeleteBonds { bonds } => Command::DeleteBonds {
            bonds: bonds
                .iter()
                .map(|id| bond_id(id))
                .collect::<Result<_, _>>()?,
        },
        CommandDto::Rename { title } => Command::Rename {
            title: title.clone(),
        },
    })
}

fn element(symbol: &str) -> Result<Element, String> {
    Element::from_symbol(symbol).ok_or_else(|| format!("{symbol:?} is not an element symbol"))
}

fn point(position: PointDto) -> Result<Point, String> {
    Point::new(position.x, position.y).map_err(|error| error.to_string())
}

fn atom_id(value: &str) -> Result<AtomId, String> {
    AtomId::parse(value).ok_or_else(|| format!("{value:?} is not an atom identifier"))
}

fn atom_ref(value: &str) -> Result<AtomRef, String> {
    match value.strip_prefix('$') {
        Some(index) if !index.is_empty() && index.bytes().all(|b| b.is_ascii_digit()) => index
            .parse()
            .map(AtomRef::Created)
            .map_err(|_| format!("{value:?} is not a command reference")),
        Some(_) => Err(format!("{value:?} is not a command reference")),
        None => atom_id(value).map(AtomRef::Id),
    }
}

fn bond_id(value: &str) -> Result<BondId, String> {
    BondId::parse(value).ok_or_else(|| format!("{value:?} is not a bond identifier"))
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DocumentResponse {
    pub api_version: &'static str,
    pub document: DocumentView,
    pub session: SessionView,
    /// A recovery draft left by an earlier session of this document, if any.
    pub recovery: Option<RecoveryView>,
}

/// An abandoned recovery draft: unsaved work of a session whose process ended.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RecoveryView {
    pub state: RecoveryState,
    /// The draft's revision, in the numbering of the process that wrote it.
    pub revision: Option<u64>,
    /// The stored revision the draft's session started from.
    pub base_revision: Option<u64>,
    /// Milliseconds since the Unix epoch.
    pub saved_at_ms: Option<u64>,
    /// Why the draft cannot be recovered in place, when it cannot.
    pub message: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryState {
    /// Storage still holds the draft's base: it can be recovered in place.
    Recoverable,
    /// Another writer saved since the draft's base: it can be recovered as a copy.
    Outdated,
    /// The draft cannot be read; it can only be discarded.
    Unreadable,
    /// Another running process owns it.
    InUse,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DocumentView {
    pub id: String,
    pub title: String,
    pub revision: u64,
    pub atoms: Vec<AtomView>,
    pub bonds: Vec<BondView>,
}

/// Positions are scene points; x grows right and y grows down.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AtomView {
    pub id: String,
    pub element: &'static str,
    pub charge: i8,
    pub isotope: Option<u16>,
    pub position: PositionView,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct PositionView {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BondView {
    pub id: String,
    pub begin: String,
    pub end: String,
    pub order: BondOrderDto,
}

/// State of the in-memory editing session that holds the working copy.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SessionView {
    /// Revision last committed to storage by this session.
    pub saved_revision: u64,
    pub unsaved_changes: bool,
    pub undo_depth: usize,
    pub redo_depth: usize,
    /// The revision this session's recovery draft holds, or `null` when it has none.
    pub draft_revision: Option<u64>,
    /// The service's latest failed attempt to draft unsaved work this draft does not hold,
    /// while it keeps retrying; `null` otherwise.
    pub draft_failure: Option<DraftFailureView>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DraftFailureView {
    /// Failed attempts in a row.
    pub attempts: u32,
    /// Milliseconds until the service tries again.
    pub retry_in_ms: u64,
    pub message: String,
}

impl DocumentResponse {
    pub(crate) fn new(
        document: &Document,
        saved_revision: u64,
        draft_revision: Option<u64>,
    ) -> Self {
        Self {
            api_version: API_VERSION,
            document: DocumentView {
                id: document.id().to_string(),
                title: document.title().to_owned(),
                revision: document.revision(),
                atoms: document
                    .atoms()
                    .map(|(id, atom)| AtomView {
                        id: id.to_string(),
                        element: atom.element.symbol(),
                        charge: atom.charge,
                        isotope: atom.isotope,
                        position: PositionView {
                            x: atom.position.x(),
                            y: atom.position.y(),
                        },
                    })
                    .collect(),
                bonds: document
                    .bonds()
                    .map(|(id, bond)| BondView {
                        id: id.to_string(),
                        begin: bond.begin.to_string(),
                        end: bond.end.to_string(),
                        order: bond.order.into(),
                    })
                    .collect(),
            },
            session: SessionView {
                saved_revision,
                unsaved_changes: saved_revision != document.revision(),
                undo_depth: document.undo_depth(),
                redo_depth: document.redo_depth(),
                draft_revision,
                draft_failure: None,
            },
            recovery: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DocumentListResponse {
    pub api_version: &'static str,
    pub documents: Vec<DocumentSummary>,
    /// Stored entries that exist but could not be read as documents.
    pub unreadable: Vec<UnreadableDocument>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DocumentSummary {
    pub id: String,
    pub title: String,
    pub revision: u64,
    pub atom_count: usize,
    pub bond_count: usize,
    pub open: bool,
    pub unsaved_changes: bool,
    pub recovery: Option<RecoveryView>,
    /// When storage last committed the document, in milliseconds since the Unix epoch.
    pub saved_at_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct UnreadableDocument {
    pub id: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CloseResponse {
    pub api_version: &'static str,
    pub id: String,
    /// True when the session had unsaved changes and they were discarded.
    pub discarded_changes: bool,
}

/// A document saved as a new, independent document.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CopyResponse {
    /// The copy, now open in its own session.
    #[serde(flatten)]
    pub copy: DocumentResponse,
    pub source: CopySource,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CopySource {
    pub id: String,
    /// True when the source's session was closed: its working copy lives on in the copy, and
    /// the source reopens from its last save. A recovered draft leaves its source's session open
    /// unless, with every session slot in use, that clean session was the only one that could
    /// make room for the copy.
    pub closed: bool,
    /// True when that session had unsaved changes, which are now only in the copy.
    pub discarded_changes: bool,
}

/// A document moved to the trash.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DeleteResponse {
    pub api_version: &'static str,
    pub id: String,
    pub title: String,
}

/// A trashed document moved back to the stored documents.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RestoreResponse {
    pub api_version: &'static str,
    pub id: String,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TrashListResponse {
    pub api_version: &'static str,
    /// Most recently trashed first.
    pub documents: Vec<TrashedSummary>,
    /// Trashed entries that exist but could not be read as documents.
    pub unreadable: Vec<UnreadableDocument>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TrashedSummary {
    pub id: String,
    pub title: String,
    /// The stored revision it was trashed at.
    pub revision: u64,
    pub atom_count: usize,
    pub bond_count: usize,
    /// Milliseconds since the Unix epoch, when known.
    pub deleted_at_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DraftDiscardResponse {
    pub api_version: &'static str,
    pub id: String,
}
