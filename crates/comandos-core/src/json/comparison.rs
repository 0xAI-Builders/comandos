use serde_json::Value;

/// Compare JSON structurally with Python numeric equality, retaining integer precision.
/// Bools compare as integers; strings and container structure keep their identity.
/// Decimal/exponent numbers outside finite f64 support never equal an identity.
pub fn python_eq(left: &Value, right: &Value) -> bool {
    fn supported_number(value: &Value) -> bool {
        match value {
            Value::Number(number) if number.as_str().contains(['.', 'e', 'E']) => {
                number.as_f64().is_some()
            }
            _ => true,
        }
    }
    match (left, right) {
        (Value::Number(_) | Value::Bool(_), Value::Number(_) | Value::Bool(_)) => {
            supported_number(left) && supported_number(right) && number_cmp(left, right).is_eq()
        }
        (Value::Array(left), Value::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| python_eq(left, right))
        }
        (Value::Object(left), Value::Object(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .all(|(key, left)| right.get(key).is_some_and(|right| python_eq(left, right)))
        }
        _ => left == right,
    }
}

// Compare JSON integers before converting mixed numeric values. Formatting a
// float's integral part as decimal exposes its exact represented integer, so
// large integers never lose digits merely because a peer happens to be a float.
pub(crate) fn number_cmp(left: &Value, right: &Value) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    fn integer(value: &Value) -> Option<String> {
        match value {
            Value::Bool(value) => Some(if *value { "1" } else { "0" }.into()),
            Value::Number(value) => {
                let text = value.to_string();
                (!text.contains(['.', 'e', 'E'])).then_some(text)
            }
            _ => None,
        }
    }
    fn integer_cmp(left: &str, right: &str) -> Ordering {
        let digits = |text: &str| {
            let raw = text
                .strip_prefix('-')
                .unwrap_or(text)
                .trim_start_matches('0');
            (
                text.starts_with('-') && !raw.is_empty(),
                if raw.is_empty() { "0" } else { raw }.to_string(),
            )
        };
        let (negative_left, left) = digits(left);
        let (negative_right, right) = digits(right);
        match (negative_left, negative_right) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => {
                let compared = left.len().cmp(&right.len()).then_with(|| left.cmp(&right));
                if negative_left {
                    compared.reverse()
                } else {
                    compared
                }
            }
        }
    }
    fn int_float(integer: &str, float: f64) -> Ordering {
        if float == f64::INFINITY {
            return Ordering::Less;
        }
        if float == f64::NEG_INFINITY {
            return Ordering::Greater;
        }
        integer_cmp(integer, &format!("{:.0}", float.trunc())).then_with(|| {
            0.0_f64
                .partial_cmp(&float.fract())
                .unwrap_or(Ordering::Equal)
        })
    }
    match (integer(left), integer(right)) {
        (Some(left), Some(right)) => integer_cmp(&left, &right),
        (Some(left), None) => int_float(&left, right.as_f64().unwrap_or(0.0)),
        (None, Some(right)) => int_float(&right, left.as_f64().unwrap_or(0.0)).reverse(),
        (None, None) => left
            .as_f64()
            .unwrap_or(0.0)
            .partial_cmp(&right.as_f64().unwrap_or(0.0))
            .unwrap_or(Ordering::Equal),
    }
}
