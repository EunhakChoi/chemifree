//! Refusal reasons of edits, transactions, and history, and the result of an applied transaction.

use std::fmt;

use super::{
    AtomId, BondId, MAX_ATOMS, MAX_BONDS, MAX_CHARGE, MAX_COMMANDS, MAX_COORDINATE, MAX_INSTANCES,
    MAX_ISOTOPE_MASS_NUMBER, MAX_TITLE_CHARS, MIN_CHARGE, ObjectId,
};
use crate::scene::{
    MAX_DEPTH, MAX_OBJECTS, MAX_REFERENCES, ObjectKind, SceneError, SceneReference,
};
use crate::settings::SettingsError;
use crate::states::StateError;
use crate::stereo::{StereoError, StereoKey};
use crate::stereo_drawing::DeriveError;
use crate::stereo_group::{GroupNote, StereoGroupError};

#[derive(Clone, Debug, PartialEq)]
pub enum EditError {
    AbbreviationInvalid,
    /// Explicit fragment members do not meet the bounded contraction profile.
    FragmentDisplayInvalid,
    DisplayLabelInvalid,
    CaptionInvalid,
    CaptionPersistenceLimit,
    GraphicGeometryInvalid,
    GraphicCommandMismatch,
    PolygonPersistenceLimit,
    SplinePersistenceLimit,
    ImageInvalid,
    ImagePixelLimit,
    ImageHistoryPixelLimit,
    ImagePersistenceLimit,
    ImageUnsupported(ObjectId),
    CaptionUnsupported(ObjectId),
    EmptyTransaction,
    TooManyCommands,
    EmptyTargetList,
    UnknownAtom(AtomId),
    /// `AtomRef::Created` must name an earlier `AddAtom` command of the same transaction.
    UnresolvedReference(usize),
    UnknownBond(BondId),
    SelfBond(AtomId),
    DuplicateBond {
        existing: BondId,
    },
    AtomLimit,
    BondLimit,
    InvalidTitle,
    CoordinateOutOfRange,
    /// Ring attachment needs a finite nonzero direction.
    RingDirectionInvalid,
    /// Kekulé is defined only for six-carbon ring recipes.
    RingPatternInvalid,
    ChargeOutOfRange(i8),
    IsotopeOutOfRange(u16),
    IdentifierSpaceExhausted,
    RevisionLimit,
    /// A scene object that does not exist.
    UnknownObject(ObjectId),
    /// A scene object of another kind than the command acts on.
    ObjectKindMismatch {
        object: ObjectId,
        expected: ObjectKind,
    },
    /// A single `position` was given while the document has several scene instances.
    PositionsRequired {
        instances: usize,
    },
    /// The positions of a new atom leave out this scene instance.
    MissingPosition(ObjectId),
    /// A command that reads one scene instance's positions (`move_atoms`,
    /// `set_configuration_from_drawing`) did not name an instance while the document has several.
    InstanceRequired {
        instances: usize,
    },
    /// A document keeps at least one scene instance of its molecule.
    LastInstance,
    InstanceLimit,
    ObjectLimit,
    DepthLimit,
    /// A group needs at least two distinct objects.
    GroupTooSmall,
    /// Objects to group do not share one parent.
    GroupMixedParents,
    /// A position among a parent's `len` children that does not exist.
    IndexOutOfRange {
        index: usize,
        len: usize,
    },
    /// A selection names what does not exist; the message says what.
    SelectionUnknown(String),
    /// A containment tree that commands could not have produced; restored data only.
    SceneTreeInvalid(String),
    /// A reference names a missing source or target, or a link that does not exist.
    ReferenceUnknown(SceneReference),
    /// A surviving owner still refers to a target being deleted.
    ReferenceInUse(SceneReference),
    ReferenceLimit,
    /// Too few independent outermost units for the requested arrangement.
    ArrangementTooFewUnits {
        required: usize,
        found: usize,
    },
    /// The object has no geometry to bound; no synthetic origin is assigned.
    ArrangementEmptyGeometry(ObjectId),
    /// Fixed endpoints cannot contain all unit extents without overlapping equal gaps.
    ArrangementNegativeGap,
    /// Rounded center alignment would not settle after this single materialization.
    ArrangementCenterUnstable,
    /// A rotation angle is not finite, even for an empty selection.
    TransformInvalidAngle,
    /// This selected rectangle cannot be represented after an arbitrary-angle rotation.
    TransformRectangleUnsupported(ObjectId),
    /// A non-quarter-turn cannot retain the ellipse's axis-aligned corner representation.
    TransformEllipseUnsupported(ObjectId),
    /// A refused combination of recorded states, with its stable code.
    InvalidState(StateError),
    Settings(SettingsError),
    /// `BondRef::Created` must name an earlier command of the same transaction that added a
    /// bond: an `AddBond`, or an `AddAtom` with `bond_to`.
    UnresolvedBondReference(usize),
    /// `GroupRef::Created` must name an earlier command of the same transaction that created a
    /// stereo group.
    UnresolvedGroupReference(usize),
    /// A refused stereo mark or configuration, with its stable code.
    Stereo(StereoError),
    /// A configuration that could not be read off the drawing for this atom (a center) or bond
    /// (a double bond), with the cause and its stable code, if it has one.
    FromDrawing {
        item: StereoKey,
        cause: DeriveError,
    },
    /// A refused stereo group edit or stored group, with its stable code.
    StereoGroup(StereoGroupError),
}

