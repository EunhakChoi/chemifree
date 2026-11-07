//! Turns a list of atoms and bonds into SMILES text by a depth-first walk.
//!
//! Atoms are given in priority order: the walk starts at the lowest-numbered atom of each
//! component and visits neighbours lowest first, so the text is a function of the numbering and
//! of nothing else. The last child of an atom continues the chain; earlier children are
//! branches. Ring labels are the lowest free number from 1; a label that closes at an atom is
//! free again for the rings that open at that same atom.
//!
//! That order can nest branches deeper than the reader accepts although another order of the
//! same tree would not (a long backbone whose pendant atoms are numbered last). Only then, the
//! child that has the deepest branching continues the chain instead (see
//! [`reorder_for_depth`]); every text that fits keeps the order above, and the order used is
//! still a function of the numbering alone.

use chemifree_application::smiles::{SmilesCode, SmilesError};
use chemifree_domain::document::BondOrder;
use chemifree_domain::stereo::Turn;

use super::double_bonds::Mark;
use super::{MAX_BRANCH_DEPTH, MAX_RING_LABEL};

/// One atom to write.
#[derive(Clone, Debug)]
pub(super) struct Item {
    /// The symbol as it appears in the text: lower case for an aromatic atom.
    pub symbol: &'static str,
    pub aromatic: bool,
    pub charge: i8,
    pub isotope: Option<u16>,
    pub hydrogens: u16,
    pub bracket: bool,
    pub turn: Option<Turn>,
}

/// A bond between two items. Only single, double, triple and aromatic bonds can be written;
/// [`emit`] refuses any other.
#[derive(Clone, Debug)]
pub(super) struct Link {
    pub begin: usize,
    pub end: usize,
    pub order: BondOrder,
    /// A directional bond, as it reads when `begin` is written before `end`; [`emit`] writes
    /// the other symbol when the walk reaches `end` first. Only a single bond that the walk
    /// writes in the chain or a branch can carry one: a ring-closure bond, or any other order,
    /// makes [`emit`] refuse.
    pub mark: Option<Mark>,
}

pub(super) struct Emitted {
    pub text: String,
    /// The item written at each position of the text.
    pub order: Vec<usize>,
    /// Neighbours in the exact order the reader encounters them, excluding bracket H.
    pub neighbors: Vec<Vec<usize>>,
    pub preceded: Vec<bool>,
    /// For every link whether the walk wrote it as a ring-closure digit.
    pub closure: Vec<bool>,
}

fn atom_text(item: &Item) -> String {
    if !item.bracket {
        return item.symbol.to_owned();
    }
    let mut text = String::from("[");
    if let Some(mass) = item.isotope {
        text.push_str(&mass.to_string());
    }
    text.push_str(item.symbol);
    match item.turn {
        Some(Turn::Anticlockwise) => text.push('@'),
        Some(Turn::Clockwise) => text.push_str("@@"),
        None => {}
    }
    match item.hydrogens {
        0 => {}
        1 => text.push('H'),
        count => text.push_str(&format!("H{count}")),
    }
    match item.charge {
        0 => {}
        1 => text.push('+'),
        -1 => text.push('-'),
        charge if charge > 0 => text.push_str(&format!("+{charge}")),
        charge => text.push_str(&format!("-{}", charge.unsigned_abs())),
    }
    text.push(']');
    text
}

/// An unspecified bond between two aromatic atoms reads as aromatic on a ring, so a single bond
/// there is written out.
fn bond_text(link: &Link, items: &[Item]) -> Result<&'static str, SmilesError> {
    match link.order {
        BondOrder::Aromatic => Ok(""),
        BondOrder::Single if items[link.begin].aromatic && items[link.end].aromatic => Ok("-"),
        BondOrder::Single => Ok(""),
        BondOrder::Double => Ok("="),
        BondOrder::Triple => Ok("#"),
        BondOrder::Dative | BondOrder::Query(_) | BondOrder::Unknown => {
            Err(SmilesError::new(SmilesCode::BondKindNotWritable))
        }
    }
}

