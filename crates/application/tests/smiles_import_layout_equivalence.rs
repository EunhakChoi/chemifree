//! The per-component layout of a coordinate-free SMILES import (P48 S1, C4): a fragment of a
//! multi-component read keeps, relative to its own box, the positions and the `layout_not_covered`
//! refusal that it has when it is imported alone into an empty drawing.

use std::collections::BTreeMap;
use std::error::Error;
use std::path::Path;
use std::sync::OnceLock;

use chemifree_application::Application;
use chemifree_providers::interchange_codec::LocalInterchangeCodec;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, OpenSmilesCodec, V2000Codec};
use serde_json::{Value, json};
use tempfile::TempDir;

type App = Application<LocalCapabilities, FileDocumentRepository>;
/// The result of a test or helper, so that a failed read is an error rather than a panic.
type TestResult<T = ()> = Result<T, Box<dyn Error>>;

/// The tolerance, in pt, of a position relative to its box corner against the same position alone.
const TOLERANCE: f64 = 1e-9;
/// The number of seeded reads that each seeded test checks.
const SEEDS: u64 = 200;
/// The fragments that the seeded reads join. Each one is imported alone to give its expected shape
/// and refusal.
const FRAGMENTS: [&str; 9] = [
    "CCO",
    "c1ccccc1",
    "CC1CCCCC1",
    "F[C@H](Cl)Br",
    "c1ccc2ccccc2c1",
    "C1CCCCCCCCCCCC1",
    "[Na+]",
    "CC(C)CO",
    "C/C=C/C",
];

/// A small deterministic generator (xorshift64), so that a failing seed can be replayed.
struct Random(u64);

impl Random {
    /// The next value below `bound`, which must be positive.
    fn below(&mut self, bound: usize) -> TestResult<usize> {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let draw = self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33;
        Ok(usize::try_from(draw)? % bound)
    }
}

/// The fragments that seed `seed` joins, in order: two to six of them, each drawn from `FRAGMENTS`
/// with replacement.
fn fragment_order(seed: u64) -> TestResult<Vec<usize>> {
    let mut random = Random(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let count = 2 + random.below(5)?;
    let mut order = Vec::with_capacity(count);
    for _ in 0..count {
        order.push(random.below(FRAGMENTS.len())?);
    }
    Ok(order)
}

/// The SMILES text of a fragment of `FRAGMENTS`, by its index.
fn fragment(kind: usize) -> TestResult<&'static str> {
    let text = FRAGMENTS
        .get(kind)
        .ok_or_else(|| format!("no fragment {kind}"))?;
    Ok(*text)
}

/// The SMILES text of the read that `order` joins: its fragments separated by `.`.
fn read_of(order: &[usize]) -> TestResult<String> {
    let mut pieces = Vec::with_capacity(order.len());
    for &kind in order {
        pieces.push(fragment(kind)?);
    }
    Ok(pieces.join("."))
}

fn app(path: &Path) -> App {
    Application::new(LocalCapabilities, FileDocumentRepository::new(path))
        .with_ctfile_codec(V2000Codec)
        .with_interchange_codec(LocalInterchangeCodec)
        .with_smiles_codec(OpenSmilesCodec)
}

fn call(app: &App, id: &str, operation: &str, parameters: Value) -> TestResult<Value> {
    let request = json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters});
    Ok(serde_json::to_value(
        app.execute(request.to_string().as_bytes()),
    )?)
}

/// A fresh application over its own directory, with one empty document, whose id is returned.
fn setup() -> TestResult<(App, TempDir, String)> {
    let directory = tempfile::tempdir()?;
    let application = app(directory.path());
    let created = call(&application, "create", "document.create", json!({}))?;
    let id = created["result"]["document"]["id"]
        .as_str()
        .ok_or_else(|| format!("no document id in {created}"))?
        .to_owned();
    Ok((application, directory, id))
}

/// Previews `source` into the empty document `id` and commits that preview, accepting no loss.
/// Returns the commit.
fn import(app: &App, id: &str, source: &str) -> TestResult<Value> {
    let preview = call(
        app,
        &format!("preview-{id}"),
        "document.import",
        json!({"document_id": id, "expected_revision": 0, "format": "smiles", "source": source, "preview": true}),
    )?;
    let digest = preview["result"]["preview_digest"]
        .as_str()
        .ok_or_else(|| format!("no preview digest in {preview}"))?;
    call(
        app,
        &format!("commit-{id}"),
        "document.import",
        json!({"document_id": id, "expected_revision": 0, "format": "smiles", "source": source, "preview_digest": digest, "accept_loss": false}),
    )
}

