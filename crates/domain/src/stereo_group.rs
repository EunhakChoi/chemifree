//! Enhanced-stereo groups: sets of stored tetrahedral centers that are stated to be absolute
//! (`absolute`) or to belong to an `and` or `or` mixture.
//!
//! What a group says. A group is a statement about which centers belong together; it is never a
//! claim about composition. `and` records that the author means a mixture of the drawn set of
//! members and its all-inverted counterpart, `or` that the members invert together or not at
//! all, and `absolute` that the members are as drawn. No ratio is stated or implied for any
//! kind, and nothing here judges that members are stereogenic or related.
//!
//! What is stored and what is computed. A group is identified by its [`GroupId`] (`e1`, `e2`,
//! ..., never reused). It stores its kind, the number as it was stated (`Option<i32>`, the full
//! `i32` range, with `0` and negatives kept as given and no special meaning) and its member
//! atoms. The interpreted number ([`StereoGroup::interpreted_number`]) and the display label
//! ([`StereoGroup::label`]) are computed and are never stored or saved: a missing number reads as
//! `0` (the number a writer attaches to an unnumbered group), so a group with no number and a
//! group numbered `0` stand for the same group on the wire and cannot both exist. This is a
//! product definition; the public specifications do not fix a default.
//!
//! Invariants, checked by [`StereoGroupTable::validate`] on every restore, load and history
//! replay: every member is a stored tetrahedral center (not `unknown`, not unspecified); a group
//! has at least one member; an atom is in at most one group; an `absolute` group carries no
//! number; the document holds at most one `absolute` group; and per kind (`and`, `or`) the
//! interpreted numbers are distinct. One `absolute` group per document matches the single
//! `STEABS` collection of the public connection-table format; whether a molecule is the unit
//! (a document has exactly one chemical graph) has not been measured. Group count and member
//! count need no cap of their own: members are distinct stored centers, so both are bounded by
//! [`crate::document::MAX_ATOMS`].
//!
//! Enhanced groups are ignored by `chem.cip`, `chem.stereo`, `chem.identity` and
//! `chem.substructure` in this build.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::document::{AtomId, EditError, GroupId};
use crate::stereo::{CenterState, StereoKey, StereoState, StereoTable};

/// Which statement a group makes. The wire name of `Absolute` is `absolute`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum GroupKind {
    Absolute,
    And,
    Or,
}

impl GroupKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Absolute => "absolute",
            Self::And => "and",
            Self::Or => "or",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [Self::Absolute, Self::And, Self::Or]
            .into_iter()
            .find(|kind| kind.as_str() == value)
    }
}

/// One group: the stored kind and number, and its members. See the module documentation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StereoGroup {
    kind: GroupKind,
    number: Option<i32>,
    members: BTreeSet<AtomId>,
}

impl StereoGroup {
    /// A group as stated. Nothing is checked here; [`StereoGroupTable::validate`] and the
    /// table's edits hold the rules.
    pub const fn new(kind: GroupKind, number: Option<i32>, members: BTreeSet<AtomId>) -> Self {
        Self {
            kind,
            number,
            members,
        }
    }

    pub const fn kind(&self) -> GroupKind {
        self.kind
    }

    /// The number as stated, kept exactly (`None`, `0` and negatives included).
    pub const fn stored_number(&self) -> Option<i32> {
        self.number
    }

    pub const fn members(&self) -> &BTreeSet<AtomId> {
        &self.members
    }

    /// The number the group has on the wire: `None` for an absolute group, otherwise the stored
    /// number, or `0` when none is stored.
    pub fn interpreted_number(&self) -> Option<i32> {
        match self.kind {
            GroupKind::Absolute => None,
            GroupKind::And | GroupKind::Or => Some(self.number.unwrap_or(0)),
        }
    }

    /// Display text such as `abs`, `and2` or `or0`: output only, never stored.
    pub fn label(&self) -> String {
        match self.interpreted_number() {
            None => "abs".to_owned(),
            Some(number) => format!("{}{number}", self.kind.as_str()),
        }
    }
}

