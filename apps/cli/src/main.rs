use std::fs::File;
use std::io::{self, BufRead, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use chemifree_application::{
    Application, ApplicationError, CommandDto, CopyResponse, DeleteResponse, DocumentListResponse,
    DocumentResponse, DraftDiscardResponse, ErrorCode, ListQuery, ListSort, OperationResponse,
    RecoveryState, RestoreResponse, TrashListResponse,
};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, default_data_dir};
use clap::{Args as ClapArgs, Parser, Subcommand, ValueEnum};
use serde::Serialize;

type LocalApplication = Application<LocalCapabilities, FileDocumentRepository>;

/// Command input larger than this is refused before parsing.
const MAX_COMMAND_INPUT_BYTES: u64 = 1024 * 1024;

#[derive(Parser)]
#[command(name = "chemifree", version, about = "Chemifree local CLI")]
struct Args {
    /// Directory for saved documents. Defaults to the platform user-data directory.
    #[arg(long, global = true, env = "CHEMIFREE_DATA_DIR")]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Inspect the local application's liveness contract without starting HTTP.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// List actual implementation status using the same application service as HTTP.
    Capabilities {
        #[arg(long)]
        json: bool,
    },
    /// Create, inspect, and edit saved documents. Every edit is saved before exit.
    #[command(subcommand)]
    Documents(DocumentCommand),
    /// Run operation contract requests: one JSON request per line on standard input, one JSON
    /// reply per line on standard output. Sessions last for this process only: edits are kept
    /// only once a document.save request commits them.
    Operations,
}

