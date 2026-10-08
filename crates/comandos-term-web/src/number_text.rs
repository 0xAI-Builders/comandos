//! Decimal text for CSS and geometry, sharing the JSON formatter already in
//! the terminal rather than linking a second floating-point formatter.

pub(crate) fn decimal(value: f64) -> String {
    let Some(number) = serde_json::Number::from_f64(value) else {
        return if value.is_nan() {
            "NaN"
        } else if value.is_sign_negative() {
            "-inf"
        } else {
            "inf"
        }
        .into();
    };
    let source = number.to_string();
    // Both integer texts come from the finite JSON formatter, never input.
    // Keep an invariant failure signal without formatting ParseIntError.
    let (mantissa, exponent) = source
        .split_once(['e', 'E'])
        .map(|(m, e)| {
            (
                m,
                e.parse::<i32>()
                    .unwrap_or_else(|_| panic!("JSON float exponent")),
            )
        })
        .unwrap_or((&source, 0));
    let negative = mantissa.starts_with('-');
    let mantissa = mantissa.trim_start_matches('-');
    let fraction = mantissa.find('.').map_or(0, |p| mantissa.len() - p - 1);
    let mut digits = mantissa.replace('.', "");
    let mut decimal_exponent = exponent - fraction as i32;
    while digits.ends_with('0') && digits.len() > 1 {
        digits.pop();
        decimal_exponent += 1;
    }
    digits = digits.trim_start_matches('0').to_owned();
    if digits.is_empty() {
        digits.push('0');
        decimal_exponent = 0;
    }
    // JSON's shortest formatter chooses the even decimal at an exact tie;
    // Rust Display chooses the larger magnitude. Detect that midpoint using
    // integer factors, without a second float conversion or formatter.
    let coefficient = digits
        .parse::<u64>()
        .unwrap_or_else(|_| panic!("JSON float significand"));
    if lower_midpoint(value, coefficient, decimal_exponent) {
        digits = (coefficient + 1).to_string();
    }
    let point = digits.len() as i32 + decimal_exponent;
    let mut result = String::with_capacity(digits.len() + point.unsigned_abs() as usize + 3);
    if negative {
        result.push('-');
    }
    if point <= 0 {
        result.push_str("0.");
        result.extend(std::iter::repeat_n('0', (-point) as usize));
        result.push_str(&digits);
    } else if point as usize >= digits.len() {
        result.push_str(&digits);
        result.extend(std::iter::repeat_n('0', point as usize - digits.len()));
    } else {
        let (integer, fraction) = digits.split_at(point as usize);
        result.push_str(integer);
        result.push('.');
        result.push_str(fraction);
    }
    result
}

#[allow(clippy::manual_is_multiple_of)] // Keep the measured integer arithmetic unchanged.
fn lower_midpoint(value: f64, coefficient: u64, exponent: i32) -> bool {
    let bits = value.to_bits();
    let biased = ((bits >> 52) & 0x7ff) as i32;
    let mut significand = bits & ((1_u64 << 52) - 1);
    let mut binary_exponent = -1074;
    if biased != 0 {
        significand |= 1_u64 << 52;
        binary_exponent = biased - 1023 - 52;
    }
    if significand == 0 {
        return false;
    }
    let trailing = significand.trailing_zeros();
    significand >>= trailing;
    binary_exponent += trailing as i32;
    let mut midpoint = coefficient * 2 + 1;
    if exponent >= 0 {
        for _ in 0..exponent {
            let Some(next) = midpoint.checked_mul(5) else {
                return false;
            };
            midpoint = next;
            if midpoint > (1_u64 << 53) {
                return false;
            }
        }
    } else {
        for _ in exponent..0 {
            if midpoint % 5 != 0 {
                return false;
            }
            midpoint /= 5;
        }
    }
    significand == midpoint && binary_exponent == exponent - 1
}

pub(crate) fn px(value: f64) -> String {
    let mut result = decimal(value);
    result.push_str("px");
    result
}

/// Same comparisons, NaN/signed-zero behavior and failure signal as f64::clamp,
/// without retaining a second float formatter solely for its invalid bounds.
#[track_caller]
pub(crate) fn clamp(value: f64, min: f64, max: f64) -> f64 {
    assert!(min <= max, "invalid terminal clamp bounds");
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

/// Preserve Rust's floating-number grammar, using the browser's IEEE-754
/// conversion on WASM instead of carrying a second decimal parser.
pub(crate) fn parse(source: &str) -> Option<f64> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        source.parse().ok()
    }
    #[cfg(target_arch = "wasm32")]
    {
        let unsigned = source.strip_prefix(['+', '-']).unwrap_or(source);
        let negative = source.starts_with('-');
        if unsigned.eq_ignore_ascii_case("nan") {
            return Some(if negative { -f64::NAN } else { f64::NAN });
        }
        if unsigned.eq_ignore_ascii_case("inf") || unsigned.eq_ignore_ascii_case("infinity") {
            return Some(if negative {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            });
        }
        let mut bytes = unsigned.bytes().peekable();
        let mut digits = 0;
        while bytes.next_if(u8::is_ascii_digit).is_some() {
            digits += 1;
        }
        if bytes.next_if_eq(&b'.').is_some() {
            while bytes.next_if(u8::is_ascii_digit).is_some() {
                digits += 1;
            }
        }
        if digits == 0 {
            return None;
        }
        if bytes.next_if(|c| matches!(c, b'e' | b'E')).is_some() {
            bytes.next_if(|c| matches!(c, b'+' | b'-'));
            bytes.next_if(u8::is_ascii_digit)?;
            while bytes.next_if(u8::is_ascii_digit).is_some() {}
        }
        if bytes.next().is_some() {
            return None;
        }
        Some(js_sys::Number::parse_float(source))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_preserves_display_for_edges_and_deterministic_bit_patterns() {
        for value in [
            0.,
            -0.,
            1.,
            -1.,
            1.25,
            1e-7,
            1e20,
            f64::MIN_POSITIVE,
            f64::MAX,
            f64::EPSILON,
            f64::from_bits(1),
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NAN,
        ] {
            assert_eq!(
                decimal(value),
                value.to_string(),
                "bits={:x}",
                value.to_bits()
            );
        }
        let mut bits = 0x1234_5678_9abc_def0_u64;
        for _ in 0..100_000 {
            bits ^= bits << 13;
            bits ^= bits >> 7;
            bits ^= bits << 17;
            let value = f64::from_bits(bits);
            assert_eq!(decimal(value), value.to_string(), "bits={bits:x}");
        }
        for (value, min, max) in [
            (f64::NAN, 0., 1.),
            (-0., 0., 1.),
            (0., -0., 1.),
            (f64::INFINITY, -1., 1.),
            (f64::NEG_INFINITY, -1., 1.),
        ] {
            assert_eq!(
                clamp(value, min, max).to_bits(),
                value.clamp(min, max).to_bits()
            );
        }
        for (min, max) in [(2., 1.), (f64::NAN, 1.), (1., f64::NAN), (1e20, 1e-6)] {
            assert!(std::panic::catch_unwind(|| clamp(0., min, max)).is_err());
            assert!(std::panic::catch_unwind(|| 0_f64.clamp(min, max)).is_err());
        }
        assert_eq!(px(-0.), "-0px");
        assert_eq!(px(12.75), "12.75px");
    }
}