/// The array under `key` of `value`.
fn items<'a>(value: &'a Value, key: &str) -> TestResult<&'a [Value]> {
    let array = value[key]
        .as_array()
        .ok_or_else(|| format!("{key} is not an array"))?;
    Ok(array.as_slice())
}

/// The `(x, y)` of a position object, such as the `position` of a document atom.
fn point(position: &Value) -> TestResult<(f64, f64)> {
    let x = position["x"]
        .as_f64()
        .ok_or("a position has no numeric x")?;
    let y = position["y"]
        .as_f64()
        .ok_or("a position has no numeric y")?;
    Ok((x, y))
}

/// The position that an import reply gave each input atom, in input order: each atom of its
/// mapping, looked up in the document that the reply returns.
fn imported_positions(reply: &Value) -> TestResult<Vec<(f64, f64)>> {
    let mut by_id = BTreeMap::new();
    for atom in items(&reply["result"]["document"], "atoms")? {
        let id = atom["id"].as_str().ok_or("an atom has no id")?;
        by_id.insert(id.to_owned(), point(&atom["position"])?);
    }
    let mut positions = Vec::new();
    for id in items(&reply["result"]["import_mapping"], "atoms")? {
        let id = id.as_str().ok_or("an import mapping entry is not an id")?;
        let position = by_id
            .get(id)
            .ok_or_else(|| format!("atom {id} is not in the document"))?;
        positions.push(*position);
    }
    Ok(positions)
}

/// The `layout_not_covered` losses of a reply, in report order.
fn layout_losses(reply: &Value) -> TestResult<Vec<&Value>> {
    Ok(items(reply, "loss_report")?
        .iter()
        .filter(|loss| loss["feature"] == "layout_not_covered")
        .collect())
}

/// The reason of a loss, which holds the refusal code of a `layout_not_covered` loss.
fn reason_of(loss: &Value) -> TestResult<String> {
    let reason = loss["reason"]
        .as_str()
        .ok_or("a layout_not_covered loss has no reason")?;
    Ok(reason.to_owned())
}

/// The `source_path` that a layout loss gives the atoms numbered `first` to `last` (from 1), as
/// `atoms[first, ..., last]`.
fn atom_path(first: usize, last: usize) -> String {
    let numbers: Vec<String> = (first..=last).map(|number| number.to_string()).collect();
    format!("atoms[{}]", numbers.join(", "))
}

/// The corner `(min x, min y)` of the box of `positions`, which must not be empty.
fn top_left(positions: &[(f64, f64)]) -> TestResult<(f64, f64)> {
    let (first, rest) = positions.split_first().ok_or("a fragment has no atoms")?;
    let mut corner = *first;
    for &(x, y) in rest {
        corner = (corner.0.min(x), corner.1.min(y));
    }
    Ok(corner)
}

/// Whether two coordinates agree within `TOLERANCE`.
fn close(actual: f64, expected: f64) -> bool {
    (actual - expected).abs() <= TOLERANCE
}

/// One fragment of `FRAGMENTS` imported alone into an empty drawing.
struct Alone {
    /// The position of each of its atoms, in input order.
    positions: Vec<(f64, f64)>,
    /// The reason of its `layout_not_covered` loss, or `None` when the layout covers it.
    not_covered: Option<String>,
}

/// Imports `source` alone into an empty drawing and records its positions and refusal.
fn alone_of(source: &str) -> TestResult<Alone> {
    let (app, _directory, id) = setup()?;
    let done = import(&app, &id, source)?;
    assert_ne!(done["outcome"], "error", "{source}: {done}");
    assert_eq!(done["document_revision"], 1, "{source}: {done}");
    let positions = imported_positions(&done)?;
    let path = atom_path(1, positions.len());
    let not_covered = match layout_losses(&done)?.as_slice() {
        [] => None,
        [loss] => {
            assert_eq!(
                loss["source_path"].as_str(),
                Some(path.as_str()),
                "{source}: {loss}"
            );
            Some(reason_of(loss)?)
        }
        found => {
            return Err(format!(
                "{source}: {} layout_not_covered losses, expected at most one",
                found.len()
            )
            .into());
        }
    };
    Ok(Alone {
        positions,
        not_covered,
    })
}