#[derive(Subcommand)]
enum DocumentCommand {
    /// List saved documents.
    List {
        /// Only documents whose title contains this text, ignoring case.
        #[arg(long)]
        query: Option<String>,
        #[arg(long, value_enum, default_value_t = Sort::Title)]
        sort: Sort,
        #[arg(long)]
        json: bool,
    },
    /// Create and save an empty document.
    Create {
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Show a saved document.
    Show {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Apply a JSON array of commands as one transaction, then save.
    Apply {
        #[command(flatten)]
        target: Target,
        /// Commands as a JSON array, e.g. '[{"op":"add_atom","element":"C","position":{"x":0,"y":0}}]'.
        #[arg(
            long,
            conflicts_with = "commands_file",
            required_unless_present = "commands_file"
        )]
        commands: Option<String>,
        /// Read the JSON command array from this file, or `-` for standard input.
        #[arg(long)]
        commands_file: Option<PathBuf>,
    },
    /// Undo the latest saved transaction as a new revision, then save.
    Undo {
        #[command(flatten)]
        target: Target,
    },
    /// Redo the latest undone transaction as a new revision, then save.
    Redo {
        #[command(flatten)]
        target: Target,
    },
    /// Save the document as a new, independent document; the original stays as saved.
    SaveAs {
        #[command(flatten)]
        target: Target,
        /// Defaults to the original title followed by " (copy)".
        #[arg(long)]
        title: Option<String>,
    },
    /// Recover the unsaved work an ended process left in a recovery draft, then save it.
    Recover {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Save a recovery draft as a new document, e.g. when the original was saved since.
    RecoverAsCopy {
        id: String,
        /// Defaults to the draft's title followed by " (recovered)".
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Delete a recovery draft an ended process left behind.
    DiscardDraft {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Rename a saved document; the new title is saved at once.
    Rename {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        title: String,
    },
    /// Move a saved document to the trash. Refused while a recovery draft holds unsaved work.
    Delete {
        #[command(flatten)]
        target: Target,
    },
    /// List documents in the trash, most recently deleted first.
    Trash {
        #[arg(long)]
        json: bool,
    },
    /// Move a document from the trash back to the saved documents.
    Restore {
        id: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Sort {
    /// By title.
    Title,
    /// Most recently saved first.
    Recent,
}

impl From<Sort> for ListSort {
    fn from(sort: Sort) -> Self {
        match sort {
            Sort::Title => Self::Title,
            Sort::Recent => Self::Recent,
        }
    }
}

#[derive(ClapArgs)]
struct Target {
    id: String,
    /// The revision the edit is based on; a different current revision is a conflict.
    #[arg(long)]
    expected_revision: u64,
    #[arg(long)]
    json: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();
    let data_dir = args.data_dir.or_else(default_data_dir);
    let documents_need_data = matches!(args.command, Command::Documents(_) | Command::Operations);
    let Some(data_dir) = data_dir.or_else(|| (!documents_need_data).then(PathBuf::new)) else {
        eprintln!("error: no user data directory found; pass --data-dir or set CHEMIFREE_DATA_DIR");
        return ExitCode::FAILURE;
    };
    // Construction performs no I/O; status and capabilities never touch storage.
    let application = Application::new(LocalCapabilities, FileDocumentRepository::new(&data_dir));

    match args.command {
        Command::Status { json } => {
            let health = application.health();
            if json {
                print_json(&health);
            } else {
                println!(
                    "{} {} · {} · API {} · {}",
                    health.service, health.version, health.stage, health.api_version, health.status
                );
                println!("Liveness only. Run `chemifree capabilities` for implementation status.");
            }
            ExitCode::SUCCESS
        }
        Command::Capabilities { json } => {
            let response = application.capabilities();
            if json {
                print_json(&response);
            } else {
                println!("Chemifree capabilities · API {}", response.api_version);
                for capability in response.capabilities {
                    println!("{}: {}", capability.id, capability.status);
                    println!("  {}", capability.description);
                }
                println!(
                    "Operation contract {:?}:",
                    response.operation_catalog.contracts
                );
                for operation in response.operation_catalog.operations {
                    println!("{}: {}", operation.id, operation.execution);
                }
            }
            ExitCode::SUCCESS
        }
        Command::Documents(command) => run_document_command(&application, command),
        Command::Operations => run_operations(&application),
    }
}

fn run_document_command(application: &LocalApplication, command: DocumentCommand) -> ExitCode {
    match command {
        DocumentCommand::List { query, sort, json } => report(
            application.list_documents(&ListQuery {
                query,
                sort: sort.into(),
            }),
            json,
            print_list,
        ),
        DocumentCommand::Create { title, json } => report(
            application.create_document(None, title.as_deref()),
            json,
            print_document,
        ),
        DocumentCommand::Show { id, json } => {
            report(application.document(&id), json, print_document)
        }
        DocumentCommand::Apply {
            target,
            commands,
            commands_file,
        } => {
            let result = read_commands(commands, commands_file).and_then(|commands| {
                let edited = application.apply_transaction(
                    &target.id,
                    None,
                    target.expected_revision,
                    commands,
                )?;
                save(application, &target.id, &edited)
            });
            report(result, target.json, print_document)
        }
        DocumentCommand::Undo { target } => {
            let result = application
                .undo(&target.id, None, target.expected_revision)
                .and_then(|undone| save(application, &target.id, &undone));
            report(result, target.json, print_document)
        }
        DocumentCommand::Redo { target } => {
            let result = application
                .redo(&target.id, None, target.expected_revision)
                .and_then(|redone| save(application, &target.id, &redone));
            report(result, target.json, print_document)
        }
        DocumentCommand::SaveAs { target, title } => report(
            application.save_as(&target.id, None, target.expected_revision, title.as_deref()),
            target.json,
            print_copy,
        ),
        DocumentCommand::Recover { id, json } => {
            let result = application
                .recover(&id, None)
                .and_then(|recovered| save(application, &id, &recovered));
            report(result, json, print_document)
        }
        DocumentCommand::RecoverAsCopy { id, title, json } => report(
            application.recover_as_copy(&id, None, title.as_deref()),
            json,
            print_copy,
        ),
        DocumentCommand::DiscardDraft { id, json } => report(
            application.discard_draft(&id, None),
            json,
            |response: &DraftDiscardResponse| {
                println!("Discarded the recovery draft of {}", response.id)
            },
        ),
        DocumentCommand::Rename { target, title } => report(
            application.rename(&target.id, None, target.expected_revision, &title),
            target.json,
            print_document,
        ),
        DocumentCommand::Delete { target } => report(
            application.delete(&target.id, None, target.expected_revision),
            target.json,
            |response: &DeleteResponse| {
                println!("Moved {} ({}) to the trash", response.title, response.id)
            },
        ),
        DocumentCommand::Trash { json } => report(application.list_trash(), json, print_trash),
        DocumentCommand::Restore { id, json } => report(
            application.restore(&id, None),
            json,
            |response: &RestoreResponse| println!("Restored {} ({})", response.title, response.id),
        ),
    }
}

/// The CLI's session ends with the process, so every edit is committed before success.
fn save(
    application: &LocalApplication,
    id: &str,
    edited: &DocumentResponse,
) -> Result<DocumentResponse, ApplicationError> {
    application.save(id, None, edited.document.revision)
}

fn read_commands(
    inline: Option<String>,
    file: Option<PathBuf>,
) -> Result<Vec<CommandDto>, ApplicationError> {
    let invalid = |message: String| ApplicationError::new(ErrorCode::InvalidRequest, message);
    let text = match (inline, file) {
        (Some(text), _) => text,
        (None, Some(path)) => {
            let reader: Box<dyn Read> = if path.as_os_str() == "-" {
                Box::new(io::stdin())
            } else {
                Box::new(File::open(&path).map_err(|error| {
                    invalid(format!("could not open {}: {error}", path.display()))
                })?)
            };
            let mut text = String::new();
            reader
                .take(MAX_COMMAND_INPUT_BYTES + 1)
                .read_to_string(&mut text)
                .map_err(|error| invalid(format!("could not read commands: {error}")))?;
            text
        }
        (None, None) => unreachable!("clap requires --commands or --commands-file"),
    };
    if text.len() as u64 > MAX_COMMAND_INPUT_BYTES {
        return Err(invalid(format!(
            "command input exceeds {MAX_COMMAND_INPUT_BYTES} bytes"
        )));
    }
    serde_json::from_str(&text).map_err(|error| invalid(format!("invalid commands: {error}")))
}

/// Prints a success to stdout. Failures go to stdout as a v1 error envelope with `--json`
/// (so scripts can parse them) or to stderr otherwise; both exit with status 1.
fn report<T: Serialize>(
    result: Result<T, ApplicationError>,
    json: bool,
    human: fn(&T),
) -> ExitCode {
    match result {
        Ok(value) if json => {
            print_json(&value);
            ExitCode::SUCCESS
        }
        Ok(value) => {
            human(&value);
            ExitCode::SUCCESS
        }
        Err(error) if json => {
            print_json(&error.to_response());
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Answers each line of standard input as one operation request, in order. A request over
/// [`MAX_COMMAND_INPUT_BYTES`], not counting its line ending, is answered as too large without
/// being read whole. Blank lines are skipped without a reply.
fn run_operations(application: &LocalApplication) -> ExitCode {
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    let mut line = Vec::new();
    loop {
        line.clear();
        // Two more bytes than a request may hold leave room for a CRLF ending.
        let read = (&mut input)
            .take(MAX_COMMAND_INPUT_BYTES + 2)
            .read_until(b'\n', &mut line);
        match read {
            Ok(0) => return ExitCode::SUCCESS,
            Ok(_) => {}
            Err(error) => {
                eprintln!("error: could not read standard input: {error}");
                return ExitCode::FAILURE;
            }
        }
        // The line ending separates requests; it is not part of the request.
        let (request, ended) = match line.strip_suffix(b"\n") {
            Some(request) => (request.strip_suffix(b"\r").unwrap_or(request), true),
            None => (&line[..], false),
        };
        let reply = if request.len() as u64 > MAX_COMMAND_INPUT_BYTES {
            if !ended && let Err(error) = input.skip_until(b'\n') {
                eprintln!("error: could not read standard input: {error}");
                return ExitCode::FAILURE;
            }
            OperationResponse::unread(
                ErrorCode::ResourceLimit.as_str(),
                format!("The request is larger than {MAX_COMMAND_INPUT_BYTES} bytes."),
            )
        } else if request.iter().all(u8::is_ascii_whitespace) {
            continue;
        } else {
            application.execute(request)
        };
        let written = serde_json::to_writer(&mut output, &reply)
            .map_err(io::Error::from)
            .and_then(|()| output.write_all(b"\n"))
            .and_then(|()| output.flush());
        if let Err(error) = written {
            eprintln!("error: could not write the reply: {error}");
            return ExitCode::FAILURE;
        }
    }
}

fn print_json<T: Serialize>(value: &T) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).expect("responses are serializable")
    );
}

fn print_list(response: &DocumentListResponse) {
    if response.documents.is_empty() {
        println!("No saved documents.");
    }
    for document in &response.documents {
        let recovery = match document.recovery.as_ref().map(|recovery| recovery.state) {
            None => "",
            Some(RecoveryState::Recoverable) => "  [recovery draft: recover or discard-draft]",
            Some(RecoveryState::Outdated) => {
                "  [outdated recovery draft: recover-as-copy or discard-draft]"
            }
            Some(RecoveryState::Unreadable) => "  [unreadable recovery draft: discard-draft]",
            Some(RecoveryState::InUse) => "  [recovery draft in use by a running process]",
        };
        println!(
            "{}  r{}  {} atoms, {} bonds  {}{recovery}",
            document.id,
            document.revision,
            document.atom_count,
            document.bond_count,
            document.title
        );
    }
    for entry in &response.unreadable {
        eprintln!("unreadable {}: {}", entry.id, entry.message);
    }
}

fn print_trash(response: &TrashListResponse) {
    if response.documents.is_empty() {
        println!("The trash is empty.");
    }
    for document in &response.documents {
        println!(
            "{}  r{}  {} atoms, {} bonds  {}",
            document.id,
            document.revision,
            document.atom_count,
            document.bond_count,
            document.title
        );
    }
    for entry in &response.unreadable {
        eprintln!("unreadable {}: {}", entry.id, entry.message);
    }
}

fn print_copy(response: &CopyResponse) {
    print_document(&response.copy);
    println!("  copied from {}", response.source.id);
}

fn print_document(response: &DocumentResponse) {
    let document = &response.document;
    println!(
        "{} · {} · revision {}",
        document.title, document.id, document.revision
    );
    for atom in &document.atoms {
        let isotope = atom
            .isotope
            .map(|mass| mass.to_string())
            .unwrap_or_default();
        let charge = match atom.charge {
            0 => String::new(),
            charge if charge > 0 => format!(" {charge:+}"),
            charge => format!(" {charge}"),
        };
        println!(
            "  {}  {isotope}{}{charge}  ({}, {})",
            atom.id, atom.element, atom.position.x, atom.position.y
        );
    }
    for bond in &document.bonds {
        let order = serde_json::to_value(bond.order).expect("bond orders are serializable");
        println!(
            "  {}  {}–{}  {}",
            bond.id,
            bond.begin,
            bond.end,
            order.as_str().unwrap_or_default()
        );
    }
    println!(
        "  undo {} · redo {}",
        response.session.undo_depth, response.session.redo_depth
    );
}
