//! The two independently optional format providers share one operation contract.
use chemifree_application::Application;
use chemifree_providers::{
    FileDocumentRepository, LocalCapabilities, V2000Codec, interchange_codec::LocalInterchangeCodec,
};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;
const MOL: &str = "Carbon\n\n\n  1  0  0  0  0  0  0  0  0  0999 V2000\n    0.0000    0.0000    0.0000 C   0  0  0  0  0  0  0  0  0  0  0  0\nM  END\n";
const XML: &str = "<CDXML><page id=\"1\"><fragment id=\"2\"><n id=\"3\" p=\"1 2\" Element=\"8\"/></fragment></page></CDXML>";

fn call(app: &App, id: &str, operation: &str, parameters: Value) -> Value {
    let request = json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters});
    serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
}

#[test]
fn catalog_and_dispatch_follow_the_providers_actually_connected() {
    for (ct, rich, label) in [
        (false, false, "Import and export"),
        (true, false, "MOL/SDF subset"),
        (false, true, "CDXML subset"),
        (true, true, "MOL/SDF and CDXML subsets"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let mut app = Application::new(LocalCapabilities, FileDocumentRepository::new(dir.path()));
        if ct {
            app = app.with_ctfile_codec(V2000Codec);
        }
        if rich {
            app = app.with_interchange_codec(LocalInterchangeCodec);
        }
        let caps = serde_json::to_value(app.capabilities()).unwrap();
        let capability = caps["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == "format_conversion")
            .unwrap();
        assert_eq!(capability["label"], label);
        assert_eq!(
            capability["status"],
            if ct || rich {
                "available"
            } else {
                "not_implemented"
            }
        );
        let created = call(&app, "create", "document.create", json!({"title":"Target"}));
        let id = created["result"]["document"]["id"].as_str().unwrap();
        let before = call(
            &app,
            "before",
            "document.snapshot",
            json!({"document_id":id}),
        );
        let mol = call(
            &app,
            "mol-preview",
            "document.import",
            json!({"format":"mol","document_id":id,"expected_revision":0,"source":MOL,"record":1,"preview":true}),
        );
        assert_eq!(mol["outcome"] == "partial", ct, "{mol}");
        if rich && !ct {
            assert_eq!(mol["diagnostics"][0]["code"], "unsupported_format");
        }
        let xml = call(
            &app,
            "xml-read",
            "document.import",
            json!({"mode":"read","format":"cdxml","source":XML}),
        );
        assert_eq!(xml["outcome"] == "ok", rich, "{xml}");
        if ct && !rich {
            assert_eq!(xml["diagnostics"][0]["code"], "unsupported_format");
            let missing = call(
                &app,
                "xml-unavailable",
                "document.export",
                json!({"format":"cdxml","document":{}}),
            );
            assert_eq!(missing["diagnostics"][0]["code"], "unsupported_format");
        }
        if rich && !ct {
            let missing = call(
                &app,
                "mol-unavailable",
                "document.export",
                json!({"format":"mol","records":[{"document_id":id}]}),
            );
            assert_eq!(missing["diagnostics"][0]["code"], "unsupported_format");
        }
        if rich {
            let exported = call(
                &app,
                "xml-write",
                "document.export",
                json!({"format":"cdxml","document":xml["result"]["document"],"original":XML}),
            );
            assert_eq!(exported["result"]["content"], XML);
        }
        if ct {
            let exported = call(
                &app,
                "mol-write",
                "document.export",
                json!({"format":"mol","records":[{"document_id":id}]}),
            );
            assert!(
                exported["result"]["content"]
                    .as_str()
                    .unwrap()
                    .contains("V2000")
            );
        }
        let unknown = call(
            &app,
            "unknown",
            "document.import",
            json!({"format":"smiles","source":"C"}),
        );
        assert!(unknown["result"].is_null());
        let after = call(
            &app,
            "after",
            "document.snapshot",
            json!({"document_id":id}),
        );
        assert_eq!(before["result"], after["result"]);
    }
}