impl EditError {
    /// The scene rule a command violated, when the error has a scene-specific code.
    pub fn scene_error(&self) -> Option<SceneError> {
        Some(match self {
            Self::AbbreviationInvalid => SceneError::AbbreviationInvalid,
            Self::FragmentDisplayInvalid => SceneError::FragmentDisplayInvalid,
            Self::DisplayLabelInvalid => SceneError::DisplayLabelInvalid,
            Self::CaptionInvalid => SceneError::CaptionInvalid,
            Self::CaptionPersistenceLimit => SceneError::CaptionPersistenceLimit,
            Self::GraphicGeometryInvalid => SceneError::GraphicGeometryInvalid,
            Self::GraphicCommandMismatch => SceneError::GraphicCommandMismatch,
            Self::PolygonPersistenceLimit => SceneError::PolygonPersistenceLimit,
            Self::SplinePersistenceLimit => SceneError::SplinePersistenceLimit,
            Self::ImageInvalid => SceneError::ImageInvalid,
            Self::ImagePixelLimit => SceneError::ImagePixelLimit,
            Self::ImageHistoryPixelLimit => SceneError::ImageHistoryPixelLimit,
            Self::ImagePersistenceLimit => SceneError::ImagePersistenceLimit,
            Self::ImageUnsupported(_) => SceneError::ImageUnsupported,
            Self::CaptionUnsupported(_) => SceneError::CaptionUnsupported,
            Self::UnknownObject(_) => SceneError::ObjectUnknown,
            Self::ObjectKindMismatch { .. } => SceneError::ObjectKindMismatch,
            Self::GroupMixedParents => SceneError::GroupMixedParents,
            Self::DepthLimit => SceneError::DepthLimit,
            Self::ObjectLimit => SceneError::ObjectLimit,
            Self::IndexOutOfRange { .. } => SceneError::IndexOutOfRange,
            Self::LastInstance => SceneError::LastInstance,
            Self::SelectionUnknown(_) => SceneError::SelectionUnknown,
            Self::ReferenceUnknown(_) => SceneError::ReferenceUnknown,
            Self::ReferenceInUse(_) => SceneError::ReferenceInUse,
            Self::ReferenceLimit => SceneError::ReferenceLimit,
            Self::ArrangementTooFewUnits { .. } => SceneError::ArrangementTooFewUnits,
            Self::ArrangementEmptyGeometry(_) => SceneError::ArrangementEmptyGeometry,
            Self::ArrangementNegativeGap => SceneError::ArrangementNegativeGap,
            Self::ArrangementCenterUnstable => SceneError::ArrangementCenterUnstable,
            Self::TransformInvalidAngle => SceneError::TransformInvalidAngle,
            Self::TransformRectangleUnsupported(_) => SceneError::TransformRectangleUnsupported,
            Self::TransformEllipseUnsupported(_) => SceneError::TransformEllipseUnsupported,
            _ => return None,
        })
    }

