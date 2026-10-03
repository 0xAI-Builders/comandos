//! Source-exact schema checking, independent of compiling the user's schema.
//! This API remains unwired while numeric, PythonRegex and traversal parity is pending.
use jsonschema::{Draft, Validator};
use serde_json::Value;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum SchemaCheckFailure {
    InvalidSchema,
    MetaschemaCompilation,
}

fn resources(draft: Draft) -> Option<(&'static str, &'static [&'static str])> {
    Some(match draft {
        Draft::Draft4 => (include_str!("metaschemas/draft4/metaschema.json"), &[]),
        Draft::Draft6 => (include_str!("metaschemas/draft6/metaschema.json"), &[]),
        Draft::Draft7 => (include_str!("metaschemas/draft7/metaschema.json"), &[]),
        Draft::Draft201909 => (
            include_str!("metaschemas/draft201909/metaschema.json"),
            &[
                include_str!("metaschemas/draft201909/vocabularies/core"),
                include_str!("metaschemas/draft201909/vocabularies/applicator"),
                include_str!("metaschemas/draft201909/vocabularies/validation"),
                include_str!("metaschemas/draft201909/vocabularies/meta-data"),
                include_str!("metaschemas/draft201909/vocabularies/format"),
                include_str!("metaschemas/draft201909/vocabularies/content"),
            ],
        ),
        Draft::Draft202012 => (
            include_str!("metaschemas/draft202012/metaschema.json"),
            &[
                include_str!("metaschemas/draft202012/vocabularies/core"),
                include_str!("metaschemas/draft202012/vocabularies/applicator"),
                include_str!("metaschemas/draft202012/vocabularies/unevaluated"),
                include_str!("metaschemas/draft202012/vocabularies/validation"),
                include_str!("metaschemas/draft202012/vocabularies/meta-data"),
                include_str!("metaschemas/draft202012/vocabularies/format-annotation"),
                include_str!("metaschemas/draft202012/vocabularies/content"),
            ],
        ),
        _ => return None,
    })
}

fn compile(draft: Draft) -> Result<Validator, SchemaCheckFailure> {
    let (root, dependencies) = resources(draft).ok_or(SchemaCheckFailure::MetaschemaCompilation)?;
    let parsed: Vec<Value> = std::iter::once(root)
        .chain(dependencies.iter().copied())
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()
        .map_err(|_| SchemaCheckFailure::MetaschemaCompilation)?;
    let mut registry = jsonschema::Registry::new();
    for resource in &parsed {
        let uri = resource
            .get("$id")
            .or_else(|| resource.get("id"))
            .and_then(Value::as_str)
            .ok_or(SchemaCheckFailure::MetaschemaCompilation)?;
        registry = registry
            .add(uri, resource)
            .map_err(|_| SchemaCheckFailure::MetaschemaCompilation)?;
    }
    let registry = registry
        .prepare()
        .map_err(|_| SchemaCheckFailure::MetaschemaCompilation)?;
    // Original metaschemas contain only regex, uri and uri-reference formats.
    // All five frozen class inventories lack URI checkers. Keep native regex
    // checking enabled; its PythonRegex differences remain in the exact audit.
    jsonschema::options()
        .offline()
        .with_draft(draft)
        .with_registry(&registry)
        .should_validate_formats(true)
        .with_format("uri", |_| true)
        .with_format("uri-reference", |_| true)
        .build(&parsed[0])
        .map_err(|_| SchemaCheckFailure::MetaschemaCompilation)
}

/// The caller passes the ORIGINAL schema and the approved selector's draft.
/// User references are strings in the instance, never registry resources.
/// No global cache or diagnostic rendering. Each worker checks one schema.
pub(super) fn check_schema(schema: &Value, draft: Draft) -> Result<(), SchemaCheckFailure> {
    compile(draft)?
        .validate(schema)
        .map_err(|_| SchemaCheckFailure::InvalidSchema)
}

#[cfg(test)]
mod tests {
    use jsonschema::Draft;
    use serde_json::{Value, json};

    fn check(schema: &Value, draft: Draft) -> bool {
        super::check_schema(schema, draft).is_ok()
    }

    #[test]
    fn draft4_duplicate_enum_is_rejected() {
        assert!(!check(&json!({"enum":[{},{}]}), Draft::Draft4));
    }
    #[test]
    fn draft4_empty_enum_is_rejected() {
        assert!(!check(&json!({"enum":[]}), Draft::Draft4));
    }
    #[test]
    fn draft7_write_only_is_unconstrained() {
        assert!(check(&json!({"writeOnly":123}), Draft::Draft7));
    }
    #[test]
    fn unused_unresolved_references_are_schema_data() {
        assert!(check(
            &json!({"properties":{"unused":{"$ref":"urn:missing"}}}),
            Draft::Draft202012
        ));
    }
    #[test]
    fn supported_drafts_check_original_schema_without_mutation() {
        for draft in [
            Draft::Draft4,
            Draft::Draft6,
            Draft::Draft7,
            Draft::Draft201909,
            Draft::Draft202012,
        ] {
            for schema in [
                json!({}),
                json!({"$schema":"unregistered schema uri with spaces", "$id":"uri with spaces", "id":"uri with spaces"}),
                json!({"properties":{"unused":{"$ref":"#/$defs/missing"}}}),
                json!({"properties":{"unused":{"$ref":"https://example.invalid/missing"}}}),
                json!({"const":{"type":123,"$ref":0},"enum":[{"type":123,"$schema":0}],"default":{"type":123},"examples":[{"type":123}]}),
                json!({"properties":{"n":{"format":"email"}}}),
            ] {
                let before = serde_json::to_vec(&schema).unwrap();
                assert_eq!(
                    super::check_schema(&schema, draft),
                    Ok(()),
                    "{draft:?}: {schema}"
                );
                assert_eq!(serde_json::to_vec(&schema).unwrap(), before);
            }
            for schema in [
                json!({"type":"unknown"}),
                json!({"maxLength":-1}),
                json!({"required":["x","x"]}),
                json!({"properties":{"unused":{"type":123}}}),
                json!({"properties":{"unused":{"pattern":"["}}}),
            ] {
                assert_eq!(
                    super::check_schema(&schema, draft),
                    Err(super::SchemaCheckFailure::InvalidSchema),
                    "{draft:?}: {schema}"
                );
            }
        }
    }

    #[test]
    fn selected_metaschema_keeps_cross_draft_declarations_as_data() {
        let schema = json!({"properties":{"unused":{"$schema":"http://json-schema.org/draft-04/schema#","exclusiveMinimum":3}}});
        assert!(!check(&schema, Draft::Draft4));
        for draft in [
            Draft::Draft6,
            Draft::Draft7,
            Draft::Draft201909,
            Draft::Draft202012,
        ] {
            assert!(check(&schema, draft));
        }
    }

    #[test]
    fn regex_property_names_follow_original_metaschema_constraints() {
        let schema = json!({"patternProperties":{"[":{}}});
        assert!(check(&schema, Draft::Draft4));
        for draft in [
            Draft::Draft6,
            Draft::Draft7,
            Draft::Draft201909,
            Draft::Draft202012,
        ] {
            assert!(!check(&schema, draft));
        }
    }

    #[test]
    fn raw_numeric_schema_values_are_preserved() {
        let raw = r#"{"minLength":1e400,"multipleOf":1e-400,"enum":[18446744073709551616,1.0]}"#;
        let schema: Value = serde_json::from_str(raw).unwrap();
        let before = serde_json::to_string(&schema).unwrap();
        let _ = super::check_schema(&schema, Draft::Draft202012);
        assert_eq!(serde_json::to_string(&schema).unwrap(), before);
        // serde_json inserts the positive exponent sign at parse time.
        // Preflight preserves that original Value representation unchanged.
        assert!(before.contains("1e+400"));
        assert!(before.contains("1e-400"));
        assert!(before.contains("18446744073709551616"));
        assert!(before.contains("1.0"));
    }

    #[test]
    fn bundled_schema_and_notice_hashes_match_the_provenance() {
        use sha2::{Digest, Sha256};
        let directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/output_schema/metaschemas");
        let manifest: Value =
            serde_json::from_str(include_str!("metaschemas/PROVENANCE.json")).unwrap();
        assert_eq!(manifest["version"], "2025.9.1");
        assert_eq!(manifest["files"].as_array().unwrap().len(), 20);
        for row in manifest["files"]
            .as_array()
            .unwrap()
            .iter()
            .chain(std::iter::once(&manifest["notice"]))
        {
            let source = row["source"].as_str().unwrap();
            let relative = if source.ends_with("/COPYING") {
                "COPYING"
            } else {
                source.split("/schemas/").nth(1).unwrap()
            };
            let bytes = std::fs::read(directory.join(relative)).unwrap();
            assert_eq!(
                format!("{:x}", Sha256::digest(&bytes)),
                row["sha256"].as_str().unwrap(),
                "{relative}"
            );
        }
    }

    #[test]
    fn schema_references_never_read_private_files_or_contact_network() {
        use std::{
            fs,
            net::{TcpListener, TcpStream},
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let marker = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let _ = listener.accept().unwrap();
        drop(marker);
        let secret = std::env::temp_dir().join(format!(
            "schema-preflight-private-{}.json",
            std::process::id()
        ));
        fs::write(&secret, r#"{"type":123}"#).unwrap();
        fs::File::open(&secret)
            .unwrap()
            .set_times(fs::FileTimes::new().set_accessed(std::time::UNIX_EPOCH))
            .unwrap();
        for reference in [
            format!("http://{}/schema", listener.local_addr().unwrap()),
            format!("file://{}", secret.display()),
        ] {
            assert!(check(&json!({"$ref":reference}), Draft::Draft202012));
        }
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        assert_eq!(
            fs::metadata(&secret).unwrap().accessed().unwrap(),
            std::time::UNIX_EPOCH
        );
        fs::remove_file(secret).unwrap();
    }

    #[test]
    #[ignore = "explicit exact Python differential audit, never ordinary discovery"]
    fn audit_probe() {
        use std::io::Read;
        super::super::resource_budget().unwrap();
        let mut input = String::new();
        std::io::stdin()
            .take(8 * 1024 * 1024)
            .read_to_string(&mut input)
            .unwrap();
        let rows: Vec<Value> = serde_json::from_str(&input).unwrap();
        let mut results = Vec::new();
        // At most five selected-draft validators in this short-lived test
        // process. Production checks one request and retains no cache.
        let mut validators: Vec<(Draft, jsonschema::Validator)> = Vec::new();
        for row in rows {
            let schema: Value = serde_json::from_str(row["schema_json"].as_str().unwrap()).unwrap();
            let status = match super::super::dialect::validation_schema(&schema) {
                Ok((_, draft)) => {
                    let index = if let Some(index) = validators
                        .iter()
                        .position(|(selected, _)| *selected == draft)
                    {
                        index
                    } else {
                        validators.push((
                            draft,
                            super::compile(draft)
                                .expect("trusted selected metaschema must compile"),
                        ));
                        validators.len() - 1
                    };
                    if validators[index].1.validate(&schema).is_ok() {
                        "valid"
                    } else {
                        "rejected"
                    }
                }
                Err(_) => "rejected",
            };
            results.push(json!({"name":row["name"],"native":status}));
        }
        println!(
            "SCHEMA_PREFLIGHT={}",
            serde_json::to_string(&results).unwrap()
        );
    }
}
