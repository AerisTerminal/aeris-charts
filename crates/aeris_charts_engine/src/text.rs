//! The engine-owned text catalog. Every user-visible string the engine generates is a key of
//! the compiled catalog under `catalog/<language>/<area>.lang` (compiled by `build.rs` into
//! the typed `text::<area>::<KEY>` constants), looked up by the chart's [`ChartLanguage`], or a
//! small function here that fills a key's placeholders. The build fails when a language lacks a
//! key, a placeholder or a plural form its grammar needs. Persisted identifiers (drawing
//! property and enum names, indicator output names, wave-degree ids) never come from this
//! catalog, and host-supplied strings are never translated.

use std::fmt::{Display, Write as _};

use aeris_charts_core::language::{ChartLanguage, PluralCategory};

include!(concat!(env!("OUT_DIR"), "/catalog.rs"));

/// One named value substituted for a `{name}` placeholder.
pub(crate) type Argument<'a> = (&'a str, &'a dyn Display);

/// A catalog text without placeholders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Phrase(u16);

impl Phrase {
    pub(crate) fn text(self, language: ChartLanguage) -> &'static str {
        PHRASES[usize::from(self.0)][column(language)]
    }
}

/// A catalog text with `{name}` placeholders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Template(u16);

impl Template {
    pub(crate) fn format(self, language: ChartLanguage, arguments: &[Argument<'_>]) -> String {
        substitute(TEMPLATES[usize::from(self.0)][column(language)], arguments)
    }
}

/// A count-dependent catalog text with one form per CLDR plural category.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Plural(u16);

impl Plural {
    /// The form for `count`, the magnitude of the counted value, with its placeholders filled
    /// in. `{count}` is an ordinary argument, so a caller can pass it pre-formatted.
    pub(crate) fn format(
        self,
        language: ChartLanguage,
        count: u64,
        arguments: &[Argument<'_>],
    ) -> String {
        let form = match language.plural_category(count) {
            PluralCategory::One => 0,
            PluralCategory::Few => 1,
            PluralCategory::Many => 2,
            PluralCategory::Other => 3,
        };
        substitute(
            PLURALS[usize::from(self.0)][column(language)][form],
            arguments,
        )
    }
}

/// The catalog column of a language: the build emits one column per language in
/// [`ChartLanguage::ALL`] order, which is the variant declaration order.
const fn column(language: ChartLanguage) -> usize {
    language as usize
}

fn substitute(text: &str, arguments: &[Argument<'_>]) -> String {
    let mut output = String::with_capacity(text.len() + 16);
    let mut rest = text;
    while let Some(position) = rest.find(['{', '}']) {
        output.push_str(&rest[..position]);
        let tail = &rest[position..];
        if tail.starts_with("{{") || tail.starts_with("}}") {
            output.push_str(&tail[..1]);
            rest = &tail[2..];
            continue;
        }
        let Some(end) = tail.find('}').filter(|_| tail.starts_with('{')) else {
            output.push_str(&tail[..1]);
            rest = &tail[1..];
            continue;
        };
        let name = &tail[1..end];
        let value = arguments.iter().find(|(argument, _)| *argument == name);
        debug_assert!(
            value.is_some(),
            "no value for the `{{{name}}}` placeholder in {text:?}"
        );
        match value {
            Some((_, value)) => {
                if write!(output, "{value}").is_err() {
                    output.push('?');
                }
            }
            None => output.push_str(&tail[..=end]),
        }
        rest = &tail[end + 1..];
    }
    output.push_str(rest);
    output
}

/// `{count} bar(s)` with the language's plural rules; the count arrives pre-formatted (it can
/// carry the typographic minus of the drawing statistics).
pub(crate) fn bars_count(language: ChartLanguage, formatted: &str, count: i64) -> String {
    drawings::BARS.format(language, count.unsigned_abs(), &[("count", &formatted)])
}

/// A fractional bar count with two decimals.
pub(crate) fn bars_fraction(language: ChartLanguage, value: f64) -> String {
    drawings::BARS_FRACTION.format(language, &[("value", &format!("{value:.2}"))])
}

/// `{n} fill(s)` of an aggregated execution marker.
pub(crate) fn fills_count(language: ChartLanguage, count: usize) -> String {
    trading::FILLS.format(language, count as u64, &[("count", &count)])
}

/// Elapsed time as its two most significant non-zero units (`1d 4h`, `45m`, `30s`).
pub(crate) fn measure_duration(language: ChartLanguage, seconds: i64) -> String {
    let units = [
        units::DURATION_DAYS,
        units::DURATION_HOURS,
        units::DURATION_MINUTES,
        units::DURATION_SECONDS,
    ];
    let magnitude = seconds.unsigned_abs();
    let parts = [
        magnitude / 86_400,
        magnitude % 86_400 / 3_600,
        magnitude % 3_600 / 60,
        magnitude % 60,
    ];
    let mut text = String::new();
    if seconds < 0 {
        text.push(aeris_charts_core::format::price_formatter::MINUS_SIGN);
    }
    let mut written = 0;
    for (value, unit) in parts.into_iter().zip(units) {
        if value == 0 || written == 2 {
            continue;
        }
        if written == 1 {
            text.push(' ');
        }
        text.push_str(&unit.format(language, &[("value", &value)]));
        written += 1;
    }
    if written == 0 {
        return units::DURATION_SECONDS.format(language, &[("value", &0)]);
    }
    text
}

/// The bar-close countdown by remaining magnitude: `mm:ss` below an hour, `hh:mm:ss` below a
/// day, `Xd Xh` at a day and beyond.
pub(crate) fn countdown(language: ChartLanguage, remaining: f64) -> String {
    let secs = remaining.max(0.0).floor() as u64;
    if secs < 3600 {
        format!("{:02}:{:02}", secs / 60, secs % 60)
    } else if secs < 86400 {
        format!(
            "{:02}:{:02}:{:02}",
            secs / 3600,
            secs % 3600 / 60,
            secs % 60
        )
    } else {
        let days = secs / 86400;
        if days > 9999 {
            units::COUNTDOWN_OVERFLOW.format(language, &[("days", &9999)])
        } else {
            format!(
                "{} {}",
                units::DURATION_DAYS.format(language, &[("value", &days)]),
                units::DURATION_HOURS.format(language, &[("value", &(secs % 86400 / 3600))])
            )
        }
    }
}

/// The display form of a persisted Elliott wave-degree id (`subminuette`..`grand_supercycle`);
/// an unknown id passes through in every language.
pub(crate) fn wave_degree(language: ChartLanguage, id: &str) -> String {
    let phrase = match id {
        "subminuette" => drawings::WAVE_DEGREE_SUBMINUETTE,
        "minuette" => drawings::WAVE_DEGREE_MINUETTE,
        "minute" => drawings::WAVE_DEGREE_MINUTE,
        "minor" => drawings::WAVE_DEGREE_MINOR,
        "intermediate" => drawings::WAVE_DEGREE_INTERMEDIATE,
        "primary" => drawings::WAVE_DEGREE_PRIMARY,
        "cycle" => drawings::WAVE_DEGREE_CYCLE,
        "supercycle" => drawings::WAVE_DEGREE_SUPERCYCLE,
        "grand_supercycle" => drawings::WAVE_DEGREE_GRAND_SUPERCYCLE,
        other => return other.to_string(),
    };
    phrase.text(language).to_string()
}

/// The open/high/low/close column labels of the financial legend.
pub(crate) fn ohlc_labels(language: ChartLanguage) -> [&'static str; 4] {
    [
        legend::OHLC_OPEN,
        legend::OHLC_HIGH,
        legend::OHLC_LOW,
        legend::OHLC_CLOSE,
    ]
    .map(|phrase| phrase.text(language))
}

/// The collapsed-rows marker of legends and tooltips (`+3 more`).
pub(crate) fn more_rows(language: ChartLanguage, hidden: usize) -> String {
    legend::MORE_ROWS.format(language, hidden as u64, &[("count", &hidden)])
}

/// The fallback title of an untitled general series (`Series 3`).
pub(crate) fn series_fallback(language: ChartLanguage, number: usize) -> String {
    legend::SERIES_FALLBACK.format(language, &[("number", &number)])
}

/// The long position tool's target line.
pub(crate) fn position_target_line(
    language: ChartLanguage,
    price: &str,
    reward_percent: f64,
    ticks: &str,
    amount: &str,
) -> String {
    drawings::POSITION_TARGET.format(
        language,
        &[
            ("price", &price),
            ("percent", &format!("{reward_percent:.3}")),
            ("ticks", &ticks),
            ("amount", &amount),
        ],
    )
}

/// The position tool's stop line.
pub(crate) fn position_stop_line(
    language: ChartLanguage,
    price: &str,
    risk_percent: f64,
    ticks: &str,
    amount: &str,
) -> String {
    drawings::POSITION_STOP.format(
        language,
        &[
            ("price", &price),
            ("percent", &format!("{risk_percent:.3}")),
            ("ticks", &ticks),
            ("amount", &amount),
        ],
    )
}

/// The position tool's open/closed P&L line.
pub(crate) fn position_pnl_line(
    language: ChartLanguage,
    closed: bool,
    pnl: &str,
    quantity: &str,
) -> String {
    let template = if closed {
        drawings::POSITION_CLOSED_PNL
    } else {
        drawings::POSITION_OPEN_PNL
    };
    template.format(language, &[("pnl", &pnl), ("quantity", &quantity)])
}

/// The position tool's risk/reward line.
pub(crate) fn risk_reward_line(language: ChartLanguage, ratio: &str) -> String {
    drawings::RISK_REWARD.format(language, &[("ratio", &ratio)])
}

/// The cancel-button tooltip of a partially filled working order.
pub(crate) fn cancel_remaining(language: ChartLanguage, quantity: &str) -> String {
    trading::CANCEL_REMAINING.format(language, &[("quantity", &quantity)])
}

/// The stop-limit trigger marker.
pub(crate) fn trigger_quantity(language: ChartLanguage, quantity: &str) -> String {
    trading::TRIGGER.format(language, &[("quantity", &quantity)])
}

/// The Info Line's pixel-distance row.
pub(crate) fn distance_px(language: ChartLanguage, px: i64) -> String {
    drawings::DISTANCE.format(language, &[("pixels", &px)])
}

/// The gantt task span summary.
pub(crate) fn gantt_task_span(
    language: ChartLanguage,
    start: &str,
    end: &str,
    percent: f64,
) -> String {
    general::GANTT_TASK.format(
        language,
        &[("start", &start), ("end", &end), ("percent", &percent)],
    )
}

/// A gantt milestone with its date.
pub(crate) fn gantt_milestone(language: ChartLanguage, date: &str) -> String {
    general::GANTT_MILESTONE.format(language, &[("date", &date)])
}

/// The mosaic accessibility label of one cell.
pub(crate) fn mosaic_cell(
    language: ChartLanguage,
    column: &str,
    segment: &str,
    share: f64,
) -> String {
    general::MOSAIC_CELL.format(
        language,
        &[
            ("column", &column),
            ("segment", &segment),
            ("share", &format!("{share:.1}")),
        ],
    )
}

/// The prompt an empty line-text drawing shows while selected.
pub(crate) fn line_text_placeholder(language: ChartLanguage) -> &'static str {
    drawings::LINE_TEXT_PLACEHOLDER.text(language)
}

/// The prompt an empty annotation shows.
pub(crate) fn annotation_placeholder(language: ChartLanguage) -> &'static str {
    drawings::ANNOTATION_PLACEHOLDER.text(language)
}

/// The label of one anchor of a drawing kind in its property schema.
pub(crate) fn anchor_label(
    language: ChartLanguage,
    kind: crate::DrawingKind,
    index: usize,
) -> String {
    use crate::DrawingKind;
    let phrase = match (kind, index) {
        (DrawingKind::LongPosition | DrawingKind::ShortPosition, 0) => drawings::ANCHOR_ENTRY,
        (DrawingKind::LongPosition | DrawingKind::ShortPosition, 1) => drawings::ANCHOR_TARGET,
        (DrawingKind::LongPosition | DrawingKind::ShortPosition, _) => drawings::ANCHOR_STOP,
        _ if kind.anchor_count() == 1 => drawings::ANCHOR_POINT,
        _ => {
            return drawings::ANCHOR_NUMBERED_POINT.format(language, &[("number", &(index + 1))]);
        }
    };
    phrase.text(language).to_string()
}

/// `{count} sample(s)` in a statistics tooltip, with the language's plural rules.
#[cfg(feature = "complete")]
pub(crate) fn samples_count(language: ChartLanguage, count: u32) -> String {
    complete::SAMPLES.format(language, u64::from(count), &[("count", &count)])
}

/// The tooltip detail of a waterfall bar's role: an increase or decrease with its span, or the
/// subtotal/total name.
pub(crate) fn waterfall_detail(
    language: ChartLanguage,
    mark: crate::GeneralWaterfallMark,
    start: &str,
    end: &str,
) -> String {
    use crate::GeneralWaterfallMark as Mark;
    let span = match mark {
        Mark::Increase => general::WATERFALL_INCREASE,
        Mark::Decrease => general::WATERFALL_DECREASE,
        Mark::Subtotal => return general::WATERFALL_SUBTOTAL.text(language).to_string(),
        Mark::Total => return general::WATERFALL_TOTAL.text(language).to_string(),
    };
    span.format(language, &[("start", &start), ("end", &end)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_columns_follow_the_language_order() {
        assert_eq!(CATALOG_LANGUAGES.len(), ChartLanguage::ALL.len());
        for (index, language) in ChartLanguage::ALL.into_iter().enumerate() {
            assert_eq!(CATALOG_LANGUAGES[index], language.code());
            assert_eq!(column(language), index);
        }
    }

    #[test]
    fn every_language_resolves_every_key_and_plural_form() {
        for language in ChartLanguage::ALL {
            let column = column(language);
            for row in PHRASES.iter().chain(&TEMPLATES) {
                assert!(!row[column].trim().is_empty(), "{language:?} {row:?}");
            }
            for row in &PLURALS {
                for form in row[column] {
                    assert!(!form.trim().is_empty(), "{language:?} {row:?}");
                }
            }
            // Every count selects a non-empty form through the language's plural rule.
            for count in [0, 1, 2, 5, 11, 21, 22, 25] {
                for index in 0..PLURALS.len() {
                    let plural = Plural(index as u16);
                    assert!(
                        !plural
                            .format(language, count, &[("count", &count)])
                            .is_empty()
                    );
                }
            }
        }
    }

    #[test]
    fn placeholders_take_named_values_and_braces_escape() {
        assert_eq!(
            substitute(
                "Buy {quantity} at {price}",
                &[("quantity", &3), ("price", &"1.5")]
            ),
            "Buy 3 at 1.5"
        );
        assert_eq!(substitute("{{literal}}", &[]), "{literal}");
    }

    #[test]
    fn russian_plurals_follow_the_one_few_many_rule() {
        let russian = |n: i64| bars_count(ChartLanguage::Russian, &n.to_string(), n);
        for n in [1, 21, 31, 101] {
            assert_eq!(russian(n), format!("{n} бар"), "{n}");
        }
        for n in [2, 3, 4, 22, 54] {
            assert_eq!(russian(n), format!("{n} бара"), "{n}");
        }
        for n in [0, 5, 11, 12, 14, 25, 111] {
            assert_eq!(russian(n), format!("{n} баров"), "{n}");
        }
        assert_eq!(bars_count(ChartLanguage::Russian, "1", 1), "1 бар");
        assert_eq!(bars_count(ChartLanguage::Russian, "3", 3), "3 бара");
        assert_eq!(bars_count(ChartLanguage::Russian, "12", 12), "12 баров");
        assert_eq!(bars_count(ChartLanguage::English, "1", 1), "1 bar");
        assert_eq!(bars_count(ChartLanguage::English, "2", 2), "2 bars");
        // The plural operand is the magnitude, so a leftward measure of one bar is singular.
        assert_eq!(bars_count(ChartLanguage::English, "−1", -1), "−1 bar");
        assert_eq!(bars_count(ChartLanguage::Russian, "−2", -2), "−2 бара");
        assert_eq!(fills_count(ChartLanguage::Russian, 2), "2 сделки");
        assert_eq!(fills_count(ChartLanguage::English, 1), "1 fill");
        assert_eq!(bars_fraction(ChartLanguage::Russian, 1.5), "1.50 бара");
    }

    #[test]
    fn durations_and_countdowns_use_the_language_units() {
        assert_eq!(measure_duration(ChartLanguage::English, 90_061), "1d 1h");
        assert_eq!(measure_duration(ChartLanguage::Russian, 90_061), "1д 1ч");
        assert_eq!(measure_duration(ChartLanguage::Russian, 0), "0с");
        assert_eq!(countdown(ChartLanguage::English, 91_800.0), "1d 1h");
        assert_eq!(countdown(ChartLanguage::Russian, 91_800.0), "1д 1ч");
        assert_eq!(countdown(ChartLanguage::Russian, 59.0), "00:59");
        assert_eq!(countdown(ChartLanguage::English, 1e12), "9999d+");
        assert_eq!(countdown(ChartLanguage::Russian, 1e12), "9999д+");
    }

    #[test]
    fn trading_phrases_localize() {
        assert_eq!(trading::BUY.text(ChartLanguage::Russian), "Покупка");
        assert_eq!(trading::SELL.text(ChartLanguage::Russian), "Продажа");
        assert_eq!(
            trading::STOP_LIMIT.text(ChartLanguage::Russian),
            "Стоп-лимит"
        );
        assert_eq!(
            risk_reward_line(ChartLanguage::Russian, "2.50"),
            "Соотношение риск/прибыль: 2.50"
        );
        assert_eq!(
            trading::CLOSE_POSITION.text(ChartLanguage::Russian),
            "Закрыть позицию"
        );
    }

    #[test]
    fn whole_sentence_keys_keep_the_english_and_russian_text() {
        let (en, ru) = (ChartLanguage::English, ChartLanguage::Russian);
        assert_eq!(
            position_target_line(en, "1.5", 2.0, "15", "1,000.00"),
            "Target: 1.5 (2.000%) 15, Amount: 1,000.00"
        );
        assert_eq!(
            position_stop_line(ru, "1.5", 2.0, "15", "1,000.00"),
            "Стоп: 1.5 (2.000%) 15, Сумма: 1,000.00"
        );
        assert_eq!(
            position_pnl_line(en, true, "5", "2"),
            "Closed P&L: 5, Qty: 2"
        );
        assert_eq!(
            position_pnl_line(ru, false, "5", "2"),
            "Открыта P&L: 5, Кол-во: 2"
        );
        assert_eq!(distance_px(ru, 12), "расстояние: 12 пикс.");
        assert_eq!(more_rows(en, 3), "+3 more");
        assert_eq!(more_rows(ru, 3), "ещё 3");
        assert_eq!(mosaic_cell(en, "A", "B", 12.345), "A, B, 12.3% of column");
        assert_eq!(gantt_task_span(ru, "1", "2", 50.0), "1 – 2 · выполнено 50%");
        assert_eq!(wave_degree(en, "grand_supercycle"), "grand_supercycle");
        assert_eq!(wave_degree(ru, "minor"), "младшая");
        assert_eq!(wave_degree(ru, "custom"), "custom");
        assert_eq!(ohlc_labels(en), ["O", "H", "L", "C"]);
        assert_eq!(
            waterfall_detail(en, crate::GeneralWaterfallMark::Increase, "1", "2"),
            "increase 1 → 2"
        );
        assert_eq!(
            waterfall_detail(ru, crate::GeneralWaterfallMark::Total, "1", "2"),
            "итог"
        );
    }
}