    /// The stable code sent as `reason` with `invalid_command`, when the refusal has one.
    pub fn reason(&self) -> Option<&'static str> {
        match self {
            Self::RingDirectionInvalid => Some("ring_direction_invalid"),
            Self::RingPatternInvalid => Some("ring_pattern_invalid"),
            Self::InvalidState(state) => Some(state.code()),
            Self::Settings(error) => Some(error.code()),
            Self::Stereo(error) => Some(error.code()),
            Self::StereoGroup(error) => Some(error.code()),
            Self::FromDrawing { cause, .. } => cause.code(),
            _ => self.scene_error().map(SceneError::code),
        }
    }
}

impl From<StateError> for EditError {
    fn from(error: StateError) -> Self {
        Self::InvalidState(error)
    }
}

impl From<SettingsError> for EditError {
    fn from(error: SettingsError) -> Self {
        Self::Settings(error)
    }
}

impl fmt::Display for EditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyTransaction => write!(formatter, "a transaction needs at least one command"),
            Self::TooManyCommands => {
                write!(
                    formatter,
                    "a transaction accepts at most {MAX_COMMANDS} commands"
                )
            }
            Self::EmptyTargetList => write!(formatter, "the target list must not be empty"),
            Self::UnknownAtom(id) => write!(formatter, "atom {id} does not exist"),
            Self::UnresolvedReference(index) => write!(
                formatter,
                "${index} does not name an earlier add_atom command in this transaction"
            ),
            Self::UnknownBond(id) => write!(formatter, "bond {id} does not exist"),
            Self::SelfBond(id) => write!(formatter, "atom {id} cannot bond to itself"),
            Self::DuplicateBond { existing } => {
                write!(
                    formatter,
                    "these atoms are already joined by bond {existing}"
                )
            }
            Self::AtomLimit => write!(formatter, "a document holds at most {MAX_ATOMS} atoms"),
            Self::BondLimit => write!(formatter, "a document holds at most {MAX_BONDS} bonds"),
            Self::InvalidTitle => write!(
                formatter,
                "a title needs 1-{MAX_TITLE_CHARS} characters without control characters"
            ),
            Self::RingDirectionInvalid => formatter.write_str("ring direction must be finite and nonzero"),
            Self::RingPatternInvalid => formatter.write_str("Kekule ring pattern requires six carbon atoms"),
            Self::CoordinateOutOfRange => write!(
                formatter,
                "coordinates must be finite and within ±{MAX_COORDINATE} pt"
            ),
            Self::ChargeOutOfRange(charge) => write!(
                formatter,
                "charge {charge} is outside {MIN_CHARGE}..={MAX_CHARGE}"
            ),
            Self::IsotopeOutOfRange(mass) => write!(
                formatter,
                "isotope mass number {mass} is outside 1..={MAX_ISOTOPE_MASS_NUMBER}"
            ),
            Self::IdentifierSpaceExhausted => {
                write!(formatter, "this document has no identifiers left")
            }
            Self::RevisionLimit => write!(formatter, "{REVISION_LIMIT_MESSAGE}"),
            Self::UnknownObject(id) => write!(formatter, "scene object {id} does not exist"),
            Self::ObjectKindMismatch { object, expected } => {
                write!(formatter, "scene object {object} is not {expected}")
            }
            Self::PositionsRequired { instances } => write!(
                formatter,
                "the molecule has {instances} scene instances; give `positions` with a point \
                 for each of them"
            ),
            Self::MissingPosition(id) => {
                write!(formatter, "no position is given for scene instance {id}")
            }
            Self::InstanceRequired { instances } => write!(
                formatter,
                "the molecule has {instances} scene instances; name the `instance` to use"
            ),
            Self::LastInstance => write!(formatter, "a molecule keeps at least one scene instance"),
            Self::InstanceLimit => write!(
                formatter,
                "a document holds at most {MAX_INSTANCES} scene instances"
            ),
            Self::ObjectLimit => write!(
                formatter,
                "a document holds at most {MAX_OBJECTS} scene objects"
            ),
            Self::DepthLimit => write!(
                formatter,
                "scene objects nest at most {MAX_DEPTH} levels deep"
            ),
            Self::GroupTooSmall => write!(formatter, "a group needs at least two objects"),
            Self::GroupMixedParents => write!(formatter, "objects to group must share one parent"),
            Self::IndexOutOfRange { index, len } => write!(
                formatter,
                "index {index} is out of range among {len} children"
            ),
            Self::SelectionUnknown(detail) => write!(formatter, "the selection names {detail}"),
            Self::ReferenceUnknown(reference) => write!(
                formatter,
                "reference from {} to {} is not valid",
                reference.source, reference.target
            ),
            Self::ReferenceInUse(reference) => write!(
                formatter,
                "{} is still referenced by scene object {}",
                reference.target, reference.source
            ),
            Self::ReferenceLimit => write!(
                formatter,
                "a document holds at most {MAX_REFERENCES} references"
            ),
            Self::ArrangementTooFewUnits { required, found } => write!(
                formatter,
                "this arrangement needs at least {required} outermost units; found {found}"
            ),
            Self::ArrangementEmptyGeometry(object) => {
                write!(formatter, "scene object {object} has no geometry to bound")
            }
            Self::ArrangementNegativeGap => formatter.write_str(
                "fixed endpoint units leave insufficient space for nonoverlapping equal gaps",
            ),
            Self::ArrangementCenterUnstable => formatter.write_str(
                "rounded center alignment would change again on repetition; no geometry was applied",
            ),
            Self::TransformInvalidAngle => formatter.write_str("rotation degrees must be finite"),
            Self::TransformRectangleUnsupported(object) => write!(
                formatter,
                "rectangle {object} requires an exact quarter-turn rotation; arbitrary angles cannot be represented by its two axis-aligned corners"
            ),
            Self::TransformEllipseUnsupported(object) => write!(formatter,
                "ellipse {object} requires an exact quarter-turn rotation; arbitrary angles cannot be represented by its two axis-aligned bounding-box corners"
            ),
            Self::GraphicGeometryInvalid => formatter.write_str("Graphic point count does not match its shape: polygons require 3–64 vertices, splines require exactly four defining points, and other shapes require exactly two points."),
            Self::GraphicCommandMismatch => formatter.write_str("Polygon and spline geometry each require their dedicated command; two-point commands accept only two-point shapes."),
            Self::ImageInvalid => formatter.write_str("Image needs a verified embedded static PNG, positive finite size, and origin/end within ±100000 pt."),
            Self::ImagePixelLimit => formatter.write_str("Current embedded images exceed the 4194304-pixel document policy."),
            Self::ImageHistoryPixelLimit => formatter.write_str("Unique image assets exceed the retained history pixel policy."),
            Self::ImagePersistenceLimit => formatter.write_str("The image-bearing edit exceeds the shared persistence budget; reduce source bytes or transaction size."),
            Self::ImageUnsupported(id) => write!(formatter, "Image {id} cannot participate in rotation or reflection; exclude it from this transform."),
            Self::SplinePersistenceLimit => formatter.write_str("The spline-bearing edit exceeds the shared variable-size persistence budget; reduce transaction size."),
            Self::PolygonPersistenceLimit => formatter.write_str("The polygon-bearing edit exceeds the shared variable-size persistence budget; reduce vertices or transaction size."),
            Self::CaptionPersistenceLimit => formatter.write_str("The text edit exceeds the persistence budget; reduce text or transaction size."),
            Self::DisplayLabelInvalid => formatter.write_str("Display label needs non-whitespace single-line Unicode text, at most 128 UTF-8 bytes; text does not define chemistry."),
            Self::FragmentDisplayInvalid => formatter.write_str("Fragment display needs disjoint connected acyclic neutral default element members, one unmarked single boundary at the anchor, no local stereo, and one explicit placement; display text does not define chemistry."),
            Self::AbbreviationInvalid => formatter.write_str("Et needs a disjoint neutral default two-carbon terminal graph, one unmarked single boundary, no local stereo, and one explicit placement."),
            Self::CaptionInvalid => formatter.write_str("Caption needs 1–32 nonempty single-line runs, at most 16384 UTF-8 bytes, non-whitespace text, and finite 4–72 pt size overrides."),
            Self::CaptionUnsupported(id) => write!(formatter, "This operation cannot preserve caption {id}; nothing was changed."),
            Self::SceneTreeInvalid(detail) => formatter.write_str(detail),
            Self::InvalidState(error) => error.fmt(formatter),
            Self::Settings(error) => error.fmt(formatter),
            Self::UnresolvedBondReference(index) => write!(
                formatter,
                "${index} does not name an earlier add_bond command in this transaction"
            ),
            Self::UnresolvedGroupReference(index) => write!(
                formatter,
                "${index} does not name an earlier command in this transaction that creates a \
                 stereo group"
            ),
            Self::Stereo(error) => error.fmt(formatter),
            Self::StereoGroup(error) => error.fmt(formatter),
            Self::FromDrawing { item, cause } => match item {
                StereoKey::Center(atom) => write!(formatter, "atom {atom}: {cause}"),
                StereoKey::DoubleBond(bond) => write!(formatter, "bond {bond}: {cause}"),
            },
        }
    }
}