/// Why a group command or a stored group is refused. Each has a stable code, sent as `reason`
/// with `invalid_command` once the commands reach the wire.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StereoGroupError {
    /// No group has this identifier.
    Unknown,
    /// A group would have no members, or a command names no atoms.
    Empty,
    /// A member that is not a stored tetrahedral center: a missing atom, one without a
    /// configuration, or one stated `unknown`.
    MemberNotCenter,
    /// An atom that is already in a group.
    MemberTaken,
    /// An atom that is not a member of the group.
    MemberAbsent,
    /// An absolute group with a number.
    NumberNotAllowed,
    /// Another group of the same kind has this number (a missing number counts as `0`). Merge
    /// the groups to state that they are one.
    NumberTaken,
    /// The document already has an absolute group.
    AbsoluteExists,
    /// A merge of a group into itself.
    Same,
    /// A merge of groups of different kinds.
    KindMismatch,
    /// A split of the absolute group (remove members instead).
    SplitAbsolute,
}

impl StereoGroupError {
    /// Every code, in documentation order.
    pub const ALL: [Self; 11] = [
        Self::Unknown,
        Self::Empty,
        Self::MemberNotCenter,
        Self::MemberTaken,
        Self::MemberAbsent,
        Self::NumberNotAllowed,
        Self::NumberTaken,
        Self::AbsoluteExists,
        Self::Same,
        Self::KindMismatch,
        Self::SplitAbsolute,
    ];

    /// The stable machine-readable code.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Unknown => "stereo_group_unknown",
            Self::Empty => "stereo_group_empty",
            Self::MemberNotCenter => "stereo_group_member_not_center",
            Self::MemberTaken => "stereo_group_member_taken",
            Self::MemberAbsent => "stereo_group_member_absent",
            Self::NumberNotAllowed => "stereo_group_number_not_allowed",
            Self::NumberTaken => "stereo_group_number_taken",
            Self::AbsoluteExists => "stereo_group_abs_exists",
            Self::Same => "stereo_group_same",
            Self::KindMismatch => "stereo_group_kind_mismatch",
            Self::SplitAbsolute => "stereo_group_split_abs",
        }
    }
}

impl fmt::Display for StereoGroupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Unknown => "no stereo group has this identifier",
            Self::Empty => "a stereo group needs at least one member; delete the group instead",
            Self::MemberNotCenter => {
                "a group member is an atom with a stored tetrahedral configuration, not unknown"
            }
            Self::MemberTaken => "an atom is in at most one stereo group",
            Self::MemberAbsent => "the atom is not a member of the group",
            Self::NumberNotAllowed => "an absolute group has no number",
            Self::NumberTaken => {
                "another group of this kind has this number (no number counts as 0); merge \
                 the groups to make them one"
            }
            Self::AbsoluteExists => "a document has at most one absolute group",
            Self::Same => "a group cannot be merged into itself",
            Self::KindMismatch => "only groups of one kind can be merged",
            Self::SplitAbsolute => {
                "the absolute group cannot be split; remove members from it instead"
            }
        })
    }
}

impl std::error::Error for StereoGroupError {}

impl From<StereoGroupError> for EditError {
    fn from(error: StereoGroupError) -> Self {
        Self::StereoGroup(error)
    }
}

/// Why a member left its group without being asked to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LeaveCause {
    /// The stored configuration is gone (cleared, or removed with the atom or by a neighbor
    /// change).
    ConfigurationCleared,
    /// The stored configuration is now `unknown`.
    ConfigurationUnknown,
}

/// A change of a group's meaning that a transaction made, for the reply to report. Only groups
/// that existed before the transaction are reported.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GroupNote {
    /// A member left because it stopped being a stored tetrahedral center. `relation_lost`: the
    /// group is `and` or `or` and had other members, whose relation to this atom is no longer
    /// stated.
    MemberLeft {
        group: GroupId,
        atom: AtomId,
        cause: LeaveCause,
        relation_lost: bool,
    },
    /// The group lost its last member and is gone.
    GroupRemoved { group: GroupId },
    /// Members of an `and` or `or` group were removed on request, so that the relation between
    /// them and the rest is no longer stated.
    MembersRemoved { group: GroupId, atoms: Vec<AtomId> },
    /// An `and` or `or` group was split: the relation between `moved` and `kept` is no longer
    /// stated.
    Split {
        group: GroupId,
        new_group: GroupId,
        moved: Vec<AtomId>,
        kept: Vec<AtomId>,
    },
    /// Two groups were merged: all members now stand in one relation that was not stated
    /// before, and `from_number` is not kept when it differs from `into_number`.
    Merged {
        into: GroupId,
        from: GroupId,
        into_number: Option<i32>,
        from_number: Option<i32>,
    },
}

