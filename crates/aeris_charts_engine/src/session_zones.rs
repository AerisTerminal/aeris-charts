//! Customizable trading sessions and killzones rendered as shared underlay bands.
//!
//! Users can configure up to [`MAX_SESSION_ZONES`] custom time windows (with overnight wrap
//! support, optional High/Low range boxes, and session labels) in **any valid IANA timezone**
//! supported by `chrono-tz`.

use std::collections::HashSet;

use aeris_charts_core::time_zone::IanaTimeZone;
use aeris_charts_render::color::Color;
use aeris_charts_render::draw_list::{IRect, Prim, TextAlign};

use crate::ChartEngine;

/// Maximum number of custom session / killzone definitions per chart.
pub const MAX_SESSION_ZONES: usize = 16;
const MAX_SESSION_ID_BYTES: usize = 64;
const MAX_SESSION_NAME_BYTES: usize = 64;
const MAX_COLOR_BYTES: usize = 128;
const MINUTES_PER_DAY: u16 = 24 * 60;

/// One user-customizable trading session or killzone window.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionZoneDefinition {
    /// Stable identifier within the config (e.g. `"asia"`, `"london"`, `"ny_am"`).
    pub id: String,
    /// Display label rendered inside the session band when `show_label` is true.
    pub name: String,
    pub enabled: bool,
    /// Local start minute of day (`0..1440`). Values greater than `end_minute` wrap overnight.
    pub start_minute: u16,
    /// Local end minute of day (`0..1440`). `0` with `start_minute > 0` means midnight (`00:00`).
    pub end_minute: u16,
    /// CSS color string (hex, rgb, rgba, hsl).
    pub color: String,
    /// Band fill opacity in `[0.0, 1.0]`.
    pub opacity: f64,
    #[serde(default = "default_true")]
    pub show_label: bool,
    #[serde(default)]
    pub show_range_box: bool,
    /// Optional per-session IANA timezone override. When `None`, inherits
    /// [`SessionZonesConfig::time_zone`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_zone: Option<String>,
}

fn default_true() -> bool {
    true
}

/// Chart-wide configuration for customizable sessions / killzones.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionZonesConfig {
    pub enabled: bool,
    /// Default IANA timezone for all sessions (e.g. `"America/New_York"`, `"Asia/Kolkata"`,
    /// `"Europe/London"`, `"UTC"`). Any valid IANA tzdb zone is accepted.
    pub time_zone: String,
    pub sessions: Vec<SessionZoneDefinition>,
}

/// Validation failure returned when installing a [`SessionZonesConfig`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionZonesError {
    pub message: String,
}

impl std::fmt::Display for SessionZonesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SessionZonesError {}

impl SessionZonesError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Parse `"HH:MM"` into minutes since midnight (`0..1440`). `"24:00"` maps to `0`.
#[must_use]
pub fn parse_hhmm(value: &str) -> Option<u16> {
    let trimmed = value.trim();
    let (h, m) = trimmed.split_once(':')?;
    if h.len() != 2 || m.len() != 2 {
        return None;
    }
    let hour: u16 = h.parse().ok()?;
    let minute: u16 = m.parse().ok()?;
    if hour == 24 && minute == 0 {
        return Some(0);
    }
    if hour < 24 && minute < 60 {
        Some(hour * 60 + minute)
    } else {
        None
    }
}

/// Format minutes since midnight (`0..1440`) as `"HH:MM"`.
#[must_use]
pub fn format_hhmm(minute: u16) -> String {
    let clamped = minute % MINUTES_PER_DAY;
    format!("{:02}:{:02}", clamped / 60, clamped % 60)
}

