//! Reserialización de las respuestas MCP tal como las emite el proxy Python.
//!
//! El proxy Python (`lib/extension_proxy.py`, SDK `mcp` 1.30.0) valida cada resultado del
//! upstream con su modelo pydantic y lo reescribe con
//! `model_dump_json(by_alias=True, exclude_none=True)`. Eso implica, por nivel de modelo:
//! campos declarados en su orden de declaración, campos extra (todos los modelos son
//! `extra="allow"`) al final en su orden original, `null` omitido solo en campos del modelo y
//! en extras de ese nivel (nunca dentro de `dict[str, Any]` como `inputSchema`), valores por
//! omisión no nulos (`CallToolResult.isError=False`), `AnyUrl` normalizado, `float` tipado
//! (`Annotations.priority`) y todos los números decimales reformateados por pydantic-core.
//! Un resultado que no valida se convierte en el `-32603` del Python.
//!
//! Las listas de campos salen de `mcp/types.py` del venv de extensiones (introspección de
//! `model_fields` de cada clase; ver el informe de la Tarea 4).
use serde_json::{Map, Number, Value, json};

/// `mcp/shared/version.py::SUPPORTED_PROTOCOL_VERSIONS` del SDK 1.30.0.
pub const SUPPORTED_PROTOCOL_VERSIONS: [&str; 4] = [
    "2024-11-05",
    "2025-03-26",
    "2025-06-18",
    LATEST_PROTOCOL_VERSION,
];
/// `mcp/types.py::LATEST_PROTOCOL_VERSION`: lo que el cliente Python pide al upstream y lo
/// que su servidor contesta si el cliente pide una versión que no soporta.
pub const LATEST_PROTOCOL_VERSION: &str = "2025-11-25";

/// El resultado no cumple el modelo pydantic (el `ValidationError` del Python).
#[derive(Debug, PartialEq)]
pub struct ValidationError;

#[derive(Clone, Copy)]
enum Kind {
    /// `Any`, `int`, `bool`, `list[str]`, `Literal` sin discriminar: valor tal cual.
    Any,
    /// `str`: debe ser cadena.
    Str,
    /// `dict[str, Any]`: debe ser objeto; su interior no se toca (salvo números).
    Dict,
    /// `float` tipado: un entero se emite como decimal (`1` → `1.0`).
    Float,
    /// `AnyUrl`: normalizado como pydantic-core (crate `url`).
    Url,
    /// `Literal[...]` discriminante: debe ser una de las cadenas.
    Literal(&'static [&'static str]),
    Model(&'static Model),
    List(&'static Model),
    /// Unión pydantic: primera variante que valida, en orden de declaración.
    Union(&'static [&'static Model]),
    ListUnion(&'static [&'static Model]),
}

#[derive(Clone, Copy)]
enum Presence {
    /// Sin valor por omisión: ausente o `null` no valida.
    Required,
    /// `X | None = None`: ausente o `null` se omite.
    Optional,
    /// `bool = False`: ausente se emite `false`; `null` no valida.
    DefaultFalse,
}

struct Field {
    key: &'static str,
    kind: Kind,
    presence: Presence,
}

const fn req(key: &'static str, kind: Kind) -> Field {
    Field {
        key,
        kind,
        presence: Presence::Required,
    }
}
const fn opt(key: &'static str, kind: Kind) -> Field {
    Field {
        key,
        kind,
        presence: Presence::Optional,
    }
}

struct Model {
    fields: &'static [Field],
}

const META: Field = opt("_meta", Kind::Dict);
const NEXT_CURSOR: Field = opt("nextCursor", Kind::Str);

