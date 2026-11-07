//! Explicit fragment contraction checks and transactional reveal rules; text is never parsed.

use std::collections::BTreeSet;

use super::{
    Atom, AtomId, ChangeSet, Command, Content, EditError, FragmentDisplay, FragmentDisplayRun,
    MAX_ATOMS, PositionKey,
};
use crate::scene::Selection;
use crate::states::AtomKind;
use crate::stereo::StereoKey;

/// The byte budget bounds both text and the number of nonempty runs without a second limit.
pub(super) fn validate_runs(runs: &[FragmentDisplayRun]) -> Result<(), EditError> {
    let mut bytes = 0usize;
    let mut nonblank = false;
    for run in runs {
        bytes = bytes.saturating_add(run.text.len());
        if run.text.is_empty()
            || !crate::scene::valid_caption_run_text(&run.text)
            || bytes > super::abbreviation::MAX_DISPLAY_LABEL_BYTES
        {
            return Err(EditError::DisplayLabelInvalid);
        }
        nonblank |= run.text.chars().any(|ch| !ch.is_whitespace());
    }
    if !nonblank {
        return Err(EditError::DisplayLabelInvalid);
    }
    Ok(())
}

impl FragmentDisplay {
    /// Validate local data even when a history entry will be discarded by retention.
    fn validate_shape(&self, anchor: AtomId) -> Result<BTreeSet<AtomId>, EditError> {
        validate_runs(&self.runs)?;
        let members: BTreeSet<AtomId> = self.members.iter().copied().collect();
        if members.is_empty()
            || members.len() > MAX_ATOMS
            || members.len() != self.members.len()
            || !members.contains(&anchor)
        {
            return Err(EditError::FragmentDisplayInvalid);
        }
        Ok(members)
    }
}

impl ChangeSet {
    pub(super) fn validate_fragment_display_changes(&self) -> Result<(), EditError> {
        let mut keys = BTreeSet::new();
        for (key, change) in &self.fragment_displays {
            if !keys.insert(*key) || change.before == change.after {
                return Err(EditError::FragmentDisplayInvalid);
            }
            for display in [&change.before, &change.after].into_iter().flatten() {
                display.validate_shape(key.atom)?;
            }
        }
        Ok(())
    }
}

impl Content {
    pub(super) fn check_fragment_display(
        &self,
        key: PositionKey,
        display: &FragmentDisplay,
    ) -> Result<(), EditError> {
        let invalid = EditError::FragmentDisplayInvalid;
        let members = display.validate_shape(key.atom)?;
        if self.instance_count() != 1 || self.require_instance(key.instance).is_err() {
            return Err(invalid);
        }
        for id in &members {
            let Some(atom) = self.atoms.get(id) else {
                return Err(invalid);
            };
            let AtomKind::Element(element) = &atom.kind else {
                return Err(invalid);
            };
            if atom != &Atom::from(*element) {
                return Err(invalid);
            }
        }
        let mut internal = Vec::new();
        let mut local = members.clone();
        let mut boundaries = 0;
        for bond in self.bonds.values() {
            let begin = members.contains(&bond.begin);
            let end = members.contains(&bond.end);
            if !begin && !end {
                continue;
            }
            if bond.order != super::BondOrder::Single || bond.mark.is_some() {
                return Err(invalid);
            }
            if begin && end {
                internal.push((bond.begin, bond.end));
            } else {
                if (begin && bond.begin != key.atom) || (end && bond.end != key.atom) {
                    return Err(invalid);
                }
                boundaries += 1;
                local.insert(if begin { bond.end } else { bond.begin });
            }
        }
        if boundaries != 1 || internal.len() != members.len() - 1 {
            return Err(invalid);
        }
        // Edge count plus explicit reachability proves that the induced graph is a tree.
        let mut reached = BTreeSet::from([key.atom]);
        loop {
            let count = reached.len();
            for (begin, end) in &internal {
                if reached.contains(begin) {
                    reached.insert(*end);
                }
                if reached.contains(end) {
                    reached.insert(*begin);
                }
            }
            if count == reached.len() {
                break;
            }
        }
        if reached != members {
            return Err(invalid);
        }
        if self.bonds.values().any(|bond| {
            bond.mark.is_some() && (local.contains(&bond.begin) || local.contains(&bond.end))
        }) || self.stereo.iter().any(|(key, _)| match key {
            StereoKey::Center(atom) => local.contains(&atom),
            StereoKey::DoubleBond(id) => self
                .bonds
                .get(&id)
                .is_some_and(|bond| local.contains(&bond.begin) || local.contains(&bond.end)),
        }) || self
            .stereo_groups
            .iter()
            .any(|(_, group)| group.members().iter().any(|atom| local.contains(atom)))
        {
            return Err(invalid);
        }
        Ok(())
    }

