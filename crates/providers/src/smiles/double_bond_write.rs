//! Where the recorded SMILES writer puts `/` and `\` for stored double-bond configurations.
//!
//! Rules are those of the OpenSMILES specification, section "Cis/Trans Configuration of Double
//! Bonds" (<https://github.com/opensmiles/OpenSMILES>): a mark states on which side of the double
//! bond the atom after it lies, relative to the atom before it, so the same stored configuration
//! is `F/C=C/F` or `F\C=C\F` depending on how the text happens to be ordered. This module
//! decides *which bonds* carry a mark and *which side* each states; [`Link::mark`] holds that
//! side in a form independent of the text, and the emitter turns it into `/` or `\` from the
//! order it actually writes. The inverse of the reader's table ([`Mark::toward`]) does the
//! conversion, so the two cannot drift apart.
//!
//! The rest is Chemifree's own policy, decided for this writer (not derived from the
//! specification):
//!
//! - Each written configuration gets the fewest marks that state it: one marked single bond at
//!   each end. A bond that a double bond already placed marks on is used again (a conjugated
//!   chain shares them); otherwise the first neighbour in emission order that is not written as
//!   a ring-closure digit is marked. A mark is never put on a ring-closure digit.
//! - The configurations are placed greedily in bond-identifier order. A configuration is
//!   omitted, and reported as a loss, rather than written when the marks it needs
//!   contradict marks already placed, or when they would also state something else: a double
//!   bond that is marked at both ends is read as configured, so it must be a configuration that
//!   is stored, not yet omitted, and agrees with the marks.
//! - Also omitted: an unknown state, a bond that is not double in the written text, whatever the
//!   reader would refuse (a cumulated bond, an end it does not read, a ring of fewer than eight
//!   atoms: [`Topology::write_fault`]), and an end with no neighbour that a mark can sit on.
//!
//! The result is never trusted: the writer reads the finished text back and requires exactly
//! the retained configurations.

use std::collections::{BTreeSet, HashMap};

use chemifree_application::smiles::{SmilesBond, SmilesDoubleBond};
use chemifree_domain::document::{AtomId, BondId, BondOrder};
use chemifree_domain::stereo::{DoubleBondState, Relation, Slot};

use super::double_bonds::{Mark, Side, Topology};
use super::emit::Link;

/// A stored double-bond state to write.
pub(super) struct Request {
    pub bond: BondId,
    /// Index of the double bond in the links.
    pub link: usize,
    pub state: DoubleBondState,
}

/// What placement needs to know about the text that is going to be written.
pub(super) struct Marking<'a> {
    /// The requests in the order they are placed.
    pub requests: &'a [Request],
    /// The drawing's atom for each written atom.
    pub ids: &'a [AtomId],
    /// The links, without marks.
    pub links: &'a [Link],
    /// The drawing's bond for each link.
    pub bonds: &'a [BondId],
    /// The reader's judgement of the atoms and bonds as they will be written.
    pub topology: &'a Topology<'a>,
    /// Every atom's neighbours in the order the text reaches them.
    pub neighbors: &'a [Vec<usize>],
    /// For every link whether the text writes it as a ring-closure digit.
    pub closure: &'a [bool],
    /// The position in the text of every atom.
    pub position: &'a [usize],
}

/// The marks to write and what became of every request.
pub(super) struct Placement {
    /// For every link the mark it carries, as [`Link::mark`] defines it.
    pub marks: Vec<Option<Mark>>,
    /// The requests (by index) the marks state, in request order.
    pub retained: Vec<usize>,
    /// The requests (by index) left out, in request order, with the reason.
    pub omitted: Vec<(usize, String)>,
}

/// The relation between the neighbours `begin_neighbor` of the bond's begin atom and
/// `end_neighbor` of its end atom that `state` stores, which names its own reference neighbours.
/// `None` for an unknown state. Both neighbours must be neighbours of their ends other than the
/// double bond's partner; each end has two positions, so the other one flips the relation.
pub(super) fn relation_between(
    state: DoubleBondState,
    begin_neighbor: AtomId,
    end_neighbor: AtomId,
) -> Option<Relation> {
    let DoubleBondState::Relative {
        begin_ref,
        end_ref,
        relation,
    } = state
    else {
        return None;
    };
    let mut relation = relation;
    for (neighbor, reference) in [(begin_neighbor, begin_ref), (end_neighbor, end_ref)] {
        if Slot::Atom(neighbor) != reference {
            relation = relation.flipped();
        }
    }
    Some(relation)
}

