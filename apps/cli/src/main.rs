use std::fs::File;
use std::io::{self, BufRead, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use chemifree_application::{
    Application, ApplicationError, AtomKindView, AtomView, BondQueryDto, BondView, CommandDto,
    CopyResponse, DeleteResponse, DocumentListResponse, DocumentResponse, DraftDiscardResponse,
    ErrorCode, GraphicShapeDto, HydrogensDto, ListQuery, ListSort, OperationResponse,
    PreferencesChange, PreferencesResponse, RadicalDto, RecoveryState, ReferenceTargetDto,
    RestoreResponse, SceneObjectView, SelectionDto, SettingsDto, SettingsSource,
    TransactionSelections, TrashListResponse,
};
use chemifree_providers::{
    FileDocumentRepository, FilePreferenceStore, LocalCapabilities, default_data_dir,
};
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
    /// Show and change the preferences of the data directory: the theme, and the settings new
    /// documents start from. Existing documents never change with them.
    #[command(subcommand)]
    Preferences(PreferenceCommand),
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
        /// Selection captured when the edit was requested, as a JSON object.
        #[arg(long)]
        selection_before: Option<String>,
        /// Selection after the edit, as JSON; scene objects can reference a command as `$N`.
        #[arg(long)]
        selection_after: Option<String>,
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
    /// Show, reset, or import a document's style settings. Edit them with `apply` and
    /// `update_settings`.
    #[command(subcommand)]
    Settings(SettingsCommand),
}

