//! `chemifree-conformance [SUITE] [--out REPORT]`: runs a conformance suite and writes its
//! report. Exits with status 1 when a case failed, 2 when the suite cannot be used or the command
//! line is wrong.

use std::path::PathBuf;
use std::process::ExitCode;
use std::{env, fs};

use chemifree_conformance::Suite;

const DEFAULT_SUITE: &str = "conformance/suite.json";
const USAGE: &str = "usage: chemifree-conformance [SUITE] [--out REPORT]";

/// What the command line asks for.
#[derive(Debug, PartialEq)]
enum Invocation {
    Help,
    Run {
        suite: PathBuf,
        out: Option<PathBuf>,
    },
}

/// Reads the arguments after the program name.
///
/// # Errors
///
/// A usage message when `--out` has no path, an option is unknown, or more than one suite is
/// named, so a mistyped command line never runs as something else.
fn parse(args: impl IntoIterator<Item = String>) -> Result<Invocation, String> {
    let mut suite = None;
    let mut out = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => match args.next() {
                Some(path) if !path.starts_with('-') => out = Some(PathBuf::from(path)),
                _ => return Err("--out needs a report path".into()),
            },
            "-h" | "--help" => return Ok(Invocation::Help),
            option if option.starts_with('-') => return Err(format!("unknown option {option}")),
            path if suite.is_none() => suite = Some(PathBuf::from(path)),
            path => return Err(format!("only one suite can be run, also given {path}")),
        }
    }
    Ok(Invocation::Run {
        suite: suite.unwrap_or_else(|| PathBuf::from(DEFAULT_SUITE)),
        out,
    })
}

fn main() -> ExitCode {
    let (suite, out) = match parse(env::args().skip(1)) {
        Ok(Invocation::Help) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Ok(Invocation::Run { suite, out }) => (suite, out),
        Err(problem) => {
            eprintln!("{problem}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
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

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn a_suite_and_report_path_are_read() {
        assert_eq!(
            parse(args(&["s.json", "--out", "r.json"])),
            Ok(Invocation::Run {
                suite: PathBuf::from("s.json"),
                out: Some(PathBuf::from("r.json")),
            })
        );
        assert_eq!(
            parse(args(&[])),
            Ok(Invocation::Run {
                suite: PathBuf::from(DEFAULT_SUITE),
                out: None,
            })
        );
    }

    #[test]
    fn a_missing_report_path_is_a_usage_error_not_stdout() {
        assert!(parse(args(&["s.json", "--out"])).is_err_and(|e| e.contains("--out")));
        assert!(parse(args(&["--out", "--help"])).is_err_and(|e| e.contains("--out")));
    }

    #[test]
    fn unknown_options_and_a_second_suite_are_usage_errors() {
        assert!(parse(args(&["--outt", "r.json"])).is_err_and(|e| e.contains("--outt")));
        assert!(parse(args(&["a.json", "b.json"])).is_err_and(|e| e.contains("b.json")));
    }
}