// Modelos anidados (`mcp/types.py`, en orden de declaración de campos).
static ICON: Model = Model {
    fields: &[
        req("src", Kind::Str),
        opt("mimeType", Kind::Str),
        opt("sizes", Kind::Any),
    ],
};
static ANNOTATIONS: Model = Model {
    fields: &[opt("audience", Kind::Any), opt("priority", Kind::Float)],
};
static TOOL_ANNOTATIONS: Model = Model {
    fields: &[
        opt("title", Kind::Str),
        opt("readOnlyHint", Kind::Any),
        opt("destructiveHint", Kind::Any),
        opt("idempotentHint", Kind::Any),
        opt("openWorldHint", Kind::Any),
    ],
};
static TOOL_EXECUTION: Model = Model {
    fields: &[opt("taskSupport", Kind::Any)],
};
static TOOL: Model = Model {
    fields: &[
        req("name", Kind::Str),
        opt("title", Kind::Str),
        opt("description", Kind::Str),
        req("inputSchema", Kind::Dict),
        opt("outputSchema", Kind::Dict),
        opt("icons", Kind::List(&ICON)),
        opt("annotations", Kind::Model(&TOOL_ANNOTATIONS)),
        META,
        opt("execution", Kind::Model(&TOOL_EXECUTION)),
    ],
};
static TEXT_RESOURCE_CONTENTS: Model = Model {
    fields: &[
        req("uri", Kind::Url),
        opt("mimeType", Kind::Str),
        META,
        req("text", Kind::Str),
    ],
};
static BLOB_RESOURCE_CONTENTS: Model = Model {
    fields: &[
        req("uri", Kind::Url),
        opt("mimeType", Kind::Str),
        META,
        req("blob", Kind::Str),
    ],
};
static RESOURCE_CONTENTS: [&Model; 2] = [&TEXT_RESOURCE_CONTENTS, &BLOB_RESOURCE_CONTENTS];
static TEXT_CONTENT: Model = Model {
    fields: &[
        req("type", Kind::Literal(&["text"])),
        req("text", Kind::Str),
        opt("annotations", Kind::Model(&ANNOTATIONS)),
        META,
    ],
};
static IMAGE_CONTENT: Model = Model {
    fields: &[
        req("type", Kind::Literal(&["image"])),
        req("data", Kind::Str),
        req("mimeType", Kind::Str),
        opt("annotations", Kind::Model(&ANNOTATIONS)),
        META,
    ],
};
static AUDIO_CONTENT: Model = Model {
    fields: &[
        req("type", Kind::Literal(&["audio"])),
        req("data", Kind::Str),
        req("mimeType", Kind::Str),
        opt("annotations", Kind::Model(&ANNOTATIONS)),
        META,
    ],
};
static RESOURCE_LINK: Model = Model {
    fields: &[
        req("name", Kind::Str),
        opt("title", Kind::Str),
        req("uri", Kind::Url),
        opt("description", Kind::Str),
        opt("mimeType", Kind::Str),
        opt("size", Kind::Any),
        opt("icons", Kind::List(&ICON)),
        opt("annotations", Kind::Model(&ANNOTATIONS)),
        META,
        req("type", Kind::Literal(&["resource_link"])),
    ],
};
static EMBEDDED_RESOURCE: Model = Model {
    fields: &[
        req("type", Kind::Literal(&["resource"])),
        req("resource", Kind::Union(&RESOURCE_CONTENTS)),
        opt("annotations", Kind::Model(&ANNOTATIONS)),
        META,
    ],
};
/// `ContentBlock`.
static CONTENT_BLOCK: [&Model; 5] = [
    &TEXT_CONTENT,
    &IMAGE_CONTENT,
    &AUDIO_CONTENT,
    &RESOURCE_LINK,
    &EMBEDDED_RESOURCE,
];
static RESOURCE: Model = Model {
    fields: &[
        req("name", Kind::Str),
        opt("title", Kind::Str),
        req("uri", Kind::Url),
        opt("description", Kind::Str),
        opt("mimeType", Kind::Str),
        opt("size", Kind::Any),
        opt("icons", Kind::List(&ICON)),
        opt("annotations", Kind::Model(&ANNOTATIONS)),
        META,
    ],
};
static RESOURCE_TEMPLATE: Model = Model {
    fields: &[
        req("name", Kind::Str),
        opt("title", Kind::Str),
        req("uriTemplate", Kind::Str),
        opt("description", Kind::Str),
        opt("mimeType", Kind::Str),
        opt("icons", Kind::List(&ICON)),
        opt("annotations", Kind::Model(&ANNOTATIONS)),
        META,
    ],
};
static PROMPT_ARGUMENT: Model = Model {
    fields: &[
        req("name", Kind::Str),
        opt("description", Kind::Str),
        opt("required", Kind::Any),
    ],
};
static PROMPT: Model = Model {
    fields: &[
        req("name", Kind::Str),
        opt("title", Kind::Str),
        opt("description", Kind::Str),
        opt("arguments", Kind::List(&PROMPT_ARGUMENT)),
        opt("icons", Kind::List(&ICON)),
        META,
    ],
};
static PROMPT_MESSAGE: Model = Model {
    fields: &[
        req("role", Kind::Literal(&["user", "assistant"])),
        req("content", Kind::Union(&CONTENT_BLOCK)),
    ],
};
static COMPLETION: Model = Model {
    fields: &[
        req("values", Kind::Any),
        opt("total", Kind::Any),
        opt("hasMore", Kind::Any),
    ],
};

