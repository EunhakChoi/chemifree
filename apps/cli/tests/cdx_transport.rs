//! CDX through the CLI's `operations` stream: one request per line, at most 1 MiB each.

use std::io::Write;
use std::process::{Command, Stdio};

use chemifree_application::transport_base64;
use serde_json::{Value, json};

fn cdx_with_text(length: usize) -> Vec<u8> {
    let mut bytes = b"VjCD0100".to_vec();
    bytes.extend_from_slice(&[4, 3, 2, 1]);
    bytes.extend_from_slice(&[0; 16]);
    bytes.extend_from_slice(&[0x00, 0x80, 0, 0, 0, 0]);
    bytes.extend_from_slice(&[0x01, 0x80, 1, 0, 0, 0]);
    bytes.extend_from_slice(&[0x06, 0x80, 2, 0, 0, 0]);
    bytes.extend_from_slice(&[0x00, 0x07, 0xff, 0xff]);
    bytes.extend_from_slice(&((length + 2) as u32).to_le_bytes());
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend(std::iter::repeat_n(b'a', length));
    bytes.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
    bytes
}

fn run(source: &str) -> Value {
    let dir = tempfile::tempdir().unwrap();
    let request = json!({"contract":1,"request_id":"r","profile":"preserve-semantics","operation":"document.import","parameters":{"mode":"read","format":"cdx","source":source}});
    let mut child = Command::new(env!("CARGO_BIN_EXE_chemifree"))
        .env_remove("CHEMIFREE_MODEL_DATA_ROOT")
        .env_remove("CHEMIFREE_MODEL_TRUSTED_REGISTRY")
        .arg("--data-dir")
        .arg(dir.path())
        .arg("operations")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let line = format!("{request}\n");
    let writer = std::thread::spawn(move || {
        // The CLI may stop reading an oversized request; a broken pipe is not a failure.
        let _ = stdin.write_all(line.as_bytes());
    });
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(output.status.success());
    serde_json::from_slice(output.stdout.split(|b| *b == b'\n').next().unwrap()).unwrap()
}

#[test]
fn a_cdx_that_fits_one_mebibyte_of_request_is_read() {
    // 700,000 bytes become about 933,400 characters.
    let reply = run(&transport_base64::encode(&cdx_with_text(700_000)));
    assert_eq!(reply["outcome"], "ok", "{reply}");
}

#[test]
fn a_cdx_whose_base64_exceeds_the_request_limit_is_refused_though_the_codec_would_accept_it() {
    // 800,000 bytes are under the codec's 1 MiB but become about 1,066,700 characters.
    let raw = cdx_with_text(800_000);
    assert!(raw.len() < 1024 * 1024);
    let reply = run(&transport_base64::encode(&raw));
    assert_eq!(reply["outcome"], "error", "{reply}");
    assert_eq!(reply["diagnostics"][0]["code"], "resource_limit");
}
