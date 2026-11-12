//! The list and trash caps (`MAX_LISTED_DOCUMENTS`) through the public `Application` API, over
//! the file store in a temporary data directory. The boundary cases are the cap itself and one
//! document past it; the few-document cases stay well below it.

use std::collections::HashSet;

use chemifree_application::{Application, CapabilityCatalog, ListQuery, MAX_LISTED_DOCUMENTS};
use chemifree_domain::Capability;
use chemifree_providers::FileDocumentRepository;

/// A count far below the cap.
const FEW_DOCUMENTS: usize = 3;

struct NoCapabilities;

impl CapabilityCatalog for NoCapabilities {
    fn capabilities(&self) -> Vec<Capability> {
        Vec::new()
    }
}

type FileApplication = Application<NoCapabilities, FileDocumentRepository>;

/// An application over a file store in a fresh temporary directory. The directory is removed
/// when the returned handle drops, so the caller keeps it alive while it uses the application.
fn application_in_temp_dir() -> (tempfile::TempDir, FileApplication) {
    let directory = tempfile::tempdir().unwrap();
    let repository = FileDocumentRepository::new(directory.path());
    (directory, Application::new(NoCapabilities, repository))
}

/// Creates `count` documents and closes each, so all of them are stored and none is open.
fn store_documents(app: &FileApplication, count: usize) -> Vec<String> {
    (0..count)
        .map(|serial| {
            let title = format!("Sketch {serial:04}");
            let id = app
                .create_document(None, Some(title.as_str()))
                .unwrap()
                .document
                .id;
            app.close(&id, None, false, None).unwrap();
            id
        })
        .collect()
}

/// Creates `count` documents and moves each to the trash. Returns the identifiers of the
/// trashed documents.
fn trash_documents(app: &FileApplication, count: usize) -> Vec<String> {
    (0..count)
        .map(|serial| {
            let title = format!("Sketch {serial:04}");
            let id = app
                .create_document(None, Some(title.as_str()))
                .unwrap()
                .document
                .id;
            app.delete(&id, None, 0).unwrap();
            id
        })
        .collect()
}

/// Stores `stored` documents and checks the stored-document listing. The flag is
/// `expected_truncated`; the listing holds the lesser of the stored count and the cap, each
/// document once, all of them stored.
fn assert_stored_list(stored: usize, expected_truncated: bool) {
    let (_directory, app) = application_in_temp_dir();
    let created = store_documents(&app, stored);
    let listed = app.list_documents(&ListQuery::default()).unwrap();
    assert_eq!(
        listed.truncated, expected_truncated,
        "{stored} stored documents against a cap of {MAX_LISTED_DOCUMENTS}"
    );
    let expected_listed = stored.min(MAX_LISTED_DOCUMENTS);
    assert_eq!(
        listed.documents.len(),
        expected_listed,
        "{stored} stored documents"
    );
    let stored_ids: HashSet<&str> = created.iter().map(String::as_str).collect();
    let listed_ids: HashSet<&str> = listed
        .documents
        .iter()
        .map(|document| document.id.as_str())
        .collect();
    assert_eq!(
        listed_ids.len(),
        expected_listed,
        "no stored document is listed twice"
    );
    assert!(
        listed_ids.is_subset(&stored_ids),
        "every listed document is one of the stored documents"
    );
}

/// Trashes `trashed` documents and checks the trash listing. The flag is `expected_truncated`.
/// The listing holds the lesser of the trashed count and the cap, each trashed document once,
/// every entry stamped, and the stamps non-increasing: newest first by the stamps the store
/// reports.
fn assert_trash_list(trashed: usize, expected_truncated: bool) {
    let (_directory, app) = application_in_temp_dir();
    let trashed_ids = trash_documents(&app, trashed);
    let listed = app.list_trash().unwrap();
    assert_eq!(
        listed.truncated, expected_truncated,
        "{trashed} trashed documents against a cap of {MAX_LISTED_DOCUMENTS}"
    );
    let stamps: Vec<Option<u64>> = listed
        .documents
        .iter()
        .map(|document| document.deleted_at_ms)
        .collect();
    assert!(
        stamps.iter().all(Option::is_some),
        "every listed trash entry carries a stamp"
    );
    assert!(
        stamps.windows(2).all(|pair| pair[0] >= pair[1]),
        "the listed trash is newest first by stamp"
    );
    let expected_listed = trashed.min(MAX_LISTED_DOCUMENTS);
    assert_eq!(
        listed.documents.len(),
        expected_listed,
        "{trashed} trashed documents"
    );
    let trashed_set: HashSet<&str> = trashed_ids.iter().map(String::as_str).collect();
    let listed_ids: HashSet<&str> = listed
        .documents
        .iter()
        .map(|document| document.id.as_str())
        .collect();
    assert_eq!(
        listed_ids.len(),
        expected_listed,
        "no trashed document is listed twice"
    );
    assert!(
        listed_ids.is_subset(&trashed_set),
        "every listed trash entry is one of the trashed documents"
    );
}

#[test]
fn the_list_cap_is_one_thousand_documents() {
    // The boundary tests follow the constant; this literal makes a changed cap a deliberate
    // decision.
    assert_eq!(
        MAX_LISTED_DOCUMENTS, 1_000,
        "the cap changed; review the boundary tests"
    );
}

#[test]
fn stored_documents_at_the_cap_are_all_listed() {
    assert_stored_list(MAX_LISTED_DOCUMENTS, false);
}

#[test]
fn stored_documents_one_past_the_cap_are_truncated() {
    assert_stored_list(MAX_LISTED_DOCUMENTS + 1, true);
}

#[test]
fn trashed_documents_at_the_cap_are_all_listed() {
    assert_trash_list(MAX_LISTED_DOCUMENTS, false);
}

/// The store stamps trashings with the wall clock, which can step backwards, so which documents
/// the cap keeps by stamp is checked deterministically in `tests/documents.rs`
/// (`the_most_recently_trashed_documents_are_listed_however_full_the_trash_is`, over the
/// `MemoryStore` logical clock). This file checks the cap through the real file store.
#[test]
fn trashed_documents_one_past_the_cap_are_truncated() {
    assert_trash_list(MAX_LISTED_DOCUMENTS + 1, true);
}

#[test]
fn a_few_stored_documents_are_not_truncated() {
    assert_stored_list(FEW_DOCUMENTS, false);
}

#[test]
fn a_few_trashed_documents_are_not_truncated() {
    assert_trash_list(FEW_DOCUMENTS, false);
}

#[test]
fn a_title_search_on_a_truncated_list_still_reports_truncated() {
    let (_directory, app) = application_in_temp_dir();
    store_documents(&app, MAX_LISTED_DOCUMENTS + 1);
    // Every stored title contains "sketch", so the search keeps all the documents that were read.
    for (query, expected_listed) in [("sketch", MAX_LISTED_DOCUMENTS), ("absent", 0)] {
        let search = ListQuery {
            query: Some(query.to_owned()),
            ..ListQuery::default()
        };
        let listed = app.list_documents(&search).unwrap();
        assert!(
            listed.truncated,
            "a search for {query:?} on a truncated list must still report truncated"
        );
        assert_eq!(
            listed.documents.len(),
            expected_listed,
            "a search for {query:?} lists the read documents whose titles match"
        );
    }
}
