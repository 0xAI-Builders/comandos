//! Unwired evaluator for exact-check_schema-proven original documents.
//! Installed Python traversal ports are covered by traversal/PROVENANCE.json.
use super::dialect::{SelectedDialect, select_with_default};
use jsonschema::Draft;
use serde_json::{Value, json};
use std::collections::HashSet;
mod local_refs;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Mode {
    FirstError,
    ExhaustErrors,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Validity {
    Valid,
    Invalid,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AbortClass {
    Unresolvable,
    RecursionError,
    TypeError,
    ValueError,
    AttributeError,
    OverflowError,
    ZeroDivisionError,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GapKind {
    Draft3,
    DialectFailure,
    Keyword,
    Numeric,
    PythonRegex,
    ResourceId,
    ResourceDialect,
    ExternalResource,
    UriJoin,
    DynamicScope,
    Annotation,
    PointerSemantics,
    DuplicateAnchor,
    FragmentShape,
    BudgetBoundary,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum EvalFailure {
    Abort(AbortClass),
    ScopeGap {
        kind: GapKind,
        schema_pointer: String,
    },
    InternalFragmentCompilation,
}
type Evaluation = Result<Validity, EvalFailure>;
fn gap(kind: GapKind, pointer: &str) -> EvalFailure {
    EvalFailure::ScopeGap {
        kind,
        schema_pointer: pointer.into(),
    }
}
fn child_pointer(parent: &str, key: &str) -> String {
    format!("{parent}/{}", key.replace('~', "~0").replace('/', "~1"))
}
fn old(draft: Draft) -> bool {
    matches!(draft, Draft::Draft4 | Draft::Draft6 | Draft::Draft7)
}
fn evolved(schema: &Value, fallback: Draft, pointer: &str) -> Result<Draft, EvalFailure> {
    match select_with_default(schema, fallback)
        .map_err(|_| gap(GapKind::DialectFailure, pointer))?
    {
        SelectedDialect::Supported(draft) => Ok(draft),
        SelectedDialect::Draft3 => Err(gap(GapKind::Draft3, pointer)),
    }
}

/// Exact selected Python check_schema success is a precondition, not preflight.
pub(super) fn evaluate_checked(
    schema: &Value,
    instance: &Value,
    draft: Draft,
    mode: Mode,
) -> Evaluation {
    let refs = local_refs::LocalRefs::new(schema, draft)?;
    let mut evaluator = Evaluator {
        refs,
        active: HashSet::new(),
        fragments: Vec::new(),
        operations: 0,
    };
    evaluator.eval(
        schema,
        instance,
        State {
            class: draft,
            filter: draft,
            resolver: 0,
            mode,
        },
        "",
    )
}
#[derive(Clone, Copy)]
struct State {
    class: Draft,
    filter: Draft,
    resolver: usize,
    mode: Mode,
}
#[derive(Hash, PartialEq, Eq)]
struct Frame {
    schema: usize,
    instance: usize,
    class: Draft,
    filter: Draft,
    resolver: usize,
    mode: Mode,
}
struct Evaluator<'a> {
    refs: local_refs::LocalRefs<'a>,
    active: HashSet<Frame>,
    fragments: Vec<(usize, Draft, String, jsonschema::Validator)>,
    operations: usize,
}
impl<'a> Evaluator<'a> {
    fn eval(
        &mut self,
        schema: &'a Value,
        instance: &Value,
        state: State,
        pointer: &str,
    ) -> Evaluation {
        let frame = Frame {
            schema: schema as *const Value as usize,
            instance: instance as *const Value as usize,
            class: state.class,
            filter: state.filter,
            resolver: state.resolver,
            mode: state.mode,
        };
        if self.active.contains(&frame) {
            return Err(EvalFailure::Abort(AbortClass::RecursionError));
        }
        self.operations += 1;
        if self.active.len() >= 128 || self.operations > 100_000 {
            return Err(gap(GapKind::BudgetBoundary, pointer));
        }
        self.active.insert(frame);
        let result = self.keywords(schema, instance, state, pointer);
        self.active.remove(&Frame {
            schema: schema as *const Value as usize,
            instance: instance as *const Value as usize,
            class: state.class,
            filter: state.filter,
            resolver: state.resolver,
            mode: state.mode,
        });
        result
    }
    fn descend(
        &mut self,
        schema: &'a Value,
        instance: &Value,
        state: State,
        mode: Mode,
        pointer: &str,
        resolved: bool,
    ) -> Evaluation {
        // Python's descend short-circuits boolean schemas before resource entry/evolve.
        if let Some(value) = schema.as_bool() {
            return Ok(if value {
                Validity::Valid
            } else {
                Validity::Invalid
            });
        }
        if !resolved {
            self.refs.enter(schema, state.class, pointer)?;
        }
        let class = evolved(schema, state.class, pointer)?;
        self.eval(
            schema,
            instance,
            State {
                class,
                filter: state.class,
                mode,
                ..state
            },
            pointer,
        )
    }
    fn probe(
        &mut self,
        schema: &'a Value,
        instance: &Value,
        state: State,
        pointer: &str,
    ) -> Evaluation {
        let class = evolved(schema, state.class, pointer)?;
        self.eval(
            schema,
            instance,
            State {
                class,
                filter: class,
                mode: Mode::FirstError,
                ..state
            },
            pointer,
        )
    }
    fn fragment(
        &mut self,
        schema: &'a Value,
        key: &str,
        value: &Value,
        instance: &Value,
        state: State,
        pointer: &str,
    ) -> Evaluation {
        if key == "type" {
            let types: Vec<&str> = if let Some(single) = value.as_str() {
                vec![single]
            } else if let Some(array) = value.as_array() {
                array
                    .iter()
                    .map(Value::as_str)
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(|| gap(GapKind::FragmentShape, pointer))?
            } else {
                return Err(gap(GapKind::FragmentShape, pointer));
            };
            if types.iter().any(|t| matches!(*t, "number" | "integer")) {
                return Err(gap(GapKind::Numeric, pointer));
            }
            if types.is_empty()
                || types
                    .iter()
                    .any(|t| !matches!(*t, "object" | "array" | "string" | "boolean" | "null"))
            {
                return Err(gap(GapKind::FragmentShape, pointer));
            }
        } else if !value
            .as_array()
            .is_some_and(|a| !a.is_empty() && a.iter().all(Value::is_string))
        {
            // Draft4 requires nonempty required. Later drafts also permit [].
            if !(state.class != Draft::Draft4 && value.as_array().is_some_and(Vec::is_empty)) {
                return Err(gap(GapKind::FragmentShape, pointer));
            }
        }
        let identity = schema as *const Value as usize;
        let index = if let Some(index) =
            self.fragments.iter().position(|(node, class, keyword, _)| {
                *node == identity && *class == state.class && keyword == key
            }) {
            index
        } else {
            let fragment = json!({key: value});
            let validator = jsonschema::options()
                .offline()
                .with_draft(state.class)
                .should_validate_formats(false)
                .build(&fragment)
                .map_err(|_| EvalFailure::InternalFragmentCompilation)?;
            self.fragments
                .push((identity, state.class, key.into(), validator));
            self.fragments.len() - 1
        };
        Ok(if self.fragments[index].3.is_valid(instance) {
            Validity::Valid
        } else {
            Validity::Invalid
        })
    }
    fn keywords(
        &mut self,
        schema: &'a Value,
        instance: &Value,
        state: State,
        pointer: &str,
    ) -> Evaluation {
        if let Some(value) = schema.as_bool() {
            return Ok(if value {
                Validity::Valid
            } else {
                Validity::Invalid
            });
        }
        let object = schema
            .as_object()
            .ok_or_else(|| gap(GapKind::FragmentShape, pointer))?;
        let filtered = old(state.filter) && object.get("$ref").is_some_and(|v| !v.is_null());
        let mut invalid = false;
        for (key, value) in object {
            if filtered && key != "$ref" {
                continue;
            }
            if !recognized(state.class, key) {
                continue;
            }
            let path = child_pointer(pointer, key);
            let result = self.keyword(schema, key, value, instance, state, &path)?;
            if result == Validity::Invalid {
                if state.mode == Mode::FirstError {
                    return Ok(Validity::Invalid);
                }
                invalid = true;
            }
        }
        Ok(if invalid {
            Validity::Invalid
        } else {
            Validity::Valid
        })
    }
    fn keyword(
        &mut self,
        schema: &'a Value,
        key: &str,
        value: &'a Value,
        instance: &Value,
        state: State,
        pointer: &str,
    ) -> Evaluation {
        match key {
            "required" | "type" => self.fragment(schema, key, value, instance, state, pointer),
            "format" => Ok(Validity::Valid),
            "properties" => {
                let Some(properties) = instance.as_object() else {
                    return Ok(Validity::Valid);
                };
                let subschemas = value
                    .as_object()
                    .ok_or_else(|| gap(GapKind::FragmentShape, pointer))?;
                let mut invalid = false;
                for (name, sub) in subschemas {
                    if let Some(content) = properties.get(name) {
                        let outcome = self.descend(
                            sub,
                            content,
                            state,
                            state.mode,
                            &child_pointer(pointer, name),
                            false,
                        )?;
                        if outcome == Validity::Invalid {
                            if state.mode == Mode::FirstError {
                                return Ok(Validity::Invalid);
                            }
                            invalid = true;
                        }
                    }
                }
                Ok(if invalid {
                    Validity::Invalid
                } else {
                    Validity::Valid
                })
            }
            "allOf" | "anyOf" | "oneOf" => {
                let branches = value
                    .as_array()
                    .ok_or_else(|| gap(GapKind::FragmentShape, pointer))?;
                let mut successes = 0;
                let mut invalid = false;
                for (index, sub) in branches.iter().enumerate() {
                    let path = child_pointer(pointer, &index.to_string());
                    let outcome = if key == "oneOf" && successes > 0 {
                        self.probe(sub, instance, state, &path)?
                    } else {
                        self.descend(
                            sub,
                            instance,
                            state,
                            if key == "allOf" {
                                state.mode
                            } else {
                                Mode::ExhaustErrors
                            },
                            &path,
                            false,
                        )?
                    };
                    if outcome == Validity::Valid {
                        successes += 1;
                        if key == "anyOf" {
                            return Ok(Validity::Valid);
                        }
                    } else if key == "allOf" {
                        if state.mode == Mode::FirstError {
                            return Ok(Validity::Invalid);
                        }
                        invalid = true;
                    }
                }
                Ok(
                    if match key {
                        "allOf" => !invalid,
                        "anyOf" => false,
                        _ => successes == 1,
                    } {
                        Validity::Valid
                    } else {
                        Validity::Invalid
                    },
                )
            }
            "not" => Ok(
                if self.probe(value, instance, state, pointer)? == Validity::Valid {
                    Validity::Invalid
                } else {
                    Validity::Valid
                },
            ),
            "if" => {
                let branch = if self.probe(value, instance, state, pointer)? == Validity::Valid {
                    "then"
                } else {
                    "else"
                };
                if let Some(sub) = schema.get(branch) {
                    self.descend(
                        sub,
                        instance,
                        state,
                        state.mode,
                        &child_pointer(pointer.rsplit_once('/').unwrap().0, branch),
                        false,
                    )
                } else {
                    Ok(Validity::Valid)
                }
            }
            "contains" => {
                let Some(items) = instance.as_array() else {
                    return Ok(Validity::Valid);
                };
                let modern = !old(state.class);
                let limit = |name, default| -> Result<u32, EvalFailure> {
                    let Some(number) = schema.get(name) else {
                        return Ok(default);
                    };
                    let raw = number.to_string();
                    if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
                        return Err(gap(GapKind::Numeric, pointer));
                    }
                    raw.parse::<u32>()
                        .map_err(|_| gap(GapKind::Numeric, pointer))
                };
                let (min, max) = if modern {
                    (
                        limit("minContains", 1)?,
                        limit(
                            "maxContains",
                            u32::try_from(items.len())
                                .map_err(|_| gap(GapKind::BudgetBoundary, pointer))?,
                        )?,
                    )
                } else {
                    (1, u32::MAX)
                };
                let mut count = 0;
                for item in items {
                    if self.probe(value, item, state, pointer)? == Validity::Valid {
                        count += 1;
                        if !modern {
                            return Ok(Validity::Valid);
                        }
                        if count > max {
                            return Ok(Validity::Invalid);
                        }
                    }
                }
                Ok(if count >= min {
                    Validity::Valid
                } else {
                    Validity::Invalid
                })
            }
            "$ref" => {
                let reference = value
                    .as_str()
                    .ok_or_else(|| gap(GapKind::FragmentShape, pointer))?;
                let (target, path, resolver) =
                    self.refs.lookup(reference, pointer, state.resolver)?;
                self.descend(
                    target,
                    instance,
                    State { resolver, ..state },
                    state.mode,
                    &path,
                    true,
                )
            }
            _ => {
                // Certify only immediate applicability no-ops, never compile the schema.
                let applicability = match key {
                    "pattern" | "minLength" | "maxLength" => instance.is_string(),
                    "minimum" | "maximum" | "exclusiveMinimum" | "exclusiveMaximum"
                    | "multipleOf" => instance.is_number(),
                    "items" | "additionalItems" | "minItems" | "maxItems" | "uniqueItems"
                    | "unevaluatedItems" => instance.is_array(),
                    "additionalProperties"
                    | "patternProperties"
                    | "dependencies"
                    | "dependentRequired"
                    | "dependentSchemas"
                    | "propertyNames"
                    | "minProperties"
                    | "maxProperties"
                    | "unevaluatedProperties" => instance.is_object(),
                    _ => true,
                };
                if !applicability {
                    return Ok(Validity::Valid);
                }
                let kind = match key {
                    "$dynamicRef" | "$recursiveRef" => GapKind::DynamicScope,
                    "unevaluatedItems" | "unevaluatedProperties" => GapKind::Annotation,
                    "pattern" | "patternProperties" => GapKind::PythonRegex,
                    "minimum" | "maximum" | "exclusiveMinimum" | "exclusiveMaximum"
                    | "multipleOf" | "minLength" | "maxLength" | "minItems" | "maxItems"
                    | "minProperties" | "maxProperties" => GapKind::Numeric,
                    _ => GapKind::Keyword,
                };
                Err(gap(kind, pointer))
            }
        }
    }
}
// Exact inventories captured from installed validators.py, hash in provenance.
fn recognized(draft: Draft, key: &str) -> bool {
    const COMMON: &[&str] = &[
        "$ref",
        "additionalProperties",
        "allOf",
        "anyOf",
        "enum",
        "format",
        "items",
        "maxItems",
        "maxLength",
        "maxProperties",
        "maximum",
        "minItems",
        "minLength",
        "minProperties",
        "minimum",
        "multipleOf",
        "not",
        "oneOf",
        "pattern",
        "patternProperties",
        "properties",
        "required",
        "type",
        "uniqueItems",
    ];
    COMMON.contains(&key)
        || match draft {
            Draft::Draft4 => ["additionalItems", "dependencies"].contains(&key),
            Draft::Draft6 => [
                "additionalItems",
                "dependencies",
                "const",
                "contains",
                "exclusiveMaximum",
                "exclusiveMinimum",
                "propertyNames",
            ]
            .contains(&key),
            Draft::Draft7 => [
                "additionalItems",
                "dependencies",
                "const",
                "contains",
                "exclusiveMaximum",
                "exclusiveMinimum",
                "propertyNames",
                "if",
            ]
            .contains(&key),
            Draft::Draft201909 => [
                "$recursiveRef",
                "additionalItems",
                "const",
                "contains",
                "dependentRequired",
                "dependentSchemas",
                "exclusiveMaximum",
                "exclusiveMinimum",
                "if",
                "propertyNames",
                "unevaluatedItems",
                "unevaluatedProperties",
            ]
            .contains(&key),
            Draft::Draft202012 => [
                "$dynamicRef",
                "const",
                "contains",
                "dependentRequired",
                "dependentSchemas",
                "exclusiveMaximum",
                "exclusiveMinimum",
                "if",
                "prefixItems",
                "propertyNames",
                "unevaluatedItems",
                "unevaluatedProperties",
            ]
            .contains(&key),
            _ => false,
        }
}
#[cfg(test)]
fn result_tag(result: &Evaluation) -> String {
    match result {
        Ok(validity) => format!("{validity:?}"),
        Err(EvalFailure::Abort(class)) => format!("Abort:{class:?}"),
        Err(EvalFailure::ScopeGap { kind, .. }) => format!("ScopeGap:{kind:?}"),
        Err(EvalFailure::InternalFragmentCompilation) => "InternalFragmentCompilation".into(),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::output_schema::dialect::{SelectedDialect, select_with_default};
    use jsonschema::Draft;
    use serde_json::Value;

    #[test]
    fn directed_results_match_the_frozen_installed_oracle() {
        let rows: Vec<Value> = serde_json::from_str(include_str!(
            "../../tests/output_schema_traversal_cases.json"
        ))
        .unwrap();
        for row in rows {
            let schema: Value = serde_json::from_str(row["schema_json"].as_str().unwrap()).unwrap();
            let instance: Value = match serde_json::from_str(row["content_json"].as_str().unwrap())
            {
                Ok(value) => value,
                Err(error) => {
                    assert!(error.to_string().contains("recursion limit exceeded"));
                    assert_eq!(row["expected_gap"], "BudgetBoundary");
                    continue;
                }
            };
            let before = (
                serde_json::to_vec(&schema).unwrap(),
                serde_json::to_vec(&instance).unwrap(),
            );
            if row["oracle"]["schema"]["result"] != "valid" {
                continue;
            }
            let selection = select_with_default(&schema, Draft::Draft202012).unwrap();
            let result = match selection {
                SelectedDialect::Draft3 => Err(EvalFailure::ScopeGap {
                    kind: GapKind::Draft3,
                    schema_pointer: String::new(),
                }),
                SelectedDialect::Supported(draft) => evaluate_checked(
                    &schema,
                    &instance,
                    draft,
                    if row["mode"] == "FirstError" {
                        Mode::FirstError
                    } else {
                        Mode::ExhaustErrors
                    },
                ),
            };
            let expected = if let Some(gap) = row["expected_gap"].as_str() {
                format!("ScopeGap:{gap}")
            } else {
                row["oracle"]["instance"]["result"].as_str().unwrap().into()
            };
            assert_eq!(result_tag(&result), expected, "{}", row["name"]);
            assert_eq!(
                (
                    serde_json::to_vec(&schema).unwrap(),
                    serde_json::to_vec(&instance).unwrap()
                ),
                before,
                "{}",
                row["name"]
            );
        }
    }

    #[test]
    fn selector_membership_and_child_fallback_match_installed_python() {
        for schema in [
            Value::Null,
            serde_json::json!(1),
            serde_json::json!(["$schema"]),
            serde_json::json!("text $schema text"),
        ] {
            assert!(select_with_default(&schema, Draft::Draft7).is_err());
        }
        for schema in [
            serde_json::json!(true),
            serde_json::json!(false),
            serde_json::json!({}),
            serde_json::json!({"$schema":"unknown"}),
            serde_json::json!([]),
            serde_json::json!("schema"),
        ] {
            assert_eq!(
                select_with_default(&schema, Draft::Draft7),
                Ok(SelectedDialect::Supported(Draft::Draft7))
            );
        }
    }

    #[test]
    fn inventories_and_notices_are_exact() {
        use sha2::{Digest, Sha256};
        let manifest: Value =
            serde_json::from_str(include_str!("traversal/PROVENANCE.json")).unwrap();
        for (draft, name) in [
            (Draft::Draft4, "Draft4Validator"),
            (Draft::Draft6, "Draft6Validator"),
            (Draft::Draft7, "Draft7Validator"),
            (Draft::Draft201909, "Draft201909Validator"),
            (Draft::Draft202012, "Draft202012Validator"),
        ] {
            let expected: HashSet<&str> = manifest["keyword_inventories"][name]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect();
            for key in [
                "$ref",
                "$dynamicRef",
                "$recursiveRef",
                "additionalItems",
                "additionalProperties",
                "allOf",
                "anyOf",
                "const",
                "contains",
                "dependencies",
                "dependentRequired",
                "dependentSchemas",
                "enum",
                "exclusiveMaximum",
                "exclusiveMinimum",
                "format",
                "if",
                "items",
                "maxItems",
                "maxLength",
                "maxProperties",
                "maximum",
                "minItems",
                "minLength",
                "minProperties",
                "minimum",
                "multipleOf",
                "not",
                "oneOf",
                "pattern",
                "patternProperties",
                "prefixItems",
                "properties",
                "propertyNames",
                "required",
                "type",
                "unevaluatedItems",
                "unevaluatedProperties",
                "uniqueItems",
                "extension",
                "then",
                "else",
                "$id",
                "$anchor",
                "default",
                "examples",
            ] {
                assert_eq!(
                    recognized(draft, key),
                    expected.contains(key),
                    "{draft:?}/{key}"
                );
            }
        }
        for (index, notice) in [
            include_bytes!("traversal/JSONSCHEMA-COPYING").as_slice(),
            include_bytes!("traversal/REFERENCING-COPYING").as_slice(),
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(
                format!("{:x}", Sha256::digest(notice)),
                manifest["notices"][index]["sha256"].as_str().unwrap()
            );
        }
    }

    #[test]
    fn productive_value_depth_and_protective_limits_are_distinct_from_cycles() {
        let schema = serde_json::json!({"properties":{"child":{"$ref":"#"}}});
        let mut instance = serde_json::json!({});
        for _ in 0..48 {
            instance = serde_json::json!({"child":instance});
        }
        assert_eq!(
            evaluate_checked(&schema, &instance, Draft::Draft202012, Mode::ExhaustErrors),
            Ok(Validity::Valid)
        );
        for _ in 48..260 {
            instance = serde_json::json!({"child":instance});
        }
        assert!(matches!(
            evaluate_checked(&schema, &instance, Draft::Draft202012, Mode::ExhaustErrors),
            Err(EvalFailure::ScopeGap {
                kind: GapKind::BudgetBoundary,
                ..
            })
        ));
    }

    #[test]
    fn evaluator_never_reads_external_file_or_connects_to_network() {
        use std::{
            fs,
            net::{TcpListener, TcpStream},
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let marker = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let _ = listener.accept().unwrap();
        drop(marker);
        let secret =
            std::env::temp_dir().join(format!("traversal-private-{}.json", std::process::id()));
        fs::write(&secret, r#"{"type":123}"#).unwrap();
        fs::File::open(&secret)
            .unwrap()
            .set_times(fs::FileTimes::new().set_accessed(std::time::UNIX_EPOCH))
            .unwrap();
        for reference in [
            format!("http://{}/schema", listener.local_addr().unwrap()),
            format!("file://{}", secret.display()),
        ] {
            let schema = serde_json::json!({"$ref":reference});
            assert_eq!(
                evaluate_checked(
                    &schema,
                    &serde_json::json!({}),
                    Draft::Draft202012,
                    Mode::ExhaustErrors
                ),
                Err(EvalFailure::Abort(AbortClass::Unresolvable))
            );
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
    #[ignore = "explicit installed Python precondition proof and differential audit only"]
    fn audit_probe() {
        use std::io::Read;
        crate::output_schema::resource_budget().unwrap();
        let mut input = String::new();
        std::io::stdin()
            .take(8 * 1024 * 1024 + 1)
            .read_to_string(&mut input)
            .unwrap();
        assert!(input.len() <= 8 * 1024 * 1024);
        let rows: Vec<Value> = serde_json::from_str(&input).unwrap();
        assert!(rows.len() <= 8);
        for row in rows {
            assert!(row["name"].as_str().unwrap().len() <= 512);
            let schema: Value = match serde_json::from_str(row["schema_json"].as_str().unwrap()) {
                Ok(schema) => schema,
                Err(error) => {
                    assert!(error.to_string().contains("recursion limit exceeded"));
                    println!(
                        "SCHEMA_TRAVERSAL={}",
                        serde_json::json!({"name":row["name"], "result":"ScopeGap:BudgetBoundary", "phase":"input"})
                    );
                    continue;
                }
            };
            let selected = select_with_default(&schema, Draft::Draft202012);
            let root = match &selected {
                Ok(SelectedDialect::Supported(Draft::Draft4)) => "Draft4Validator",
                Ok(SelectedDialect::Supported(Draft::Draft6)) => "Draft6Validator",
                Ok(SelectedDialect::Supported(Draft::Draft7)) => "Draft7Validator",
                Ok(SelectedDialect::Supported(Draft::Draft201909)) => "Draft201909Validator",
                Ok(SelectedDialect::Supported(Draft::Draft202012)) => "Draft202012Validator",
                Ok(SelectedDialect::Draft3) => "ScopeGap:Draft3",
                _ => "ScopeGap:DialectFailure",
            };
            if row["oracle_checked"] != true {
                println!(
                    "SCHEMA_TRAVERSAL={}",
                    serde_json::json!({"name":row["name"], "root":root, "result":"not-run:precondition"})
                );
                continue;
            }
            let content: Value = match serde_json::from_str(row["content_json"].as_str().unwrap()) {
                Ok(content) => content,
                Err(error) => {
                    assert!(error.to_string().contains("recursion limit exceeded"));
                    println!(
                        "SCHEMA_TRAVERSAL={}",
                        serde_json::json!({"name":row["name"], "root":root, "result":"ScopeGap:BudgetBoundary", "phase":"input"})
                    );
                    continue;
                }
            };
            let result = match selected {
                Ok(SelectedDialect::Supported(draft)) => evaluate_checked(
                    &schema,
                    &content,
                    draft,
                    if row["mode"] == "FirstError" {
                        Mode::FirstError
                    } else {
                        Mode::ExhaustErrors
                    },
                ),
                Ok(SelectedDialect::Draft3) => Err(gap(GapKind::Draft3, "")),
                Err(_) => Err(gap(GapKind::DialectFailure, "")),
            };
            let pointer = if let Err(EvalFailure::ScopeGap { schema_pointer, .. }) = &result {
                schema_pointer.chars().take(4096).collect::<String>()
            } else {
                String::new()
            };
            println!(
                "SCHEMA_TRAVERSAL={}",
                serde_json::json!({"name":row["name"], "root":root, "result":result_tag(&result), "pointer":pointer, "phase":"evaluation"})
            );
        }
    }
}