impl SessionZonesConfig {
    /// Institutional killzones preset in `America/New_York` time:
    /// Asia (`20:00–00:00`), London (`02:00–05:00`), NY AM (`08:30–11:00`), NY PM (`13:30–16:00`).
    #[must_use]
    pub fn default_killzones() -> Self {
        Self {
            enabled: true,
            time_zone: "America/New_York".to_string(),
            sessions: vec![
                SessionZoneDefinition {
                    id: "asia".to_string(),
                    name: "Asia".to_string(),
                    enabled: true,
                    start_minute: 20 * 60,
                    end_minute: 0,
                    color: "#ab47bc".to_string(),
                    opacity: 0.10,
                    show_label: true,
                    show_range_box: false,
                    time_zone: None,
                },
                SessionZoneDefinition {
                    id: "london".to_string(),
                    name: "London".to_string(),
                    enabled: true,
                    start_minute: 2 * 60,
                    end_minute: 5 * 60,
                    color: "#2962ff".to_string(),
                    opacity: 0.10,
                    show_label: true,
                    show_range_box: false,
                    time_zone: None,
                },
                SessionZoneDefinition {
                    id: "ny_am".to_string(),
                    name: "NY AM".to_string(),
                    enabled: true,
                    start_minute: 8 * 60 + 30,
                    end_minute: 11 * 60,
                    color: "#26a69a".to_string(),
                    opacity: 0.10,
                    show_label: true,
                    show_range_box: false,
                    time_zone: None,
                },
                SessionZoneDefinition {
                    id: "ny_pm".to_string(),
                    name: "NY PM".to_string(),
                    enabled: true,
                    start_minute: 13 * 60 + 30,
                    end_minute: 16 * 60,
                    color: "#ff9800".to_string(),
                    opacity: 0.10,
                    show_label: true,
                    show_range_box: false,
                    time_zone: None,
                },
            ],
        }
    }

    /// Validate timezone identifiers, minute bounds, colors, opacities, and session limits.
    pub fn validate(&self) -> Result<(), SessionZonesError> {
        if IanaTimeZone::parse(&self.time_zone).is_none() {
            return Err(SessionZonesError::new(format!(
                "invalid session zones time zone: {}",
                self.time_zone
            )));
        }
        if self.sessions.len() > MAX_SESSION_ZONES {
            return Err(SessionZonesError::new(format!(
                "session zone count {} exceeds limit {}",
                self.sessions.len(),
                MAX_SESSION_ZONES
            )));
        }
        let mut ids = HashSet::with_capacity(self.sessions.len());
        for (index, session) in self.sessions.iter().enumerate() {
            let id = session.id.trim();
            if id.is_empty() || id.len() > MAX_SESSION_ID_BYTES {
                return Err(SessionZonesError::new(format!(
                    "session {index} has invalid id"
                )));
            }
            if !ids.insert(id.to_string()) {
                return Err(SessionZonesError::new(format!(
                    "duplicate session zone id: {id}"
                )));
            }
            if session.name.len() > MAX_SESSION_NAME_BYTES {
                return Err(SessionZonesError::new(format!(
                    "session '{id}' name exceeds {MAX_SESSION_NAME_BYTES} bytes"
                )));
            }
            if session.start_minute >= MINUTES_PER_DAY || session.end_minute >= MINUTES_PER_DAY {
                return Err(SessionZonesError::new(format!(
                    "session '{id}' start/end minutes must be in 0..1440"
                )));
            }
            if session.start_minute == session.end_minute {
                return Err(SessionZonesError::new(format!(
                    "session '{id}' start_minute and end_minute must differ"
                )));
            }
            if session.color.len() > MAX_COLOR_BYTES || Color::parse_css(&session.color).is_none() {
                return Err(SessionZonesError::new(format!(
                    "session '{id}' has invalid CSS color: {}",
                    session.color
                )));
            }
            if !session.opacity.is_finite() || !(0.0..=1.0).contains(&session.opacity) {
                return Err(SessionZonesError::new(format!(
                    "session '{id}' opacity must be finite in [0.0, 1.0]"
                )));
            }
            if let Some(tz) = session.time_zone.as_deref()
                && IanaTimeZone::parse(tz).is_none()
            {
                return Err(SessionZonesError::new(format!(
                    "session '{id}' has invalid time zone: {tz}"
                )));
            }
        }
        Ok(())
    }
}