impl std::error::Error for EditError {}

/// A rejected transaction. `index` names the failing command when there is one.
#[derive(Clone, Debug, PartialEq)]
pub struct TransactionError {
    pub index: Option<usize>,
    pub error: EditError,
}

impl fmt::Display for TransactionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.index {
            Some(index) => write!(formatter, "command {index}: {}", self.error),
            None => self.error.fmt(formatter),
        }
    }
}

impl std::error::Error for TransactionError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    Changed,
    /// The commands were valid but left the document identical; no revision or undo entry.
    Unchanged,
}

/// The result of a transaction that was applied: its outcome, and the stereo records that
/// existed before it and are gone after it because a change invalidated them or deleted what
/// they belong to (the record of a deleted atom or bond is listed), in key order. Records a
/// `Set*` command cleared explicitly are not listed, nor are records created and removed within
/// the transaction, nor a record that was removed and set again. `group_notes` lists, in the
/// order they happened, the changes of meaning the transaction made to groups that existed
/// before it (see [`GroupNote`]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Applied {
    pub outcome: Outcome,
    pub cleared: Vec<StereoKey>,
    pub group_notes: Vec<GroupNote>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryError {
    NothingToUndo,
    NothingToRedo,
    RevisionLimit,
}

const REVISION_LIMIT_MESSAGE: &str = "this document has reached its last revision number";

impl fmt::Display for HistoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NothingToUndo => "there is nothing to undo",
            Self::NothingToRedo => "there is nothing to redo",
            Self::RevisionLimit => REVISION_LIMIT_MESSAGE,
        })
    }
}

impl std::error::Error for HistoryError {}

/// No revision remains above the ones already handed out.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RevisionsExhausted;

impl fmt::Display for RevisionsExhausted {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(REVISION_LIMIT_MESSAGE)
    }
}

impl std::error::Error for RevisionsExhausted {}
