//! Private CPython numeric semantics over immutable JSON tokens.
//! Source ports and notice mapping: numbers/PROVENANCE.json.
use super::{AbortClass, EvalFailure, GapKind, Validity, gap};
use jsonschema::Draft;
use num_bigint::BigInt;
use num_traits::{ToPrimitive, Zero};
use serde_json::Value;
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
};

// Captured from the installed oracle; this primitive guard is not a complete
// document parse proof (in particular, overwritten duplicate values are lost).
const INTEGER_DIGIT_LIMIT: usize = 4300;
const WORK_LIMIT: usize = 100_000;

#[derive(Clone, Debug)]
pub(super) enum PythonNumber {
    Integer(BigInt),
    Float(f64),
}
impl PythonNumber {
    pub(super) fn parse(token: &str) -> Result<Self, AbortClass> {
        if token.contains(['.', 'e', 'E']) {
            // No decimal scaling: even a gigantic exponent costs token length.
            token
                .parse::<f64>()
                .map(Self::Float)
                .map_err(|_| AbortClass::ValueError)
        } else {
            if token.trim_start_matches('-').len() > INTEGER_DIGIT_LIMIT {
                return Err(AbortClass::ValueError);
            }
            BigInt::parse_bytes(token.as_bytes(), 10)
                .map(Self::Integer)
                .ok_or(AbortClass::ValueError)
        }
    }
    pub(super) fn to_float(&self) -> Result<f64, AbortClass> {
        match self {
            Self::Float(f) => Ok(*f),
            Self::Integer(n) => n
                .to_f64()
                .filter(|f| f.is_finite())
                .ok_or(AbortClass::OverflowError),
        }
    }
    fn ratio(&self) -> Result<(BigInt, BigInt), AbortClass> {
        match self {
            Self::Integer(n) => Ok((n.clone(), BigInt::from(1))),
            Self::Float(f) if f.is_nan() => Err(AbortClass::ValueError),
            Self::Float(f) if f.is_infinite() => Err(AbortClass::OverflowError),
            Self::Float(f) => {
                let bits = f.to_bits();
                let exponent = ((bits >> 52) & 0x7ff) as i32;
                let significand = bits & ((1 << 52) - 1);
                let (significand, shift) = if exponent == 0 {
                    (significand, -1074)
                } else {
                    (significand | (1 << 52), exponent - 1023 - 52)
                };
                let mut numerator = BigInt::from(significand);
                if f.is_sign_negative() {
                    numerator = -numerator;
                }
                Ok(if shift >= 0 {
                    (numerator << shift as usize, BigInt::from(1))
                } else {
                    (numerator, BigInt::from(1) << (-shift) as usize)
                })
            }
        }
    }
    pub(super) fn compare(&self, other: &Self) -> Option<Ordering> {
        match (self, other) {
            (Self::Float(a), Self::Float(b)) => a.partial_cmp(b),
            (Self::Integer(a), Self::Integer(b)) => Some(a.cmp(b)),
            (Self::Integer(_), Self::Float(b)) if b.is_nan() => None,
            (Self::Integer(_), Self::Float(b)) if b.is_infinite() => {
                Some(if b.is_sign_positive() {
                    Ordering::Less
                } else {
                    Ordering::Greater
                })
            }
            (Self::Float(_), Self::Integer(_)) => other.compare(self).map(Ordering::reverse),
            _ => {
                let (a, ad) = self.ratio().expect("finite comparison operand");
                let (b, bd) = other.ratio().expect("finite comparison operand");
                Some((a * bd).cmp(&(b * ad)))
            }
        }
    }
    fn integer_type(&self, draft: Draft) -> bool {
        match self {
            Self::Integer(_) => true,
            Self::Float(f) => draft != Draft::Draft4 && f.is_finite() && f.fract() == 0.0,
        }
    }
    pub(super) fn multiple_of(&self, divisor: &Self) -> Result<bool, AbortClass> {
        match divisor {
            Self::Float(_) => {
                // Python evaluates conversions/division before the recovery block.
                let numerator = self.to_float()?;
                let denominator = divisor.to_float()?;
                if denominator == 0.0 {
                    return Err(AbortClass::ZeroDivisionError);
                }
                let quotient = numerator / denominator;
                if quotient.is_nan() {
                    return Err(AbortClass::ValueError);
                }
                if quotient.is_finite() {
                    return Ok(quotient.trunc() == quotient);
                }
                // Only int(infinite quotient)'s OverflowError enters this fallback.
                let (a, ad) = self.ratio()?;
                let (b, bd) = divisor.ratio()?;
                if b.is_zero() {
                    return Err(AbortClass::ZeroDivisionError);
                }
                Ok(((a * bd) % (ad * b)).is_zero())
            }
            Self::Integer(b) => match self {
                Self::Integer(a) => {
                    if b.is_zero() {
                        return Err(AbortClass::ZeroDivisionError);
                    }
                    Ok((a % b).is_zero())
                }
                Self::Float(a) => {
                    let denominator = divisor.to_float()?;
                    if denominator == 0.0 {
                        return Err(AbortClass::ZeroDivisionError);
                    }
                    let mut remainder = a % denominator;
                    if remainder != 0.0 {
                        if remainder.is_sign_negative() != denominator.is_sign_negative() {
                            remainder += denominator;
                        }
                    } else {
                        remainder = 0.0_f64.copysign(denominator);
                    }
                    // NaN remainder is truthy in Python: ordinary Invalid.
                    Ok(remainder == 0.0)
                }
            },
        }
    }
}