/// Whether the text read back states exactly the configurations of `expected`: the same double
/// bonds, none more and none fewer, each with the relation it stores.
///
/// `read` and `read_bonds` are what the reader found in the text; `order` is the written atom at
/// each position of the text.
pub(super) fn read_back_matches(
    read: &[SmilesDoubleBond],
    read_bonds: &[SmilesBond],
    order: &[usize],
    ids: &[AtomId],
    links: &[Link],
    expected: &[&Request],
) -> bool {
    let link_between: HashMap<(usize, usize), usize> = links
        .iter()
        .enumerate()
        .map(|(index, link)| ((link.begin.min(link.end), link.begin.max(link.end)), index))
        .collect();
    let mut found = BTreeSet::new();
    for configuration in read {
        let bond = &read_bonds[configuration.bond];
        let (one, other) = (order[bond.begin], order[bond.end]);
        let Some(&link) = link_between.get(&(one.min(other), one.max(other))) else {
            return false;
        };
        let (neighbor_of_one, neighbor_of_other) = (
            order[configuration.begin_neighbor],
            order[configuration.end_neighbor],
        );
        let (begin_neighbor, end_neighbor) = if one == links[link].begin {
            (neighbor_of_one, neighbor_of_other)
        } else {
            (neighbor_of_other, neighbor_of_one)
        };
        let Some(request) = expected.iter().find(|request| request.link == link) else {
            return false;
        };
        let stored = relation_between(request.state, ids[begin_neighbor], ids[end_neighbor]);
        if stored != Some(configuration.relation) || !found.insert(link) {
            return false;
        }
    }
    found.len() == expected.len()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Status {
    /// Not placed yet, and not ruled out.
    Pending,
    Retained,
    Omitted,
}

struct Placer<'a> {
    input: &'a Marking<'a>,
    marks: Vec<Option<Mark>>,
    status: Vec<Status>,
    reasons: Vec<Option<String>>,
    /// The request of each double-bond link that has one.
    request_of: HashMap<usize, usize>,
    /// The link joining each pair of atoms, smaller index first.
    link_between: HashMap<(usize, usize), usize>,
    atom_of: HashMap<AtomId, usize>,
}

/// Places the marks of `input.requests`.
pub(super) fn place(input: &Marking<'_>) -> Placement {
    let mut placer = Placer {
        input,
        marks: vec![None; input.links.len()],
        status: vec![Status::Pending; input.requests.len()],
        reasons: vec![None; input.requests.len()],
        request_of: input
            .requests
            .iter()
            .enumerate()
            .map(|(index, request)| (request.link, index))
            .collect(),
        link_between: input
            .links
            .iter()
            .enumerate()
            .map(|(index, link)| ((link.begin.min(link.end), link.begin.max(link.end)), index))
            .collect(),
        atom_of: input
            .ids
            .iter()
            .enumerate()
            .map(|(index, &id)| (id, index))
            .collect(),
    };
    // Whatever can be decided without looking at marks is decided first, so that the
    // placement below knows which double bonds may be marked at both ends.
    for index in 0..input.requests.len() {
        if let Err(reason) = placer.screen(index) {
            placer.omit(index, reason);
        }
    }
    for index in 0..input.requests.len() {
        if placer.status[index] != Status::Pending {
            continue;
        }
        match placer.attempt(index) {
            Ok(()) => placer.status[index] = Status::Retained,
            Err(reason) => placer.omit(index, reason),
        }
    }
    let mut retained = Vec::new();
    let mut omitted = Vec::new();
    for index in 0..input.requests.len() {
        match (placer.status[index], placer.reasons[index].take()) {
            (Status::Retained, _) => retained.push(index),
            (_, Some(reason)) => omitted.push((index, reason)),
            (_, None) => unreachable!("an omitted request always has a reason"),
        }
    }
    Placement {
        marks: placer.marks,
        retained,
        omitted,
    }
}

