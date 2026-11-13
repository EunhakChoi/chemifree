//! Assembly of the local `Application` for the entry adapters: the HTTP API, the CLI and the
//! conformance runner. Each adapter keeps its own transport, data-directory resolution and
//! host-only settings, and builds the application through this crate. Only the HTTP API takes the
//! data directory's [`ServerLock`], before it prunes or serves, so a second API server refuses that
//! directory; the CLI and the conformance runner do not take it.

mod inchi;
mod server_lock;

use std::path::Path;

use chemifree_application::Application;
use chemifree_providers::interchange_codec::LocalInterchangeCodec;
use chemifree_providers::png_image::EmbeddedPngDecoder;
use chemifree_providers::{
    FileDocumentRepository, FilePreferenceStore, LocalCapabilities, OpenSmilesCodec, V2000Codec,
    configure_selected_registry,
};

pub use inchi::InchiArgs;
pub use server_lock::{LockHolder, ServerLock, ServerLockError};

/// The application the local hosts run: the local capability catalog over saved documents.
pub type LocalApplication = Application<LocalCapabilities, FileDocumentRepository>;

/// The file stores of one data directory: saved documents and preferences.
#[derive(Debug)]
pub struct LocalStores {
    documents: FileDocumentRepository,
    preferences: FilePreferenceStore,
}

impl LocalStores {
    /// Points both stores at `data_dir`. Nothing is read or created here; the stores touch the
    /// disk when they are used.
    #[must_use]
    pub fn new(data_dir: &Path) -> Self {
        Self {
            documents: FileDocumentRepository::new(data_dir),
            preferences: FilePreferenceStore::new(data_dir),
        }
    }

    /// Removes temporary files that interrupted writes abandoned more than an hour ago: first
    /// from the document store, then from the preference store. Younger files may belong to a
    /// write in progress in another process. [`local_application`] does not call this.
    pub fn prune_stale_temporaries(&self) {
        self.documents.prune_stale_temporaries();
        self.preferences.prune_stale_temporaries();
    }

    /// The directory that holds the saved documents.
    #[must_use]
    pub fn documents_directory(&self) -> &Path {
        self.documents.directory()
    }
}

/// Connects the embedded PNG decoder, the interchange codec, the CTfile codec, the SMILES codec
/// and the file preferences in `stores`. It performs no I/O and prunes nothing. It sets no
/// user-selected DATA registry, no optional InChI provider, no compute limits and no concurrency
/// limit; a caller adds those with the `Application` builders.
#[must_use]
pub fn local_application(stores: LocalStores) -> LocalApplication {
    Application::new(LocalCapabilities, stores.documents)
        .with_image_decoder(EmbeddedPngDecoder)
        .with_interchange_codec(LocalInterchangeCodec)
        .with_ctfile_codec(V2000Codec)
        .with_smiles_codec(OpenSmilesCodec)
        .with_preferences(stores.preferences)
}

