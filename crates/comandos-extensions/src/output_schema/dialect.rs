//! Installed Python root dialect lookup and private compilation clone.
use serde_json::Value;
use std::borrow::Cow;

#[derive(Debug, PartialEq, Eq)]
enum RootDialect {
    Registered(jsonschema::Draft, &'static str),
    Draft3,
    Latest,
}

// Port only urllib.parse.urlsplit(uri).geturl(), used by Python's URIDict.
// A general URL parser changes host case, escapes and paths during lookup.
fn python_uri_key(uri: &str) -> Result<String, ()> {
    let cleaned: String = uri
        .trim_start_matches(|c: char| c <= '\u{20}')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\r' | '\n'))
        .collect();
    let mut rest = cleaned.as_str();
    let mut scheme = String::new();
    if let Some((candidate, tail)) = rest.split_once(':')
        && candidate
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic)
        && candidate
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"+-.".contains(&c))
    {
        scheme = candidate.to_ascii_lowercase();
        rest = tail;
    }
    let mut netloc = "";
    if let Some(tail) = rest.strip_prefix("//") {
        let end = tail.find(['/', '?', '#']).unwrap_or(tail.len());
        netloc = &tail[..end];
        rest = &tail[end..];
        check_python_netloc(netloc)?;
    }
    let (rest, fragment) = rest.split_once('#').unwrap_or((rest, ""));
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    // This list comes from CPython 3.11.15's uses_netloc, not RFC rules.
    const USES_NETLOC: &[&str] = &[
        "", "ftp", "http", "gopher", "nntp", "telnet", "imap", "wais", "file", "mms", "https",
        "shttp", "snews", "prospero", "rtsp", "rtsps", "rtspu", "rsync", "svn", "svn+ssh", "sftp",
        "nfs", "git", "git+ssh", "ws", "wss",
    ];
    let mut key = String::new();
    if !scheme.is_empty() {
        key.push_str(&scheme);
        key.push(':');
    }
    if !netloc.is_empty()
        || (!scheme.is_empty() && USES_NETLOC.contains(&scheme.as_str()))
        || path.starts_with("//")
    {
        key.push_str("//");
        key.push_str(netloc);
        if !path.is_empty() && !path.starts_with('/') {
            key.push('/');
        }
    }
    key.push_str(path);
    if !query.is_empty() {
        key.push('?');
        key.push_str(query);
    }
    if !fragment.is_empty() {
        key.push('#');
        key.push_str(fragment);
    }
    Ok(key)
}

fn check_python_netloc(netloc: &str) -> Result<(), ()> {
    if netloc.contains('[') != netloc.contains(']') {
        return Err(());
    }
    if netloc.contains('[') {
        let host_port = netloc.rsplit('@').next().ok_or(())?;
        let host = if let Some((before, tail)) = host_port.split_once('[') {
            let (host, port) = tail.split_once(']').ok_or(())?;
            if !before.is_empty() || (!port.is_empty() && !port.starts_with(':')) {
                return Err(());
            }
            host
        } else {
            host_port.split(':').next().ok_or(())?
        };
        if let Some(future) = host.strip_prefix('v') {
            let (version, address) = future.split_once('.').ok_or(())?;
            if version.is_empty()
                || !version.bytes().all(|c| c.is_ascii_hexdigit())
                || address.is_empty()
            {
                return Err(());
            }
        } else {
            let address = if let Some((address, scope)) = host.split_once('%') {
                if scope.is_empty() || scope.contains('%') {
                    return Err(());
                }
                address
            } else {
                host
            };
            address.parse::<std::net::Ipv6Addr>().map_err(|_| ())?;
        }
    }
    // Exhaustively captured from the installed Python UCD 14.0.0: these
    // non-ASCII scalars introduce / ? # @ : under NFKC. Normalization cannot
    // compose a new ASCII delimiter across scalars, so no Unicode crate or
    // host/path rewriting is needed for urllib's delimiter rejection.
    if netloc.chars().any(|c| {
        matches!(
            c,
            '\u{2047}'
                | '\u{2048}'
                | '\u{2049}'
                | '\u{2100}'
                | '\u{2101}'
                | '\u{2105}'
                | '\u{2106}'
                | '\u{2a74}'
                | '\u{fe13}'
                | '\u{fe16}'
                | '\u{fe55}'
                | '\u{fe56}'
                | '\u{fe5f}'
                | '\u{fe6b}'
                | '\u{ff03}'
                | '\u{ff0f}'
                | '\u{ff1a}'
                | '\u{ff1f}'
                | '\u{ff20}'
        )
    }) {
        return Err(());
    }
    Ok(())
}

fn select_python_root_dialect(schema: &Value) -> Result<RootDialect, ()> {
    use jsonschema::Draft;
    let Some(uri) = schema.get("$schema") else {
        return Ok(RootDialect::Latest);
    };
    let key = python_uri_key(uri.as_str().ok_or(())?)?;
    Ok(match key.as_str() {
        "http://json-schema.org/draft-03/schema" => RootDialect::Draft3,
        "http://json-schema.org/draft-04/schema" => {
            RootDialect::Registered(Draft::Draft4, "http://json-schema.org/draft-04/schema#")
        }
        "http://json-schema.org/draft-06/schema" => {
            RootDialect::Registered(Draft::Draft6, "http://json-schema.org/draft-06/schema#")
        }
        "http://json-schema.org/draft-07/schema" => {
            RootDialect::Registered(Draft::Draft7, "http://json-schema.org/draft-07/schema#")
        }
        "https://json-schema.org/draft/2019-09/schema" => RootDialect::Registered(
            Draft::Draft201909,
            "https://json-schema.org/draft/2019-09/schema",
        ),
        "https://json-schema.org/draft/2020-12/schema" => RootDialect::Registered(
            Draft::Draft202012,
            "https://json-schema.org/draft/2020-12/schema",
        ),
        _ => RootDialect::Latest,
    })
}