// Resultados por método reenviado.
static LIST_TOOLS_RESULT: Model = Model {
    fields: &[META, NEXT_CURSOR, req("tools", Kind::List(&TOOL))],
};
static CALL_TOOL_RESULT: Model = Model {
    fields: &[
        META,
        req("content", Kind::ListUnion(&CONTENT_BLOCK)),
        opt("structuredContent", Kind::Dict),
        Field {
            key: "isError",
            kind: Kind::Any,
            presence: Presence::DefaultFalse,
        },
    ],
};
static LIST_RESOURCES_RESULT: Model = Model {
    fields: &[META, NEXT_CURSOR, req("resources", Kind::List(&RESOURCE))],
};
static LIST_RESOURCE_TEMPLATES_RESULT: Model = Model {
    fields: &[
        META,
        NEXT_CURSOR,
        req("resourceTemplates", Kind::List(&RESOURCE_TEMPLATE)),
    ],
};
static READ_RESOURCE_RESULT: Model = Model {
    fields: &[META, req("contents", Kind::ListUnion(&RESOURCE_CONTENTS))],
};
static LIST_PROMPTS_RESULT: Model = Model {
    fields: &[META, NEXT_CURSOR, req("prompts", Kind::List(&PROMPT))],
};
static GET_PROMPT_RESULT: Model = Model {
    fields: &[
        META,
        opt("description", Kind::Str),
        req("messages", Kind::List(&PROMPT_MESSAGE)),
    ],
};
static COMPLETE_RESULT: Model = Model {
    fields: &[META, req("completion", Kind::Model(&COMPLETION))],
};
static ERROR_DATA: Model = Model {
    fields: &[
        req("code", Kind::Any),
        req("message", Kind::Str),
        opt("data", Kind::Any),
    ],
};
/// `CompletionsCapability`: sin campos declarados, solo extras.
static COMPLETIONS_CAPABILITY: Model = Model { fields: &[] };

fn result_model(method: &str) -> Option<&'static Model> {
    Some(match method {
        "tools/list" => &LIST_TOOLS_RESULT,
        "tools/call" => &CALL_TOOL_RESULT,
        "resources/list" => &LIST_RESOURCES_RESULT,
        "resources/templates/list" => &LIST_RESOURCE_TEMPLATES_RESULT,
        "resources/read" => &READ_RESOURCE_RESULT,
        "prompts/list" => &LIST_PROMPTS_RESULT,
        "prompts/get" => &GET_PROMPT_RESULT,
        "completion/complete" => &COMPLETE_RESULT,
        _ => return None,
    })
}

