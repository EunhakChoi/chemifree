//! Boundary utility for the separate public-spec CDX/CDXML comparison.
use chemifree_application::interchange_codec::{InterchangeCodec, InterchangeFormat};
use chemifree_providers::interchange_codec::LocalInterchangeCodec;
use std::{
    env,
    error::Error,
    fs,
    io::{self, Write},
};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args().skip(1);
    let direction = args.next().ok_or("expected direction")?;
    let file = args.next().ok_or("expected input path")?;
    let (input, output) = match direction.as_str() {
        "to-xml" => (InterchangeFormat::Cdx, InterchangeFormat::Cdxml),
        "to-cdx" => (InterchangeFormat::Cdxml, InterchangeFormat::Cdx),
        _ => return Err("unknown direction".into()),
    };
    let codec = LocalInterchangeCodec;
    let read = codec.read(input, &fs::read(file)?)?;
    let written = codec.write(output, &read.document, None, false)?;
    io::stdout().write_all(&written.bytes)?;
    Ok(())
}