pub(super) fn validation_schema(schema: &Value) -> Result<(Cow<'_, Value>, jsonschema::Draft), ()> {
    let selected = select_python_root_dialect(schema)?;
    let (draft, canonical) = match selected {
        // Draft3 remains a rejecting, separately audited compatibility gap.
        RootDialect::Draft3 => return Err(()),
        RootDialect::Registered(draft, uri) => (draft, Some(uri)),
        RootDialect::Latest => (jsonschema::Draft::Draft202012, None),
    };
    let mut validation = Cow::Borrowed(schema);
    if schema.get("$schema").is_some() {
        let object = validation.to_mut().as_object_mut().ok_or(())?;
        if let Some(uri) = canonical {
            object.insert("$schema".into(), Value::String(uri.into()));
        } else {
            object.shift_remove("$schema");
        }
    }
    Ok((validation, draft))
}

#[cfg(test)]
mod dialect_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn root_lookup_and_uri_keys_match_installed_python_capture() {
        let rows: Value =
            serde_json::from_str(include_str!("../../tests/output_schema_dialect_cases.json"))
                .unwrap();
        for row in rows.as_array().unwrap() {
            let schema: Value = serde_json::from_str(row["schema_json"].as_str().unwrap()).unwrap();
            let Some(uri) = schema.get("$schema").and_then(Value::as_str) else {
                continue;
            };
            let expected = row["selected"].as_str().unwrap();
            let actual = select_python_root_dialect(&schema);
            if expected.starts_with("error:") {
                assert!(actual.is_err(), "{}", row["name"]);
                assert!(python_uri_key(uri).is_err());
                continue;
            }
            assert_eq!(
                python_uri_key(uri).unwrap(),
                row["normalized"].as_str().unwrap(),
                "{}",
                row["name"]
            );
            let actual = match actual.unwrap() {
                RootDialect::Draft3 => "Draft3Validator",
                RootDialect::Registered(jsonschema::Draft::Draft4, _) => "Draft4Validator",
                RootDialect::Registered(jsonschema::Draft::Draft6, _) => "Draft6Validator",
                RootDialect::Registered(jsonschema::Draft::Draft7, _) => "Draft7Validator",
                RootDialect::Registered(jsonschema::Draft::Draft201909, _) => {
                    "Draft201909Validator"
                }
                RootDialect::Registered(jsonschema::Draft::Draft202012, _)
                | RootDialect::Latest => "Draft202012Validator",
                RootDialect::Registered(_, _) => panic!("unregistered future draft"),
            };
            assert_eq!(actual, expected, "{}", row["name"]);
        }
        assert_eq!(
            select_python_root_dialect(&json!({})),
            Ok(RootDialect::Latest)
        );
        for root in [
            Value::Null,
            json!(1),
            json!(true),
            json!(false),
            json!([]),
            json!({}),
        ] {
            assert!(validation_schema(&json!({"$schema":root})).is_err());
        }
        assert!(
            validation_schema(&json!({"$schema":"http://json-schema.org/draft-03/schema#"}))
                .is_err()
        );
    }

    #[test]
    fn compilation_clone_changes_only_the_root_declaration() {
        for (root, canonical, draft) in [
            (
                "https://example.invalid/root",
                None,
                jsonschema::Draft::Draft202012,
            ),
            (
                " HTTP://json-schema.org/draft-07/schema?#",
                Some("http://json-schema.org/draft-07/schema#"),
                jsonschema::Draft::Draft7,
            ),
        ] {
            let schema = json!({"$schema":root,"$id":"urn:original", "id":"urn:legacy", "$ref":"#/definitions/item",
                "definitions":{"item":{"$schema":"http://json-schema.org/draft-04/schema#","type":"integer"}},
                "properties":{"nested":{"$schema":"https://example.invalid/nested","$id":"urn:nested"}},
                "const":{"$schema":"literal-const"}, "enum":[{"$schema":"literal-enum"}],
                "default":{"$schema":"literal-default"}, "examples":[{"$schema":"literal-example"}]});
            let bytes = serde_json::to_vec(&schema).unwrap();
            let (copy, actual_draft) = validation_schema(&schema).unwrap();
            assert!(matches!(copy, Cow::Owned(_)));
            assert_eq!(actual_draft, draft);
            let mut expected = schema.clone();
            if let Some(uri) = canonical {
                expected["$schema"] = json!(uri);
            } else {
                expected.as_object_mut().unwrap().shift_remove("$schema");
            }
            assert_eq!(copy.as_ref(), &expected);
            assert_eq!(
                serde_json::to_vec(copy.as_ref()).unwrap(),
                serde_json::to_vec(&expected).unwrap()
            );
            assert_eq!(serde_json::to_vec(&schema).unwrap(), bytes);
        }
        assert!(matches!(
            validation_schema(&json!({})).unwrap().0,
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn removing_unknown_root_keeps_other_keyword_insertion_order() {
        let schema: Value = serde_json::from_str(
            r#"{"$schema":"unknown","required":["n"],"properties":{"n":{"$ref":"urn:missing"}},"default":{"$schema":"literal"}}"#,
        ).unwrap();
        let original = serde_json::to_vec(&schema).unwrap();
        let (copy, _) = validation_schema(&schema).unwrap();
        let expected = r#"{"required":["n"],"properties":{"n":{"$ref":"urn:missing"}},"default":{"$schema":"literal"}}"#;
        assert_eq!(serde_json::to_string(copy.as_ref()).unwrap(), expected);
        assert_eq!(serde_json::to_vec(&schema).unwrap(), original);
    }
}
