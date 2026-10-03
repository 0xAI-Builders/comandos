//! Private, unwired Py311SreV1 literal/dot/anchor boundary.

mod compiler;
mod parser;
mod semantic;
mod sre;
mod syntax;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RegexPhase {
    Decode,
    Compile,
    Search,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RegexGap {
    Grammar,
    Unicode14,
    MatcherFeature,
    TextTransport,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(
    clippy::enum_variant_names,
    reason = "Preserve Python exception class names"
)]
enum CompileAbort {
    OverflowError,
    ValueError,
    RecursionError,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RegexFailure {
    ReError {
        position: Option<usize>,
    },
    CompileAbort(CompileAbort),
    ScopeGap {
        kind: RegexGap,
        position: Option<usize>,
    },
    BudgetBoundary {
        phase: RegexPhase,
    },
    InternalProgram,
}

struct RegexBudget {
    work: usize,
    memory: usize,
    pattern_points: usize,
    subject_points: usize,
    words: usize,
}
impl Default for RegexBudget {
    fn default() -> Self {
        Self {
            work: 2_000_000,
            memory: 8 * 1024 * 1024,
            pattern_points: 16_384,
            subject_points: 262_144,
            words: 65_536,
        }
    }
}
impl RegexBudget {
    fn charge(&mut self, work: usize, phase: RegexPhase) -> Result<(), RegexFailure> {
        self.work = self
            .work
            .checked_sub(work)
            .ok_or(RegexFailure::BudgetBoundary { phase })?;
        Ok(())
    }
    fn allocation(&mut self, elements: usize, phase: RegexPhase) -> Result<(), RegexFailure> {
        let bytes = elements
            .checked_mul(size_of::<u32>())
            .ok_or(RegexFailure::BudgetBoundary { phase })?;
        self.memory = self
            .memory
            .checked_sub(bytes)
            .ok_or(RegexFailure::BudgetBoundary { phase })?;
        Ok(())
    }
    fn bytes(&mut self, bytes: usize, phase: RegexPhase) -> Result<(), RegexFailure> {
        self.memory = self
            .memory
            .checked_sub(bytes)
            .ok_or(RegexFailure::BudgetBoundary { phase })?;
        Ok(())
    }
    fn input(&self, count: usize, pattern: bool) -> Result<(), RegexFailure> {
        let limit = if pattern {
            self.pattern_points
        } else {
            self.subject_points
        };
        if count > limit {
            Err(RegexFailure::BudgetBoundary {
                phase: RegexPhase::Decode,
            })
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy)]
struct RegexText<'a>(&'a [u32]);
impl<'a> RegexText<'a> {
    fn new(
        points: &'a [u32],
        pattern: bool,
        budget: &mut RegexBudget,
    ) -> Result<Self, RegexFailure> {
        budget.input(points.len(), pattern)?;
        for (position, &point) in points.iter().enumerate() {
            budget.charge(1, RegexPhase::Decode)?;
            if point > 0x10ffff {
                return Err(RegexFailure::ScopeGap {
                    kind: RegexGap::TextTransport,
                    position: Some(position),
                });
            }
        }
        Ok(Self(points))
    }
}

// Count under the same aggregate budget before allocating. vec![0; count]
// owns exactly count elements of capacity; no incremental Vec growth occurs.
fn scalar_points(
    text: &str,
    pattern: bool,
    budget: &mut RegexBudget,
) -> Result<Vec<u32>, RegexFailure> {
    let mut count = 0usize;
    for _ in text.chars() {
        budget.charge(1, RegexPhase::Decode)?;
        count = count.checked_add(1).ok_or(RegexFailure::BudgetBoundary {
            phase: RegexPhase::Decode,
        })?;
        budget.input(count, pattern)?;
    }
    budget.allocation(count, RegexPhase::Decode)?;
    let mut points = vec![0; count];
    for (slot, c) in points.iter_mut().zip(text.chars()) {
        budget.charge(1, RegexPhase::Decode)?;
        *slot = u32::from(c);
    }
    Ok(points)
}

struct PythonRegex {
    words: Vec<u32>,
}
fn compile(pattern: RegexText<'_>, budget: &mut RegexBudget) -> Result<PythonRegex, RegexFailure> {
    budget.input(pattern.0.len(), true)?;
    let syntax = parser::parse_validate(pattern, budget, &mut syntax::WarningSink::default())?;
    let words = compiler::emit(&syntax, budget)?;
    compiler::validate(&words, budget)?;
    Ok(PythonRegex { words })
}
fn check_syntax(pattern: RegexText<'_>, budget: &mut RegexBudget) -> Result<(), RegexFailure> {
    parser::parse_validate(pattern, budget, &mut syntax::WarningSink::default()).map(|_| ())
}
impl PythonRegex {
    fn search(&self, text: RegexText<'_>, budget: &mut RegexBudget) -> Result<bool, RegexFailure> {
        budget.input(text.0.len(), false)?;
        sre::search(&self.words, text, budget)
    }
}
fn check_regex_format_value(
    value: &serde_json::Value,
    budget: &mut RegexBudget,
) -> Result<(), RegexFailure> {
    let Some(pattern) = value.as_str() else {
        return Ok(());
    };
    let points = scalar_points(pattern, true, budget)?;
    check_syntax(RegexText::new(&points, true, budget)?, budget)
}

#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
enum PatternValidity {
    Valid,
    Invalid,
}
#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
enum PatternProbeFailure {
    SchemaScope,
    Regex(RegexFailure),
}
#[cfg(test)]
impl From<RegexFailure> for PatternProbeFailure {
    fn from(error: RegexFailure) -> Self {
        Self::Regex(error)
    }
}
#[cfg(test)]
fn probe_checked_pattern(
    schema: &serde_json::Value,
    instance: &serde_json::Value,
    draft: jsonschema::Draft,
    budget: &mut RegexBudget,
) -> Result<PatternValidity, PatternProbeFailure> {
    use super::dialect::{SelectedDialect, select_with_default};
    let object = schema.as_object().ok_or(PatternProbeFailure::SchemaScope)?;
    if object
        .keys()
        .any(|key| key != "pattern" && key != "$schema")
    {
        return Err(PatternProbeFailure::SchemaScope);
    }
    let pattern = object
        .get("pattern")
        .and_then(serde_json::Value::as_str)
        .ok_or(PatternProbeFailure::SchemaScope)?;
    // Registered declarations select the same class under distinct fallbacks.
    // Unknown declarations inherit those different fallbacks. This preserves
    // the existing selector's URI normalization without copying its registry.
    if object.contains_key("$schema")
        && [jsonschema::Draft::Draft4, jsonschema::Draft::Draft202012]
            .into_iter()
            .any(|fallback| {
                !matches!(select_with_default(schema, fallback),
                Ok(SelectedDialect::Supported(selected)) if selected == draft)
            })
    {
        return Err(PatternProbeFailure::SchemaScope);
    }
    let Some(text) = instance.as_str() else {
        return Ok(PatternValidity::Valid);
    };
    let points = scalar_points(pattern, true, budget)?;
    let regex = compile(RegexText::new(&points, true, budget)?, budget)?;
    let subject = scalar_points(text, false, budget)?;
    Ok(
        if regex.search(RegexText::new(&subject, false, budget)?, budget)? {
            PatternValidity::Valid
        } else {
            PatternValidity::Invalid
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn failure(error: RegexFailure) -> Value {
        match error {
            RegexFailure::ReError { position } => json!({"result":"ReError","position":position}),
            RegexFailure::ScopeGap { kind, position } => {
                json!({"result":format!("ScopeGap:{kind:?}"),"position":position})
            }
            RegexFailure::BudgetBoundary { phase } => {
                json!({"result":format!("BudgetBoundary:{phase:?}")})
            }
            RegexFailure::CompileAbort(class) => {
                json!({"result":format!("CompileAbort:{class:?}")})
            }
            RegexFailure::InternalProgram => json!({"result":"InternalProgram"}),
        }
    }
    fn vector(
        value: &Value,
        pattern: bool,
        budget: &mut RegexBudget,
    ) -> Result<Vec<u32>, RegexFailure> {
        let values = value.as_array().ok_or(RegexFailure::InternalProgram)?;
        budget.input(values.len(), pattern)?;
        budget.allocation(values.len(), RegexPhase::Decode)?;
        let mut points = vec![0; values.len()];
        for (position, (slot, value)) in points.iter_mut().zip(values).enumerate() {
            budget.charge(1, RegexPhase::Decode)?;
            *slot = value
                .as_u64()
                .and_then(|p| u32::try_from(p).ok())
                .filter(|&p| p <= 0x10ffff)
                .ok_or(RegexFailure::ScopeGap {
                    kind: RegexGap::TextTransport,
                    position: Some(position),
                })?;
        }
        Ok(points)
    }
    fn row_result(row: &Value) -> Value {
        let mut budget = RegexBudget::default();
        for (key, limit) in [
            ("work", &mut budget.work),
            ("memory", &mut budget.memory),
            ("words", &mut budget.words),
        ] {
            if let Some(value) = row["budget"][key].as_u64() {
                *limit = (*limit).min(usize::try_from(value).unwrap());
            }
        }
        let run = || -> Result<Value, RegexFailure> {
            match row["op"].as_str().unwrap() {
                "format" => {
                    check_regex_format_value(&row["value"], &mut budget)?;
                    Ok(json!({"result":"FormatValid"}))
                }
                "search" => {
                    let pattern = vector(&row["pattern"], true, &mut budget)?;
                    let regex = compile(RegexText::new(&pattern, true, &mut budget)?, &mut budget)?;
                    let text = vector(&row["text"], false, &mut budget)?;
                    let found =
                        regex.search(RegexText::new(&text, false, &mut budget)?, &mut budget)?;
                    Ok(json!({"result":"Match","value":found}))
                }
                "schema" => {
                    assert_eq!(
                        row["oracle_checked"], true,
                        "exact selected-class check_schema proof required"
                    );
                    use sha2::{Digest, Sha256};
                    let raw = row["schema_json"].as_str().unwrap();
                    assert_eq!(
                        row["oracle"]["proof"]["schema_sha256"],
                        format!("{:x}", Sha256::digest(raw.as_bytes()))
                    );
                    let (draft, class) = match row["draft"].as_str().unwrap() {
                        "draft4" => (jsonschema::Draft::Draft4, "Draft4Validator"),
                        "draft6" => (jsonschema::Draft::Draft6, "Draft6Validator"),
                        "draft7" => (jsonschema::Draft::Draft7, "Draft7Validator"),
                        "draft201909" => (jsonschema::Draft::Draft201909, "Draft201909Validator"),
                        "draft202012" => (jsonschema::Draft::Draft202012, "Draft202012Validator"),
                        _ => panic!("unsupported selected draft"),
                    };
                    assert_eq!(row["oracle"]["proof"]["selected_class"], class);
                    assert_eq!(row["oracle"]["schema"]["result"], "valid");
                    let parse = |raw: &str| {
                        serde_json::from_str::<Value>(raw).map_err(|_| RegexFailure::ScopeGap {
                            kind: RegexGap::TextTransport,
                            position: None,
                        })
                    };
                    let schema = parse(raw)?;
                    let instance = parse(row["content_json"].as_str().unwrap())?;
                    match probe_checked_pattern(&schema, &instance, draft, &mut budget) {
                        Ok(value) => Ok(json!({"result":format!("{value:?}")})),
                        Err(PatternProbeFailure::Regex(error)) => Err(error),
                        Err(PatternProbeFailure::SchemaScope) => {
                            Ok(json!({"result":"ScopeGap:SchemaScope"}))
                        }
                    }
                }
                _ => Err(RegexFailure::InternalProgram),
            }
        };
        let mut run = run;
        run().unwrap_or_else(failure)
    }

    fn fixtures() -> Vec<Value> {
        serde_json::from_str(include_str!("../../tests/output_schema_regex_cases.json")).unwrap()
    }

    #[test]
    fn installed_literal_anchor_dot_and_escape_results() {
        for row in fixtures() {
            if row["disposition"] != "implemented"
                || row.get("protective").is_some()
                || row.get("transport").is_some()
                || row["op"] == "schema"
            {
                continue;
            }
            let expected = if row["op"] == "format" {
                row["oracle"].clone()
            } else if row["oracle"]["compile"]["result"] == "Compiled" {
                row["oracle"]["search"].clone()
            } else {
                row["oracle"]["compile"].clone()
            };
            let native = row_result(&row);
            assert_eq!(native["result"], expected["result"], "{}", row["name"]);
            for key in ["value", "position"] {
                assert_eq!(native[key], expected[key], "{} {key}", row["name"]);
            }
        }
    }

    #[test]
    fn complete_frontend_resolves_grammar_before_matcher_admission() {
        for row in fixtures() {
            if row["disposition"] == "Grammar" && row["op"] != "schema" {
                assert_ne!(
                    row_result(&row)["result"],
                    "ScopeGap:Grammar",
                    "{}",
                    row["name"]
                );
            }
        }
    }

    #[test]
    fn selected_class_schema_assertion_and_applicability() {
        for mut row in fixtures() {
            if row["op"] != "schema" || row["oracle"]["schema"]["result"] != "valid" {
                continue;
            }
            row["oracle_checked"] = true.into();
            let expected = match row["disposition"].as_str().unwrap() {
                "implemented" => row["oracle"]["instance"]["result"].as_str().unwrap(),
                "Grammar" => "ScopeGap:MatcherFeature",
                "TextTransport" => "ScopeGap:TextTransport",
                "SchemaScope" => "ScopeGap:SchemaScope",
                _ => panic!("unknown cohort"),
            };
            assert_eq!(row_result(&row)["result"], expected, "{}", row["name"]);
        }
    }

    #[test]
    fn resource_and_transport_boundaries_are_explicit() {
        for row in fixtures() {
            if row.get("protective").is_none() && row.get("transport").is_none() {
                continue;
            }
            let native = row_result(&row);
            if row.get("protective").is_some() {
                assert!(
                    native["result"]
                        .as_str()
                        .unwrap()
                        .starts_with("BudgetBoundary:"),
                    "{} {native}",
                    row["name"]
                );
            }
            if row.get("transport").is_some() {
                assert_eq!(native["result"], "ScopeGap:TextTransport");
            }
        }
    }

    #[test]
    fn program_validation_checks_limits_operands_and_terminal_boundary() {
        use compiler::*;
        let mut zero = RegexBudget {
            words: 0,
            ..RegexBudget::default()
        };
        assert_eq!(
            validate(&[SUCCESS], &mut zero),
            Err(RegexFailure::BudgetBoundary {
                phase: RegexPhase::Compile
            })
        );
        for words in [
            vec![],
            vec![99],
            vec![LITERAL],
            vec![AT],
            vec![SUCCESS, SUCCESS],
            vec![LITERAL, 0x110000, SUCCESS],
            vec![AT, 1, SUCCESS],
            vec![LITERAL, 97],
            vec![ANY],
        ] {
            assert_eq!(
                validate(&words, &mut RegexBudget::default()),
                Err(RegexFailure::InternalProgram),
                "{words:?}"
            );
        }
        assert_eq!(
            validate(
                &[LITERAL, 0xd800, AT, END, SUCCESS],
                &mut RegexBudget::default()
            ),
            Ok(())
        );
        assert_eq!(
            sre::search(&[99], RegexText(&[]), &mut RegexBudget::default()),
            Err(RegexFailure::InternalProgram)
        );
    }

    #[test]
    fn aggregate_exact_work_and_memory_edges_cover_all_phases() {
        let run = |b: &mut RegexBudget| {
            let p = RegexText::new(&[97], true, b)?;
            compile(p, b)
        };
        let mut baseline = RegexBudget::default();
        run(&mut baseline).unwrap();
        let work = 2_000_000 - baseline.work;
        let memory = 8 * 1024 * 1024 - baseline.memory;
        assert_eq!((work, memory), (20, 632));
        for delta in [-1i32, 0, 1] {
            let mut b = RegexBudget {
                work: work.checked_add_signed(delta as isize).unwrap(),
                ..RegexBudget::default()
            };
            assert_eq!(run(&mut b).is_ok(), delta >= 0);
            let mut b = RegexBudget {
                memory: memory.checked_add_signed(delta as isize).unwrap(),
                ..RegexBudget::default()
            };
            assert_eq!(run(&mut b).is_ok(), delta >= 0);
        }
        let mut b = RegexBudget::default();
        let regex = run(&mut b).unwrap();
        let before = b.work;
        assert_eq!(regex.search(RegexText(&[97]), &mut b), Ok(true));
        let search_work = before - b.work;
        assert_eq!(search_work, 5);
        let mut edge = RegexBudget {
            work: work + search_work - 1,
            memory,
            ..RegexBudget::default()
        };
        let regex = run(&mut edge).unwrap();
        assert!(matches!(
            regex.search(RegexText(&[97]), &mut edge),
            Err(RegexFailure::BudgetBoundary {
                phase: RegexPhase::Search
            })
        ));
        assert_eq!(check_regex_format_value(&json!(false), &mut edge), Ok(()));
    }

    #[test]
    #[ignore = "explicit installed oracle and exact selected-class schema proof only"]
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
            let mut result = row_result(&row);
            result["name"] = row["name"].clone();
            println!("SCHEMA_REGEX={result}");
        }
    }
}
#[path = "python_regex/unicode14.rs"]
mod unicode14;
#[cfg(test)]
mod frontend_tests {
    use super::*;
    #[test]
    fn complete_semantic_syntax_accepts_valid_unsupported_matching() {
        for pattern in [
            "a|b",
            "(?P<name>a)",
            "(?<=ab|cd)x",
            "a{2,3}",
            "(?=a*)",
            "(?(1)a)()",
        ] {
            let points: Vec<u32> = pattern.chars().map(u32::from).collect();
            assert_eq!(
                check_syntax(RegexText(&points), &mut RegexBudget::default()),
                Ok(()),
                "{pattern}"
            );
        }
    }
    #[test]
    fn complete_semantic_compiler_errors_have_nullable_positions() {
        for pattern in ["(?<=a|bc)x", "(?t)a*", "(?<=a{4294967294}aa)x"] {
            let points: Vec<u32> = pattern.chars().map(u32::from).collect();
            assert_eq!(
                check_syntax(RegexText(&points), &mut RegexBudget::default()),
                Err(RegexFailure::ReError { position: None }),
                "{pattern}"
            );
        }
    }
    #[test]
    fn complete_semantic_format_does_not_consume_program_budget() {
        let mut budget = RegexBudget {
            words: 0,
            ..RegexBudget::default()
        };
        assert_eq!(
            check_regex_format_value(&serde_json::json!("a|b"), &mut budget),
            Ok(())
        );
    }
}
#[cfg(test)]
mod frontend_audit {
    use super::*;
    use serde_json::{Value, json};
    fn failure(e: RegexFailure) -> Value {
        match e {
            RegexFailure::ReError { position } => json!({"result":"error","position":position}),
            RegexFailure::CompileAbort(c) => json!({"result":format!("{c:?}"),"position":null}),
            RegexFailure::ScopeGap { kind, position } => {
                json!({"result":format!("ScopeGap:{kind:?}"),"position":position})
            }
            RegexFailure::BudgetBoundary { phase } => {
                json!({"result":format!("BudgetBoundary:{phase:?}")})
            }
            RegexFailure::InternalProgram => json!({"result":"InternalProgram"}),
        }
    }
    fn width_number(width: u128) -> Value {
        serde_json::from_str(&width.to_string()).unwrap()
    }
    fn evaluate(row: &Value) -> Value {
        let mut b = RegexBudget::default();
        let mut warnings = syntax::WarningSink::default();
        let points: Vec<u32> = serde_json::from_value(row["pattern"].clone()).unwrap();
        let syntax = RegexText::new(&points, true, &mut b)
            .and_then(|t| parser::parse_validate(t, &mut b, &mut warnings));
        let mut result = match syntax {
            Err(e) => failure(e),
            Ok(s) => {
                let p = &s.syntax;
                let names: Vec<Value> = p
                    .groups
                    .iter()
                    .enumerate()
                    .filter_map(|(id, g)| g.name.map(|n| json!([&points[n.start..n.end], id])))
                    .collect();
                let mut r = json!({"result":"Compiled","flags":p.flags,"groups":p.groups.len()-1,"names":names,"width_diagnostic":[width_number(p.nodes[p.root].width.lo),width_number(p.nodes[p.root].width.hi)],"peaks":{"frames":p.peak_frames,"frame_capacity_bytes":p.frame_capacity_bytes,"node_size":size_of::<syntax::Node>(),"group_size":size_of::<syntax::Group>(),"class_entry_size":size_of::<syntax::ClassEntry>(),"nodes":p.nodes.len(),"edges":p.edges.len(),"class_entries":p.classes.len(),"group_slots":p.groups.len(),"node_capacity_bytes":p.nodes.capacity()*size_of::<syntax::Node>(),"edge_capacity_bytes":p.edges.capacity()*size_of::<usize>(),"class_capacity_bytes":p.classes.capacity()*size_of::<syntax::ClassEntry>(),"group_capacity_bytes":p.groups.capacity()*size_of::<syntax::Group>()}});
                if let Some(text) = row.get("search") {
                    let emitted = compiler::emit(&s, &mut b).and_then(|words| {
                        compiler::validate(&words, &mut b)?;
                        Ok(PythonRegex { words })
                    });
                    match emitted {
                        Ok(regex) => {
                            r["emitted_words"] = json!(regex.words.len());
                            let text: Vec<u32> = serde_json::from_value(text.clone()).unwrap();
                            r["search"] = match RegexText::new(&text, false, &mut b)
                                .and_then(|t| regex.search(t, &mut b))
                            {
                                Ok(found) => json!(found),
                                Err(e) => failure(e),
                            };
                        }
                        Err(e) => {
                            r["search"] = failure(e);
                        }
                    }
                }
                r
            }
        };
        if row["op"] == "format" {
            result["format_result"] = match check_regex_format_value(&row["value"], &mut b) {
                Ok(()) => json!("FormatValid"),
                Err(RegexFailure::ReError { .. }) => json!("FormatError"),
                Err(e) => json!(format!("{}", failure(e)["result"].as_str().unwrap())),
            };
        }
        if row["op"] == "schema" {
            let raw = row["schema_json"].as_str().unwrap();
            use sha2::{Digest, Sha256};
            let proof_hash = format!("{:x}", Sha256::digest(raw));
            let parsed = serde_json::from_str::<Value>(raw).map_err(|_| RegexFailure::ScopeGap {
                kind: RegexGap::TextTransport,
                position: None,
            });
            let instance = serde_json::from_str::<Value>(row["content_json"].as_str().unwrap());
            let draft = match row["draft"].as_str().unwrap() {
                "draft4" => jsonschema::Draft::Draft4,
                "draft6" => jsonschema::Draft::Draft6,
                "draft7" => jsonschema::Draft::Draft7,
                "draft201909" => jsonschema::Draft::Draft201909,
                "draft202012" => jsonschema::Draft::Draft202012,
                _ => unreachable!(),
            };
            result["schema_identity_sha256"] = json!(proof_hash);
            result["schema_result"] = match (parsed, instance) {
                (Ok(schema), Ok(instance)) => {
                    match probe_checked_pattern(&schema, &instance, draft, &mut b) {
                        Ok(v) => json!({"result":format!("{v:?}")}),
                        Err(PatternProbeFailure::SchemaScope) => {
                            json!({"result":"ScopeGap:SchemaScope"})
                        }
                        Err(PatternProbeFailure::Regex(e)) => failure(e),
                    }
                }
                _ => failure(RegexFailure::ScopeGap {
                    kind: RegexGap::TextTransport,
                    position: None,
                }),
            };
        }
        result["warnings"] = json!(
            warnings
                .observations
                .iter()
                .map(|w| json!({"category":format!("{:?}",w.category),"position":w.position}))
                .collect::<Vec<_>>()
        );
        result["id"] = row["id"].clone();
        result["work_used"] = json!(2_000_000 - b.work);
        result["accounted_bytes"] = json!(8 * 1024 * 1024 - b.memory);
        result
    }
    fn fixtures() -> (Vec<Value>, Vec<Value>) {
        let rows = serde_json::from_str(include_str!(
            "../../tests/fixtures/regex_frontend/cases.json"
        ))
        .unwrap();
        let oracle = include_str!("../../tests/fixtures/regex_frontend/oracle.jsonl")
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        (rows, oracle)
    }
    #[test]
    fn frozen_installed_frontend_class_metadata_and_warnings() {
        let (rows, oracle) = fixtures();
        assert_eq!(rows.len(), oracle.len());
        let mut errors = Vec::new();
        for (row, expected) in rows.iter().zip(&oracle) {
            let actual = evaluate(row);
            if row["family"] == "budget-depth" && row["recipe"].as_str().unwrap().contains("129") {
                assert_eq!(actual["result"], "BudgetBoundary:Compile");
                continue;
            }
            let keys = if expected["result"] == "Compiled" {
                vec!["result", "flags", "groups", "names", "width_diagnostic"]
            } else {
                vec!["result", "position"]
            };
            for k in keys {
                if actual[k] != expected[k] {
                    errors.push(format!(
                        "{} {} {} {k}: actual={} expected={}",
                        row["id"], row["recipe"], row["pattern"], actual[k], expected[k]
                    ));
                }
            }
            let wa = actual["warnings"].as_array().unwrap();
            let we = expected["warnings"].as_array().unwrap();
            if wa.len() != we.len()
                || wa
                    .iter()
                    .zip(we)
                    .any(|(a, e)| a["category"] != e["category"] || a["position"] != e["position"])
            {
                errors.push(format!(
                    "{} warnings actual={} expected={}",
                    row["id"], actual["warnings"], expected["warnings"]
                ));
            }
            if row["op"] == "format" {
                assert_eq!(
                    actual["format_result"], expected["format_result"],
                    "{}",
                    row["id"]
                );
            }
            if row["op"] == "schema" {
                assert_eq!(
                    actual["schema_identity_sha256"], expected["proof"]["schema_sha256"],
                    "{}",
                    row["id"]
                );
                let s = actual["schema_result"]["result"].as_str().unwrap();
                if !s.starts_with("ScopeGap:") {
                    assert_eq!(
                        actual["schema_result"]["result"], expected["instance"]["result"],
                        "{}",
                        row["id"]
                    );
                }
            }
            if actual["search"].is_boolean() {
                assert_eq!(actual["search"], expected["search"], "{}", row["id"]);
            } else if actual.get("search").is_some() {
                assert_eq!(
                    actual["search"]["result"], "ScopeGap:MatcherFeature",
                    "{}",
                    row["id"]
                );
            }
        }
        assert!(
            errors.is_empty(),
            "{} mismatches:\n{}",
            errors.len(),
            errors.join("\n")
        );
    }
    #[test]
    #[ignore = "private explicit bounded frontend development protocol"]
    fn audit_probe() {
        use std::io::Read;
        crate::output_schema::resource_budget().unwrap();
        let mut bytes = Vec::new();
        std::io::stdin()
            .take(8 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .unwrap();
        assert!(bytes.len() <= 8 * 1024 * 1024);
        let rows: Vec<Value> = serde_json::from_slice(&bytes).unwrap();
        assert!(rows.len() <= 8);
        for row in rows {
            println!("REGEX_FRONTEND={}", evaluate(&row));
        }
    }
}
#[cfg(test)]
mod frontend_boundaries {
    use super::*;
    #[test]
    fn synthetic_arena_word_and_allocation_limits() {
        for limit in [0, 1, 2] {
            let mut b = RegexBudget::default();
            let mut v = Vec::new();
            for _ in 0..limit {
                syntax::push(&mut v, 0u32, limit, &mut b).unwrap();
            }
            assert_eq!(
                syntax::push(&mut v, 0u32, limit, &mut b),
                Err(syntax::boundary())
            );
        }
        for words in [2, 3, 4] {
            let mut b = RegexBudget {
                words,
                ..RegexBudget::default()
            };
            assert_eq!(compile(RegexText(&[97]), &mut b).is_ok(), words >= 3);
        }
        let mut b = RegexBudget::default();
        let before = b.memory;
        b.allocation(3, RegexPhase::Compile).unwrap();
        assert_eq!(before - b.memory, 12);
        assert!(b.allocation(usize::MAX, RegexPhase::Compile).is_err());
    }
    #[test]
    fn synthetic_group_slot_order_and_limit() {
        let limit = 3;
        let mut groups = Vec::new();
        let mut b = RegexBudget::default();
        for number in 0..4 {
            syntax::push(
                &mut groups,
                syntax::Group {
                    name: None,
                    width: None,
                },
                16_384,
                &mut b,
            )
            .unwrap();
            assert_eq!(groups.len(), number + 1);
            assert_eq!(groups.len() > limit, number == 3);
        }
        assert_eq!(syntax::MAXGROUPS, 1_073_741_823);
    }
    #[test]
    fn exact_syntax_work_and_bytes_and_unicode_lookup_budget_propagation() {
        let p: Vec<u32> = "\\N{LF}".chars().map(u32::from).collect();
        let mut b = RegexBudget::default();
        check_syntax(RegexText(&p), &mut b).unwrap();
        let work = 2_000_000 - b.work;
        let memory = 8 * 1024 * 1024 - b.memory;
        for delta in [-1isize, 0, 1] {
            let mut b = RegexBudget {
                work: work.checked_add_signed(delta).unwrap(),
                ..RegexBudget::default()
            };
            assert_eq!(check_syntax(RegexText(&p), &mut b).is_ok(), delta >= 0);
            let mut b = RegexBudget {
                memory: memory.checked_add_signed(delta).unwrap(),
                ..RegexBudget::default()
            };
            assert_eq!(check_syntax(RegexText(&p), &mut b).is_ok(), delta >= 0);
        }
    }
    #[test]
    fn width_saturation_preserves_u128_sentinel_and_zero_repetition() {
        for (pattern, expected) in [
            ("(?:(?:a{4294967294}){4294967294}){2}", syntax::MAXWIDTH),
            ("(?:)*", 0),
        ] {
            let p: Vec<u32> = pattern.chars().map(u32::from).collect();
            let syntax = parser::parse_validate(
                RegexText(&p),
                &mut RegexBudget::default(),
                &mut syntax::WarningSink::default(),
            )
            .unwrap();
            assert_eq!(syntax.syntax.nodes[syntax.syntax.root].width.hi, expected);
        }
    }
}
