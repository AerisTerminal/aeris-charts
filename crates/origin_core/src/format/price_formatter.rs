//! Price formatter. Port of `src/formatters/price-formatter.ts`.
//!
//! Note: reference uses U+2212 (minus sign) instead of '-' because it has the same advance width as
//! '+', keeping axis labels stable when values flip sign.

pub const MINUS_SIGN: char = '\u{2212}';

/// Pads `value` with leading zeros to `length` digits. Port of `numberToStringWithLeadingZero`.
fn number_to_string_with_leading_zero(value: u64, length: usize) -> String {
    assert!(length <= 16, "invalid length");
    if length == 0 {
        return value.to_string();
    }
    let s = value.to_string();
    if s.len() >= length {
        s[s.len() - length..].to_string()
    } else {
        format!("{}{}", "0".repeat(length - s.len()), s)
    }
}

#[derive(Clone, Debug)]
pub struct PriceFormatter {
    price_scale: i64,
    min_move: f64,
    fractional_length: usize,
}

impl Default for PriceFormatter {
    fn default() -> Self {
        Self::new(100, 1.0)
    }
}

impl PriceFormatter {
    /// `price_scale` = 10^precision (e.g. 100 for 2 decimals), `min_move` = minimal price step
    /// in scaled units (usually 1).
    pub fn new(price_scale: i64, min_move: f64) -> Self {
        let min_move = if min_move == 0.0 { 1.0 } else { min_move };
        let price_scale = if price_scale < 0 { 100 } else { price_scale };

        // fractional length = number of decimal digits of price_scale
        let mut fractional_length = 0usize;
        if price_scale > 0 && min_move > 0.0 {
            let mut base = price_scale as f64;
            while base > 1.0 {
                base /= 10.0;
                fractional_length += 1;
            }
        }

        Self {
            price_scale,
            min_move,
            fractional_length,
        }
    }

    /// Constructs from a priceFormat option: precision + minMove (e.g. precision 2, minMove 0.01).
    pub fn from_precision(precision: u32, min_move: f64) -> Self {
        let price_scale = 10i64.pow(precision);
        // reference computes priceScale = round(1/minMove-ish) via series options; this helper covers
        // the common case where min_move = 10^-precision.
        let scaled_min_move = (min_move * price_scale as f64).round();
        Self::new(price_scale, scaled_min_move.max(1.0))
    }

    pub fn format(&self, price: f64) -> String {
        let sign = if price < 0.0 {
            MINUS_SIGN.to_string()
        } else {
            String::new()
        };
        format!("{}{}", sign, self.format_as_decimal(price.abs()))
    }

    fn format_as_decimal(&self, price: f64) -> String {
        let base = self.price_scale as f64 / self.min_move;

        let mut int_part = price.floor();
        let mut frac_string = String::new();
        let frac_length = self.fractional_length;

        if base > 1.0 {
            let mut frac_part = (price * base).round() - int_part * base;
            // fixed-point cleanup, port of toFixed(fractionalLength) roundtrip
            let fixup = 10f64.powi(frac_length as i32);
            frac_part = (frac_part * fixup).round() / fixup;

            if frac_part >= base {
                frac_part -= base;
                int_part += 1.0;
            }

            let scaled = ((frac_part * fixup).round() / fixup * self.min_move).round() as u64;
            frac_string = format!(
                ".{}",
                number_to_string_with_leading_zero(scaled, frac_length)
            );
        } else {
            // round int part to min move
            int_part = (int_part * base).round() / base;
            if frac_length > 0 {
                frac_string = format!(".{}", number_to_string_with_leading_zero(0, frac_length));
            }
        }

        format!("{}{}", int_part as i64, frac_string)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_two_decimals() {
        let f = PriceFormatter::new(100, 1.0);
        assert_eq!(f.format(1.5), "1.50");
        assert_eq!(f.format(0.0), "0.00");
        assert_eq!(f.format(123.456), "123.46");
        assert_eq!(f.format(123.454), "123.45");
    }

    #[test]
    fn negative_uses_unicode_minus() {
        let f = PriceFormatter::new(100, 1.0);
        assert_eq!(f.format(-1.5), "\u{2212}1.50");
    }

    #[test]
    fn integer_scale() {
        let f = PriceFormatter::new(1, 1.0);
        assert_eq!(f.format(5.2), "5");
        assert_eq!(f.format(5.7), "5"); // floor of int part; matches reference
    }

    #[test]
    fn three_decimals() {
        let f = PriceFormatter::new(1000, 1.0);
        assert_eq!(f.format(0.1234), "0.123");
        assert_eq!(f.format(0.0005), "0.001"); // wait: 0.0005*1000=0.5 -> round -> 1 (ties away)
    }

    #[test]
    fn carry_into_integer_part() {
        let f = PriceFormatter::new(100, 1.0);
        assert_eq!(f.format(1.999), "2.00");
    }

    #[test]
    fn from_precision_helper() {
        let f = PriceFormatter::from_precision(2, 0.01);
        assert_eq!(f.format(10.5), "10.50");
    }
}
