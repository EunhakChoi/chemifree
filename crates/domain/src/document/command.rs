//! The edit commands that a transaction applies to a document's graph, scene and settings.

use super::{
    Atom, AtomEdit, AtomId, AtomRef, BondId, BondOrder, BondRef, BondTo, FragmentDisplayRun,
    GroupRef, NewAtomPositions, ObjectId, Point, RingPattern, RingSize, Vector,
};
use crate::image::{Image, ImageSize};
use crate::scene::{
    Caption, DeleteMode, Graphic, ObjectArrangement, SceneReference, Selection, SelectionTransform,
};
use crate::settings::SettingsEdit;
use crate::stereo::{AtomConfiguration, BondConfiguration, StereoMark};
use crate::stereo_group::GroupKind;

/// One edit. A transaction applies an ordered list of commands all-or-nothing.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Contracts explicitly named graph members; text never defines chemistry.
    ContractFragment {
        instance: ObjectId,
        anchor: AtomId,
        members: Vec<AtomId>,
        text: String,
    },
    /// Changes only an existing fragment's presentation text.
    SetFragmentDisplayText {
        instance: ObjectId,
        anchor: AtomId,
        text: String,
    },
    /// Changes explicit text runs only; script never defines chemistry.
    SetFragmentDisplayRuns {
        instance: ObjectId,
        anchor: AtomId,
        runs: Vec<FragmentDisplayRun>,
    },
    /// Reveals a retained fragment graph and its original coordinates.
    ExpandFragment {
        instance: ObjectId,
        anchor: AtomId,
    },
    /// Hides an existing, checked terminal ethyl graph in one explicit placement.
    CollapseEthyl {
        instance: ObjectId,
        anchor: AtomId,
        terminal: AtomId,
    },
    /// Reveals the stored atoms and coordinates without changing chemistry.
    ExpandEthyl {
        instance: ObjectId,
        anchor: AtomId,
    },
    /// Changes presentation text only; None restores the default Et label.
    UpdateAbbreviationLabel {
        instance: ObjectId,
        anchor: AtomId,
        text: Option<String>,
    },
    /// Appends an independent regular carbon ring in the only placement.
    /// Uses the current document bond length; creates no transaction-local `$N` target.
    InsertRing {
        instance: ObjectId,
        center: Point,
        size: RingSize,
        pattern: RingPattern,
    },
    /// Connects a fresh regular carbon ring to an existing atom with one single bond.
    /// Direction is finite and nonzero; its length is ignored. Stereo follows ordinary AddBond.
    AttachRing {
        instance: ObjectId,
        atom: AtomId,
        direction: Vector,
        size: RingSize,
        pattern: RingPattern,
    },
    /// Adds an atom with the given state, optionally bonded from another atom (`bond_to.atom`
    /// is the bond's `begin`, so the donor of a dative bond).
    AddAtom {
        atom: Atom,
        positions: NewAtomPositions,
        bond_to: Option<BondTo>,
    },
    AddBond {
        begin: AtomRef,
        end: AtomRef,
        order: BondOrder,
    },
    /// Changes only the fields of `edit` that are `Some`. The atom that results must be valid
    /// as a whole: turning an atom into a query needs its charge and isotope cleared too.
    UpdateAtom {
        atom: AtomId,
        edit: AtomEdit,
    },
    /// Translates each listed atom once, even if it is listed repeatedly, in one scene
    /// instance. `instance` may be left out only while the document has exactly one.
    MoveAtoms {
        atoms: Vec<AtomId>,
        delta: Vector,
        instance: Option<ObjectId>,
    },
    UpdateBond {
        bond: BondId,
        order: BondOrder,
    },
    /// Deletes the atoms and every bond incident to them.
    DeleteAtoms {
        atoms: Vec<AtomId>,
    },
    DeleteBonds {
        bonds: Vec<BondId>,
    },
    Rename {
        title: String,
    },
    /// Adds a scene instance of the molecule as the last page root, with the positions of
    /// `source` (the first instance when `None`) translated by `offset`.
    AddInstance {
        source: Option<ObjectId>,
        offset: Vector,
    },
    /// Translates every position of one scene instance once.
    MoveInstance {
        instance: ObjectId,
        offset: Vector,
    },
    /// Deletes a scene instance and its positions; the last one is kept.
    DeleteInstance {
        instance: ObjectId,
    },
    /// Adds a graphic under `parent` (a group, or the page when `None`) at `index` among its
    /// children (the end when `None`).
    /// Adds a bounded independent caption to the ordered scene.
    AddCaption {
        caption: Caption,
        parent: Option<ObjectId>,
        index: Option<usize>,
    },
    /// Replaces a caption's complete anchor, alignment and run state atomically.
    UpdateCaption {
        object: ObjectId,
        caption: Caption,
    },
    AddImage {
        image: Image,
        parent: Option<ObjectId>,
        index: Option<usize>,
    },
    UpdateImageGeometry {
        object: ObjectId,
        origin: Point,
        size_pt: ImageSize,
    },
    AddGraphic {
        graphic: Graphic,
        parent: Option<ObjectId>,
        index: Option<usize>,
    },
    /// Adds one open stroke-only cubic, preserving P0, C1, C2, P3 explicitly.
    AddSpline {
        points: [Point; 4],
        line_width_pt: Option<f64>,
        parent: Option<ObjectId>,
        index: Option<usize>,
    },
    /// Replaces the complete cubic defining-point state without interpolation.
    UpdateSplinePoints {
        object: ObjectId,
        points: [Point; 4],
    },
    /// Adds a closed stroke-only polygon.
    AddPolygon {
        vertices: Vec<Point>,
        line_width_pt: Option<f64>,
        parent: Option<ObjectId>,
        index: Option<usize>,
    },
    /// Replaces every vertex of an existing polygon.
    UpdatePolygonVertices {
        object: ObjectId,
        vertices: Vec<Point>,
    },
    /// Replaces only an existing graphic's two points; shape, identity and links stay intact.
    UpdateGraphicPoints {
        object: ObjectId,
        points: [Point; 2],
    },
    /// Sets one graphic's bounded line width, or restores dynamic document inheritance.
    SetGraphicLineWidth {
        object: ObjectId,
        line_width_pt: Option<f64>,
    },
    /// Puts at least two objects sharing one parent into a new group, which takes the place of
    /// the earliest of them; they keep their order.
    Group {
        objects: Vec<ObjectId>,
    },
    /// Removes a group; its children take its place.
    Ungroup {
        group: ObjectId,
    },
    /// Moves an object to `index` among its parent's children.
    Reorder {
        object: ObjectId,
        index: usize,
    },
    /// Deletes objects with or without what they contain. Deleting an instance deletes its
    /// positions; the last instance is kept.
    DeleteObjects {
        objects: Vec<ObjectId>,
        mode: DeleteMode,
    },
    /// Translates every graphic and instance the listed objects are or contain once, even when
    /// both a group and something inside it are listed.
    MoveObjects {
        objects: Vec<ObjectId>,
        delta: Vector,
    },
    /// Moves the union of selected object subtrees, explicit atoms and selected bond
    /// endpoints once. Explicit graph targets use the named instance, or the only one.
    /// This selection determines geometry targets; transaction selection before/after
    /// independently controls the selection recorded with history.
    MoveSelection {
        selection: Selection,
        instance: Option<ObjectId>,
        delta: Vector,
    },
    /// Rigidly aligns or distributes the outermost selected scene objects using only
    /// atom positions and graphic points. Targets are recomputed at this command's turn.
    ArrangeObjects {
        objects: Vec<ObjectId>,
        arrangement: ObjectArrangement,
    },
    /// Transforms the resolved union of geometry once around an explicit pivot.
    /// Stored chemical configurations, marks and enhanced group membership do not change.
    TransformSelection {
        selection: Selection,
        instance: Option<ObjectId>,
        pivot: Point,
        transform: SelectionTransform,
    },
    /// Adds or removes a directed scene reference without assigning chemical meaning to it.
    AddReference {
        reference: SceneReference,
    },
    RemoveReference {
        reference: SceneReference,
    },
    /// Changes the named style settings as part of this transaction.
    UpdateSettings(SettingsEdit),
    /// Sets or, with `None`, clears how a bond is drawn. The mark must fit the bond's order. The
    /// bond's `begin` and `end` stay as they are.
    SetBondMark {
        bond: BondRef,
        mark: Option<StereoMark>,
    },
    /// Sets or, with `None`, clears the stated configuration of a stereocenter. The neighbors
    /// are checked against the center's bonds at this point of the transaction.
    SetAtomConfiguration {
        atom: AtomRef,
        configuration: Option<AtomConfiguration>,
    },
    /// Sets or, with `None`, clears the stated configuration of a double bond.
    SetBondConfiguration {
        bond: BondRef,
        configuration: Option<BondConfiguration>,
    },
    /// Stores the configuration the bond marks and the positions of one scene instance show
    /// (see [`crate::stereo_drawing`]).
    ///
    /// For every item it processes it replaces the stored record, whatever it is: a statement
    /// made by [`Self::SetAtomConfiguration`] or [`Self::SetBondConfiguration`], `unknown`, or
    /// another label all give way to what the drawing shows. A drawn mark never blocks a stated
    /// configuration when that is written, but this command reads marks, so it overwrites the
    /// statement for the items it processes. It writes nothing for an item it skips or does not
    /// name, and nothing at all when it refuses.
    ///
    /// With neither list, the items are those a mark bears on: each possible center at the
    /// narrow end of a wedge or hash or at an end of a wavy bond, and each crossed double bond.
    /// A mark whose narrow end could never hold a center record (a terminal atom, a query atom,
    /// an atom with five neighbors) is not in the scope at all. A crossed double bond that
    /// fails structurally is skipped: any structurally impossible item is skipped without a
    /// note, and the reply does not list what was skipped. Two causes can occur there,
    /// [`StereoError::GeometryUnsupported`] (a cumulated double or triple bond at an end) and
    /// [`StereoError::ReferenceInvalid`] (an end with more than two other neighbors);
    /// [`StereoError::CenterUnsupported`] and [`StereoError::DoubleBondRequired`] cannot,
    /// because the scope holds only possible centers and double bonds. Any other item the
    /// drawing does not determine refuses the command.
    ///
    /// With a list (`atoms` and/or `bonds`, each non-empty), exactly the named items are
    /// derived and every one must derive, with its structural reason when the model cannot hold
    /// it. A list goes through the same de-duplication as `move_atoms`: repeats, also two
    /// references that resolve to one atom (`"a1"` and `"$0"`), are one item, and items are
    /// processed centers first and then double bonds, each by ascending identifier, so a failing
    /// item is named by its lowest identifier, not by its position in the list.
    ///
    /// `instance` follows `move_atoms`: it may be left out only while the document has one
    /// scene instance. The command runs against the transaction's state at its position, so it
    /// may name atoms and bonds created earlier in the same transaction. Storing what is already
    /// stored changes nothing.
    ///
    /// [`StereoError::GeometryUnsupported`]: crate::stereo::StereoError::GeometryUnsupported
    /// [`StereoError::ReferenceInvalid`]: crate::stereo::StereoError::ReferenceInvalid
    /// [`StereoError::CenterUnsupported`]: crate::stereo::StereoError::CenterUnsupported
    /// [`StereoError::DoubleBondRequired`]: crate::stereo::StereoError::DoubleBondRequired
    SetConfigurationFromDrawing {
        atoms: Option<Vec<AtomRef>>,
        bonds: Option<Vec<BondRef>>,
        instance: Option<ObjectId>,
    },
    /// Creates an enhanced-stereo group (see [`crate::stereo_group`]) of stored tetrahedral
    /// centers that are in no group. `number` is stored as stated (`0` and negatives too); an
    /// absolute group takes none.
    CreateStereoGroup {
        kind: GroupKind,
        number: Option<i32>,
        atoms: Vec<AtomRef>,
    },
    /// Deletes a group; its members stay configured and become ungrouped.
    DeleteStereoGroup {
        group: GroupRef,
    },
    SetStereoGroupKind {
        group: GroupRef,
        kind: GroupKind,
    },
    /// Sets or, with `None`, clears the stored number. A number another group of the kind
    /// already has is refused: merge the groups instead.
    SetStereoGroupNumber {
        group: GroupRef,
        number: Option<i32>,
    },
    /// Adds centers that are in no group; centers already in this group are left as they are.
    AddStereoGroupMembers {
        group: GroupRef,
        atoms: Vec<AtomRef>,
    },
    /// Removes members, which become ungrouped; it never empties the group (delete it instead).
    RemoveStereoGroupMembers {
        group: GroupRef,
        atoms: Vec<AtomRef>,
    },
    /// Moves every member of `from` into `into` and deletes `from`; `into` keeps its kind and
    /// number. The groups must be of one kind.
    MergeStereoGroups {
        into: GroupRef,
        from: GroupRef,
    },
    /// Moves some, not all, members of an `and` or `or` group into a new group of the same
    /// kind with `number`. The new group is `$N` for later commands.
    SplitStereoGroup {
        group: GroupRef,
        atoms: Vec<AtomRef>,
        number: Option<i32>,
    },
}
