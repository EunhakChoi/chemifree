//! Commits by this store are recorded in its stored-revision cache when they are made, so the
//! next clean-copy check of an own commit is a stat and does not read the file. A commit by
//! another writer is not recorded here, so it is read. The stamp is the length, the modification
//! time and the inode: a commit that matches in length and time is seen through its inode alone.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use chemifree_application::{DocumentRepository, StorageError};
use chemifree_domain::document::{Command, Document};
use chemifree_providers::FileDocumentRepository;

/// A document at revision 1: created, then given one edit that renames it.
fn document_at_revision_one(repository: &FileDocumentRepository) -> Document {
    let mut document = Document::new(repository.allocate_id().unwrap(), "Stored").unwrap();
    document
        .apply(&[Command::Rename {
            title: "Edited".into(),
        }])
        .unwrap();
    assert_eq!(document.revision(), 1, "one edit makes revision 1");
    document
}

/// The one file that a save committed under the repository's documents directory.
fn committed_file(repository: &FileDocumentRepository) -> PathBuf {
    let mut files = fs::read_dir(repository.directory())
        .unwrap()
        .map(|entry| entry.unwrap().path());
    let file = files.next().expect("a save commits a document file");
    assert!(
        files.next().is_none(),
        "the documents directory holds one file"
    );
    file
}

#[cfg(unix)]
fn inode(metadata: &fs::Metadata) -> u64 {
    std::os::unix::fs::MetadataExt::ino(metadata)
}

#[cfg(not(unix))]
fn inode(_metadata: &fs::Metadata) -> u64 {
    0
}

/// Replaces the committed bytes with the same number of bytes that are not a document, then
/// restores the modification time. The length, the modification time and the inode, which is
/// the stamp `stored_revision` compares, are all unchanged.
fn corrupt_keeping_stamp(file: &Path) {
    let before = fs::metadata(file).unwrap();
    let modified = before.modified().unwrap();
    let garbage = vec![b'x'; usize::try_from(before.len()).unwrap()];
    let mut handle = OpenOptions::new().write(true).open(file).unwrap();
    handle.write_all(&garbage).unwrap();
    handle.set_modified(modified).unwrap();
    drop(handle);
    let after = fs::metadata(file).unwrap();
    assert!(
        fs::read(file).unwrap() == garbage,
        "the committed bytes are not replaced"
    );
    assert_eq!(after.len(), before.len(), "the length is unchanged");
    assert_eq!(
        after.modified().unwrap(),
        modified,
        "the modification time is restored"
    );
    assert_eq!(inode(&after), inode(&before), "the file keeps its inode");
}

#[test]
fn own_commit_is_answered_from_its_recorded_stamp() {
    let directory = tempfile::tempdir().unwrap();
    let repository = FileDocumentRepository::new(directory.path());
    let document = document_at_revision_one(&repository);
    let id = document.id().clone();
    repository.save(&document, None).unwrap();
    corrupt_keeping_stamp(&committed_file(&repository));
    assert_eq!(
        repository.stored_revision(&id),
        Ok(Some(1)),
        "the commit recorded its stamp, so the check does not read the corrupted file"
    );
}

#[test]
fn a_repository_without_the_commit_reads_the_corrupted_file() {
    let directory = tempfile::tempdir().unwrap();
    let saver = FileDocumentRepository::new(directory.path());
    let document = document_at_revision_one(&saver);
    let id = document.id().clone();
    saver.save(&document, None).unwrap();
    corrupt_keeping_stamp(&committed_file(&saver));
    // The same root and the same corrupted bytes, but this instance never saved or cached.
    let reader = FileDocumentRepository::new(directory.path());
    let answer = reader.stored_revision(&id);
    assert!(
        matches!(answer, Err(StorageError::Corrupt(_))),
        "without a recorded commit the corrupted file is read and refused, got {answer:?}"
    );
}

#[cfg(unix)]
#[test]
fn another_writers_commit_is_read_even_when_length_and_time_match() {
    let directory = tempfile::tempdir().unwrap();
    let writer_a = FileDocumentRepository::new(directory.path());
    let writer_b = FileDocumentRepository::new(directory.path());
    let first = document_at_revision_one(&writer_a);
    let id = first.id().clone();
    writer_a.save(&first, None).unwrap();
    let file = committed_file(&writer_a);
    let seen_by_a = fs::metadata(&file).unwrap();

    // Writer B loads revision 1 and undoes the rename: revision 2, with the same encoded length.
    let mut second = writer_b
        .load(&id)
        .unwrap()
        .expect("writer B loads the document that A saved");
    second.undo().unwrap();
    assert_eq!(second.revision(), 2, "one undo makes revision 2");
    writer_b.save(&second, Some(1)).unwrap();

    // Give B's commit A's modification time, as a commit within one clock tick would have. The
    // rename gave the file a new inode, which is then the only difference that A can see.
    let committed_by_b = fs::metadata(&file).unwrap();
    assert_eq!(
        committed_by_b.len(),
        seen_by_a.len(),
        "the two revisions encode to the same length"
    );
    assert_ne!(
        inode(&committed_by_b),
        inode(&seen_by_a),
        "a rename commits a new inode"
    );
    let modified = seen_by_a.modified().unwrap();
    OpenOptions::new()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    assert_eq!(
        fs::metadata(&file).unwrap().modified().unwrap(),
        modified,
        "the modification time is set to A's"
    );

    assert_eq!(
        writer_a.stored_revision(&id),
        Ok(Some(2)),
        "A's recorded stamp has the earlier inode, so the file that B committed is read"
    );
}
