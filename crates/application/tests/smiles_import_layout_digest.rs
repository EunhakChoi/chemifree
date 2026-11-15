//! The exact layout of seeded multi-component SMILES imports. Each import of a joined text
//! contributes its positions and loss report to one FNV-1a 64 digest, which origin/main recorded
//! before the layout refactor: a tolerance cannot show bit identity, so this pins every bit.

use std::collections::BTreeMap;
use std::error::Error;

use chemifree_application::Application;
use chemifree_providers::interchange_codec::LocalInterchangeCodec;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, OpenSmilesCodec, V2000Codec};
use serde_json::{Value, json};
use tempfile::TempDir;

type App = Application<LocalCapabilities, FileDocumentRepository>;
/// The result of a test or helper, so that a failed read is an error rather than a panic.
type TestResult<T = ()> = Result<T, Box<dyn Error>>;

/// The fragments that the seeded joined imports are drawn from, as in the equivalence test.
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

/// The FNV-1a 64 digest of the 200 seeded imports as origin/main (d7ea7da5) laid them out.
const RECORDED: u64 = 0x54be_e891_8bc4_a96c;

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

/// Feeds `bytes` into the FNV-1a 64 hash `state`, one byte at a time.
fn feed(state: &mut u64, bytes: &[u8]) {
    for &byte in bytes {
        *state ^= u64::from(byte);
        *state = state.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

fn app(path: &std::path::Path) -> App {
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

/// Creates an empty document and returns its id; `request` names the call.
fn new_document(app: &App, request: &str) -> TestResult<String> {
    let created = call(app, request, "document.create", json!({}))?;
    let id = created["result"]["document"]["id"]
        .as_str()
        .ok_or_else(|| format!("no document id in {created}"))?;
    Ok(id.to_owned())
}

/// A fresh application over its own directory, with one empty document.
fn setup() -> TestResult<(App, TempDir, String)> {
    let directory = tempfile::tempdir()?;
    let application = app(directory.path());
    let id = new_document(&application, "create")?;
    Ok((application, directory, id))
}

/// The preview of `source` as an import into document `id` at `revision`.
fn preview(app: &App, id: &str, revision: u64, source: &str) -> TestResult<Value> {
    call(
        app,
        &format!("preview-{id}-{revision}"),
        "document.import",
        json!({"document_id": id, "expected_revision": revision, "format": "smiles", "source": source, "preview": true}),
    )
}

/// The commit of `source` into document `id` at `revision` with `digest`, accepting no loss.
fn commit(app: &App, id: &str, revision: u64, source: &str, digest: &str) -> TestResult<Value> {
    call(
        app,
        &format!("commit-{id}-{revision}"),
        "document.import",
        json!({"document_id": id, "expected_revision": revision, "format": "smiles", "source": source, "preview_digest": digest, "accept_loss": false}),
    )
}

/// Previews `source` into document `id` at `revision` and commits that preview, returning the commit.
fn import(app: &App, id: &str, revision: u64, source: &str) -> TestResult<Value> {
    let previewed = preview(app, id, revision, source)?;
    let digest = previewed["result"]["preview_digest"]
        .as_str()
        .ok_or_else(|| format!("no preview digest in {previewed}"))?;
    commit(app, id, revision, source, digest)
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

/// The positions that an import reply gave its input atoms, in input order.
fn imported_positions(reply: &Value) -> TestResult<Vec<(f64, f64)>> {
    let mut by_id = BTreeMap::new();
    for atom in items(&reply["result"]["document"], "atoms")? {
        let id = atom["id"].as_str().ok_or("an atom has no id")?;
        by_id.insert(id, point(&atom["position"])?);
    }
    let mut positions = Vec::new();
    for mapped in items(&reply["result"]["import_mapping"], "atoms")? {
        let id = mapped
            .as_str()
            .ok_or("an import mapping entry is not an id")?;
        let position = by_id
            .get(id)
            .ok_or_else(|| format!("atom {id} is not in the document"))?;
        positions.push(*position);
    }
    Ok(positions)
}

#[test]
fn two_hundred_seeded_joined_imports_match_the_digest_recorded_on_origin_main() -> TestResult {
    let mut digest: u64 = 0xcbf2_9ce4_8422_2325;
    for seed in 0..200_u64 {
        let source = read_of(&fragment_order(seed)?)?;
        let (app, _directory, id) = setup()?;
        let done = import(&app, &id, 0, &source)?;
        feed(&mut digest, &seed.to_le_bytes());
        for (x, y) in imported_positions(&done)? {
            feed(&mut digest, &x.to_bits().to_le_bytes());
            feed(&mut digest, &y.to_bits().to_le_bytes());
        }
        for loss in items(&done, "loss_report")? {
            for field in ["feature", "source_path", "reason"] {
                let text = loss[field]
                    .as_str()
                    .ok_or_else(|| format!("a loss has no {field}: {done}"))?;
                feed(&mut digest, text.as_bytes());
                feed(&mut digest, &[0_u8]);
            }
        }
    }
    assert_eq!(
        digest, RECORDED,
        "the seeded imports changed their positions or losses; digest {digest:#018x}"
    );
    Ok(())
}