/// The groups of a document, by identifier.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StereoGroupTable {
    groups: BTreeMap<GroupId, StereoGroup>,
}

impl StereoGroupTable {
    pub fn get(&self, id: GroupId) -> Option<&StereoGroup> {
        self.groups.get(&id)
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = (GroupId, &StereoGroup)> {
        self.groups.iter().map(|(id, group)| (*id, group))
    }

    pub fn len(&self) -> usize {
        self.groups.len()
    }

    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    /// The group that holds `atom`.
    pub fn group_of(&self, atom: AtomId) -> Option<GroupId> {
        self.groups
            .iter()
            .find_map(|(id, group)| group.members.contains(&atom).then_some(*id))
    }

    /// The meaning of the groups with every label removed: each group's kind and member set,
    /// whatever its identifier or number. Two tables with an equal partition say the same about
    /// which centers belong together; numbers only tell groups of one kind apart.
    pub fn partition(&self) -> BTreeSet<(GroupKind, BTreeSet<AtomId>)> {
        self.groups
            .values()
            .map(|group| (group.kind, group.members.clone()))
            .collect()
    }

    /// The numbers a writer gives the groups, from the membership alone: per kind (`and`,
    /// `or`) dense from 1 in the order of each group's lowest member, none for `absolute`.
    /// Neither stored numbers nor identifiers enter, so relabelling a document does not change
    /// the result. A later writer uses these as the wire numbering.
    pub fn canonical_numbers(&self) -> BTreeMap<GroupId, u32> {
        let mut numbers = BTreeMap::new();
        for kind in [GroupKind::And, GroupKind::Or] {
            let mut of_kind: Vec<(&AtomId, GroupId)> = self
                .groups
                .iter()
                .filter(|(_, group)| group.kind == kind)
                .filter_map(|(id, group)| group.members.first().map(|first| (first, *id)))
                .collect();
            of_kind.sort_unstable();
            for (number, (_, id)) in (1..).zip(of_kind) {
                numbers.insert(id, number);
            }
        }
        numbers
    }

    pub(crate) fn from_groups(groups: BTreeMap<GroupId, StereoGroup>) -> Self {
        Self { groups }
    }

    pub(crate) const fn groups(&self) -> &BTreeMap<GroupId, StereoGroup> {
        &self.groups
    }

    pub(crate) fn groups_mut(&mut self) -> &mut BTreeMap<GroupId, StereoGroup> {
        &mut self.groups
    }

    /// Checks every invariant (see the module documentation) in one pass.
    pub(crate) fn validate(&self, stereo: &StereoTable) -> Result<(), EditError> {
        let mut seen_atoms = BTreeSet::new();
        let mut seen_numbers = BTreeSet::new();
        let mut absolute = false;
        for group in self.groups.values() {
            if group.members.is_empty() {
                return Err(StereoGroupError::Empty.into());
            }
            for atom in &group.members {
                require_center(stereo, *atom)?;
                if !seen_atoms.insert(*atom) {
                    return Err(StereoGroupError::MemberTaken.into());
                }
            }
            match group.kind {
                GroupKind::Absolute => {
                    if group.number.is_some() {
                        return Err(StereoGroupError::NumberNotAllowed.into());
                    }
                    if std::mem::replace(&mut absolute, true) {
                        return Err(StereoGroupError::AbsoluteExists.into());
                    }
                }
                GroupKind::And | GroupKind::Or => {
                    if !seen_numbers.insert((group.kind, group.interpreted_number())) {
                        return Err(StereoGroupError::NumberTaken.into());
                    }
                }
            }
        }
        Ok(())
    }

    /// Whether the kind and number could belong to a group other than `except`.
    fn check_label(
        &self,
        kind: GroupKind,
        number: Option<i32>,
        except: Option<GroupId>,
    ) -> Result<(), StereoGroupError> {
        if kind == GroupKind::Absolute && number.is_some() {
            return Err(StereoGroupError::NumberNotAllowed);
        }
        for (id, group) in &self.groups {
            if Some(*id) == except || group.kind != kind {
                continue;
            }
            if kind == GroupKind::Absolute {
                return Err(StereoGroupError::AbsoluteExists);
            }
            if group.interpreted_number() == Some(number.unwrap_or(0)) {
                return Err(StereoGroupError::NumberTaken);
            }
        }
        Ok(())
    }

    fn require(&self, id: GroupId) -> Result<&StereoGroup, StereoGroupError> {
        self.groups.get(&id).ok_or(StereoGroupError::Unknown)
    }

    pub(crate) fn create(
        &mut self,
        id: GroupId,
        group: StereoGroup,
        stereo: &StereoTable,
    ) -> Result<(), StereoGroupError> {
        if group.members.is_empty() {
            return Err(StereoGroupError::Empty);
        }
        self.check_free_centers(&group.members, stereo)?;
        self.check_label(group.kind, group.number, None)?;
        self.groups.insert(id, group);
        Ok(())
    }

    pub(crate) fn delete(&mut self, id: GroupId) -> Result<(), StereoGroupError> {
        self.groups
            .remove(&id)
            .map(drop)
            .ok_or(StereoGroupError::Unknown)
    }

    pub(crate) fn set_kind(
        &mut self,
        id: GroupId,
        kind: GroupKind,
    ) -> Result<(), StereoGroupError> {
        let number = self.require(id)?.number;
        self.check_label(kind, number, Some(id))?;
        self.group_mut(id).kind = kind;
        Ok(())
    }

    pub(crate) fn set_number(
        &mut self,
        id: GroupId,
        number: Option<i32>,
    ) -> Result<(), StereoGroupError> {
        let kind = self.require(id)?.kind;
        self.check_label(kind, number, Some(id))?;
        self.group_mut(id).number = number;
        Ok(())
    }

    pub(crate) fn add_members(
        &mut self,
        id: GroupId,
        atoms: &BTreeSet<AtomId>,
        stereo: &StereoTable,
    ) -> Result<(), StereoGroupError> {
        let group = self.require(id)?;
        let new: BTreeSet<AtomId> = atoms.difference(&group.members).copied().collect();
        self.check_free_centers(&new, stereo)?;
        self.group_mut(id).members.extend(new);
        Ok(())
    }

    /// Removes members; the note says what became unstated, if anything.
    pub(crate) fn remove_members(
        &mut self,
        id: GroupId,
        atoms: &BTreeSet<AtomId>,
    ) -> Result<Option<GroupNote>, StereoGroupError> {
        let group = self.require(id)?;
        if !atoms.is_subset(&group.members) {
            return Err(StereoGroupError::MemberAbsent);
        }
        if atoms.len() == group.members.len() {
            return Err(StereoGroupError::Empty);
        }
        let relational = group.kind != GroupKind::Absolute && group.members.len() >= 2;
        self.group_mut(id)
            .members
            .retain(|atom| !atoms.contains(atom));
        Ok(relational.then(|| GroupNote::MembersRemoved {
            group: id,
            atoms: atoms.iter().copied().collect(),
        }))
    }

    pub(crate) fn merge(
        &mut self,
        into: GroupId,
        from: GroupId,
    ) -> Result<GroupNote, StereoGroupError> {
        if into == from {
            return Err(StereoGroupError::Same);
        }
        let (target, source) = (self.require(into)?, self.require(from)?);
        if target.kind != source.kind {
            return Err(StereoGroupError::KindMismatch);
        }
        let note = GroupNote::Merged {
            into,
            from,
            into_number: target.number,
            from_number: source.number,
        };
        let moved = self.groups.remove(&from).map(|group| group.members);
        self.group_mut(into)
            .members
            .extend(moved.into_iter().flatten());
        Ok(note)
    }

    /// Moves `atoms` (some, not all, of the members) into the new group `new_group` of the same
    /// kind with `number`.
    pub(crate) fn split(
        &mut self,
        id: GroupId,
        new_group: GroupId,
        atoms: &BTreeSet<AtomId>,
        number: Option<i32>,
    ) -> Result<GroupNote, StereoGroupError> {
        let group = self.require(id)?;
        if group.kind == GroupKind::Absolute {
            return Err(StereoGroupError::SplitAbsolute);
        }
        if !atoms.is_subset(&group.members) {
            return Err(StereoGroupError::MemberAbsent);
        }
        if atoms.is_empty() || atoms.len() == group.members.len() {
            return Err(StereoGroupError::Empty);
        }
        let kind = group.kind;
        self.check_label(kind, number, None)?;
        let kept: BTreeSet<AtomId> = group.members.difference(atoms).copied().collect();
        self.group_mut(id).members = kept.clone();
        self.groups
            .insert(new_group, StereoGroup::new(kind, number, atoms.clone()));
        Ok(GroupNote::Split {
            group: id,
            new_group,
            moved: atoms.iter().copied().collect(),
            kept: kept.into_iter().collect(),
        })
    }

    /// Removes every member that is no longer a stored tetrahedral center, and every group left
    /// without members, and describes what left.
    pub(crate) fn prune(&mut self, stereo: &StereoTable) -> Vec<GroupNote> {
        let mut notes = Vec::new();
        let mut emptied = Vec::new();
        for (id, group) in &mut self.groups {
            let gone: Vec<(AtomId, LeaveCause)> = group
                .members
                .iter()
                .filter_map(|atom| match center_state(stereo, *atom) {
                    Some(CenterState::Tetrahedral { .. }) => None,
                    Some(CenterState::Unknown { .. }) => {
                        Some((*atom, LeaveCause::ConfigurationUnknown))
                    }
                    None => Some((*atom, LeaveCause::ConfigurationCleared)),
                })
                .collect();
            if gone.is_empty() {
                continue;
            }
            let relation_lost = group.kind != GroupKind::Absolute && group.members.len() >= 2;
            for (atom, cause) in gone {
                group.members.remove(&atom);
                notes.push(GroupNote::MemberLeft {
                    group: *id,
                    atom,
                    cause,
                    relation_lost,
                });
            }
            if group.members.is_empty() {
                emptied.push(*id);
            }
        }
        for id in emptied {
            self.groups.remove(&id);
            notes.push(GroupNote::GroupRemoved { group: id });
        }
        notes
    }

    fn group_mut(&mut self, id: GroupId) -> &mut StereoGroup {
        self.groups
            .get_mut(&id)
            .expect("the caller checked that the group exists")
    }

    /// Every atom must be a stored tetrahedral center that no group holds yet.
    fn check_free_centers(
        &self,
        atoms: &BTreeSet<AtomId>,
        stereo: &StereoTable,
    ) -> Result<(), StereoGroupError> {
        for atom in atoms {
            require_center(stereo, *atom)?;
        }
        if self
            .groups
            .values()
            .any(|group| !group.members.is_disjoint(atoms))
        {
            return Err(StereoGroupError::MemberTaken);
        }
        Ok(())
    }
}

fn center_state(stereo: &StereoTable, atom: AtomId) -> Option<&CenterState> {
    match stereo.get(StereoKey::Center(atom)) {
        Some(StereoState::Center(state)) => Some(state),
        _ => None,
    }
}

fn require_center(stereo: &StereoTable, atom: AtomId) -> Result<(), StereoGroupError> {
    match center_state(stereo, atom) {
        Some(CenterState::Tetrahedral { .. }) => Ok(()),
        _ => Err(StereoGroupError::MemberNotCenter),
    }
}

/// Keeps the notes about groups and members that existed in `original`, the table before the
/// transaction: what a transaction built and tore down itself is not reported.
pub(crate) fn report_existing(
    notes: Vec<GroupNote>,
    original: &StereoGroupTable,
) -> Vec<GroupNote> {
    let had = |group: GroupId, atom: AtomId| {
        original
            .get(group)
            .is_some_and(|group| group.members.contains(&atom))
    };
    notes
        .into_iter()
        .filter_map(|note| match note {
            GroupNote::MemberLeft { group, atom, .. } => had(group, atom).then_some(note),
            GroupNote::GroupRemoved { group } => original.get(group).is_some().then_some(note),
            GroupNote::MembersRemoved { group, atoms } => {
                let atoms: Vec<AtomId> =
                    atoms.into_iter().filter(|atom| had(group, *atom)).collect();
                (!atoms.is_empty()).then_some(GroupNote::MembersRemoved { group, atoms })
            }
            GroupNote::Split { group, .. } => original.get(group).is_some().then_some(note),
            GroupNote::Merged { into, from, .. } => {
                (original.get(into).is_some() && original.get(from).is_some()).then_some(note)
            }
        })
        .collect()
}
