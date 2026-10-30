//! Randomized edits, undo, and redo: every state they reach restores from its parts
//! identically, so storage can always reopen what editing produced.

use std::collections::{BTreeMap, BTreeSet};

use chemifree_domain::document::{
    Atom, AtomId, AtomRef, BondOrder, BondTo, Command, Document, DocumentId, NewAtomPositions,
    ObjectId, Point, Vector,
};
use chemifree_domain::element::Element;
use chemifree_domain::scene::{
    DeleteMode, Graphic, GraphicShape, ObjectRef, SceneObject, Selection, SelectionRequest,
    TransactionSelection,
};

/// A small deterministic generator (xorshift64*), so a failing seed can be replayed.
struct Random(u64);

impl Random {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(bound.max(1)).unwrap()).unwrap()
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> Option<T> {
        (!items.is_empty()).then(|| items[self.below(items.len())])
    }

    /// A random subset, each item kept with even odds.
    fn subset<T: Copy>(&mut self, items: &[T]) -> Vec<T> {
        items.iter().copied().filter(|_| self.chance(50)).collect()
    }

    fn coordinate(&mut self) -> f64 {
        f64::from(u32::try_from(self.below(400)).unwrap()) - 200.0
    }
}

fn point(random: &mut Random) -> Point {
    Point::new(random.coordinate(), random.coordinate()).unwrap()
}

fn delta(random: &mut Random) -> Vector {
    Vector {
        dx: random.coordinate() / 10.0,
        dy: random.coordinate() / 10.0,
    }
}

struct Scene {
    objects: Vec<ObjectId>,
    groups: Vec<ObjectId>,
    instances: Vec<ObjectId>,
    /// Each parent's children in order; `None` is the page.
    children: BTreeMap<Option<ObjectId>, Vec<ObjectId>>,
    atoms: Vec<AtomId>,
}

fn scene(document: &Document) -> Scene {
    let mut scene = Scene {
        objects: Vec::new(),
        groups: Vec::new(),
        instances: document.instances().collect(),
        children: BTreeMap::from([(None, Vec::new())]),
        atoms: document.atoms().map(|(id, _)| id).collect(),
    };
    for (id, parent, object) in document.scene_objects() {
        scene.objects.push(id);
        scene.children.entry(parent).or_default().push(id);
        if matches!(object, SceneObject::Group { .. }) {
            scene.groups.push(id);
            scene.children.entry(Some(id)).or_default();
        }
    }
    scene
}

fn command(random: &mut Random, scene: &Scene) -> Command {
    let shapes = [
        GraphicShape::Line,
        GraphicShape::Rectangle,
        GraphicShape::Arrow,
    ];
    let parents: Vec<Option<ObjectId>> = scene.children.keys().copied().collect();
    match random.below(10) {
        0 => {
            let positions = scene
                .instances
                .iter()
                .map(|instance| (*instance, point(random)))
                .collect();
            Command::AddAtom {
                atom: Atom::from(Element::CARBON),
                positions: NewAtomPositions::PerInstance(positions),
                bond_to: random.pick(&scene.atoms).map(|atom| BondTo {
                    atom: AtomRef::Id(atom),
                    order: BondOrder::Single,
                }),
            }
        }
        1 => Command::AddInstance {
            source: random.pick(&scene.instances),
            offset: delta(random),
        },
        2 => {
            let parent = random.pick(&parents).flatten();
            let siblings = scene.children.get(&parent).map_or(0, Vec::len);
            Command::AddGraphic {
                graphic: Graphic {
                    shape: random.pick(&shapes).unwrap(),
                    points: [point(random), point(random)],
                },
                parent,
                index: random.chance(50).then(|| random.below(siblings + 1)),
            }
        }
        3 => {
            let parent = random.pick(&parents).flatten();
            let siblings = scene.children.get(&parent).cloned().unwrap_or_default();
            Command::Group {
                objects: random.subset(&siblings),
            }
        }
        4 => Command::Ungroup {
            group: random.pick(&scene.groups).unwrap_or(scene.objects[0]),
        },
        5 => {
            let object = random.pick(&scene.objects).unwrap();
            let siblings = scene
                .children
                .values()
                .find(|children| children.contains(&object))
                .map_or(1, Vec::len);
            Command::Reorder {
                object,
                index: random.below(siblings),
            }
        }
        6 => Command::DeleteObjects {
            objects: random.subset(&scene.objects),
            mode: if random.chance(50) {
                DeleteMode::Subtree
            } else {
                DeleteMode::ObjectOnly
            },
        },
        7 => Command::MoveObjects {
            objects: random.subset(&scene.objects),
            delta: delta(random),
        },
        8 => Command::DeleteAtoms {
            atoms: random.subset(&scene.atoms),
        },
        _ => Command::MoveInstance {
            instance: random.pick(&scene.instances).unwrap(),
            offset: delta(random),
        },
    }
}

