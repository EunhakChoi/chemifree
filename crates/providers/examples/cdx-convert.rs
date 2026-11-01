//! Test utility: convert between CDX and CDXML through the shared codec.
//!
//! `cdx-convert to-cdxml` reads CDX bytes on standard input and writes CDXML text.
//! `cdx-convert to-cdx` reads CDXML text and writes CDX bytes. Losses go to standard error.
use std::io::{self, Read, Write};

use chemifree_application::interchange_codec::{InterchangeCodec, InterchangeFormat};
use chemifree_providers::interchange_codec::LocalInterchangeCodec;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let direction = std::env::args().nth(1).unwrap_or_default();
    let (from, to) = match direction.as_str() {
        "to-cdxml" => (InterchangeFormat::Cdx, InterchangeFormat::Cdxml),
        "to-cdx" => (InterchangeFormat::Cdxml, InterchangeFormat::Cdx),
        _ => return Err("usage: cdx-convert to-cdxml|to-cdx".into()),
    };
    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input)?;
    let codec = LocalInterchangeCodec;
    let read = codec
        .read(from, &input)
        .map_err(|error| format!("{}: {}", error.code, error.message))?;
    let written = codec
        .write(to, &read.document, None, false)
        .map_err(|error| format!("{}: {}", error.code, error.message))?;
    for loss in &written.loss_report {
        eprintln!(
            "loss {} {:?}: {}",
            loss.feature, loss.disposition, loss.reason
        );
    }
    io::stdout().write_all(&written.bytes)?;
    Ok(())
}
