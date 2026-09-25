//! Volume formatter. Port of `src/formatters/volume-formatter.ts`:
//! abbreviates with K/M/B suffixes, `precision` decimals under 995 * unit.

#[derive(Clone, Debug)]
pub struct VolumeFormatter {
    precision: u32,
}

impl VolumeFormatter {
    pub fn new(precision: u32) -> Self {
        Self { precision }
    }

    pub fn format(&self, mut value: f64) -> String {
        let mut sign = String::new();
        if value < 0.0 {
            sign = "-".to_string();
            value = -value;
        }

        if value < 995.0 {
            return format!("{}{}", sign, self.format_number(value));
        } else if value < 999_995.0 {
            return format!("{}{}K", sign, self.format_number(value / 1000.0));
        } else if value < 999_999_995.0 {
            return format!("{}{}M", sign, self.format_number(value / 1_000_000.0));
        }
        format!("{}{}B", sign, self.format_number(value / 1_000_000_000.0))
    }

    fn format_number(&self, value: f64) -> String {
        let pow = 10f64.powi(self.precision as i32);
        let v = (value * pow).round() / pow;
        // trim trailing zeros like the reference's implementation does (it formats then strips .0+)
        let s = format!("{:.*}", self.precision as usize, v);
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abbreviates() {
        let f = VolumeFormatter::new(1);
        assert_eq!(f.format(100.0), "100");
        assert_eq!(f.format(1500.0), "1.5K");
        assert_eq!(f.format(2_500_000.0), "2.5M");
        assert_eq!(f.format(3_100_000_000.0), "3.1B");
        assert_eq!(f.format(-1500.0), "-1.5K");
    }
}