/// Proleptic Gregorian days-from-civil (Howard Hinnant), used to group bars into one local
/// session trading day cleanly across month/year boundaries.
fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let y = i64::from(year) - i64::from(month <= 2);
    let era = if y >= 0 { y } else { y - 399 }.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(month);
    let d = i64::from(day);
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2).div_euclid(5) + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Evaluate whether `utc_seconds` falls inside `[start_minute, end_minute)` in `tz`.
/// Returns `Some(session_day_key)` so overnight sessions (`start > end`) stay grouped under the
/// evening day they started on.
pub(crate) fn session_day_key(
    tz: IanaTimeZone,
    utc_seconds: i64,
    start_minute: u16,
    end_minute: u16,
) -> Option<i64> {
    let parts = tz.local_parts(utc_seconds)?;
    let minute = (parts.hour * 60 + parts.minute) as u16;
    let local_day = days_from_civil(parts.year, parts.month, parts.day);
    if start_minute < end_minute {
        (minute >= start_minute && minute < end_minute).then_some(local_day)
    } else if minute >= start_minute {
        Some(local_day)
    } else if minute < end_minute {
        Some(local_day - 1)
    } else {
        None
    }
}

impl ChartEngine {
    /// Install or replace customizable session / killzone highlights.
    pub fn set_session_zones(
        &mut self,
        config: SessionZonesConfig,
    ) -> Result<(), SessionZonesError> {
        config.validate()?;
        self.session_zones = Some(config);
        self.invalidate_frame_scene();
        Ok(())
    }

    /// Current customizable session / killzone configuration, if installed.
    #[must_use]
    pub fn session_zones(&self) -> Option<&SessionZonesConfig> {
        self.session_zones.as_ref()
    }

    /// Remove customizable session / killzone highlights from the chart.
    pub fn clear_session_zones(&mut self) -> bool {
        if self.session_zones.take().is_some() {
            self.invalidate_frame_scene();
            true
        } else {
            false
        }
    }