#[derive(Subcommand)]
enum SettingsCommand {
    /// Show a saved document's settings.
    Show {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Replace the settings with the built-in defaults or the preferred new-document settings,
    /// as one undoable edit, then save.
    Reset {
        #[command(flatten)]
        target: Target,
        #[arg(long, value_enum)]
        to: ResetTo,
    },
    /// Apply another document's settings as one undoable edit, then save.
    Import {
        #[command(flatten)]
        target: Target,
        /// The document whose settings are applied.
        #[arg(long)]
        source: String,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum ResetTo {
    /// The built-in defaults.
    Defaults,
    /// The settings a new document would start from now.
    Preferences,
}

impl From<ResetTo> for SettingsSource {
    fn from(to: ResetTo) -> Self {
        match to {
            ResetTo::Defaults => Self::Defaults,
            ResetTo::Preferences => Self::Preferences,
        }
    }
}

#[derive(Subcommand)]
enum PreferenceCommand {
    /// Show the preferences.
    Show {
        #[arg(long)]
        json: bool,
    },
    /// Replace the preferences, e.g. '{"ui":{"theme":"dark"},"new_documents":{"bond_length_pt":40}}'
    /// (the body of PUT /api/v1/preferences without request_id and expected_revision).
    Set {
        #[arg(long)]
        preferences: String,
        #[command(flatten)]
        revision: PreferenceRevision,
    },
    /// Reset the preferences to the built-in defaults.
    Reset {
        #[command(flatten)]
        revision: PreferenceRevision,
    },
    /// Make new documents start from every setting of a saved document.
    Import {
        document: String,
        #[command(flatten)]
        revision: PreferenceRevision,
    },
}

#[derive(ClapArgs)]
struct PreferenceRevision {
    /// The preferences revision the change is based on; a different one is a conflict.
    #[arg(long)]
    expected_revision: u64,
    #[arg(long)]
    json: bool,
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
    let documents_need_data = matches!(
        args.command,
        Command::Documents(_) | Command::Preferences(_) | Command::Operations
    );
    let Some(data_dir) = data_dir.or_else(|| (!documents_need_data).then(PathBuf::new)) else {
        eprintln!("error: no user data directory found; pass --data-dir or set CHEMIFREE_DATA_DIR");
        return ExitCode::FAILURE;
    };
    // Construction performs no I/O; status and capabilities never touch storage.
    let application = Application::new(LocalCapabilities, FileDocumentRepository::new(&data_dir))
        .with_preferences(FilePreferenceStore::new(&data_dir));

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
        Command::Preferences(command) => run_preference_command(&application, command),
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
            selection_before,
            selection_after,
        } => {
            let result = read_commands(commands, commands_file).and_then(|commands| {
                let selection = TransactionSelections {
                    before: read_selection(selection_before)?,
                    after: read_selection(selection_after)?,
                };
                let edited = application.apply_transaction_selecting(
                    &target.id,
                    None,
                    target.expected_revision,
                    commands,
                    selection,
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
        DocumentCommand::Settings(SettingsCommand::Show { id, json }) => report(
            application
                .document(&id)
                .map(|response| response.document.settings),
            json,
            |settings: &SettingsDto| println!("{}", settings_text(settings)),
        ),
        DocumentCommand::Settings(SettingsCommand::Reset { target, to }) => {
            let result = application
                .reset_settings(&target.id, None, target.expected_revision, to.into())
                .and_then(|reset| save(application, &target.id, &reset));
            report(result, target.json, print_document)
        }
        DocumentCommand::Settings(SettingsCommand::Import { target, source }) => {
            let result = application
                .import_settings(&target.id, None, target.expected_revision, &source)
                .and_then(|imported| save(application, &target.id, &imported));
            report(result, target.json, print_document)
        }
    }
}

fn run_preference_command(application: &LocalApplication, command: PreferenceCommand) -> ExitCode {
    let print = |response: &PreferencesResponse| print!("{}", preferences_text(response));
    match command {
        PreferenceCommand::Show { json } => report(application.preferences(), json, print),
        PreferenceCommand::Set {
            preferences,
            revision,
        } => {
            let result = serde_json::from_str::<PreferencesChange>(&preferences)
                .map_err(|error| {
                    ApplicationError::new(
                        ErrorCode::InvalidRequest,
                        format!("invalid preferences: {error}"),
                    )
                })
                .and_then(|change| {
                    application.set_preferences(None, revision.expected_revision, change)
                });
            report(result, revision.json, print)
        }
        PreferenceCommand::Reset { revision } => report(
            application.reset_preferences(None, revision.expected_revision),
            revision.json,
            print,
        ),
        PreferenceCommand::Import { document, revision } => report(
            application.preferences_from_document(None, revision.expected_revision, &document),
            revision.json,
            print,
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

fn read_selection(value: Option<String>) -> Result<Option<SelectionDto>, ApplicationError> {
    let invalid = |message: String| ApplicationError::new(ErrorCode::InvalidRequest, message);
    value
        .map(|text| {
            if text.len() as u64 > MAX_COMMAND_INPUT_BYTES {
                return Err(invalid(format!(
                    "selection input exceeds {MAX_COMMAND_INPUT_BYTES} bytes"
                )));
            }
            serde_json::from_str(&text)
                .map_err(|error| invalid(format!("invalid selection: {error}")))
        })
        .transpose()
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
    print!("{}", document_text(response));
}

/// The text `print_document` prints: one line per atom and bond, with every recorded state
/// that is stated. Inferred hydrogens print nothing, since nothing counts them.
fn document_text(response: &DocumentResponse) -> String {
    let document = &response.document;
    let mut lines = vec![format!(
        "{} · {} · revision {}",
        document.title, document.id, document.revision
    )];
    for atom in &document.atoms {
        lines.push(format!(
            "  {}  {}  ({}, {})",
            atom.id,
            atom_text(atom),
            atom.position.x,
            atom.position.y
        ));
    }
    let instances = &document.scene.instances;
    if let [first, _, ..] = instances.as_slice() {
        let ids: Vec<&str> = instances
            .iter()
            .map(|instance| instance.id.as_str())
            .collect();
        lines.push(format!(
            "  scene instances {} (positions above are {}'s)",
            ids.join(", "),
            first.id
        ));
    }
    for bond in &document.bonds {
        lines.push(format!(
            "  {}  {}–{}  {}",
            bond.id,
            bond.begin,
            bond.end,
            bond_text(bond)
        ));
    }
    for object in &document.scene.objects {
        match object {
            SceneObjectView::Instance {
                id,
                parent: Some(parent),
                ..
            } => {
                lines.push(format!("  {id}  instance in {parent}"));
            }
            SceneObjectView::Instance { .. } => {}
            SceneObjectView::Group {
                id,
                parent,
                children,
            } => {
                lines.push(format!(
                    "  {id}  group in {}: {}",
                    parent.as_deref().unwrap_or("page"),
                    children.join(", ")
                ));
            }
            SceneObjectView::Graphic {
                id,
                parent,
                shape,
                points,
            } => {
                let shape = match shape {
                    GraphicShapeDto::Line => "line",
                    GraphicShapeDto::Rectangle => "rectangle",
                    GraphicShapeDto::Arrow => "arrow",
                };
                lines.push(format!(
                    "  {id}  {shape} in {}  ({}, {})–({}, {})",
                    parent.as_deref().unwrap_or("page"),
                    points[0].x,
                    points[0].y,
                    points[1].x,
                    points[1].y
                ));
            }
        }
    }
    for reference in &document.scene.references {
        let (kind, id) = match &reference.target {
            ReferenceTargetDto::Object { id } => ("object", id),
            ReferenceTargetDto::Atom { id } => ("atom", id),
            ReferenceTargetDto::Bond { id } => ("bond", id),
        };
        lines.push(format!("  {} references {kind} {id}", reference.source));
    }
    if !document.selection.objects.is_empty()
        || !document.selection.atoms.is_empty()
        || !document.selection.bonds.is_empty()
    {
        lines.push(format!(
            "  selected: objects [{}], atoms [{}], bonds [{}]",
            document.selection.objects.join(", "),
            document.selection.atoms.join(", "),
            document.selection.bonds.join(", ")
        ));
    }
    lines.push(format!("  settings  {}", settings_text(&document.settings)));
    lines.push(format!(
        "  undo {} · redo {}",
        response.session.undo_depth, response.session.redo_depth
    ));
    lines.iter().map(|line| format!("{line}\n")).collect()
}

/// Every setting on one line; lengths in pt whatever the display unit.
fn settings_text(settings: &SettingsDto) -> String {
    format!(
        "bond length {} pt · line width {} pt · labels {} {} pt · carbon labels {} · display \
         unit {}",
        settings.bond_length_pt,
        settings.line_width_pt,
        name(&settings.label_font),
        settings.label_size_pt,
        name(&settings.carbon_labels),
        name(&settings.display_unit)
    )
}

/// The theme, the settings new documents start from (and which of them preferences set), and
/// the computation preferences, which are not implemented.
fn preferences_text(response: &PreferencesResponse) -> String {
    let mut lines = vec![
        format!("Preferences · revision {}", response.revision),
        format!("  theme {}", name(&response.ui.theme)),
        format!(
            "  new documents  {}",
            settings_text(&response.new_document_settings)
        ),
    ];
    let set = serde_json::to_value(&response.new_documents).expect("settings are serializable");
    let set: Vec<&String> = set
        .as_object()
        .map(|set| set.keys().collect())
        .unwrap_or_default();
    if !set.is_empty() {
        let set: Vec<&str> = set.into_iter().map(String::as_str).collect();
        lines.push(format!("    set: {}", set.join(", ")));
    }
    for (id, setting) in [
        ("nmr", &response.computation.nmr),
        ("numeric_precision", &response.computation.numeric_precision),
    ] {
        lines.push(format!(
            "  computation.{id}  not implemented ({})",
            setting.followup
        ));
    }
    lines.iter().map(|line| format!("{line}\n")).collect()
}

/// A unit enum's name as the JSON contract spells it.
fn name<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .expect("names are serializable")
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

/// `13C-1`, `query ![N,O]`, followed by the stated hydrogens, radical, and attachment points.
fn atom_text(atom: &AtomView) -> String {
    let isotope = atom
        .isotope
        .map(|mass| mass.to_string())
        .unwrap_or_default();
    let charge = match atom.charge {
        0 => String::new(),
        charge if charge > 0 => format!(" {charge:+}"),
        charge => format!(" {charge}"),
    };
    let label = match atom.kind {
        AtomKindView::Element => atom.element.clone(),
        AtomKindView::Query => format!("query {}", atom.element),
    };
    let mut text = format!("{isotope}{label}{charge}");
    match atom.hydrogens {
        HydrogensDto::Inferred {} => {}
        HydrogensDto::Unknown {} => text.push_str("  H unknown"),
        HydrogensDto::Specified { count } => text.push_str(&format!("  H{count} stated")),
    }
    let radical = match atom.radical {
        RadicalDto::None => None,
        RadicalDto::Singlet => Some("singlet"),
        RadicalDto::Doublet => Some("doublet"),
        RadicalDto::Triplet => Some("triplet"),
        RadicalDto::Unknown => Some("unknown"),
    };
    if let Some(radical) = radical {
        text.push_str(&format!("  radical {radical}"));
    }
    if !atom.attachment_points.is_empty() {
        let points: Vec<String> = atom.attachment_points.iter().map(u8::to_string).collect();
        text.push_str(&format!("  attachment {}", points.join(",")));
    }
    text
}

/// The bond's order, and for a query bond what it stands for.
fn bond_text(bond: &BondView) -> String {
    let order = serde_json::to_value(bond.order).expect("bond orders are serializable");
    let order = order.as_str().unwrap_or_default();
    match &bond.query {
        None => order.to_owned(),
        Some(BondQueryDto::Any {}) => format!("{order} any"),
        Some(BondQueryDto::List { orders }) => {
            let kinds: Vec<String> = orders
                .iter()
                .map(|kind| {
                    serde_json::to_value(kind)
                        .expect("bond kinds are serializable")
                        .as_str()
                        .unwrap_or_default()
                        .to_owned()
                })
                .collect();
            format!("{order} {}", kinds.join("|"))
        }
    }
}

#[cfg(test)]
mod tests {
    use chemifree_application::{Application, CommandDto};
    use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
    use serde_json::json;

    use super::*;

    fn shown(commands: serde_json::Value) -> String {
        let data = tempfile::tempdir().unwrap();
        let application =
            Application::new(LocalCapabilities, FileDocumentRepository::new(data.path()));
        let id = application
            .create_document(None, Some("Text"))
            .unwrap()
            .document
            .id;
        let commands: Vec<CommandDto> = serde_json::from_value(commands).unwrap();
        let response = application
            .apply_transaction(&id, None, 0, commands)
            .unwrap();
        document_text(&response)
    }

    #[test]
    fn stated_states_are_shown_and_inferred_hydrogens_are_not() {
        let text = shown(json!([
            {"op": "add_atom", "element": "C", "position": {"x": 0, "y": 0},
             "hydrogens": {"mode": "specified", "count": 3}, "radical": "doublet",
             "attachment_points": [1, 2]},
            {"op": "add_atom", "element": "N", "position": {"x": 30, "y": 0},
             "hydrogens": {"mode": "unknown"}, "bond_to": {"atom": "$0", "order": "aromatic"}},
            {"op": "add_atom", "element": "O", "position": {"x": 60, "y": 0}},
            {"op": "add_bond", "begin": "$1", "end": "$2", "order": "dative"}
        ]));
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines[1..=3],
            [
                "  a1  C  H3 stated  radical doublet  attachment 1,2  (0, 0)",
                "  a2  N  H unknown  (30, 0)",
                "  a3  O  (60, 0)",
            ],
            "{text}"
        );
        assert_eq!(
            lines[4..=5],
            ["  b1  a1–a2  aromatic", "  b2  a2–a3  dative"]
        );
    }

    #[test]
    fn query_atoms_and_bonds_show_their_expression() {
        let text = shown(json!([
            {"op": "add_atom", "query": {"kind": "list", "elements": ["Cl", "Br", "I"], "negated": true},
             "position": {"x": 0, "y": 0}, "hydrogens": {"mode": "specified", "count": 0}},
            {"op": "add_atom", "query": {"kind": "any"}, "position": {"x": 30, "y": 0},
             "bond_to": {"atom": "$0", "order": "query", "query": {"kind": "list", "orders": ["single", "aromatic"]}}},
            {"op": "add_atom", "element": "C", "position": {"x": 60, "y": 0},
             "bond_to": {"atom": "$1", "order": "query", "query": {"kind": "any"}}}
        ]));
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines[1..=2],
            [
                "  a1  query ![Cl,Br,I]  H0 stated  (0, 0)",
                "  a2  query *  (30, 0)",
            ],
            "{text}"
        );
        assert_eq!(
            lines[4..=5],
            [
                "  b1  a1–a2  query single|aromatic",
                "  b2  a2–a3  query any"
            ]
        );
    }
    #[test]
    fn the_settings_are_shown_with_every_document() {
        let text = shown(json!([
            {"op": "update_settings", "bond_length_pt": 42.5, "label_font": "serif",
             "carbon_labels": "all", "display_unit": "mm"}
        ]));
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines[lines.len() - 2..],
            [
                "  settings  bond length 42.5 pt · line width 1.2 pt · labels serif 12 pt · \
                 carbon labels all · display unit mm",
                "  undo 1 · redo 0"
            ],
            "{text}"
        );
    }

    #[test]
    fn preferences_show_new_document_settings_and_unimplemented_computation() {
        let data = tempfile::tempdir().unwrap();
        let application =
            Application::new(LocalCapabilities, FileDocumentRepository::new(data.path()));
        let change: PreferencesChange = serde_json::from_value(json!({
            "ui": {"theme": "dark"}, "new_documents": {"line_width_pt": 2}
        }))
        .unwrap();
        let response = application.set_preferences(None, 0, change).unwrap();
        assert_eq!(
            preferences_text(&response),
            "Preferences · revision 1\n\
             \x20 theme dark\n\
             \x20 new documents  bond length 30 pt · line width 2 pt · labels sans-serif 12 pt · \
             carbon labels default · display unit pt\n\
             \x20   set: line_width_pt\n\
             \x20 computation.nmr  not implemented (P66)\n\
             \x20 computation.numeric_precision  not implemented (P15)\n"
        );
    }
}
