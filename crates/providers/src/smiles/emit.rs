//! Turns a list of atoms and bonds into SMILES text by a depth-first walk.
//!
//! Atoms are given in priority order: the walk starts at the lowest-numbered atom of each
//! component and visits neighbours lowest first, so the text is a function of the numbering and
//! of nothing else. The last child of an atom continues the chain; earlier children are
//! branches. Ring labels are the lowest free number from 1; a label that closes at an atom is
//! free again for the rings that open at that same atom.

use chemifree_application::smiles::{SmilesCode, SmilesError};

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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Kind {
    Single,
    Double,
    Triple,
    Aromatic,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Link {
    pub begin: usize,
    pub end: usize,
    pub kind: Kind,
}

pub(super) struct Emitted {
    pub text: String,
    /// The item written at each position of the text.
    pub order: Vec<usize>,
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
fn bond_text(link: &Link, items: &[Item]) -> &'static str {
    match link.kind {
        Kind::Aromatic => "",
        Kind::Single if items[link.begin].aromatic && items[link.end].aromatic => "-",
        Kind::Single => "",
        Kind::Double => "=",
        Kind::Triple => "#",
    }
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
                walk.children[atom].push((neighbour, link));
                stack.push((neighbour, 0));
            }
        }
    }
    walk
}

/// Writes the items and links, or fails when the text would need more ring labels or deeper
/// branches than the reader accepts.
pub(super) fn emit(items: &[Item], links: &[Link]) -> Result<Emitted, SmilesError> {
    let walk = walk(items.len(), links);
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
                    text.push_str(bond_text(&links[link], items));
                }
                text.push_str(&atom_text(&items[atom]));
                order.push(atom);
                let mut closing: Vec<usize> =
                    walk.closes[atom].iter().map(|l| label_of[*l]).collect();
                closing.sort_unstable();
                for label in closing {
                    text.push_str(&label_text(label));
                    taken[label] = false;
                }
                let mut opening = walk.opens[atom].clone();
                opening.sort_unstable();
                for (_, link) in opening {
                    let Some(label) = (1..=MAX_RING_LABEL).find(|label| !taken[*label]) else {
                        return Err(SmilesError::new(SmilesCode::RingLabelLimit));
                    };
                    taken[label] = true;
                    label_of[link] = label;
                    text.push_str(bond_text(&links[link], items));
                    text.push_str(&label_text(label));
                }
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
    Ok(Emitted { text, order })
}
