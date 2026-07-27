//! Percentage formatter. Port of `src/formatters/percentage-formatter.ts`
//! (a PriceFormatter with base 100 that appends '%').

use super::price_formatter::PriceFormatter;

#[derive(Clone, Debug)]
pub struct PercentageFormatter {
    inner: PriceFormatter,
}

impl Default for PercentageFormatter {
    fn default() -> Self {
        Self {
            inner: PriceFormatter::new(100, 1.0),
        }
    }
}

impl PercentageFormatter {
    /// A percentage formatter with a custom price scale (`10^precision` for `precision`
    /// decimals; the default is 100 = 2 decimals).
    pub fn with_price_scale(price_scale: i64) -> Self {
        Self {
            inner: PriceFormatter::new(price_scale, 1.0),
        }
    }

    pub fn format(&self, value: f64) -> String {
        format!("{}%", self.inner.format(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_percent() {
        let f = PercentageFormatter::default();
        assert_eq!(f.format(12.345), "12.35%");
        assert_eq!(f.format(-3.0), "\u{2212}3.00%");
    }
}