fn convert(kind: Kind, value: Value) -> Result<Value, ValidationError> {
    match kind {
        Kind::Any => Ok(value),
        Kind::Str => value.is_string().then_some(value).ok_or(ValidationError),
        Kind::Dict => value.is_object().then_some(value).ok_or(ValidationError),
        Kind::Float => match &value {
            Value::Number(number) => number
                .as_str()
                .parse::<f64>()
                .ok()
                .and_then(Number::from_f64)
                .map(Value::Number)
                .ok_or(ValidationError),
            _ => Ok(value),
        },
        Kind::Url => {
            let text = value.as_str().ok_or(ValidationError)?;
            let url = url::Url::parse(text).map_err(|_| ValidationError)?;
            Ok(Value::String(url.into()))
        }
        Kind::Literal(allowed) => value
            .as_str()
            .is_some_and(|text| allowed.contains(&text))
            .then_some(value)
            .ok_or(ValidationError),
        Kind::Model(model) => normalize_model(model, value),
        Kind::List(model) => list(value, |item| normalize_model(model, item)),
        Kind::Union(models) => union(models, value),
        Kind::ListUnion(models) => list(value, |item| union(models, item)),
    }
}

fn list(
    value: Value,
    item: impl Fn(Value) -> Result<Value, ValidationError>,
) -> Result<Value, ValidationError> {
    let Value::Array(items) = value else {
        return Err(ValidationError);
    };
    items
        .into_iter()
        .map(item)
        .collect::<Result<_, _>>()
        .map(Value::Array)
}

fn union(models: &[&Model], value: Value) -> Result<Value, ValidationError> {
    models
        .iter()
        .find_map(|model| normalize_model(model, value.clone()).ok())
        .ok_or(ValidationError)
}

fn normalize_model(model: &Model, value: Value) -> Result<Value, ValidationError> {
    let Value::Object(mut input) = value else {
        return Err(ValidationError);
    };
    let mut out = Map::new();
    for field in model.fields {
        match (input.shift_remove(field.key), field.presence) {
            (None | Some(Value::Null), Presence::Optional) => {}
            (None, Presence::DefaultFalse) => {
                out.insert(field.key.into(), Value::Bool(false));
            }
            (None | Some(Value::Null), _) => return Err(ValidationError),
            (Some(value), _) => {
                out.insert(field.key.into(), convert(field.kind, value)?);
            }
        }
    }
    out.extend(input.into_iter().filter(|(_, value)| !value.is_null()));
    Ok(Value::Object(out))
}