/// The text of the bond that joins `child` to the atom written before it. A directional mark is
/// written as the walk meets it: the symbol of [`Link::mark`] when the parent is `begin`.
fn chain_bond_text(link: &Link, items: &[Item], child: usize) -> Result<&'static str, SmilesError> {
    let Some(mark) = link.mark else {
        return bond_text(link, items);
    };
    if link.order != BondOrder::Single {
        return Err(SmilesError::new(SmilesCode::ReadbackMismatch)
            .because("a directional bond is placed on a bond that is not single"));
    }
    let parent_is_begin = link.end == child;
    Ok(if parent_is_begin {
        mark
    } else {
        mark.flipped()
    }
    .symbol())
}

fn label_text(label: usize) -> String {
    if label < 10 {
        label.to_string()
    } else {
        format!("%{label:02}")
    }
}

enum Work {
    Atom(usize, Option<usize>),
    Open,
    Close,
    Dot,
}

/// The spanning forest of the walk: tree children per atom, and per atom the ring closures
/// that open and close there.
struct Walk {
    roots: Vec<usize>,
    /// Every atom in the order the walk reached it: a parent before each of its children.
    reached: Vec<usize>,
    children: Vec<Vec<(usize, usize)>>,
    opens: Vec<Vec<(usize, usize)>>,
    closes: Vec<Vec<usize>>,
}

fn walk(count: usize, links: &[Link]) -> Walk {
    debug_assert!(
        links
            .iter()
            .all(|link| link.begin != link.end && link.begin.max(link.end) < count),
        "the writer refuses bonds that do not join two different atoms"
    );
    let mut adjacent: Vec<Vec<(usize, usize)>> = vec![Vec::new(); count];
    for (index, link) in links.iter().enumerate() {
        adjacent[link.begin].push((link.end, index));
        adjacent[link.end].push((link.begin, index));
    }
    for neighbours in &mut adjacent {
        neighbours.sort_unstable();
    }
    let mut walk = Walk {
        roots: Vec::new(),
        reached: Vec::with_capacity(count),
        children: vec![Vec::new(); count],
        opens: vec![Vec::new(); count],
        closes: vec![Vec::new(); count],
    };
    let mut visited = vec![false; count];
    let mut used = vec![false; links.len()];
    for start in 0..count {
        if visited[start] {
            continue;
        }
        walk.roots.push(start);
        walk.reached.push(start);
        visited[start] = true;
        let mut stack = vec![(start, 0_usize)];
        while let Some((atom, next)) = stack.last().copied() {
            let Some(&(neighbour, link)) = adjacent[atom].get(next) else {
                stack.pop();
                continue;
            };
            if let Some(top) = stack.last_mut() {
                top.1 += 1;
            }
            if std::mem::replace(&mut used[link], true) {
                continue;
            }
            if visited[neighbour] {
                // An unused link to a visited atom reaches an ancestor still on the stack.
                walk.opens[neighbour].push((atom, link));
                walk.closes[atom].push(link);
            } else {
                visited[neighbour] = true;
                walk.reached.push(neighbour);
                walk.children[atom].push((neighbour, link));
                stack.push((neighbour, 0));
            }
        }
    }
    walk
}

/// The deepest branch nesting the children of each atom would write below that atom, in the
/// order they are in now, or in the order that minimises it when `minimise` is set. With
/// `minimise`, each atom's deepest child is moved last (the one latest in the current order
/// among equals) and the others keep their relative order. The deepest child continuing the
/// chain is the order of least depth: the chain adds no nesting, every branch adds one.
fn nesting(walk: &mut Walk, minimise: bool) -> Vec<usize> {
    let mut depth = vec![0_usize; walk.children.len()];
    // Children are reached after their parent, so the reverse order sees children first.
    for &atom in walk.reached.iter().rev() {
        let children = &mut walk.children[atom];
        if minimise && let Some(deepest) = (0..children.len()).max_by_key(|&i| depth[children[i].0])
        {
            let child = children.remove(deepest);
            children.push(child);
        }
        depth[atom] = match children.split_last() {
            None => 0,
            Some((&(last, _), earlier)) => earlier
                .iter()
                .map(|&(child, _)| depth[child] + 1)
                .fold(depth[last], usize::max),
        };
    }
    depth
}

