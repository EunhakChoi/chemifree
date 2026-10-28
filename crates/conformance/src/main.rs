//! `chemifree-conformance [SUITE] [--out REPORT]`: runs a conformance suite and writes its
//! report. Exits with status 1 when a case failed, 2 when the suite cannot be used.

use std::path::PathBuf;
use std::process::ExitCode;
use std::{env, fs};

use chemifree_conformance::Suite;

const DEFAULT_SUITE: &str = "conformance/suite.json";

fn main() -> ExitCode {
    let mut suite = PathBuf::from(DEFAULT_SUITE);
    let mut out = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => out = args.next().map(PathBuf::from),
            "-h" | "--help" => {
                println!("usage: chemifree-conformance [SUITE] [--out REPORT]");
                return ExitCode::SUCCESS;
            }
            path => suite = PathBuf::from(path),
        }
    }
    let suite = match Suite::load(&suite) {
        Ok(suite) => suite,
        Err(error) => {
            eprintln!("conformance suite unusable: {error}");
            return ExitCode::from(2);
        }
    };
    let report = suite.run();
    let json = serde_json::to_string_pretty(&report).expect("the report serializes");
    match &out {
        Some(path) => {
            if let Err(error) = fs::write(path, format!("{json}\n")) {
                eprintln!("could not write {}: {error}", path.display());
                return ExitCode::from(2);
            }
        }
        None => println!("{json}"),
    }
    for result in report
        .results
        .iter()
        .filter(|result| result.detail.is_some())
    {
        eprintln!(
            "{:?} {}: {}",
            result.status,
            result.id,
            result.detail.as_deref().unwrap_or_default()
        );
    }
    eprintln!(
        "conformance suite {}: {:?}",
        report.suite_version, report.counts
    );
    if report.failed() == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
