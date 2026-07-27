//! Price tick span calculation. Port of `src/model/price-tick-span-calculator.ts`.

use crate::helpers::mathex::{equal, greater_or_equal, is_base_decimal};

const TICK_SPAN_EPSILON: f64 = 1e-14;

pub struct PriceTickSpanCalculator {
    base: i64,
    integral_dividers: Vec<f64>,
    fractional_dividers: Vec<f64>,
}

impl PriceTickSpanCalculator {
    /// `base` is the price scale base (e.g. 100 for 2 decimals). Panics on unexpected bases,
    /// matching the reference's thrown errors.
    pub fn new(base: i64, integral_dividers: Vec<f64>) -> Self {
        let fractional_dividers = if is_base_decimal(base) {
            vec![2.0, 2.5, 2.0]
        } else {
            let mut dividers = Vec::new();
            let mut base_rest = base;
            // reference throws on a base with prime factors other than 2 and 5 (a user-supplied
            // `min_move` like 0.03 produces one). A JS throw is catchable; a wasm panic aborts
            // the whole chart, so stop decomposing instead and use the dividers found so far.
            while base_rest != 1 && dividers.len() <= 100 {
                if base_rest % 2 == 0 {
                    dividers.push(2.0);
                    base_rest /= 2;
                } else if base_rest % 5 == 0 {
                    dividers.push(2.0);
                    dividers.push(2.5);
                    base_rest /= 5;
                } else {
                    break;
                }
            }
            dividers
        };

        Self {
            base,
            integral_dividers,
            fractional_dividers,
        }
    }

    pub fn tick_span(&self, high: f64, low: f64, max_tick_span: f64) -> f64 {
        let min_movement = if self.base == 0 {
            0.0
        } else {
            1.0 / self.base as f64
        };

        let mut result_tick_span = 10f64.powf(0f64.max((high - low).log10().ceil()));

        let mut index = 0usize;
        let mut c = self.integral_dividers[0];

        loop {
            // the second condition matters for very small values like 1e-10 where
            // greater_or_equal alone fails
            let larger_min_movement =
                greater_or_equal(result_tick_span, min_movement, TICK_SPAN_EPSILON)
                    && result_tick_span > (min_movement + TICK_SPAN_EPSILON);
            let larger_max_tick_span =
                greater_or_equal(result_tick_span, max_tick_span * c, TICK_SPAN_EPSILON);
            let larger_1 = greater_or_equal(result_tick_span, 1.0, TICK_SPAN_EPSILON);

            if !(larger_min_movement && larger_max_tick_span && larger_1) {
                break;
            }

            result_tick_span /= c;
            index += 1;
            c = self.integral_dividers[index % self.integral_dividers.len()];
        }

        if result_tick_span <= min_movement + TICK_SPAN_EPSILON {
            result_tick_span = min_movement;
        }

        result_tick_span = result_tick_span.max(1.0);

        if !self.fractional_dividers.is_empty() && equal(result_tick_span, 1.0, TICK_SPAN_EPSILON) {
            index = 0;
            c = self.fractional_dividers[0];
            while greater_or_equal(result_tick_span, max_tick_span * c, TICK_SPAN_EPSILON)
                && result_tick_span > (min_movement + TICK_SPAN_EPSILON)
            {
                result_tick_span /= c;
                index += 1;
                c = self.fractional_dividers[index % self.fractional_dividers.len()];
            }
        }

        result_tick_span
    }
}

/// The composite span used by the tick mark builder: the minimum over the three divider cycles.
/// Port of `PriceTickMarkBuilder.tickSpan()` (`src/model/price-tick-mark-builder.ts`).
pub fn composite_tick_span(
    high: f64,
    low: f64,
    base: i64,
    scale_height: f64,
    tick_mark_height: f64,
) -> f64 {
    assert!(high >= low, "high < low");

    let max_tick_span = (high - low) * tick_mark_height / scale_height;

    let c1 = PriceTickSpanCalculator::new(base, vec![2.0, 2.5, 2.0]);
    let c2 = PriceTickSpanCalculator::new(base, vec![2.0, 2.0, 2.5]);
    let c3 = PriceTickSpanCalculator::new(base, vec![2.5, 2.0, 2.0]);

    c1.tick_span(high, low, max_tick_span)
        .min(c2.tick_span(high, low, max_tick_span))
        .min(c3.tick_span(high, low, max_tick_span))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integral_spans() {
        // hand-traced from the algorithm (see RENDERING_SPEC.md §9):
        // base=100, dividers [2, 2.5, 2], high-low=100 -> start=100
        // maxTickSpan=5: 100/2=50 /2.5=20 /2=10 /2=5, 5 >= 12.5? no -> 5
        let calc = PriceTickSpanCalculator::new(100, vec![2.0, 2.5, 2.0]);
        assert_eq!(calc.tick_span(100.0, 0.0, 5.0), 5.0);
    }

    #[test]
    fn fractional_spans_go_below_one() {
        // range 0..1, generous space: span should drop below 1 using fractional dividers
        let calc = PriceTickSpanCalculator::new(100, vec![2.0, 2.5, 2.0]);
        let span = calc.tick_span(1.0, 0.0, 0.1);
        assert!(span < 1.0);
        assert!(span >= 0.01); // never below min movement
    }

    #[test]
    fn never_below_min_movement() {
        let calc = PriceTickSpanCalculator::new(100, vec![2.0, 2.5, 2.0]);
        let span = calc.tick_span(0.02, 0.0, 1e-9);
        assert!(span >= 0.01 - 1e-14);
    }

    #[test]
    fn composite_takes_min_of_cycles() {
        let a = composite_tick_span(100.0, 0.0, 100, 500.0, 30.0);
        // each individual cycle produces >= a
        for dividers in [
            vec![2.0, 2.5, 2.0],
            vec![2.0, 2.0, 2.5],
            vec![2.5, 2.0, 2.0],
        ] {
            let c = PriceTickSpanCalculator::new(100, dividers);
            assert!(c.tick_span(100.0, 0.0, 100.0 * 30.0 / 500.0) >= a);
        }
    }

    #[test]
    fn non_decimal_base_fractional_dividers() {
        // base 25 = 5*5 -> dividers [2, 2.5, 2, 2.5]
        let calc = PriceTickSpanCalculator::new(25, vec![2.0, 2.5, 2.0]);
        let span = calc.tick_span(1.0, 0.0, 0.2);
        assert!(span <= 1.0);
        assert!(span >= 1.0 / 25.0 - 1e-14);
    }
}
