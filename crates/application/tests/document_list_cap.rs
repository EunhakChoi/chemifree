//! The list and trash caps (`MAX_LISTED_DOCUMENTS`) through the public `Application` API, over
//! the file store in a temporary data directory. The boundary cases are the cap itself and one
//! document past it; the few-document cases stay well below it.

use std::collections::HashSet;
use std::thread;
use std::time::Duration;

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

/// Creates `count` documents and moves each to the trash. Returns the identifiers in trashing
/// order, oldest first.
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
            // The store stamps a trashing in whole milliseconds. Without this pause, two moves
            // can share a stamp, and the cap would then keep by identifier instead of by time.
            thread::sleep(Duration::from_millis(1));
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

/// Trashes `trashed` documents and checks the trash listing. The flag is `expected_truncated`;
/// the listing holds the most recently trashed documents up to the cap, newest first.
fn assert_trash_list(trashed: usize, expected_truncated: bool) {
    let (_directory, app) = application_in_temp_dir();
    let trashing_order = trash_documents(&app, trashed);
    let listed = app.list_trash().unwrap();
    assert_eq!(
        listed.truncated, expected_truncated,
        "{trashed} trashed documents against a cap of {MAX_LISTED_DOCUMENTS}"
    );
    assert!(
        listed
            .documents
            .windows(2)
            .all(|pair| pair[0].deleted_at_ms > pair[1].deleted_at_ms),
        "the listed trash stamps must be distinct and newest first"
    );
    let expected: Vec<&str> = trashing_order
        .iter()
        .rev()
        .take(MAX_LISTED_DOCUMENTS)
        .map(String::as_str)
        .collect();
    let listed_ids: Vec<&str> = listed
        .documents
        .iter()
        .map(|document| document.id.as_str())
        .collect();
    assert_eq!(
        listed_ids, expected,
        "the most recently trashed documents, newest first"
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

#[test]
fn trashed_documents_one_past_the_cap_keep_the_most_recent() {
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
