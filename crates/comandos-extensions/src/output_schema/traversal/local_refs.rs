//! Restricted empty-base immutable resource resolution, derived from referencing.
use super::{AbortClass, EvalFailure, GapKind, child_pointer, gap, old};
use jsonschema::Draft;
use serde_json::Value;
use std::collections::HashMap;

type Anchors<'a> = HashMap<String, (&'a Value, String, bool)>;
#[derive(Clone)]
struct Registry<'a> {
    root: &'a Value,
    specification: Draft,
    root_path: String,
    anchors: Option<Anchors<'a>>,
}
pub(super) struct LocalRefs<'a> {
    registries: Vec<Registry<'a>>,
    crawled: HashMap<usize, usize>,
}
impl<'a> LocalRefs<'a> {
    pub(super) fn new(root: &'a Value, specification: Draft) -> Result<Self, EvalFailure> {
        let result = Self {
            registries: vec![Registry {
                root,
                specification,
                root_path: String::new(),
                anchors: None,
            }],
            crawled: HashMap::new(),
        };
        result.enter(root, specification, "")?;
        Ok(result)
    }
    pub(super) fn enter(
        &self,
        schema: &Value,
        specification: Draft,
        pointer: &str,
    ) -> Result<(), EvalFailure> {
        if schema.is_boolean() {
            return Ok(());
        }
        let object = schema
            .as_object()
            .ok_or_else(|| gap(GapKind::ResourceId, pointer))?;
        if old(specification) && object.contains_key("$ref") {
            return Ok(());
        }
        let key = if specification == Draft::Draft4 {
            "id"
        } else {
            "$id"
        };
        if let Some(id) = object.get(key) {
            if id.is_null() {
                return Ok(());
            }
            let id = id
                .as_str()
                .ok_or_else(|| gap(GapKind::ResourceId, pointer))?;
            if old(specification) && id.starts_with('#') {
                return Ok(());
            }
            if !id.trim_end_matches('#').is_empty() {
                return Err(gap(GapKind::ResourceId, pointer));
            }
        }
        Ok(())
    }
    pub(super) fn lookup(
        &mut self,
        reference: &str,
        pointer: &str,
        context: usize,
    ) -> Result<(&'a Value, String, usize), EvalFailure> {
        if reference.is_empty() || reference == "#" {
            return Ok((
                self.registries[context].root,
                self.registries[context].root_path.clone(),
                context,
            ));
        }
        if let Some(fragment) = reference.strip_prefix('#') {
            if fragment.starts_with('/') {
                return self.pointer(fragment, pointer, context);
            }
            let resolved_context = self.crawl(context)?;
            let Some((schema, path, dynamic)) = self.registries[resolved_context]
                .anchors
                .as_ref()
                .unwrap()
                .get(fragment)
            else {
                return Err(EvalFailure::Abort(AbortClass::Unresolvable));
            };
            if *dynamic {
                return Err(gap(GapKind::DynamicScope, path));
            }
            return Ok((*schema, path.clone(), resolved_context));
        }
        if reference.contains('#') {
            return Err(gap(GapKind::UriJoin, pointer));
        }
        // Empty-base join and no-fragment defrag are proven identity operations.
        self.crawl(context)?;
        let provenance: Value =
            serde_json::from_str(include_str!("../metaschemas/PROVENANCE.json"))
                .map_err(|_| EvalFailure::InternalFragmentCompilation)?;
        if provenance["files"].as_array().unwrap().iter().any(|row| {
            row["uri"]
                .as_str()
                .is_some_and(|uri| uri.trim_end_matches('#') == reference)
        }) {
            return Err(gap(GapKind::ExternalResource, pointer));
        }
        Err(EvalFailure::Abort(AbortClass::Unresolvable))
    }
    fn pointer(
        &self,
        fragment: &str,
        pointer: &str,
        context: usize,
    ) -> Result<(&'a Value, String, usize), EvalFailure> {
        let decoded =
            decode_percent(&fragment[1..]).map_err(|_| gap(GapKind::PointerSemantics, pointer))?;
        let registry = &self.registries[context];
        let mut current = registry.root;
        let mut path = registry.root_path.clone();
        let mut segments: Vec<String> = Vec::new();
        for raw in decoded.split('/') {
            let segment;
            if let Some(array) = current.as_array() {
                let digits = raw.strip_prefix(['+', '-']).unwrap_or(raw);
                if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(gap(GapKind::PointerSemantics, pointer));
                }
                let index: i64 = raw
                    .parse()
                    .map_err(|_| gap(GapKind::PointerSemantics, pointer))?;
                let len = i64::try_from(array.len())
                    .map_err(|_| gap(GapKind::PointerSemantics, pointer))?;
                let index = if index < 0 {
                    len.checked_add(index)
                        .ok_or_else(|| gap(GapKind::PointerSemantics, pointer))?
                } else {
                    index
                };
                current = usize::try_from(index)
                    .ok()
                    .and_then(|i| array.get(i))
                    .ok_or(EvalFailure::Abort(AbortClass::Unresolvable))?;
                segment = index.to_string();
            } else if let Some(object) = current.as_object() {
                segment = raw.replace("~1", "/").replace("~0", "~");
                current = object
                    .get(&segment)
                    .ok_or(EvalFailure::Abort(AbortClass::Unresolvable))?;
            } else {
                return Err(gap(GapKind::PointerSemantics, pointer));
            }
            path = child_pointer(&path, &segment);
            segments.push(segment);
            if boundary(registry.specification, &segments, current) {
                self.enter(current, registry.specification, &path)?;
                // in_subresource returns the same resolver when id() is None.
                if effective_id_present(current, registry.specification) {
                    segments.clear()
                }
            }
        }
        Ok((current, path, context))
    }
    fn crawl(&mut self, context: usize) -> Result<usize, EvalFailure> {
        if self.registries[context].anchors.is_some() {
            return Ok(context);
        }
        if let Some(crawled) = self.crawled.get(&context) {
            return Ok(*crawled);
        }
        let mut registry = self.registries[context].clone();
        let mut binding_parent: Option<String> = None;
        let mut anchors = HashMap::new();
        let mut pending = vec![(
            registry.root,
            registry.specification,
            registry.root_path.clone(),
            0usize,
        )];
        let mut count = 0;
        while let Some((schema, specification, path, depth)) = pending.pop() {
            count += 1;
            if count > 100_000 || depth > 512 {
                return Err(gap(GapKind::BudgetBoundary, &path));
            }
            self.enter(schema, specification, &path)?;
            if depth > 0 && effective_id_present(schema, specification) {
                // A unique empty ID rebinds deterministically. Siblings in the
                // same source map/array have a defined order; different source
                // containers can depend on Python set iteration order.
                let parent = path
                    .rsplit_once('/')
                    .map(|(parent, _)| parent)
                    .unwrap_or("");
                if binding_parent.as_ref().is_some_and(|first| first != parent) {
                    return Err(gap(GapKind::ResourceId, &path));
                }
                binding_parent = Some(parent.into());
                registry.root = schema;
                registry.specification = specification;
                registry.root_path = path.clone();
            }
            if schema.is_boolean() {
                continue;
            }
            let object = schema
                .as_object()
                .ok_or_else(|| gap(GapKind::ResourceDialect, &path))?;
            let anchor_key = if specification == Draft::Draft4 {
                "id"
            } else if old(specification) {
                "$id"
            } else {
                "$anchor"
            };
            if let Some(value) = object.get(anchor_key) {
                let name = value
                    .as_str()
                    .ok_or_else(|| gap(GapKind::ResourceId, &path))?;
                let anchor = if old(specification) {
                    name.strip_prefix('#')
                } else {
                    Some(name)
                };
                if let Some(name) = anchor
                    && anchors
                        .insert(name.into(), (schema, path.clone(), false))
                        .is_some()
                {
                    return Err(gap(GapKind::DuplicateAnchor, &path));
                }
            }
            if specification == Draft::Draft202012
                && let Some(value) = object.get("$dynamicAnchor")
            {
                let name = value
                    .as_str()
                    .ok_or_else(|| gap(GapKind::DynamicScope, &path))?;
                if anchors
                    .insert(name.into(), (schema, path.clone(), true))
                    .is_some()
                {
                    return Err(gap(GapKind::DuplicateAnchor, &path));
                }
            }
            for (child, child_path) in subresources(schema, specification, &path)? {
                let child_spec = detect(child, specification, &child_path)?;
                pending.push((child, child_spec, child_path, depth + 1));
            }
        }
        registry.anchors = Some(anchors);
        let resolved = self.registries.len();
        self.registries.push(registry);
        self.crawled.insert(context, resolved);
        Ok(resolved)
    }
}
fn effective_id_present(schema: &Value, draft: Draft) -> bool {
    if old(draft) && schema.get("$ref").is_some() {
        return false;
    }
    let Some(value) = schema.get(if draft == Draft::Draft4 { "id" } else { "$id" }) else {
        return false;
    };
    value
        .as_str()
        .is_some_and(|id| !old(draft) || !id.starts_with('#'))
}
fn detect(schema: &Value, fallback: Draft, pointer: &str) -> Result<Draft, EvalFailure> {
    let Some(value) = schema.get("$schema") else {
        return Ok(fallback);
    };
    let uri = value
        .as_str()
        .ok_or_else(|| gap(GapKind::ResourceDialect, pointer))?
        .trim_end_matches('#');
    match uri {
        "http://json-schema.org/draft-04/schema" => Ok(Draft::Draft4),
        "http://json-schema.org/draft-06/schema" => Ok(Draft::Draft6),
        "http://json-schema.org/draft-07/schema" => Ok(Draft::Draft7),
        "https://json-schema.org/draft/2019-09/schema" => Ok(Draft::Draft201909),
        "https://json-schema.org/draft/2020-12/schema" => Ok(Draft::Draft202012),
        _ => Err(gap(GapKind::ResourceDialect, pointer)),
    }
}
fn inventory(
    draft: Draft,
) -> (
    &'static [&'static str],
    &'static [&'static str],
    &'static [&'static str],
) {
    match draft {
        Draft::Draft4 => (
            &["not"],
            &["definitions", "patternProperties", "properties"],
            &["allOf", "anyOf", "oneOf"],
        ),
        Draft::Draft6 => (
            &[
                "additionalItems",
                "additionalProperties",
                "contains",
                "not",
                "propertyNames",
            ],
            &["definitions", "patternProperties", "properties"],
            &["allOf", "anyOf", "oneOf"],
        ),
        Draft::Draft7 => (
            &[
                "additionalItems",
                "additionalProperties",
                "contains",
                "else",
                "if",
                "not",
                "propertyNames",
                "then",
            ],
            &["definitions", "patternProperties", "properties"],
            &["allOf", "anyOf", "oneOf"],
        ),
        Draft::Draft201909 => (
            &[
                "additionalItems",
                "additionalProperties",
                "contains",
                "contentSchema",
                "else",
                "if",
                "not",
                "propertyNames",
                "then",
                "unevaluatedItems",
                "unevaluatedProperties",
            ],
            &[
                "$defs",
                "definitions",
                "dependentSchemas",
                "patternProperties",
                "properties",
            ],
            &["allOf", "anyOf", "oneOf"],
        ),
        _ => (
            &[
                "additionalProperties",
                "contains",
                "contentSchema",
                "else",
                "if",
                "items",
                "not",
                "propertyNames",
                "then",
                "unevaluatedItems",
                "unevaluatedProperties",
            ],
            &[
                "$defs",
                "definitions",
                "dependentSchemas",
                "patternProperties",
                "properties",
            ],
            &["allOf", "anyOf", "oneOf", "prefixItems"],
        ),
    }
}
fn subresources<'a>(
    schema: &'a Value,
    draft: Draft,
    path: &str,
) -> Result<Vec<(&'a Value, String)>, EvalFailure> {
    let (values, maps, arrays) = inventory(draft);
    let mut children = Vec::new();
    for key in values {
        if let Some(value) = schema.get(key) {
            children.push((value, child_pointer(path, key)))
        }
    }
    for key in arrays {
        if let Some(value) = schema.get(key) {
            let array = value
                .as_array()
                .ok_or_else(|| gap(GapKind::ResourceDialect, path))?;
            for (index, child) in array.iter().enumerate() {
                children.push((
                    child,
                    child_pointer(&child_pointer(path, key), &index.to_string()),
                ))
            }
        }
    }
    for key in maps {
        if let Some(value) = schema.get(key) {
            let map = value
                .as_object()
                .ok_or_else(|| gap(GapKind::ResourceDialect, path))?;
            for (name, child) in map {
                children.push((child, child_pointer(&child_pointer(path, key), name)))
            }
        }
    }
    if draft != Draft::Draft202012
        && let Some(items) = schema.get("items").filter(|v| !v.is_null())
    {
        if let Some(array) = items.as_array() {
            for (index, child) in array.iter().enumerate() {
                children.push((
                    child,
                    child_pointer(&child_pointer(path, "items"), &index.to_string()),
                ))
            }
        } else if items.is_string() {
            return Err(gap(GapKind::ResourceDialect, path));
        } else {
            children.push((items, child_pointer(path, "items")))
        }
    }
    if old(draft)
        && let Some(dependencies) = schema.get("dependencies").filter(|v| !v.is_null())
    {
        let map = dependencies
            .as_object()
            .ok_or_else(|| gap(GapKind::ResourceDialect, path))?;
        if map.values().next().is_some_and(Value::is_object) {
            for (name, child) in map {
                children.push((
                    child,
                    child_pointer(&child_pointer(path, "dependencies"), name),
                ))
            }
        }
    }
    if draft == Draft::Draft4 {
        for key in ["additionalItems", "additionalProperties"] {
            if let Some(value) = schema.get(key).filter(|v| v.is_object()) {
                children.push((value, child_pointer(path, key)))
            }
        }
    }
    Ok(children)
}
fn boundary(draft: Draft, segments: &[String], target: &Value) -> bool {
    let (values, maps, arrays) = inventory(draft);
    let mut cursor = 0;
    while cursor < segments.len() {
        let segment = segments[cursor].as_str();
        cursor += 1;
        if draft != Draft::Draft202012
            && target.is_object()
            && (segment == "items" || old(draft) && segment == "dependencies")
        {
            return true;
        }
        let direct = values.contains(&segment)
            || draft == Draft::Draft4
                && ["additionalItems", "additionalProperties"].contains(&segment);
        if !direct {
            if !(maps.contains(&segment) || arrays.contains(&segment)) || cursor == segments.len() {
                return false;
            }
            cursor += 1;
        }
    }
    true
}
fn decode_percent(input: &str) -> Result<String, ()> {
    let bytes = input.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let digit = |byte: u8| (byte as char).to_digit(16).map(|n| n as u8);
            if let (Some(high), Some(low)) = (digit(bytes[index + 1]), digit(bytes[index + 2])) {
                output.push(high * 16 + low);
                index += 3;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(output).map_err(|_| ())
}
