//! Bounded Et display invariants. This module never changes the chemical graph.

use std::collections::BTreeSet;

use super::{Atom, AtomId, BondOrder, Command, Content, EditError, PositionKey};
use crate::element::Element;
use crate::scene::Selection;
use crate::stereo::StereoKey;

impl Content {
    pub(super) fn check_ethyl(&self, key: PositionKey, terminal: AtomId) -> Result<(), EditError> {
        let invalid = EditError::AbbreviationInvalid;
        if key.atom == terminal
            || self.instance_count() != 1
            || self.require_instance(key.instance).is_err()
        {
            return Err(invalid);
        }
        let carbon = Atom::from(Element::CARBON);
        if self.atoms.get(&key.atom) != Some(&carbon) || self.atoms.get(&terminal) != Some(&carbon)
        {
            return Err(invalid);
        }
        let members = [key.atom, terminal];
        let mut internal = 0;
        let mut boundary = 0;
        let mut local = BTreeSet::from(members);
        for bond in self.bonds.values() {
            let begin = members.contains(&bond.begin);
            let end = members.contains(&bond.end);
            if !begin && !end {
                continue;
            }
            if bond.order != BondOrder::Single || bond.mark.is_some() {
                return Err(invalid);
            }
            if begin && end {
                internal += 1;
            } else {
                // The visible anchor, never the hidden terminal, owns the one boundary.
                if (begin && bond.begin != key.atom) || (end && bond.end != key.atom) {
                    return Err(invalid);
                }
                boundary += 1;
                local.insert(if begin { bond.end } else { bond.begin });
            }
        }
        if internal != 1 || boundary != 1 {
            return Err(invalid);
        }
        // A boundary neighbor may carry a center or a marked/double configuration that this
        // compact depiction cannot faithfully show. Unrelated stereo elsewhere is retained.
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

    pub(super) fn validate_abbreviations(&self) -> Result<(), EditError> {
        let mut members = BTreeSet::new();
        for (key, terminal) in &self.abbreviations {
            self.check_ethyl(*key, *terminal)?;
            if !members.insert(key.atom) || !members.insert(*terminal) {
                return Err(EditError::AbbreviationInvalid);
            }
        }
        Ok(())
    }

    /// Drop stale display metadata in the same transaction as the triggering command.
    pub(super) fn reconcile_abbreviations(&mut self, before: &Self, command: &Command) {
        let remove: Vec<PositionKey> = self
            .abbreviations
            .iter()
            .filter_map(|(key, terminal)| {
                if self.check_ethyl(*key, *terminal).is_err() {
                    return Some(*key);
                }
                if before.abbreviations.get(key) != Some(terminal) {
                    return None;
                }
                let members = [key.atom, *terminal];
                // Even a supported state change must reveal what was edited. Boundary changes
                // count as well, including a change to its outside endpoint's recorded state.
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
                if delta(key.atom) == delta(*terminal) {
                    return None;
                }
                // Check command scope, rather than floating-point delta equality, for rigid
                // moves whose rounded coordinates may subtract differently at large magnitudes.
                let full_scope = match command {
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
                        && members.iter().all(|atom| atoms.contains(atom)) =>
                    {
                        return None;
                    }
                    Command::MoveInstance { instance, .. } if *instance == key.instance => {
                        return None;
                    }
                    _ => return Some(*key),
                };
                if full_scope.is_ok_and(|scope| {
                    members.iter().all(|atom| {
                        scope.positions.contains(&PositionKey {
                            instance: key.instance,
                            atom: *atom,
                        })
                    })
                }) {
                    return None;
                }
                Some(*key)
            })
            .collect();
        for key in remove {
            self.abbreviations.remove(&key);
        }
    }
}