fn selection(random: &mut Random, scene: &Scene, document: &Document) -> Selection {
    Selection {
        objects: random.subset(&scene.objects).into_iter().collect(),
        atoms: random.subset(&scene.atoms).into_iter().collect(),
        bonds: random
            .subset(&document.bonds().map(|(id, _)| id).collect::<Vec<_>>())
            .into_iter()
            .collect(),
    }
}

fn transaction(random: &mut Random, document: &Document) -> (Vec<Command>, TransactionSelection) {
    let scene = scene(document);
    let commands: Vec<Command> = (0..=random.below(3))
        .map(|_| command(random, &scene))
        .collect();
    let before = random
        .chance(50)
        .then(|| selection(random, &scene, document));
    let after = random.chance(60).then(|| {
        let mut request = SelectionRequest::default();
        // Sometimes refused: an object deleted by the commands, or `$N` of any command.
        for id in random.subset(&scene.objects) {
            request.objects.push(ObjectRef::Id(id));
        }
        if random.chance(50) {
            request
                .objects
                .push(ObjectRef::Created(random.below(commands.len() + 1)));
        }
        request.atoms = random.subset(&scene.atoms);
        request
    });
    (commands, TransactionSelection { before, after })
}

fn object_ids(document: &Document) -> BTreeSet<ObjectId> {
    document
        .scene_objects()
        .into_iter()
        .map(|(id, _, _)| id)
        .collect()
}

#[test]
fn every_reachable_state_restores_identically() {
    let mut changed = 0;
    for seed in 0..300 {
        let mut random = Random::new(seed);
        let mut document = Document::new(DocumentId::parse("doc-1").unwrap(), "Random").unwrap();
        for _ in 0..40 {
            let before = document.clone();
            match random.below(6) {
                0 => {
                    if document.undo().is_ok() {
                        assert_eq!(document.revision(), before.revision() + 1);
                    }
                }
                1 => {
                    if document.redo().is_ok() {
                        assert_eq!(document.revision(), before.revision() + 1);
                    }
                }
                _ => {
                    let (commands, selection) = transaction(&mut random, &document);
                    match document.apply_selecting(&commands, &selection) {
                        Ok(_) if document != before => {
                            changed += 1;
                            assert_eq!(document.revision(), before.revision() + 1);
                            let next = before.numbering().next_object;
                            let existing = object_ids(&before);
                            for id in object_ids(&document).difference(&existing) {
                                assert!(
                                    id.serial() >= next,
                                    "seed {seed}: {id} reuses an identifier"
                                );
                            }
                        }
                        Ok(_) => {}
                        Err(_) => assert_eq!(document, before, "seed {seed}: refused changes"),
                    }
                }
            }
            let restored = Document::restore(document.to_parts())
                .unwrap_or_else(|error| panic!("seed {seed}: {error}"));
            assert_eq!(restored, document, "seed {seed}");
        }
    }
    assert!(
        changed > 1000,
        "too few edits succeeded to mean much: {changed}"
    );
}