    pub(super) fn check_fragment_overlap(
        &self,
        key: PositionKey,
        display: &FragmentDisplay,
    ) -> Result<(), EditError> {
        if self.abbreviations.iter().any(|(other, terminal)| {
            display.members.contains(&other.atom) || display.members.contains(terminal)
        }) || self.fragment_displays.iter().any(|(other, stored)| {
            // Repeating an identical membership request is harmless; editing text is explicit.
            (*other != key || stored.members != display.members)
                && stored.members.iter().any(|id| display.members.contains(id))
        }) {
            return Err(EditError::FragmentDisplayInvalid);
        }
        Ok(())
    }

    pub(super) fn validate_fragment_displays(&self) -> Result<(), EditError> {
        for (key, display) in &self.fragment_displays {
            self.check_fragment_display(*key, display)?;
            self.check_fragment_overlap(*key, display)?;
        }
        Ok(())
    }

    pub(super) fn reconcile_fragment_displays(&mut self, before: &Self, command: &Command) {
        let remove: Vec<PositionKey> = self
            .fragment_displays
            .iter()
            .filter_map(|(key, display)| {
                if self.check_fragment_display(*key, display).is_err() {
                    return Some(*key);
                }
                let old = before.fragment_displays.get(key)?;
                if old.members != display.members {
                    return None;
                }
                let members: BTreeSet<AtomId> = display.members.iter().copied().collect();
                let incident = |content: &Self| {
                    content
                        .bonds
                        .iter()
                        .filter(|(_, bond)| {
                            members.contains(&bond.begin) || members.contains(&bond.end)
                        })
                        .map(|(id, bond)| (*id, bond.clone()))
                        .collect::<Vec<_>>()
                };
                let old_bonds = incident(before);
                let new_bonds = incident(self);
                let touched: BTreeSet<AtomId> = old_bonds
                    .iter()
                    .chain(&new_bonds)
                    .flat_map(|(_, bond)| [bond.begin, bond.end])
                    .chain(display.members.iter().copied())
                    .collect();
                if old_bonds != new_bonds
                    || touched
                        .iter()
                        .any(|id| before.atoms.get(id) != self.atoms.get(id))
                {
                    return Some(*key);
                }
                let delta = |atom| {
                    let position = PositionKey {
                        instance: key.instance,
                        atom,
                    };
                    before
                        .positions
                        .get(&position)
                        .zip(self.positions.get(&position))
                        .map(|(a, b)| (b.x() - a.x(), b.y() - a.y()))
                };
                if display
                    .members
                    .iter()
                    .all(|atom| delta(*atom) == delta(key.atom))
                {
                    return None;
                }
                // Full command scope preserves a rigid transform despite floating point rounding.
                let scope = match command {
                    Command::MoveSelection {
                        selection,
                        instance,
                        ..
                    }
                    | Command::TransformSelection {
                        selection,
                        instance,
                        ..
                    } => before.selection_scope(selection, *instance),
                    Command::MoveObjects { objects, .. }
                    | Command::ArrangeObjects { objects, .. } => before.selection_scope(
                        &Selection {
                            objects: objects.iter().copied().collect(),
                            ..Selection::default()
                        },
                        None,
                    ),
                    Command::MoveAtoms {
                        atoms, instance, ..
                    } if instance.is_none_or(|id| id == key.instance)
                        && display.members.iter().all(|atom| atoms.contains(atom)) =>
                    {
                        return None;
                    }
                    Command::MoveInstance { instance, .. } if *instance == key.instance => {
                        return None;
                    }
                    _ => return Some(*key),
                };
                if scope.is_ok_and(|scope| {
                    display.members.iter().all(|atom| {
                        scope.positions.contains(&PositionKey {
                            instance: key.instance,
                            atom: *atom,
                        })
                    })
                }) {
                    None
                } else {
                    Some(*key)
                }
            })
            .collect();
        for key in remove {
            self.fragment_displays.remove(&key);
        }
    }
}