/// Números como los reemite pydantic-core: los decimales pasan por `f64` y se escriben con
/// el formato corto de `serde_json`, que coincide con el pydantic-core 2.46 del venv
/// (`1.50` → `1.5`, `1e5` → `100000.0`, `1e16` → `1e+16`, `1e-7` → `1e-7`);
/// los no finitos salen `null` y `-0` entero sale `0`. Los enteros grandes se conservan.
fn python_numbers(value: &mut Value) {
    match value {
        Value::Number(number) => {
            let raw = number.as_str();
            if raw.contains(['.', 'e', 'E']) {
                *value = raw
                    .parse::<f64>()
                    .ok()
                    .and_then(Number::from_f64)
                    .map_or(Value::Null, Value::Number);
            } else if raw == "-0" {
                *value = json!(0);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(python_numbers),
        Value::Object(map) => map.values_mut().for_each(python_numbers),
        _ => {}
    }
}

/// Resultado de un método reenviado, reescrito como el proxy Python.
pub fn result(method: &str, value: Value) -> Result<Value, ValidationError> {
    let mut value = match result_model(method) {
        Some(model) => normalize_model(model, value)?,
        None => value,
    };
    python_numbers(&mut value);
    Ok(value)
}

/// Objeto `error` de una respuesta del upstream (`ErrorData`). Si no valida se deja igual.
pub fn error(value: Value) -> Value {
    let mut value = normalize_model(&ERROR_DATA, value.clone()).unwrap_or(value);
    python_numbers(&mut value);
    value
}

/// Versión que contesta el servidor Python: la pedida si la soporta, si no la última.
pub fn negotiated_version(requested: &Value) -> &str {
    requested
        .as_str()
        .and_then(|v| SUPPORTED_PROTOCOL_VERSIONS.iter().find(|s| **s == v))
        .copied()
        .unwrap_or(LATEST_PROTOCOL_VERSION)
}

/// `InitializeResult` hacia el cliente (`exclude_none`): solo las capacidades que el proxy
/// Python reexpone, en el orden de `ServerCapabilities`, e `instructions` solo si es cadena.
pub fn initialize_result(name: &str, version: &str, upstream: &Value) -> Value {
    let mut capabilities = Map::new();
    for key in ["prompts", "resources", "tools", "completions"] {
        let Some(cap) = upstream["capabilities"].get(key).filter(|v| !v.is_null()) else {
            continue;
        };
        let value = if key == "completions" {
            normalize_model(&COMPLETIONS_CAPABILITY, cap.clone()).unwrap_or_else(|_| cap.clone())
        } else {
            json!({})
        };
        capabilities.insert(key.into(), value);
    }
    let mut result = json!({
        "protocolVersion": version,
        "capabilities": capabilities,
        "serverInfo": {"name": format!("comandos-{name}"), "version": "1"}
    });
    if let Some(instructions) = upstream.get("instructions").filter(|v| v.is_string()) {
        result["instructions"] = instructions.clone();
    }
    python_numbers(&mut result);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn normalized(method: &str, raw: &str) -> String {
        result(method, serde_json::from_str(raw).unwrap())
            .unwrap()
            .to_string()
    }

    // Las salidas esperadas se obtuvieron con el python del venv de extensiones (`mcp`
    // 1.30.0, pydantic 2.13.5, pydantic-core 2.46.5), por el mismo camino que el proxy:
    // `JSONRPCMessage.model_validate_json`, `<Modelo>.model_validate(result)` y
    // `model_dump_json(by_alias=True, exclude_none=True)`.
    #[test]
    fn call_tool_result_strips_nulls_per_model_level() {
        assert_eq!(
            normalized(
                "tools/call",
                r#"{"content":[{"type":"text","text":"x","annotations":null,"_meta":null,"zz":null}],"structuredContent":{"a":null}}"#
            ),
            r#"{"content":[{"type":"text","text":"x"}],"structuredContent":{"a":null},"isError":false}"#
        );
        assert_eq!(
            normalized(
                "tools/call",
                r#"{"zz":1,"content":[{"type":"text","text":"x"}],"structuredContent":null,"_meta":null,"aa":null}"#
            ),
            r#"{"content":[{"type":"text","text":"x"}],"isError":false,"zz":1}"#
        );
        assert_eq!(
            normalized(
                "tools/call",
                r#"{"isError":true,"zz":1,"content":[],"_meta":{"k":1},"structuredContent":{"b":1}}"#
            ),
            r#"{"_meta":{"k":1},"content":[],"structuredContent":{"b":1},"isError":true,"zz":1}"#
        );
    }

    #[test]
    fn list_tools_result_keeps_schema_nulls() {
        assert_eq!(
            normalized(
                "tools/list",
                r#"{"tools":[{"inputSchema":{"type":"object","properties":{"a":{"default":null}}},"name":"a","title":null,"annotations":null}]}"#
            ),
            r#"{"tools":[{"name":"a","inputSchema":{"type":"object","properties":{"a":{"default":null}}}}]}"#
        );
    }

    #[test]
    fn nested_models_follow_declaration_order() {
        assert_eq!(
            normalized(
                "tools/call",
                r#"{"content":[{"type":"resource_link","uri":"https://example.com","name":"r","size":null},{"resource":{"blob":"QQ==","uri":"file:///a","mimeType":null},"type":"resource","annotations":{"priority":1,"audience":null}}]}"#
            ),
            r#"{"content":[{"name":"r","uri":"https://example.com/","type":"resource_link"},{"type":"resource","resource":{"uri":"file:///a","blob":"QQ=="},"annotations":{"priority":1.0}}],"isError":false}"#
        );
        assert_eq!(
            normalized(
                "prompts/get",
                r#"{"messages":[{"content":{"text":"t","type":"text","annotations":null},"role":"user"}],"description":null}"#
            ),
            r#"{"messages":[{"role":"user","content":{"type":"text","text":"t"}}]}"#
        );
        assert_eq!(
            normalized(
                "resources/list",
                r#"{"resources":[{"uri":"HTTPS://Example.COM/a/../b","name":"n","title":null,"icons":[{"sizes":null,"src":"i"}]}],"nextCursor":null}"#
            ),
            r#"{"resources":[{"name":"n","uri":"https://example.com/b","icons":[{"src":"i"}]}]}"#
        );
    }

    #[test]
    fn numbers_match_pydantic_core() {
        assert_eq!(
            normalized(
                "tools/list",
                r#"{"tools":[{"name":"a","inputSchema":{"m":1.50,"e":1e5,"big":100000000000000000000000,"neg":-0.0,"f":0.1,"i":3,"a":1e16,"b":1e-7,"c":1.5e300,"d":123456789012345678.0,"g":2.5E3,"h":-0,"j":1E+2,"k":0.000001,"l":123456789.123456789,"inf":-1e400}}]}"#
            ),
            r#"{"tools":[{"name":"a","inputSchema":{"m":1.5,"e":100000.0,"big":100000000000000000000000,"neg":-0.0,"f":0.1,"i":3,"a":1e+16,"b":1e-7,"c":1.5e+300,"d":1.2345678901234568e+17,"g":2500.0,"h":0,"j":100.0,"k":1e-6,"l":123456789.12345679,"inf":null}}]}"#
        );
    }

    #[test]
    fn invalid_results_are_validation_errors() {
        for (method, raw) in [
            ("tools/call", r#"{"isError":true}"#),
            ("tools/call", r#"{"content":[],"isError":null}"#),
            ("tools/call", r#"{"content":[{"type":"video"}]}"#),
            ("tools/list", r#"{"tools":[{"name":"a"}]}"#),
            ("tools/list", r#"{"tools":[{"name":1,"inputSchema":{}}]}"#),
            (
                "resources/list",
                r#"{"resources":[{"name":"n","uri":"sin esquema"}]}"#,
            ),
            (
                "prompts/get",
                r#"{"messages":[{"role":"system","content":{"type":"text","text":"t"}}]}"#,
            ),
        ] {
            assert_eq!(
                result(method, serde_json::from_str(raw).unwrap()),
                Err(ValidationError),
                "{method} {raw}"
            );
        }
    }

    #[test]
    fn initialize_matches_python_server() {
        assert_eq!(negotiated_version(&json!("2024-11-05")), "2024-11-05");
        assert_eq!(
            negotiated_version(&json!("1999-01-01")),
            LATEST_PROTOCOL_VERSION
        );
        assert_eq!(negotiated_version(&json!(5)), LATEST_PROTOCOL_VERSION);
        let upstream = json!({"protocolVersion":"2025-11-25","instructions":null,
            "capabilities":{"tools":{"listChanged":true},"completions":{"x":null,"y":1},"logging":{}}});
        assert_eq!(
            initialize_result("fake", "2025-06-18", &upstream).to_string(),
            r#"{"protocolVersion":"2025-06-18","capabilities":{"tools":{},"completions":{"y":1}},"serverInfo":{"name":"comandos-fake","version":"1"}}"#
        );
    }
}