/// Every fragment of `FRAGMENTS` imported alone, in the order of `FRAGMENTS`. It is computed once
/// for the test binary, and a failed import is reported to every test that reads it.
fn alone_imports() -> TestResult<&'static [Alone]> {
    static ALONE: OnceLock<Result<Vec<Alone>, String>> = OnceLock::new();
    let imports = ALONE.get_or_init(|| {
        FRAGMENTS
            .iter()
            .copied()
            .map(alone_of)
            .collect::<TestResult<Vec<Alone>>>()
            .map_err(|error| error.to_string())
    });
    match imports {
        Ok(imports) => Ok(imports.as_slice()),
        Err(message) => Err(message.as_str().into()),
    }
}

#[test]
fn each_fragment_of_200_seeded_joined_imports_keeps_its_alone_shape_and_refusal() -> TestResult {
    let alone = alone_imports()?;
    for seed in 0..SEEDS {
        let order = fragment_order(seed)?;
        let source = read_of(&order)?;
        let (app, _directory, id) = setup()?;
        let done = import(&app, &id, &source)?;
        assert_ne!(done["outcome"], "error", "seed {seed}: {source}: {done}");
        assert_eq!(
            done["document_revision"], 1,
            "seed {seed}: {source}: {done}"
        );
        let joined = imported_positions(&done)?;
        let losses = layout_losses(&done)?;
        let mut expected_losses = 0usize;
        let mut start = 0usize;
        for (index, &kind) in order.iter().enumerate() {
            let part = alone
                .get(kind)
                .ok_or_else(|| format!("no alone import for fragment {kind}"))?;
            let count = part.positions.len();
            let atoms = joined
                .get(start..start + count)
                .ok_or_else(|| format!("seed {seed}: fragment {index} has no atoms in {source}"))?;
            let here = format!(
                "seed {seed}, fragment {index} ({}) of {source}",
                fragment(kind)?
            );
            let joined_corner = top_left(atoms)?;
            let alone_corner = top_left(&part.positions)?;
            for (atom, (got, want)) in atoms.iter().zip(&part.positions).enumerate() {
                let relative = (got.0 - joined_corner.0, got.1 - joined_corner.1);
                let expected = (want.0 - alone_corner.0, want.1 - alone_corner.1);
                assert!(
                    close(relative.0, expected.0) && close(relative.1, expected.1),
                    "{here}: atom {} (input atom {}) is {relative:?} from its box corner, alone it is {expected:?}",
                    atom + 1,
                    start + atom + 1
                );
            }
            let path = atom_path(start + 1, start + count);
            let found: Vec<&Value> = losses
                .iter()
                .filter(|loss| loss["source_path"].as_str() == Some(path.as_str()))
                .copied()
                .collect();
            let joined_reason = match found.as_slice() {
                [] => None,
                [loss] => Some(reason_of(loss)?),
                more => {
                    return Err(format!(
                        "{here}: {} layout_not_covered losses name {path}",
                        more.len()
                    )
                    .into());
                }
            };
            assert_eq!(
                joined_reason, part.not_covered,
                "{here}: the layout_not_covered refusal of {path}"
            );
            expected_losses += usize::from(part.not_covered.is_some());
            start += count;
        }
        assert_eq!(
            joined.len(),
            start,
            "seed {seed}: {source}: one position per input atom"
        );
        assert_eq!(
            losses.len(),
            expected_losses,
            "seed {seed}: {source}: a layout_not_covered loss names no fragment of the read: {done}"
        );
    }
    Ok(())
}

#[test]
fn the_seeded_reads_reach_every_fragment_after_another_and_both_outcomes() -> TestResult {
    let mut after_another = vec![false; FRAGMENTS.len()];
    for seed in 0..SEEDS {
        let order = fragment_order(seed)?;
        for &kind in order.iter().skip(1) {
            let seen = after_another
                .get_mut(kind)
                .ok_or_else(|| format!("no fragment {kind}"))?;
            *seen = true;
        }
    }
    for (kind, seen) in after_another.iter().enumerate() {
        let text = fragment(kind)?;
        assert!(
            *seen,
            "{text} never follows another fragment in the {SEEDS} seeded reads"
        );
    }
    let alone = alone_imports()?;
    let refused = alone
        .iter()
        .filter(|part| part.not_covered.is_some())
        .count();
    let laid_out = alone.len() - refused;
    assert!(
        laid_out > 0 && refused > 0,
        "the fragments need both outcomes, got {laid_out} laid out and {refused} refused"
    );
    Ok(())
}