    /// Emit customizable session / killzone underlay bands, optional High/Low range boxes, and
    /// labels into the primary price pane's underlay layer.
    pub(crate) fn build_session_zones_frame(
        &self,
        pane_index: usize,
        from: i64,
        to: i64,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
    ) {
        if pane_index != 0 {
            return;
        }
        let Some(config) = self.session_zones.as_ref().filter(|cfg| cfg.enabled) else {
            return;
        };
        if config.sessions.is_empty() {
            return;
        }
        let Some(default_tz) = IanaTimeZone::parse(&config.time_zone) else {
            return;
        };
        let Some(pane) = self.panes.get(pane_index) else {
            return;
        };
        // Anchor timestamps and optional High/Low range boxes to the primary visible series on
        // pane 0 (preferring a non-indicator series such as the candlestick/line price series).
        let primary = self
            .series
            .iter()
            .find(|s| {
                s.visible
                    && s.pane_index == pane_index
                    && self.indicator_info(s.id).is_none()
                    && self
                        .data
                        .series_data(s.id)
                        .is_some_and(|(times, _)| !times.is_empty())
            })
            .or_else(|| {
                self.series.iter().find(|s| {
                    s.visible
                        && s.pane_index == pane_index
                        && self
                            .data
                            .series_data(s.id)
                            .is_some_and(|(times, _)| !times.is_empty())
                })
            });
        let Some(series) = primary else {
            return;
        };
        let Some((times, values)) = self.data.series_data(series.id) else {
            return;
        };
        let scale = pane.scale(series.price_scale_target);
        let base_value = self.series_base_value(series.id, from).unwrap_or(0.0);
        let plot = self.data.plot(series.id);
        let pane_width = (self.pane_w * hpr).round().max(1.0) as i32;
        let pane_top_px = (pane.top * vpr).round() as i32;
        let pane_height_px = (pane.height * vpr).round().max(1.0) as i32;
        let half_width = (self.time_scale.bar_spacing() * 0.5 * hpr).max(0.5);
        let font_family = self.options.get().layout.font_family.clone();

        // Collect visible rows once so multi-session rendering avoids repeated binary searches.
        let mut visible_rows: Vec<(i64, i64, f64, f64)> = Vec::new();
        for row in plot.visible_rows(from, to) {
            if let (Some(logical), Some(&time)) = (plot.index_at(row), times.get(row)) {
                let high = values[1].get(row).copied().unwrap_or(f64::NAN);
                let low = values[2].get(row).copied().unwrap_or(f64::NAN);
                visible_rows.push((logical, time, high, low));
            }
        }
        if visible_rows.is_empty() {
            return;
        }

        struct ActiveSpan {
            day_key: i64,
            left: i32,
            right: i32,
            high: f64,
            low: f64,
        }

        for session in &config.sessions {
            if !session.enabled {
                continue;
            }
            let Some(base_color) = Color::parse_css(&session.color) else {
                continue;
            };
            let tz = session
                .time_zone
                .as_deref()
                .and_then(IanaTimeZone::parse)
                .unwrap_or(default_tz);
            let fill_alpha = (session.opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
            let fill_color =
                Color::rgba(base_color.r(), base_color.g(), base_color.b(), fill_alpha);
            let box_fill_alpha =
                ((session.opacity.clamp(0.0, 1.0) * 1.6).min(0.45) * 255.0).round() as u8;
            let box_fill = Color::rgba(
                base_color.r(),
                base_color.g(),
                base_color.b(),
                box_fill_alpha,
            );
            let border_color = Color::rgba(base_color.r(), base_color.g(), base_color.b(), 165);
            let label_color = Color::rgba(base_color.r(), base_color.g(), base_color.b(), 220);

            let flush = |span: ActiveSpan, out: &mut Vec<Prim>| {
                if span.right <= span.left {
                    return;
                }
                let width = span.right - span.left;
                out.push(Prim::Rect {
                    rect: IRect {
                        x: span.left,
                        y: pane_top_px,
                        w: width,
                        h: pane_height_px,
                    },
                    color: fill_color,
                });
                if session.show_range_box
                    && let Some(price_scale) = scale
                    && !price_scale.is_empty()
                    && span.high.is_finite()
                    && span.low.is_finite()
                    && span.high >= span.low
                {
                    let y_high = (price_scale.price_to_coordinate(span.high, base_value) * vpr)
                        .round() as i32;
                    let y_low = (price_scale.price_to_coordinate(span.low, base_value) * vpr)
                        .round() as i32;
                    let top = y_high
                        .min(y_low)
                        .clamp(pane_top_px, pane_top_px + pane_height_px);
                    let bottom = y_high
                        .max(y_low)
                        .clamp(pane_top_px, pane_top_px + pane_height_px);
                    let box_h = (bottom - top).max(1);
                    let box_rect = IRect {
                        x: span.left,
                        y: top,
                        w: width,
                        h: box_h,
                    };
                    out.push(Prim::Rect {
                        rect: box_rect,
                        color: box_fill,
                    });
                    out.push(Prim::RectFrame {
                        rect: box_rect,
                        border: (1.0 * hpr.min(vpr)).round().max(1.0) as i32,
                        color: border_color,
                    });
                }
                if session.show_label && !session.name.is_empty() && (width as f64 / hpr) >= 28.0 {
                    out.push(Prim::Text {
                        x: (span.left as f64 + 4.0 * hpr) as f32,
                        y: (pane_top_px as f64 + 10.0 * vpr) as f32,
                        text: session.name.clone(),
                        color: label_color,
                        size: (10.0 * vpr) as f32,
                        family: font_family.clone(),
                        align: TextAlign::Left,
                        weight: 600,
                        italic: false,
                    });
                }
            };

            let mut active: Option<ActiveSpan> = None;
            for &(logical, time, high, low) in &visible_rows {
                let Some(day_key) =
                    session_day_key(tz, time, session.start_minute, session.end_minute)
                else {
                    if let Some(span) = active.take() {
                        flush(span, out);
                    }
                    continue;
                };
                let x = self.time_scale.index_to_coordinate(logical) * hpr;
                let left = (x - half_width).round().clamp(0.0, pane_width as f64) as i32;
                let right = (x + half_width).round().clamp(0.0, pane_width as f64) as i32;
                if right <= left {
                    continue;
                }
                match active.as_mut() {
                    Some(span) if span.day_key == day_key => {
                        span.left = span.left.min(left);
                        span.right = span.right.max(right);
                        if high.is_finite() {
                            span.high = if span.high.is_finite() {
                                span.high.max(high)
                            } else {
                                high
                            };
                        }
                        if low.is_finite() {
                            span.low = if span.low.is_finite() {
                                span.low.min(low)
                            } else {
                                low
                            };
                        }
                    }
                    _ => {
                        if let Some(span) = active.take() {
                            flush(span, out);
                        }
                        active = Some(ActiveSpan {
                            day_key,
                            left,
                            right,
                            high,
                            low,
                        });
                    }
                }
            }
            if let Some(span) = active {
                flush(span, out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hhmm_parsing_and_formatting_cover_midnight_and_reject_invalid() {
        assert_eq!(parse_hhmm("00:00"), Some(0));
        assert_eq!(parse_hhmm("08:30"), Some(510));
        assert_eq!(parse_hhmm("20:00"), Some(1200));
        assert_eq!(parse_hhmm("23:59"), Some(1439));
        assert_eq!(parse_hhmm("24:00"), Some(0));
        assert_eq!(parse_hhmm("24:01"), None);
        assert_eq!(parse_hhmm("8:30"), None);
        assert_eq!(parse_hhmm("12:60"), None);
        assert_eq!(format_hhmm(0), "00:00");
        assert_eq!(format_hhmm(510), "08:30");
        assert_eq!(format_hhmm(1200), "20:00");
    }

    #[test]
    fn session_zones_validation_accepts_any_iana_timezone_and_rejects_bad_configs() {
        let mut config = SessionZonesConfig::default_killzones();
        assert!(config.validate().is_ok());

        // Non-TradingView IANA zones (e.g. America/Denver, Asia/Kolkata, UTC) are valid.
        for tz in [
            "UTC",
            "Etc/UTC",
            "Asia/Kolkata",
            "America/Denver",
            "Europe/London",
            "Asia/Tokyo",
        ] {
            config.time_zone = tz.to_string();
            assert!(config.validate().is_ok(), "timezone {tz} must validate");
        }

        config.time_zone = "Mars/Olympus_Mons".to_string();
        assert!(config.validate().is_err());

        let mut same_time = SessionZonesConfig::default_killzones();
        same_time.sessions[0].start_minute = 600;
        same_time.sessions[0].end_minute = 600;
        assert!(same_time.validate().is_err());

        let mut dup_id = SessionZonesConfig::default_killzones();
        dup_id.sessions[1].id = dup_id.sessions[0].id.clone();
        assert!(dup_id.validate().is_err());
    }

    #[test]
    fn overnight_wrap_and_dst_group_bars_in_user_timezone() {
        let ny = IanaTimeZone::parse("America/New_York").unwrap();
        // Winter (EST = UTC-5): 2026-01-15 01:00 UTC is 2026-01-14 20:00 EST (start of 20:00-00:00 Asia).
        let est_20_00 = 1_768_438_800; // 2026-01-15 01:00:00 UTC
        let est_23_30 = est_20_00 + 3 * 3600 + 1800;
        let est_00_00 = est_20_00 + 4 * 3600;
        let key_start = session_day_key(ny, est_20_00, 20 * 60, 0);
        let key_end = session_day_key(ny, est_23_30, 20 * 60, 0);
        assert!(key_start.is_some());
        assert_eq!(key_start, key_end);
        assert_eq!(session_day_key(ny, est_00_00, 20 * 60, 0), None);

        // Overnight wrap past midnight (22:00 - 02:00): 23:00 and 01:00 next day share the same session key.
        let est_23_00 = est_20_00 + 3 * 3600;
        let est_01_00 = est_20_00 + 5 * 3600;
        let k1 = session_day_key(ny, est_23_00, 22 * 60, 2 * 60);
        let k2 = session_day_key(ny, est_01_00, 22 * 60, 2 * 60);
        assert!(k1.is_some());
        assert_eq!(k1, k2);

        // Summer (EDT = UTC-4): 2026-07-15 12:30 UTC is 08:30 EDT (NY AM open).
        let edt_08_30 = 1_784_118_600;
        assert!(session_day_key(ny, edt_08_30, 8 * 60 + 30, 11 * 60).is_some());
        assert!(session_day_key(ny, edt_08_30 - 60, 8 * 60 + 30, 11 * 60).is_none());

        // Switching to Asia/Kolkata (UTC+5:30): 09:15 IST is 03:45 UTC (2026-01-15 03:45 UTC).
        let ist = IanaTimeZone::parse("Asia/Kolkata").unwrap();
        let ist_09_15 = 1_768_448_700;
        assert!(session_day_key(ist, ist_09_15, 9 * 60 + 15, 15 * 60 + 30).is_some());
        assert!(session_day_key(ist, ist_09_15 - 60, 9 * 60 + 15, 15 * 60 + 30).is_none());
    }
}