/// Attaches the user-selected DATA registry, then the optional InChI provider. A refused DATA
/// selection becomes a reported state and never fails the host. An InChI refusal is also reported
/// on standard error.
#[must_use]
pub fn with_selected_providers(
    application: LocalApplication,
    model_data_root: Option<&Path>,
    model_trusted_registry: Option<&Path>,
    inchi: InchiArgs,
) -> LocalApplication {
    let registry = configure_selected_registry(model_data_root, model_trusted_registry);
    inchi.configure(application.with_data_registry(registry))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs::{self, File};
    use std::time::{Duration, SystemTime};

    use chemifree_application::PreferencesChange;
    use serde_json::{Value, json};

    /// Two hours: older than the hour after which the stores prune an abandoned temporary.
    const STALE_AGE: Duration = Duration::from_secs(2 * 60 * 60);
    /// The `format_conversion` label with every codec that `local_application` attaches.
    const ALL_CODECS_LABEL: &str = "MOL/SDF, CDXML, CDX, SMILES and SVG subsets";
    /// The largest `time_limit_ms` that the default compute limits accept.
    const DEFAULT_MAX_TIME_MS: u64 = 8_000;

    /// An empty file whose modification time is two hours old, like an interrupted write.
    fn stale_file(path: &Path) {
        File::create(path)
            .unwrap()
            .set_modified(SystemTime::now() - STALE_AGE)
            .unwrap();
    }

    /// An empty file written now, like a write that may still be in progress.
    fn young_file(path: &Path) {
        File::create(path).unwrap();
    }

    fn application_in(data: &Path) -> LocalApplication {
        local_application(LocalStores::new(data))
    }

    /// Runs one request in process and returns its reply as JSON, the form the adapters print.
    fn reply(application: &LocalApplication, request: &Value) -> Value {
        serde_json::to_value(application.execute(request.to_string().as_bytes())).unwrap()
    }

    /// The DATA state as the `/api/v1/data` route reports it.
    fn data_status(application: &LocalApplication) -> Value {
        serde_json::to_value(application.data_status()).unwrap()
    }

    /// A `chem.inchi` request for a document that does not exist. With no provider the reply
    /// names the unavailable provider and never looks the document up.
    fn inchi_request() -> Value {
        json!({"contract": 1, "request_id": "inchi", "profile": "preserve-semantics",
            "operation": "chem.inchi", "parameters": {"document_id": "not-created"}})
    }

    /// A `chem.aromaticity_convert` request to Kekule form at revision 0 with `time_limit_ms`.
    fn aromaticity_request(document_id: &str, time_limit_ms: u64) -> Value {
        json!({"contract": 1, "request_id": "aromaticity", "profile": "preserve-semantics",
            "operation": "chem.aromaticity_convert",
            "parameters": {"document_id": document_id, "expected_revision": 0,
                "to": "kekule", "time_limit_ms": time_limit_ms}})
    }

    #[test]
    fn local_stores_prune_only_stale_temporaries() {
        let directory = tempfile::tempdir().unwrap();
        let data = directory.path();
        let documents = data.join("documents");
        fs::create_dir_all(&documents).unwrap();
        // An interrupted write leaves `.<stem>.<pid>.<counter>.tmp` behind. The documents store
        // prunes such names in its directory; the preferences store prunes those that start
        // with `.preferences.` in the data directory.
        let stale_document = documents.join(".drawing.4242.0.tmp");
        let young_document = documents.join(".drawing.4242.1.tmp");
        let stale_preferences = data.join(".preferences.4242.0.tmp");
        let young_preferences = data.join(".preferences.4242.1.tmp");
        // A saved file is not a temporary, however old it is.
        let saved_document = documents.join("drawing.json");
        let saved_preferences = data.join("preferences.json");
        stale_file(&stale_document);
        young_file(&young_document);
        stale_file(&stale_preferences);
        young_file(&young_preferences);
        stale_file(&saved_document);
        stale_file(&saved_preferences);

        let stores = LocalStores::new(data);
        stores.prune_stale_temporaries();

        assert!(
            !stale_document.exists(),
            "the abandoned document temporary must be removed"
        );
        assert!(
            young_document.exists(),
            "a document temporary that may be in progress must remain"
        );
        assert!(
            !stale_preferences.exists(),
            "the abandoned preferences temporary must be removed"
        );
        assert!(
            young_preferences.exists(),
            "a preferences temporary that may be in progress must remain"
        );
        assert!(
            saved_document.exists(),
            "a saved document must not be pruned"
        );
        assert!(
            saved_preferences.exists(),
            "saved preferences must not be pruned"
        );
        assert_eq!(stores.documents_directory(), documents.as_path());
    }

    #[test]
    fn local_application_connects_every_codec_and_file_preferences() {
        let directory = tempfile::tempdir().unwrap();
        let data = directory.path();
        let application = application_in(data);

        let capabilities = serde_json::to_value(application.capabilities()).unwrap();
        let conversion = capabilities["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["id"] == "format_conversion")
            .expect("format_conversion is listed");
        assert_eq!(
            conversion["label"], ALL_CODECS_LABEL,
            "with every codec attached, the label names each of them"
        );

        let change: PreferencesChange = serde_json::from_value(json!({
            "ui": {"theme": "dark"}, "new_documents": {"bond_length_pt": 40}
        }))
        .unwrap();
        let saved = application.set_preferences(None, 0, change).unwrap();
        assert_eq!(
            saved.revision, 1,
            "a first write moves preferences to revision 1"
        );
        assert!(
            data.join("preferences.json").is_file(),
            "file-backed preferences are saved as preferences.json in the data directory"
        );
    }

    #[test]
    fn local_application_has_default_limits_and_no_optional_providers() {
        let directory = tempfile::tempdir().unwrap();
        let application = application_in(directory.path());

        assert_eq!(
            data_status(&application),
            json!({"api_version": "v1", "state": "not_configured", "reason": null,
                "registrations": [], "engine": "not_implemented"}),
            "no DATA selection is configured by default"
        );
        let inchi = reply(&application, &inchi_request());
        assert_eq!(inchi["outcome"], "unsupported");
        assert_eq!(inchi["diagnostics"][0]["code"], "inchi_unavailable");
        assert_eq!(
            inchi["diagnostics"][0]["details"],
            json!({}),
            "without a registration error there is no reason_code"
        );

        let document = application.create_document(None, Some("Limits")).unwrap();
        let over_default = reply(
            &application,
            &aromaticity_request(&document.document.id, DEFAULT_MAX_TIME_MS + 1),
        );
        assert_eq!(
            over_default["outcome"], "error",
            "a time limit above the default maximum is refused"
        );
        assert!(
            over_default["result"].is_null(),
            "a refused request has no result"
        );
        let diagnostic = &over_default["diagnostics"][0];
        assert_eq!(diagnostic["code"], "invalid_request");
        assert_eq!(diagnostic["path"], "parameters.time_limit_ms");
        assert_eq!(diagnostic["details"]["maximum_ms"], DEFAULT_MAX_TIME_MS);
        let at_default = reply(
            &application,
            &aromaticity_request(&document.document.id, DEFAULT_MAX_TIME_MS),
        );
        assert_eq!(
            at_default["outcome"], "ok",
            "the default maximum itself is accepted"
        );
    }

    #[test]
    fn selected_providers_attach_the_data_selection_then_inchi() {
        let directory = tempfile::tempdir().unwrap();
        // A DATA root without its trusted registry is an incomplete selection, reported as a state.
        let root = directory.path().join("missing");
        let selected = with_selected_providers(
            application_in(directory.path()),
            Some(root.as_path()),
            None,
            InchiArgs::default(),
        );
        assert_eq!(
            data_status(&selected),
            json!({"api_version": "v1", "state": "refused",
                "reason": "registry_configuration_incomplete", "registrations": [],
                "engine": "not_implemented"}),
            "a DATA root without a trusted registry is refused as an incomplete selection"
        );
        let inchi = reply(&selected, &inchi_request());
        assert_eq!(inchi["diagnostics"][0]["code"], "inchi_unavailable");
        assert_eq!(
            inchi["diagnostics"][0]["details"],
            json!({}),
            "InchiArgs::default() selects no provider, so there is no reason_code"
        );

        let unselected = with_selected_providers(
            application_in(directory.path()),
            None,
            None,
            InchiArgs::default(),
        );
        assert_eq!(
            data_status(&unselected),
            json!({"api_version": "v1", "state": "not_configured", "reason": null,
                "registrations": [], "engine": "not_implemented"}),
            "with no selection at all, DATA is not configured"
        );
    }
}