#[derive(Default)]
pub(super) struct Numbers {
    cache: HashMap<usize, PythonNumber>,
    work: usize,
}
impl Numbers {
    fn view(&mut self, value: &Value, pointer: &str) -> Result<PythonNumber, EvalFailure> {
        let number = value
            .as_number()
            .ok_or_else(|| gap(GapKind::Numeric, pointer))?;
        let address = number as *const serde_json::Number as usize;
        if let Some(number) = self.cache.get(&address) {
            return Ok(number.clone());
        }
        let parsed = PythonNumber::parse(number.as_str()).map_err(EvalFailure::Abort)?;
        self.cache.insert(address, parsed.clone());
        Ok(parsed)
    }
    fn step(&mut self, depth: usize, pointer: &str) -> Result<(), EvalFailure> {
        self.work += 1;
        if depth >= 128 || self.work > WORK_LIMIT {
            return Err(gap(GapKind::BudgetBoundary, pointer));
        }
        Ok(())
    }
    pub(super) fn equal(
        &mut self,
        a: &Value,
        b: &Value,
        pointer: &str,
    ) -> Result<bool, EvalFailure> {
        self.equal_at(a, b, 0, pointer)
    }
    fn equal_at(
        &mut self,
        a: &Value,
        b: &Value,
        depth: usize,
        pointer: &str,
    ) -> Result<bool, EvalFailure> {
        self.step(depth, pointer)?;
        Ok(match (a, b) {
            (Value::Number(_), Value::Number(_)) => {
                self.view(a, pointer)?.compare(&self.view(b, pointer)?) == Some(Ordering::Equal)
            }
            (Value::Array(a), Value::Array(b)) => {
                if a.len() != b.len() {
                    return Ok(false);
                }
                for (a, b) in a.iter().zip(b) {
                    if !self.equal_at(a, b, depth + 1, pointer)? {
                        return Ok(false);
                    }
                }
                true
            }
            (Value::Object(a), Value::Object(b)) => {
                if a.len() != b.len() {
                    return Ok(false);
                }
                for (key, a) in a {
                    let Some(b) = b.get(key) else {
                        return Ok(false);
                    };
                    if !self.equal_at(a, b, depth + 1, pointer)? {
                        return Ok(false);
                    }
                }
                true
            }
            _ => a == b,
        })
    }
    pub(super) fn unique(&mut self, array: &[Value], pointer: &str) -> Result<bool, EvalFailure> {
        for (i, a) in array.iter().enumerate() {
            for b in &array[..i] {
                if self.equal(a, b, pointer)? {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
    pub(super) fn count_compare(
        &mut self,
        count: usize,
        parameter: &Value,
        pointer: &str,
    ) -> Result<Option<Ordering>, EvalFailure> {
        let parameter = self.view(parameter, pointer)?;
        Ok(PythonNumber::Integer(BigInt::from(count)).compare(&parameter))
    }
    pub(super) fn handler(
        &mut self,
        schema: &Value,
        key: &str,
        value: &Value,
        instance: &Value,
        draft: Draft,
        pointer: &str,
    ) -> Option<super::Evaluation> {
        let result = self.assertion(schema, key, value, instance, draft, pointer);
        result.map(|result| {
            result.map(|valid| {
                if valid {
                    Validity::Valid
                } else {
                    Validity::Invalid
                }
            })
        })
    }
    fn assertion(
        &mut self,
        schema: &Value,
        key: &str,
        value: &Value,
        instance: &Value,
        draft: Draft,
        pointer: &str,
    ) -> Option<Result<bool, EvalFailure>> {
        let mut result = || -> Result<bool, EvalFailure> {
            match key {
                "type" => {
                    let types: Vec<&str> = if let Some(t) = value.as_str() {
                        vec![t]
                    } else if let Some(a) = value.as_array() {
                        a.iter()
                            .map(Value::as_str)
                            .collect::<Option<_>>()
                            .ok_or_else(|| gap(GapKind::FragmentShape, pointer))?
                    } else {
                        return Err(gap(GapKind::FragmentShape, pointer));
                    };
                    if types.iter().copied().collect::<HashSet<_>>().len() != types.len() {
                        // Keep the reviewed gap for uncertified literal-pointer
                        // numeric duplicates; never normalize the original array.
                        let kind = if types.iter().any(|t| matches!(*t, "number" | "integer")) {
                            GapKind::Numeric
                        } else {
                            GapKind::FragmentShape
                        };
                        return Err(gap(kind, pointer));
                    }
                    if types.is_empty()
                        || types.iter().any(|t| {
                            !matches!(
                                *t,
                                "integer"
                                    | "number"
                                    | "object"
                                    | "array"
                                    | "string"
                                    | "boolean"
                                    | "null"
                            )
                        })
                    {
                        return Err(gap(GapKind::FragmentShape, pointer));
                    }
                    for t in types {
                        let valid = match t {
                            "number" => instance.is_number(),
                            "integer" => {
                                instance.is_number()
                                    && self.view(instance, pointer)?.integer_type(draft)
                            }
                            "object" => instance.is_object(),
                            "array" => instance.is_array(),
                            "string" => instance.is_string(),
                            "boolean" => instance.is_boolean(),
                            "null" => instance.is_null(),
                            _ => unreachable!(),
                        };
                        if valid {
                            return Ok(true);
                        }
                    }
                    Ok(false)
                }
                "minimum" | "maximum" | "exclusiveMinimum" | "exclusiveMaximum" | "multipleOf" => {
                    if !instance.is_number() {
                        return Ok(true);
                    }
                    let instance = self.view(instance, pointer)?;
                    let parameter = self.view(value, pointer)?;
                    if key == "multipleOf" {
                        return instance.multiple_of(&parameter).map_err(EvalFailure::Abort);
                    }
                    let exclusive = draft == Draft::Draft4
                        && match key {
                            "minimum" => schema.get("exclusiveMinimum"),
                            "maximum" => schema.get("exclusiveMaximum"),
                            _ => None,
                        }
                        .is_some_and(|v| v == true);
                    if draft == Draft::Draft4 {
                        let sibling = match key {
                            "minimum" => schema.get("exclusiveMinimum"),
                            "maximum" => schema.get("exclusiveMaximum"),
                            _ => None,
                        };
                        if sibling.is_some_and(|v| !v.is_boolean()) {
                            return Err(gap(GapKind::FragmentShape, pointer));
                        }
                    }
                    let comparison = instance.compare(&parameter);
                    let failed = match key {
                        "minimum" if !exclusive => comparison == Some(Ordering::Less),
                        "maximum" if !exclusive => comparison == Some(Ordering::Greater),
                        "minimum" | "exclusiveMinimum" => {
                            matches!(comparison, Some(Ordering::Less | Ordering::Equal))
                        }
                        _ => matches!(comparison, Some(Ordering::Greater | Ordering::Equal)),
                    };
                    Ok(!failed)
                }
                "const" => self.equal(instance, value, pointer),
                "enum" => {
                    let candidates = value
                        .as_array()
                        .ok_or_else(|| gap(GapKind::FragmentShape, pointer))?;
                    for candidate in candidates {
                        if self.equal(instance, candidate, pointer)? {
                            return Ok(true);
                        }
                    }
                    Ok(false)
                }
                "uniqueItems" => {
                    let Some(array) = instance.as_array() else {
                        return Ok(true);
                    };
                    let enabled = value
                        .as_bool()
                        .ok_or_else(|| gap(GapKind::FragmentShape, pointer))?;
                    if enabled {
                        self.unique(array, pointer)
                    } else {
                        Ok(true)
                    }
                }
                _ => {
                    let count = match key {
                        "minLength" | "maxLength" => instance.as_str().map(|s| s.chars().count()),
                        "minItems" | "maxItems" => instance.as_array().map(Vec::len),
                        "minProperties" | "maxProperties" => {
                            instance.as_object().map(serde_json::Map::len)
                        }
                        _ => unreachable!(),
                    };
                    let Some(count) = count else { return Ok(true) };
                    let parameter = self.view(value, pointer)?;
                    if !parameter.integer_type(Draft::Draft202012)
                        || parameter.compare(&PythonNumber::Integer(BigInt::from(0)))
                            == Some(Ordering::Less)
                    {
                        return Err(gap(GapKind::Numeric, pointer));
                    }
                    let comparison = PythonNumber::Integer(BigInt::from(count)).compare(&parameter);
                    Ok(if key.starts_with("min") {
                        comparison != Some(Ordering::Less)
                    } else {
                        comparison != Some(Ordering::Greater)
                    })
                }
            }
        };
        if matches!(
            key,
            "type"
                | "minimum"
                | "maximum"
                | "exclusiveMinimum"
                | "exclusiveMaximum"
                | "multipleOf"
                | "const"
                | "enum"
                | "uniqueItems"
                | "minLength"
                | "maxLength"
                | "minItems"
                | "maxItems"
                | "minProperties"
                | "maxProperties"
        ) {
            Some(result())
        } else {
            None
        }
    }
}

#[cfg(test)]
pub(super) fn primitive_probe(row: &Value) -> Value {
    let run = || -> Result<Value, EvalFailure> {
        let a: Value = serde_json::from_str(row["a_json"].as_str().unwrap()).unwrap();
        let b: Option<Value> = row
            .get("b_json")
            .map(|v| serde_json::from_str(v.as_str().unwrap()).unwrap());
        let mut numbers = Numbers::default();
        let op = row["op"].as_str().unwrap();
        let mut result = serde_json::json!({"result":"Primitive"});
        match op {
            "parse" => match numbers.view(&a, "")? {
                PythonNumber::Integer(_) => {
                    result["kind"] = "int".into();
                }
                PythonNumber::Float(f) => {
                    result["kind"] = "float".into();
                    result["bits"] = format!("{:016x}", f.to_bits()).into();
                }
            },
            "to_float" => {
                result["bits"] = format!(
                    "{:016x}",
                    numbers
                        .view(&a, "")?
                        .to_float()
                        .map_err(EvalFailure::Abort)?
                        .to_bits()
                )
                .into();
            }
            "compare" => {
                result["comparison"] = match numbers
                    .view(&a, "")?
                    .compare(&numbers.view(b.as_ref().unwrap(), "")?)
                {
                    Some(Ordering::Less) => (-1).into(),
                    Some(Ordering::Equal) => 0.into(),
                    Some(Ordering::Greater) => 1.into(),
                    None => Value::Null,
                };
            }
            "multiple" => {
                result["value"] = numbers
                    .view(&a, "")?
                    .multiple_of(&numbers.view(b.as_ref().unwrap(), "")?)
                    .map_err(EvalFailure::Abort)?
                    .into();
            }
            "equal" => {
                result["value"] = numbers.equal(&a, b.as_ref().unwrap(), "")?.into();
            }
            "unique" => {
                result["value"] = numbers.unique(a.as_array().unwrap(), "")?.into();
            }
            _ => panic!("unknown primitive operation"),
        }
        Ok(result)
    };
    match run() {
        Ok(result) => result,
        Err(EvalFailure::Abort(class)) => serde_json::json!({"result":format!("Abort:{class:?}")}),
        Err(EvalFailure::ScopeGap { kind, .. }) => {
            serde_json::json!({"result":format!("ScopeGap:{kind:?}")})
        }
        Err(error) => panic!("unexpected primitive failure: {error:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_cpython_notice_has_frozen_hash() {
        use sha2::{Digest, Sha256};
        let provenance: Value =
            serde_json::from_str(include_str!("numbers/PROVENANCE.json")).unwrap();
        assert_eq!(
            format!(
                "{:x}",
                Sha256::digest(include_bytes!("numbers/CPYTHON-LICENSE"))
            ),
            provenance["notices"][1]["sha256"].as_str().unwrap()
        );
    }
    #[test]
    fn frozen_primitive_bits_and_categories() {
        let rows: Vec<Value> = serde_json::from_str(include_str!(
            "../../../tests/output_schema_numbers_cases.json"
        ))
        .unwrap();
        for row in rows {
            if row.get("op").is_some() && row["oracle"]["result"] != "not-run:parse-precondition" {
                assert_eq!(primitive_probe(&row), row["oracle"], "{}", row["name"]);
            }
        }
    }
    #[test]
    fn digit_guard_is_primitive_only_and_float_kind_bypasses_it() {
        assert!(matches!(
            PythonNumber::parse(&"9".repeat(INTEGER_DIGIT_LIMIT)),
            Ok(PythonNumber::Integer(_))
        ));
        assert!(matches!(
            PythonNumber::parse(&"9".repeat(INTEGER_DIGIT_LIMIT + 1)),
            Err(AbortClass::ValueError)
        ));
        assert!(
            matches!(PythonNumber::parse(&format!("{}.0","9".repeat(INTEGER_DIGIT_LIMIT+1))),Ok(PythonNumber::Float(f)) if f==f64::INFINITY)
        );
    }
    #[test]
    fn unordered_internal_nan_and_zero_are_distinct_from_nonstandard_input() {
        let nan = PythonNumber::Float(f64::NAN);
        let zero = PythonNumber::Integer(BigInt::from(0));
        assert_eq!(nan.compare(&zero), None);
        assert_eq!(zero.compare(&nan), None);
        assert_eq!(
            nan.multiple_of(&PythonNumber::Float(2.0)),
            Err(AbortClass::ValueError)
        );
        assert_eq!(
            nan.multiple_of(&PythonNumber::Integer(BigInt::from(2))),
            Ok(false)
        );
        assert_eq!(
            zero.compare(&PythonNumber::Float(-0.0)),
            Some(Ordering::Equal)
        );
    }
    #[test]
    fn equality_work_and_depth_guards_do_not_report_invalid() {
        let values: Vec<Value> = (0..500).map(|i| serde_json::json!(i)).collect();
        assert!(matches!(
            Numbers::default().unique(&values, "/uniqueItems"),
            Err(EvalFailure::ScopeGap {
                kind: GapKind::BudgetBoundary,
                ..
            })
        ));
        let mut a = Value::Null;
        let mut b = Value::Null;
        for _ in 0..128 {
            a = serde_json::json!([a]);
            b = serde_json::json!([b]);
        }
        assert!(matches!(
            Numbers::default().equal(&a, &b, "/const"),
            Err(EvalFailure::ScopeGap {
                kind: GapKind::BudgetBoundary,
                ..
            })
        ));
    }
}
