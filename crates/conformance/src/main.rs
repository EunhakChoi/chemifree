//! `chemifree-conformance [--out PATH] SUITE`: runs a conformance suite and writes its record.
//!
//! Exit status: 0 when no case failed (pending cases do not run), 1 when a case failed or the
//! suite could not be loaded or the record written, 2 for a usage error (nothing runs).
//! A panic in the application aborts the run (exit 101) without writing a record.

use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use chemifree_conformance::{load_suite, run_suite};

const USAGE: &str = "usage: chemifree-conformance [--out PATH] SUITE";

#[derive(Debug, Eq, PartialEq)]
struct Args {
    out: Option<PathBuf>,
    suite: PathBuf,
}

#[derive(Debug, Eq, PartialEq)]
enum Parsed {
    Run(Args),
    Help,
}

/// Parses the arguments after the program name.
fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Parsed, String> {
    let mut out = None;
    let mut suite = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--help" | "-h") => return Ok(Parsed::Help),
            Some("--out") => {
                if out.is_some() {
                    return Err("--out is given twice".to_owned());
                }
                let path = args.next().ok_or("--out needs a path")?;
                if path.is_empty() || path.to_str().is_some_and(|path| path.starts_with('-')) {
                    return Err("--out needs a path".to_owned());
                }
                out = Some(PathBuf::from(path));
            }
            Some(option) if option.starts_with('-') => {
                return Err(format!("unknown option {option:?}"));
            }
            _ => {
                if suite.is_some() {
                    return Err("only one suite can be run".to_owned());
                }
                suite = Some(PathBuf::from(arg));
            }
        }
    }
    let suite = suite.ok_or("no suite is given")?;
    Ok(Parsed::Run(Args { out, suite }))
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args_os().skip(1)) {
        Ok(Parsed::Run(args)) => args,
        Ok(Parsed::Help) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            eprintln!("chemifree-conformance: {message}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let suite = match load_suite(&args.suite) {
        Ok(suite) => suite,
        Err(problem) => {
            eprintln!("chemifree-conformance: {}: {problem}", args.suite.display());
            return ExitCode::FAILURE;
        }
    };
    let report = run_suite(&suite);
    let text = match serde_json::to_string_pretty(&report) {
        Ok(text) => text + "\n",
        Err(error) => {
            eprintln!("chemifree-conformance: cannot encode the record: {error}");
            return ExitCode::FAILURE;
        }
    };
    match &args.out {
        Some(path) => {
            let written = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .map_or(Ok(()), fs::create_dir_all)
                .and_then(|()| fs::write(path, &text));
            if let Err(error) = written {
                eprintln!(
                    "chemifree-conformance: cannot write {}: {error}",
                    path.display()
                );
                return ExitCode::FAILURE;
            }
        }
        None => print!("{text}"),
    }
    for result in report
        .results
        .iter()
        .filter(|result| result.failure.is_some())
    {
        eprintln!(
            "FAILED {}: {}",
            result.id,
            result.failure.as_deref().unwrap_or_default()
        );
    }
    let total = report.counts.total;
    eprintln!(
        "conformance: {} cases: {} passed, {} failed, {} not run (pending)",
        report.results.len(),
        total.passed,
        total.failed,
        total.not_run
    );
    if report.failed() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Parsed, String> {
        parse_args(args.iter().map(OsString::from))
    }

    #[test]
    fn reads_the_suite_and_an_output_path() {
        assert_eq!(
            parse(&["--out", "r.json", "suite.json"]),
            Ok(Parsed::Run(Args {
                out: Some(PathBuf::from("r.json")),
                suite: PathBuf::from("suite.json"),
            }))
        );
        assert_eq!(
            parse(&["suite.json"]),
            Ok(Parsed::Run(Args {
                out: None,
                suite: PathBuf::from("suite.json"),
            }))
        );
    }

    #[test]
    fn refuses_out_without_a_path() {
        assert_eq!(
            parse(&["suite.json", "--out"]),
            Err("--out needs a path".to_owned())
        );
        // The suite must not be taken as the output path.
        assert_eq!(
            parse(&["--out", "--help"]),
            Err("--out needs a path".to_owned())
        );
    }

    #[test]
    fn refuses_unknown_options() {
        assert_eq!(
            parse(&["--outt", "r.json", "suite.json"]),
            Err("unknown option \"--outt\"".to_owned())
        );
    }

    #[test]
    fn refuses_a_missing_or_second_suite() {
        assert_eq!(parse(&[]), Err("no suite is given".to_owned()));
        assert_eq!(
            parse(&["--out", "r.json"]),
            Err("no suite is given".to_owned())
        );
        assert_eq!(
            parse(&["a.json", "b.json"]),
            Err("only one suite can be run".to_owned())
        );
    }

    #[test]
    fn refuses_a_second_out() {
        assert_eq!(
            parse(&["--out", "a", "--out", "b", "s.json"]),
            Err("--out is given twice".to_owned())
        );
    }
}