impl Placer<'_> {
    fn omit(&mut self, index: usize, reason: String) {
        self.status[index] = Status::Omitted;
        self.reasons[index] = Some(format!(
            "This double-bond configuration is not written as / or \\: {reason}."
        ));
    }

    fn link_of(&self, one: usize, other: usize) -> usize {
        self.link_between[&(one.min(other), one.max(other))]
    }

    /// The neighbours of `atom` that a mark can sit on, in the order the text reaches them: the
    /// bond is single and is not written as a ring-closure digit.
    fn markable(&self, atom: usize, partner: usize) -> impl Iterator<Item = usize> + '_ {
        self.input.neighbors[atom]
            .iter()
            .copied()
            .filter(move |&other| other != partner)
            .filter(move |&other| {
                let link = self.link_of(atom, other);
                !self.input.closure[link] && self.input.links[link].order == BondOrder::Single
            })
    }

    /// Why the request cannot be written whatever else is written, when it cannot.
    fn screen(&self, index: usize) -> Result<(), String> {
        let request = &self.input.requests[index];
        if self.input.links[request.link].order != BondOrder::Double {
            return Err("the bond is not a double bond in the written text".into());
        }
        let DoubleBondState::Relative {
            begin_ref, end_ref, ..
        } = request.state
        else {
            return Err("an unknown configuration has no directional-bond form".into());
        };
        if let Some(fault) = self.input.topology.write_fault(request.link) {
            return Err(format!("{fault}, so the reader would refuse it"));
        }
        let link = &self.input.links[request.link];
        for (atom, partner, reference) in [
            (link.begin, link.end, begin_ref),
            (link.end, link.begin, end_ref),
        ] {
            let referred = match reference {
                Slot::Atom(id) => self.atom_of.get(&id).copied(),
                Slot::ImplicitHydrogen | Slot::LonePair => None,
            };
            if !referred.is_some_and(|other| {
                other != partner && self.input.neighbors[atom].contains(&other)
            }) {
                return Err(
                    "an end of the double bond is referred to by a position that is not \
                    a bonded atom (an implicit hydrogen or a lone pair), which no mark can name"
                        .into(),
                );
            }
            if self.markable(atom, partner).next().is_none() {
                return Err(
                    "an end of the double bond has no single bond that is not a \
                    ring-closure digit for a mark to sit on"
                        .into(),
                );
            }
        }
        Ok(())
    }

    /// The side of the neighbour joined to `atom` by `link`, when the link is marked.
    fn side(&self, atom: usize, link: usize) -> Option<Side> {
        let mark = self.marks[link]?;
        Some(mark.side_from(atom == self.input.links[link].begin))
    }

    /// The marked neighbours of `atom` other than across `except`, in the order of the text,
    /// with the side each lies on.
    fn marked_at(&self, atom: usize, except: usize) -> Vec<(usize, Side)> {
        self.input.neighbors[atom]
            .iter()
            .filter_map(|&other| {
                let link = self.link_of(atom, other);
                if link == except {
                    return None;
                }
                Some((other, self.side(atom, link)?))
            })
            .collect()
    }

    /// The neighbour of `atom` this end of the double bond is stated by: a neighbour that is
    /// already marked, so that a bond serves every double bond it touches, or else the first
    /// markable one.
    fn chosen(&self, atom: usize, partner: usize) -> usize {
        let link = self.link_of(atom, partner);
        self.marked_at(atom, link)
            .first()
            .map(|&(other, _)| other)
            .or_else(|| self.markable(atom, partner).next())
            .expect("screening found a markable neighbour at every end")
    }

    fn atom_id(&self, atom: usize) -> AtomId {
        self.input.ids[atom]
    }

    /// Writes the marks of one configuration, or reverts them and says why not.
    fn attempt(&mut self, index: usize) -> Result<(), String> {
        let request = &self.input.requests[index];
        let link = &self.input.links[request.link];
        // The ends are taken in the order the text reaches them. Where nothing fixes the sides,
        // the first end chooses them (see below), so the choice follows the text, not the end the
        // drawing names as the bond's begin.
        let mut ends = [(link.begin, link.end), (link.end, link.begin)];
        ends.sort_by_key(|&(atom, _)| self.input.position[atom]);
        let [(first, first_partner), (second, second_partner)] = ends;
        let first_neighbor = self.chosen(first, first_partner);
        let second_neighbor = self.chosen(second, second_partner);
        let (begin_neighbor, end_neighbor) = if first == link.begin {
            (first_neighbor, second_neighbor)
        } else {
            (second_neighbor, first_neighbor)
        };
        let wanted = relation_between(
            request.state,
            self.atom_id(begin_neighbor),
            self.atom_id(end_neighbor),
        )
        .expect("only stored relative states are placed");
        let first_link = self.link_of(first, first_neighbor);
        let second_link = self.link_of(second, second_neighbor);
        let follows = |side: Side, relation: Relation| match relation {
            Relation::SameSide => side,
            Relation::OppositeSides => side.opposite(),
        };
        let (first_side, second_side) =
            match (self.side(first, first_link), self.side(second, second_link)) {
                (Some(one), Some(other)) => {
                    if (one == other) != (wanted == Relation::SameSide) {
                        return Err(
                            "directional bonds already written for a neighbouring double \
                        bond put its substituents on the other sides"
                                .into(),
                        );
                    }
                    (one, other)
                }
                (Some(one), None) => (one, follows(one, wanted)),
                (None, Some(other)) => (follows(other, wanted), other),
                (None, None) => {
                    // Nothing fixes the sides yet, so the first end's neighbour is put on
                    // `Side::Up` when the text writes it after that end and on `Side::Down` when
                    // before. That writes the mark on the bond between them as `/`; the other
                    // choice states the same configuration with both of its marks reversed.
                    let one = if self.input.position[first] < self.input.position[first_neighbor] {
                        Side::Up
                    } else {
                        Side::Down
                    };
                    (one, follows(one, wanted))
                }
            };
        let mut changed = Vec::new();
        for (atom, link, side) in [
            (first, first_link, first_side),
            (second, second_link, second_side),
        ] {
            if self.marks[link].is_none() {
                let from_first = atom == self.input.links[link].begin;
                self.marks[link] = Some(Mark::toward(side, from_first));
                changed.push(link);
            }
        }
        match self.consequences(request.link, &changed) {
            Ok(()) => Ok(()),
            Err(reason) => {
                for link in changed {
                    self.marks[link] = None;
                }
                Err(reason)
            }
        }
    }

    /// Whether the marks just placed on `changed` leave every double bond they touch (and
    /// `own`, the one being placed) stating only what is stored.
    fn consequences(&self, own: usize, changed: &[usize]) -> Result<(), String> {
        let links = self.input.links;
        let mut touched = BTreeSet::from([own]);
        for &link in changed {
            for atom in [links[link].begin, links[link].end] {
                for &other in &self.input.neighbors[atom] {
                    let candidate = self.link_of(atom, other);
                    if links[candidate].order == BondOrder::Double {
                        touched.insert(candidate);
                    }
                }
            }
        }
        touched
            .into_iter()
            .try_for_each(|link| self.states_only_what_is_stored(link))
    }

    /// How the reader reads the double bond `link` from the marks as they are now: a bond
    /// marked at only one end states nothing; one marked at both ends is configured, so it must
    /// be a request still being written, whose relation the marks agree with.
    fn states_only_what_is_stored(&self, link: usize) -> Result<(), String> {
        let double = &self.input.links[link];
        let at_begin = self.marked_at(double.begin, link);
        let at_end = self.marked_at(double.end, link);
        let (Some(&(begin_neighbor, begin_side)), Some(&(end_neighbor, end_side))) =
            (at_begin.first(), at_end.first())
        else {
            return Ok(());
        };
        let name = |atom: usize| self.atom_id(atom);
        for marks in [&at_begin, &at_end] {
            if let [(_, one), (_, other)] = marks.as_slice()
                && one == other
            {
                return Err(format!(
                    "the directional bonds it needs would put both neighbours of one end of \
                     double bond {} on the same side",
                    self.bond_name(link)
                ));
            }
        }
        let stated = if begin_side == end_side {
            Relation::SameSide
        } else {
            Relation::OppositeSides
        };
        let stored = self.request_of.get(&link).and_then(|&request| {
            (self.status[request] != Status::Omitted).then(|| {
                relation_between(
                    self.input.requests[request].state,
                    name(begin_neighbor),
                    name(end_neighbor),
                )
            })
        });
        match stored {
            Some(Some(relation)) if relation == stated => Ok(()),
            Some(Some(_)) => Err(format!(
                "the directional bonds it needs would state the opposite configuration for \
                 double bond {}",
                self.bond_name(link)
            )),
            _ => Err(format!(
                "the directional bonds it needs would also state a configuration for double \
                 bond {}, which is not stored or is not written",
                self.bond_name(link)
            )),
        }
    }

    fn bond_name(&self, link: usize) -> String {
        self.input.bonds[link].to_string()
    }
}
