use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use chemifree_conformance::{Report, load_suite, run_suite};
use clap::Parser;

/// Runs a conformance suite and reports passed, failed, and not-run cases per kind.
/// Exits 1 when a case fails or the suite cannot be loaded, and 2 on a usage error.
#[derive(Parser)]
#[command(name = "chemifree-conformance", version)]
struct Args {
    /// The suite file.
    #[arg(default_value = "conformance/suite.json")]
    suite: PathBuf,
    /// Write the JSON report to this file instead of standard output.
    #[arg(long, value_name = "PATH")]
    out: Option<PathBuf>,
}

fn main() -> ExitCode {
    let args = Args::parse();
    let suite = match load_suite(&args.suite) {
        Ok(suite) => suite,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let report = run_suite(&suite);
    let text = format!(
        "{}\n",
        serde_json::to_string_pretty(&report).expect("reports serialize to JSON")
    );
    match &args.out {
        Some(path) => {
            if let Err(error) = write(path, &text) {
                eprintln!("error: could not write {}: {error}", path.display());
                return ExitCode::FAILURE;
            }
        }
        None => print!("{text}"),
    }
    summarize(&report);
    if report.succeeded() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn write(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, text)
}

/// Prints the counts and every failure to standard error.
fn summarize(report: &Report) {
    let counts = report.counts;
    eprintln!(
        "conformance suite {}: accept {} passed, {} failed, {} not run; reject {} passed, {} failed, {} not run",
        report.suite_version,
        counts.accept.passed,
        counts.accept.failed,
        counts.accept.not_run,
        counts.reject.passed,
        counts.reject.failed,
        counts.reject.not_run,
    );
    for result in &report.results {
        if let Some(failure) = &result.failure {
            eprintln!("FAILED {}: {}", result.id, failure.message);
        }
    }
}
