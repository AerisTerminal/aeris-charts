//! Chart display-time-zone support.
//!
//! Canonical chart data remains UTC. This module converts UTC instants to local civil time only
//! for presentation semantics: time-axis tick weighting, labels, crosshair labels, and host clocks.

use chrono::{Datelike, Duration, LocalResult, Offset, TimeZone, Timelike, Utc};
use chrono_tz::Tz;

pub const DEFAULT_TIME_ZONE: &str = "Etc/UTC";

/// The exact built-in time-zone identifiers documented by TradingView Advanced Charts.
///
/// Hosts can expose this list directly without maintaining a second copy. The chart display-time
/// parser accepts this parity surface exactly, including TradingView's retained `Asia/Astana`
/// identifier, which is mapped internally to current tzdb rules.
pub const TRADINGVIEW_TIME_ZONES: &[&str] = &[
    "Etc/UTC",
    "Africa/Cairo",
    "Africa/Casablanca",
    "Africa/Johannesburg",
    "Africa/Lagos",
    "Africa/Nairobi",
    "Africa/Tunis",
    "America/Anchorage",
    "America/Argentina/Buenos_Aires",
    "America/Bogota",
    "America/Caracas",
    "America/Chicago",
    "America/El_Salvador",
    "America/Halifax",
    "America/Juneau",
    "America/Lima",
    "America/Los_Angeles",
    "America/Mexico_City",
    "America/New_York",
    "America/Phoenix",
    "America/Santiago",
    "America/Sao_Paulo",
    "America/Toronto",
    "America/Vancouver",
    "Asia/Astana",
    "Asia/Ashkhabad",
    "Asia/Bahrain",
    "Asia/Bangkok",
    "Asia/Chongqing",
    "Asia/Colombo",
    "Asia/Dhaka",
    "Asia/Dubai",
    "Asia/Ho_Chi_Minh",
    "Asia/Hong_Kong",
    "Asia/Jakarta",
    "Asia/Jerusalem",
    "Asia/Karachi",
    "Asia/Kabul",
    "Asia/Kathmandu",
    "Asia/Kolkata",
    "Asia/Kuala_Lumpur",
    "Asia/Kuwait",
    "Asia/Manila",
    "Asia/Muscat",
    "Asia/Nicosia",
    "Asia/Qatar",
    "Asia/Riyadh",
    "Asia/Seoul",
    "Asia/Shanghai",
    "Asia/Singapore",
    "Asia/Taipei",
    "Asia/Tehran",
    "Asia/Tokyo",
    "Asia/Yangon",
    "Atlantic/Azores",
    "Atlantic/Reykjavik",
    "Australia/Adelaide",
    "Australia/Brisbane",
    "Australia/Perth",
    "Australia/Sydney",
    "Europe/Amsterdam",
    "Europe/Athens",
    "Europe/Belgrade",
    "Europe/Berlin",
    "Europe/Bratislava",
    "Europe/Brussels",
    "Europe/Bucharest",
    "Europe/Budapest",
    "Europe/Copenhagen",
    "Europe/Dublin",
    "Europe/Helsinki",
    "Europe/Istanbul",
    "Europe/Lisbon",
    "Europe/Ljubljana",
    "Europe/London",
    "Europe/Luxembourg",
    "Europe/Madrid",
    "Europe/Malta",
    "Europe/Moscow",
    "Europe/Oslo",
    "Europe/Paris",
    "Europe/Prague",
    "Europe/Riga",
    "Europe/Rome",
    "Europe/Sofia",
    "Europe/Stockholm",
    "Europe/Tallinn",
    "Europe/Vienna",
    "Europe/Vilnius",
    "Europe/Warsaw",
    "Europe/Zagreb",
    "Europe/Zurich",
    "Pacific/Auckland",
    "Pacific/Chatham",
    "Pacific/Fakaofo",
    "Pacific/Honolulu",
    "Pacific/Norfolk",
    "US/Mountain",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChartTimeZone {
    id: &'static str,
    inner: Tz,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalTimeParts {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub offset_seconds: i32,
}

impl Default for ChartTimeZone {
    fn default() -> Self {
        Self {
            id: DEFAULT_TIME_ZONE,
            inner: chrono_tz::Etc::UTC,
        }
    }
}

impl ChartTimeZone {
    fn resolve_local(self, naive: chrono::NaiveDateTime) -> Option<chrono::DateTime<Tz>> {
        match self.inner.from_local_datetime(&naive) {
            LocalResult::Single(value) => Some(value),
            LocalResult::Ambiguous(first, second) => {
                Some(if first.timestamp_millis() <= second.timestamp_millis() {
                    first
                } else {
                    second
                })
            }
            LocalResult::None => {
                // A civil boundary can land in a DST gap. Advance to the first representable
                // local minute rather than dropping a whole day/month/year tick.
                (1..=180).find_map(|minutes| {
                    let candidate = naive.checked_add_signed(Duration::minutes(minutes))?;
                    match self.inner.from_local_datetime(&candidate) {
                        LocalResult::Single(value) => Some(value),
                        LocalResult::Ambiguous(first, second) => {
                            Some(if first.timestamp_millis() <= second.timestamp_millis() {
                                first
                            } else {
                                second
                            })
                        }
                        LocalResult::None => None,
                    }
                })
            }
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        let id = TRADINGVIEW_TIME_ZONES
            .iter()
            .copied()
            .find(|candidate| *candidate == value)?;
        // TradingView retains this historical identifier while current tzdb releases use
        // Asia/Almaty for the same Kazakhstan civil-time rules.
        let canonical = if id == "Asia/Astana" {
            "Asia/Almaty"
        } else {
            id
        };
        canonical.parse::<Tz>().ok().map(|inner| Self { id, inner })
    }

    #[must_use]
    pub fn id(self) -> &'static str {
        self.id
    }

    #[must_use]
    pub fn is_tradingview_supported(value: &str) -> bool {
        TRADINGVIEW_TIME_ZONES.contains(&value)
    }

    #[must_use]
    pub fn local_parts(self, utc_seconds: i64) -> Option<LocalTimeParts> {
        let local = self.inner.timestamp_opt(utc_seconds, 0).single()?;
        Some(LocalTimeParts {
            year: local.year(),
            month: local.month(),
            day: local.day(),
            hour: local.hour(),
            minute: local.minute(),
            second: local.second(),
            offset_seconds: local.offset().fix().local_minus_utc(),
        })
    }

    #[must_use]
    pub fn local_epoch_seconds(self, utc_seconds: i64) -> i64 {
        self.local_parts(utc_seconds).map_or(utc_seconds, |parts| {
            utc_seconds.saturating_add(i64::from(parts.offset_seconds))
        })
    }

    /// Resolve a pseudo-epoch millisecond value whose civil fields represent local wall time into
    /// the corresponding UTC instant. Ambiguous fall-back times choose the earlier occurrence;
    /// nonexistent spring-forward times advance to the first representable local minute.
    #[must_use]
    pub fn utc_millis_from_local_epoch_millis(self, local_millis: i64) -> Option<i64> {
        let naive = chrono::DateTime::<Utc>::from_timestamp_millis(local_millis)?.naive_utc();
        Some(self.resolve_local(naive)?.timestamp_millis())
    }

    #[must_use]
    pub fn abbreviation(self, utc_seconds: i64) -> String {
        self.inner
            .timestamp_opt(utc_seconds, 0)
            .single()
            .map_or_else(|| "UTC".to_string(), |local| local.format("%Z").to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tradingview_zone_parses() {
        for zone in TRADINGVIEW_TIME_ZONES {
            assert!(
                ChartTimeZone::parse(zone).is_some(),
                "unsupported zone: {zone}"
            );
        }
    }

    #[test]
    fn new_york_observes_dst() {
        let zone = ChartTimeZone::parse("America/New_York").unwrap();
        // 2026-01-15 12:00 UTC and 2026-07-15 12:00 UTC.
        let winter = zone.local_parts(1_768_478_400).unwrap();
        let summer = zone.local_parts(1_784_116_800).unwrap();
        assert_eq!(winter.offset_seconds, -18_000);
        assert_eq!(summer.offset_seconds, -14_400);
    }
}
