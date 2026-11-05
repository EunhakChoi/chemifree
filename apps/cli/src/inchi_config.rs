//! Explicit optional InChI registration for a local host. Keep both entry adapters aligned;
//! the provider owns release, digest, library and ABI validation.

use std::path::PathBuf;

use chemifree_application::inchi::{InchiError, InchiErrorKind};
use chemifree_providers::{InchiRegistration, RegisteredInchiProvider};

use crate::LocalApplication;

/// Opt-in fields have no library, version or digest defaults and trigger no discovery.
#[derive(clap::Args, Debug, Default)]
pub(super) struct InchiArgs {
    /// Explicit absolute path to a trusted official InChI shared library. Never searched or downloaded.
    #[arg(long, global = true, env = "CHEMIFREE_INCHI_LIBRARY")]
    inchi_library: Option<PathBuf>,
    /// Explicitly declared library release; only 1.07.5 is supported. Not detected at runtime.
    #[arg(long, global = true, env = "CHEMIFREE_INCHI_VERSION")]
    inchi_version: Option<String>,
    /// Expected SHA-256 of the library, as 64 lowercase hexadecimal characters.
    #[arg(long, global = true, env = "CHEMIFREE_INCHI_SHA256")]
    inchi_sha256: Option<String>,
    /// Native cooperative timeout in seconds, 1 to 60 (default 5). Cannot interrupt library loading or a hung native call.
    #[arg(long, global = true, env = "CHEMIFREE_INCHI_TIMEOUT_SECONDS")]
    inchi_timeout_seconds: Option<String>,
}

fn invalid(message: &str) -> InchiError {
    InchiError {
        kind: InchiErrorKind::Unavailable,
        code: "inchi_registration_invalid",
        message: message.into(),
    }
}

impl InchiArgs {
    fn registration(self) -> Result<Option<InchiRegistration>, InchiError> {
        if self.inchi_library.is_none()
            && self.inchi_version.is_none()
            && self.inchi_sha256.is_none()
            && self.inchi_timeout_seconds.is_none()
        {
            return Ok(None);
        }
        let (Some(library_path), Some(version), Some(sha256)) =
            (self.inchi_library, self.inchi_version, self.inchi_sha256)
        else {
            return Err(invalid(
                "Optional InChI registration requires --inchi-library, --inchi-version (1.07.5), and --inchi-sha256 together.",
            ));
        };
        let timeout_seconds: u16 = self
            .inchi_timeout_seconds
            .as_deref()
            .unwrap_or("5")
            .parse()
            .ok()
            .filter(|seconds| (1..=60).contains(seconds))
            .ok_or_else(|| invalid("InChI timeout must be an integer from 1 to 60 seconds."))?;
        Ok(Some(InchiRegistration {
            library_path,
            version,
            sha256,
            timeout_seconds,
        }))
    }

    /// A rejected registration disables only this optional provider, never the host.
    pub(super) fn configure(self, application: LocalApplication) -> LocalApplication {
        let loaded = self
            .registration()
            .and_then(|registration| registration.map(RegisteredInchiProvider::load).transpose());
        match loaded {
            Ok(Some(provider)) => application.with_inchi_provider(provider),
            Ok(None) => application,
            Err(error) => {
                eprintln!(
                    "Optional InChI provider unavailable ({}): {}",
                    error.code, error.message
                );
                application.with_inchi_provider_error(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
    use serde_json::{Value, json};

    fn selected() -> InchiArgs {
        InchiArgs {
            inchi_library: Some(std::env::temp_dir().join("inchi-test-never-discovered.so")),
            inchi_version: Some("1.07.5".into()),
            inchi_sha256: Some("0".repeat(64)),
            inchi_timeout_seconds: None,
        }
    }

    #[test]
    fn no_selection_never_fabricates_a_registration() {
        assert_eq!(InchiArgs::default().registration(), Ok(None));
    }

    #[test]
    fn library_version_and_digest_are_required_together() {
        for args in [
            InchiArgs {
                inchi_version: None,
                ..selected()
            },
            InchiArgs {
                inchi_sha256: None,
                ..selected()
            },
            InchiArgs {
                inchi_library: None,
                ..selected()
            },
        ] {
            let error = args.registration().unwrap_err();
            assert_eq!(error.code, "inchi_registration_invalid");
            assert!(error.message.contains("together"), "{error}");
        }
    }

    #[test]
    fn timeout_is_bounded_without_rejecting_host_arguments() {
        for timeout in ["0", "61", "65536", "-1", "NaN", ""] {
            let args = InchiArgs {
                inchi_timeout_seconds: Some(timeout.into()),
                ..selected()
            };
            assert_eq!(
                args.registration().unwrap_err().code,
                "inchi_registration_invalid",
                "{timeout}"
            );
        }
        for (input, expected) in [(None, 5), (Some("1"), 1), (Some("60"), 60)] {
            let args = InchiArgs {
                inchi_timeout_seconds: input.map(str::to_owned),
                ..selected()
            };
            let registration = args.registration().unwrap().unwrap();
            assert_eq!(registration.timeout_seconds, expected);
            assert_eq!(registration.version, "1.07.5");
            assert_eq!(registration.sha256, "0".repeat(64));
        }
    }

    #[test]
    fn bad_registration_reports_its_reason_and_keeps_document_creation_available() {
        let directory = tempfile::tempdir().unwrap();
        let application = LocalApplication::new(
            LocalCapabilities,
            FileDocumentRepository::new(directory.path()),
        );
        let args = InchiArgs {
            inchi_timeout_seconds: Some("0".into()),
            ..selected()
        };
        let application = args.configure(application);
        let request = json!({"contract":1,"request_id":"unavailable","profile":"preserve-semantics",
            "operation":"chem.inchi","parameters":{"document_id":"not-created"}});
        let reply: Value =
            serde_json::to_value(application.execute(request.to_string().as_bytes())).unwrap();
        assert_eq!(reply["outcome"], "unsupported");
        assert_eq!(reply["diagnostics"][0]["code"], "inchi_unavailable");
        assert_eq!(
            reply["diagnostics"][0]["details"]["reason_code"],
            "inchi_registration_invalid"
        );
        assert_eq!(
            reply["diagnostics"][0]["details"]["reason"],
            "InChI timeout must be an integer from 1 to 60 seconds."
        );
        assert!(reply["result"].is_null());
        let created = application
            .create_document(None, Some("Independent drawing"))
            .unwrap();
        assert_eq!(created.document.title, "Independent drawing");
        assert_eq!(created.document.revision, 0);
    }
}
