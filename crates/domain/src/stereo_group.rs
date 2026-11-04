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

    /// The number the group has on the wire (read by the P27b snapshot and writers): `None` for an absolute group, otherwise the stored
    /// number, or `0` when none is stored.
    pub fn interpreted_number(&self) -> Option<i32> {
        match self.kind {
            GroupKind::Absolute => None,
            GroupKind::And | GroupKind::Or => Some(self.number.unwrap_or(0)),
        }
    }

    /// Display text such as `abs`, `and2` or `or0`: output only, never stored (the P27b snapshot
    /// and the P27c group list show it).
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

/// A change of meaning that a transaction made to a group that existed before it, for the
/// reply to report. Notes are computed from the table before and after the transaction, by
/// atom (see [`meaning_changes`]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GroupNote {
    /// A member stopped being a stored tetrahedral center and so left its group. `relation_lost`:
    /// the group is `and` or `or` and had other members, whose relation to this atom is no
    /// longer stated.
    MemberLeft {
        group: GroupId,
        atom: AtomId,
        cause: LeaveCause,
        relation_lost: bool,
    },
    /// Every member of the group stopped being a stored tetrahedral center.
    GroupRemoved { group: GroupId },
    /// At least two surviving members of an `and` or `or` group are no longer all in one group
    /// (members removed, the group split or deleted, or members moved elsewhere). `together`
    /// holds the surviving members by the group they are in now, `ungrouped` those in none.
    RelationLost {
        group: GroupId,
        together: Vec<Vec<AtomId>>,
        ungrouped: Vec<AtomId>,
    },
    /// The surviving members of an `and` or `or` group are together in a group of another
    /// kind, or an `and` and an `or` group exchanged meaning: what the relation says changed.
    KindChanged {
        group: GroupId,
        atoms: Vec<AtomId>,
        from: GroupKind,
        to: GroupKind,
    },
    /// Members of two or more groups of one kind that existed before are now in `group`, so
    /// the members of different groups, which were independent, now belong together.
    /// `kind` is the kind of `group` now; `sources` holds each former group with the kind and
    /// interpreted number it had, which may differ from the new ones (then this note is also the
    /// report of that change, and no separate [`GroupNote::KindChanged`] is made for them); `intact`: every former
    /// group's surviving members are all in `group`.
    Merged {
        group: GroupId,
        number: Option<i32>,
        kind: GroupKind,
        sources: Vec<(GroupId, GroupKind, Option<i32>)>,
        intact: bool,
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

    /// The meaning of the groups with every label removed: each group's kind and member set,
    /// whatever its identifier or number. Two tables with an equal partition say the same about
    /// which centers belong together; numbers only tell groups of one kind apart. The P27b
    /// import and export checks compare documents through it.
    pub fn partition(&self) -> BTreeSet<(GroupKind, BTreeSet<AtomId>)> {
        self.groups
            .values()
            .map(|group| (group.kind, group.members.clone()))
            .collect()
    }

    /// The numbers a writer gives the groups, from the membership alone: per kind (`and`,
    /// `or`) dense from 1 in the order of each group's lowest member, none for `absolute`.
    /// Neither stored numbers nor identifiers enter, so relabelling a document does not change
    /// the result. The P27b writers use these as the wire numbering.
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

    /// Removes members, which become ungrouped; it never empties the group.
    pub(crate) fn remove_members(
        &mut self,
        id: GroupId,
        atoms: &BTreeSet<AtomId>,
    ) -> Result<(), StereoGroupError> {
        let group = self.require(id)?;
        if !atoms.is_subset(&group.members) {
            return Err(StereoGroupError::MemberAbsent);
        }
        if atoms.len() == group.members.len() {
            return Err(StereoGroupError::Empty);
        }
        self.group_mut(id)
            .members
            .retain(|atom| !atoms.contains(atom));
        Ok(())
    }

    pub(crate) fn merge(&mut self, into: GroupId, from: GroupId) -> Result<(), StereoGroupError> {
        if into == from {
            return Err(StereoGroupError::Same);
        }
        let (target, source) = (self.require(into)?, self.require(from)?);
        if target.kind != source.kind {
            return Err(StereoGroupError::KindMismatch);
        }
        let moved = self.groups.remove(&from).map(|group| group.members);
        self.group_mut(into)
            .members
            .extend(moved.into_iter().flatten());
        Ok(())
    }

    /// Moves `atoms` (some, not all, of the members) into the new group `new_group` of the same
    /// kind with `number`.
    pub(crate) fn split(
        &mut self,
        id: GroupId,
        new_group: GroupId,
        atoms: &BTreeSet<AtomId>,
        number: Option<i32>,
    ) -> Result<(), StereoGroupError> {
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
        Ok(())
    }

    /// Removes every member that is no longer a stored tetrahedral center, and every group left
    /// without members. Called once at the end of a transaction, so that a center cleared and
    /// stated again within it keeps its group; until then a group command sees members as they
    /// were stated.
    pub(crate) fn prune(&mut self, stereo: &StereoTable) {
        for group in self.groups.values_mut() {
            group.members.retain(|atom| {
                matches!(
                    center_state(stereo, *atom),
                    Some(CenterState::Tetrahedral { .. })
                )
            });
        }
        self.groups.retain(|_, group| !group.members.is_empty());
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

/// What a transaction changed in the meaning of the groups that existed before it, from the
/// table `before` and the table `after` (already pruned) and the stored configurations `after`.
///
/// One rule, applied by atom and not by group identifier: a note is produced when a relation
/// among at least two members of one group of `before` no longer holds, or says something else,
/// at the end of the transaction. A member that stopped being a stored tetrahedral center is a
/// [`GroupNote::MemberLeft`] (also for a one-member group) and a group that lost all members a
/// [`GroupNote::GroupRemoved`]. Of the members that survive as centers, those of an `and` or
/// `or` group that are no longer all in one group are a [`GroupNote::RelationLost`]; those
/// together in a group of another kind, a [`GroupNote::KindChanged`] (except that an `absolute`
/// group stated no relation to lose or change); and members of two or more groups of `before`,
/// of whatever kinds they had, now in one `and` or `or` group, a [`GroupNote::Merged`], which
/// also states the kind change when the kinds differ and then replaces their `KindChanged`
/// notes. A group of `absolute` kind states no relation, so members that end up in it are
/// never a merge (the kind change of the groups that had a relation is still reported).
/// Adding members, renumbering, and a group made and changed within the transaction are not
/// reported.
pub(crate) fn meaning_changes(
    before: &StereoGroupTable,
    after: &StereoGroupTable,
    stereo: &StereoTable,
) -> Vec<GroupNote> {
    let now: BTreeMap<AtomId, GroupId> = after
        .groups
        .iter()
        .flat_map(|(id, group)| group.members.iter().map(move |atom| (*atom, *id)))
        .collect();
    let mut notes = Vec::new();
    // Former groups by the group their surviving members are in now.
    let mut merged: BTreeMap<GroupId, Vec<(GroupId, bool)>> = BTreeMap::new();
    let mut kind_changes = Vec::new();
    let mut relations = Vec::new();
    for (id, group) in &before.groups {
        let relational = group.kind != GroupKind::Absolute && group.members.len() >= 2;
        let mut survivors = Vec::new();
        for atom in &group.members {
            match center_state(stereo, *atom) {
                Some(CenterState::Tetrahedral { .. }) => survivors.push(*atom),
                other => notes.push(GroupNote::MemberLeft {
                    group: *id,
                    atom: *atom,
                    cause: match other {
                        Some(_) => LeaveCause::ConfigurationUnknown,
                        None => LeaveCause::ConfigurationCleared,
                    },
                    relation_lost: relational,
                }),
            }
        }
        if survivors.is_empty() {
            notes.push(GroupNote::GroupRemoved { group: *id });
            continue;
        }
        let mut together: BTreeMap<GroupId, Vec<AtomId>> = BTreeMap::new();
        let mut ungrouped = Vec::new();
        for atom in &survivors {
            match now.get(atom) {
                Some(home) => together.entry(*home).or_default().push(*atom),
                None => ungrouped.push(*atom),
            }
        }
        for home in together.keys() {
            merged
                .entry(*home)
                .or_default()
                .push((*id, together.len() == 1 && ungrouped.is_empty()));
        }
        if survivors.len() < 2 || group.kind == GroupKind::Absolute {
            continue;
        }
        match (together.len(), ungrouped.is_empty()) {
            (1, true) => {
                let (home, atoms) = together.into_iter().next().expect("one group");
                let to = after.groups[&home].kind;
                if to != group.kind {
                    kind_changes.push((home, id, atoms, group.kind, to));
                }
            }
            _ => relations.push(GroupNote::RelationLost {
                group: *id,
                together: together.into_values().collect(),
                ungrouped,
            }),
        }
    }
    let mut merges = Vec::new();
    let mut merged_homes = BTreeSet::new();
    for (home, formers) in merged {
        let target = &after.groups[&home];
        if formers.len() >= 2 && target.kind != GroupKind::Absolute {
            merged_homes.insert(home);
            merges.push(GroupNote::Merged {
                group: home,
                number: target.interpreted_number(),
                kind: target.kind,
                intact: formers.iter().all(|(_, intact)| *intact),
                sources: formers
                    .into_iter()
                    .map(|(id, _)| {
                        let former = &before.groups[&id];
                        (id, former.kind, former.interpreted_number())
                    })
                    .collect(),
            });
        }
    }
    for (home, id, atoms, from, to) in kind_changes {
        if !merged_homes.contains(&home) {
            relations.push(GroupNote::KindChanged {
                group: *id,
                atoms,
                from,
                to,
            });
        }
    }
    notes.extend(relations);
    notes.extend(merges);
    notes
}