/// Leaves the order alone when the text fits the reader's branch depth, and otherwise puts the
/// deepest child of every atom last, which is the least depth any order gives this tree.
/// Whether the order is changed depends only on the numbering of the atoms and bonds.
fn reorder_for_depth(walk: &mut Walk) {
    let depth = nesting(walk, false);
    let deepest = walk.roots.iter().map(|&root| depth[root]).max();
    if deepest.is_some_and(|deepest| deepest > MAX_BRANCH_DEPTH) {
        nesting(walk, true);
    }
}

/// Writes the items and links, or fails when the text would need more ring labels or deeper
/// branches than the reader accepts.
pub(super) fn emit(items: &[Item], links: &[Link]) -> Result<Emitted, SmilesError> {
    let mut walk = walk(items.len(), links);
    reorder_for_depth(&mut walk);
    let mut work = Vec::new();
    for (position, &root) in walk.roots.iter().enumerate().rev() {
        work.push(Work::Atom(root, None));
        if position > 0 {
            work.push(Work::Dot);
        }
    }
    let mut text = String::new();
    let mut order = Vec::with_capacity(items.len());
    let mut taken = [false; MAX_RING_LABEL + 1];
    let mut label_of = vec![0_usize; links.len()];
    let mut depth = 0_usize;
    let mut neighbors = vec![Vec::new(); items.len()];
    let mut preceded = vec![false; items.len()];
    let mut closure = vec![false; links.len()];
    while let Some(step) = work.pop() {
        match step {
            Work::Dot => text.push('.'),
            Work::Open => {
                depth += 1;
                if depth > MAX_BRANCH_DEPTH {
                    return Err(SmilesError::new(SmilesCode::BranchDepthLimit));
                }
                text.push('(');
            }
            Work::Close => {
                depth -= 1;
                text.push(')');
            }
            Work::Atom(atom, via) => {
                if let Some(link) = via {
                    text.push_str(chain_bond_text(&links[link], items, atom)?);
                    let edge = &links[link];
                    neighbors[atom].push(if edge.begin == atom {
                        edge.end
                    } else {
                        edge.begin
                    });
                    preceded[atom] = true;
                }
                text.push_str(&atom_text(&items[atom]));
                order.push(atom);
                let mut closing: Vec<usize> = walk.closes[atom].to_vec();
                closing.sort_unstable_by_key(|&link| label_of[link]);
                for link in closing {
                    let label = label_of[link];
                    let edge = &links[link];
                    neighbors[atom].push(if edge.begin == atom {
                        edge.end
                    } else {
                        edge.begin
                    });
                    text.push_str(&label_text(label));
                    taken[label] = false;
                    closure[link] = true;
                }
                let mut opening = walk.opens[atom].clone();
                opening.sort_unstable();
                for (other, link) in opening {
                    neighbors[atom].push(other);
                    let Some(label) = (1..=MAX_RING_LABEL).find(|label| !taken[*label]) else {
                        return Err(SmilesError::new(SmilesCode::RingLabelLimit));
                    };
                    taken[label] = true;
                    label_of[link] = label;
                    if links[link].mark.is_some() {
                        return Err(SmilesError::new(SmilesCode::ReadbackMismatch)
                            .because("a directional bond is placed on a ring-closure digit"));
                    }
                    text.push_str(bond_text(&links[link], items)?);
                    text.push_str(&label_text(label));
                }
                neighbors[atom].extend(walk.children[atom].iter().map(|&(child, _)| child));
                if let Some((&(last, last_link), earlier)) = walk.children[atom].split_last() {
                    work.push(Work::Atom(last, Some(last_link)));
                    for &(child, link) in earlier.iter().rev() {
                        work.push(Work::Close);
                        work.push(Work::Atom(child, Some(link)));
                        work.push(Work::Open);
                    }
                }
            }
        }
    }
    Ok(Emitted {
        text,
        order,
        neighbors,
        preceded,
        closure,
    })
}
