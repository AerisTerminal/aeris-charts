//! Time-axis label formatting. Ports of
//! `src/model/horz-scale-behavior-time/default-tick-mark-formatter.ts` (en-US behavior;
//! an `Intl`-backed locale hook comes with the JS API) and the crosshair date-time format
//! (`DateTimeFormatter` with the reference's default `dd MMM 'yy` date format).
//!
//! The date-format pattern language is tokenized here (reference `localization.dateFormat`,
//! formatters/format-date.ts): `dd`/`d` day, `MM`/`M`/`MMM`/`MMMM` month, `yy`/`yyyy` year,
//! with ICU-style `'…'` quoted literals (`''` is an escaped quote). Month names come from a
//! per-locale table (reference `localization.locale`); the headless default is English and hosts
//! inject `Intl`-derived names.

use std::sync::LazyLock;

use crate::language::ChartLanguage;
use crate::scale::time_tick_marks::{TickMarkWeight, civil_from_timestamp};
use crate::time_zone::{ChartTimeZone, LocalTimeParts};

/// The built-in stand-alone month names of a language: the abbreviated and the wide forms.
/// Languages with letter case start each name with a capital, as axis labels do.
const fn builtin_month_names(language: ChartLanguage) -> [[&'static str; 12]; 2] {
    match language {
        ChartLanguage::English => [
            [
                "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
            ],
            [
                "January",
                "February",
                "March",
                "April",
                "May",
                "June",
                "July",
                "August",
                "September",
                "October",
                "November",
                "December",
            ],
        ],
        ChartLanguage::German => [
            [
                "Jan", "Feb", "Mär", "Apr", "Mai", "Jun", "Jul", "Aug", "Sep", "Okt", "Nov", "Dez",
            ],
            [
                "Januar",
                "Februar",
                "März",
                "April",
                "Mai",
                "Juni",
                "Juli",
                "August",
                "September",
                "Oktober",
                "November",
                "Dezember",
            ],
        ],
        ChartLanguage::Spanish => [
            [
                "Ene", "Feb", "Mar", "Abr", "May", "Jun", "Jul", "Ago", "Sept", "Oct", "Nov", "Dic",
            ],
            [
                "Enero",
                "Febrero",
                "Marzo",
                "Abril",
                "Mayo",
                "Junio",
                "Julio",
                "Agosto",
                "Septiembre",
                "Octubre",
                "Noviembre",
                "Diciembre",
            ],
        ],
        ChartLanguage::French => [
            [
                "Janv.", "Févr.", "Mars", "Avr.", "Mai", "Juin", "Juil.", "Août", "Sept.", "Oct.",
                "Nov.", "Déc.",
            ],
            [
                "Janvier",
                "Février",
                "Mars",
                "Avril",
                "Mai",
                "Juin",
                "Juillet",
                "Août",
                "Septembre",
                "Octobre",
                "Novembre",
                "Décembre",
            ],
        ],
        ChartLanguage::Portuguese => [
            [
                "Jan.", "Fev.", "Mar.", "Abr.", "Mai.", "Jun.", "Jul.", "Ago.", "Set.", "Out.",
                "Nov.", "Dez.",
            ],
            [
                "Janeiro",
                "Fevereiro",
                "Março",
                "Abril",
                "Maio",
                "Junho",
                "Julho",
                "Agosto",
                "Setembro",
                "Outubro",
                "Novembro",
                "Dezembro",
            ],
        ],
        ChartLanguage::Russian => [
            [
                "Янв", "Фев", "Мар", "Апр", "Май", "Июн", "Июл", "Авг", "Сен", "Окт", "Ноя", "Дек",
            ],
            [
                "Январь",
                "Февраль",
                "Март",
                "Апрель",
                "Май",
                "Июнь",
                "Июль",
                "Август",
                "Сентябрь",
                "Октябрь",
                "Ноябрь",
                "Декабрь",
            ],
        ],
        ChartLanguage::Hindi => [
            [
                "जन॰",
                "फ़र॰",
                "मार्च",
                "अप्रैल",
                "मई",
                "जून",
                "जुल॰",
                "अग॰",
                "सित॰",
                "अक्तू॰",
                "नव॰",
                "दिस॰",
            ],
            [
                "जनवरी",
                "फ़रवरी",
                "मार्च",
                "अप्रैल",
                "मई",
                "जून",
                "जुलाई",
                "अगस्त",
                "सितंबर",
                "अक्तूबर",
                "नवंबर",
                "दिसंबर",
            ],
        ],
        ChartLanguage::ChineseSimplified => [
            [
                "1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月",
                "12月",
            ],
            [
                "一月",
                "二月",
                "三月",
                "四月",
                "五月",
                "六月",
                "七月",
                "八月",
                "九月",
                "十月",
                "十一月",
                "十二月",
            ],
        ],
        ChartLanguage::Japanese => [
            [
                "1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月",
                "12月",
            ],
            [
                "1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月",
                "12月",
            ],
        ],
        ChartLanguage::Korean => [
            [
                "1월", "2월", "3월", "4월", "5월", "6월", "7월", "8월", "9월", "10월", "11월",
                "12월",
            ],
            [
                "1월", "2월", "3월", "4월", "5월", "6월", "7월", "8월", "9월", "10월", "11월",
                "12월",
            ],
        ],
    }
}

/// The numeric day/month/year pattern of a language (tokens as in [`format_date_pattern`]),
/// which the axis month tag under each bar uses.
pub const fn numeric_date_pattern(language: ChartLanguage) -> &'static str {
    match language {
        ChartLanguage::English => "M/d/yyyy",
        ChartLanguage::German | ChartLanguage::Russian => "d.M.yyyy",
        ChartLanguage::Spanish
        | ChartLanguage::French
        | ChartLanguage::Portuguese
        | ChartLanguage::Hindi => "d/M/yyyy",
        ChartLanguage::ChineseSimplified | ChartLanguage::Japanese => "yyyy/M/d",
        ChartLanguage::Korean => "yyyy. M. d.",
    }
}

/// The short weekday headers of a language, Sunday first, as a calendar chart shows them
/// without host labels.
pub const fn weekday_initials(language: ChartLanguage) -> [&'static str; 7] {
    match language {
        ChartLanguage::English => ["S", "M", "T", "W", "T", "F", "S"],
        ChartLanguage::German => ["S", "M", "D", "M", "D", "F", "S"],
        ChartLanguage::Spanish => ["D", "L", "M", "X", "J", "V", "S"],
        ChartLanguage::French => ["D", "L", "M", "M", "J", "V", "S"],
        ChartLanguage::Portuguese => ["D", "S", "T", "Q", "Q", "S", "S"],
        ChartLanguage::Russian => ["В", "П", "В", "С", "Ч", "П", "С"],
        ChartLanguage::Hindi => ["र", "सो", "मं", "बु", "गु", "शु", "श"],
        ChartLanguage::ChineseSimplified => ["日", "一", "二", "三", "四", "五", "六"],
        ChartLanguage::Japanese => ["日", "月", "火", "水", "木", "金", "土"],
        ChartLanguage::Korean => ["일", "월", "화", "수", "목", "금", "토"],
    }
}

/// the reference's default `localization.dateFormat` (chart-options-defaults.ts:36).
pub const DEFAULT_DATE_FORMAT: &str = "dd MMM 'yy";

/// Per-locale month-name tables backing the `MMM`/`MMMM` date-format tokens and the month
/// tick labels (reference `localization.locale`). The engine stays headless: hosts generate the
/// names (browser builds use `Intl.DateTimeFormat`) and inject them; the default is English.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonthNames {
    pub short: [String; 12],
    pub long: [String; 12],
}

impl MonthNames {
    /// The built-in English tables (the reference's en-US `toLocaleString` output).
    pub fn english() -> Self {
        Self::of(ChartLanguage::English)
    }

    /// The built-in tables of a display language.
    pub fn of(language: ChartLanguage) -> Self {
        let [short, long] = builtin_month_names(language);
        Self {
            short: short.map(String::from),
            long: long.map(String::from),
        }
    }
}

impl Default for MonthNames {
    fn default() -> Self {
        Self::english()
    }
}

/// The shared English tables (allocated once) for callers that never install a locale.
pub fn english_months() -> &'static MonthNames {
    language_months(ChartLanguage::English)
}

/// The shared tables of a display language (each allocated once).
pub fn language_months(language: ChartLanguage) -> &'static MonthNames {
    static TABLES: LazyLock<[MonthNames; 10]> =
        LazyLock::new(|| ChartLanguage::ALL.map(MonthNames::of));
    // `ChartLanguage::ALL` lists the variants in declaration order.
    &TABLES[language as usize]
}

/// Port of `TickMarkType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TickMarkType {
    Year,
    Month,
    DayOfMonth,
    Time,
    TimeWithSeconds,
}

/// Port of `weightToTickMarkType` (`horz-scale-behavior-time.ts`).
pub fn weight_to_tick_mark_type(
    weight: u8,
    time_visible: bool,
    seconds_visible: bool,
) -> TickMarkType {
    let w = weight;
    if w <= TickMarkWeight::Second as u8 {
        if time_visible {
            if seconds_visible {
                TickMarkType::TimeWithSeconds
            } else {
                TickMarkType::Time
            }
        } else {
            TickMarkType::DayOfMonth
        }
    } else if w < TickMarkWeight::Day as u8 {
        // minutes and hours
        if time_visible {
            TickMarkType::Time
        } else {
            TickMarkType::DayOfMonth
        }
    } else if w < TickMarkWeight::Month as u8 {
        TickMarkType::DayOfMonth
    } else if w < TickMarkWeight::Year as u8 {
        TickMarkType::Month
    } else {
        TickMarkType::Year
    }
}

fn hms(ts: i64) -> (i64, i64, i64) {
    let secs_of_day = ts.rem_euclid(86_400);
    (
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60,
    )
}

fn localized_parts(ts: i64, time_zone: ChartTimeZone) -> LocalTimeParts {
    time_zone.local_parts(ts).unwrap_or_else(|| {
        let (year, month, day) = civil_from_timestamp(ts);
        let (hour, minute, second) = hms(ts);
        LocalTimeParts {
            year: year as i32,
            month,
            day,
            hour: hour as u32,
            minute: minute as u32,
            second: second as u32,
            offset_seconds: 0,
        }
    })
}

/// Tick label for a UTC timestamp — matches `defaultTickMarkFormatter` output for en-US.
pub fn format_tick_label(ts: i64, mark_type: TickMarkType) -> String {
    format_tick_label_with(ts, mark_type, english_months())
}

/// [`format_tick_label`] with per-locale month names for the `Month` mark (reference
/// `localization.locale` applied to the time axis).
pub fn format_tick_label_with(ts: i64, mark_type: TickMarkType, months: &MonthNames) -> String {
    format_tick_label_with_time_zone(ts, mark_type, months, ChartTimeZone::default())
}

pub fn format_tick_label_with_time_zone(
    ts: i64,
    mark_type: TickMarkType,
    months: &MonthNames,
    time_zone: ChartTimeZone,
) -> String {
    let parts = localized_parts(ts, time_zone);
    let (year, month, day) = (parts.year, parts.month, parts.day);
    match mark_type {
        TickMarkType::Year => format!("{year}"),
        TickMarkType::Month => months.short[(month - 1) as usize].clone(),
        TickMarkType::DayOfMonth => format!("{day}"),
        TickMarkType::Time => format!("{:02}:{:02}", parts.hour, parts.minute),
        TickMarkType::TimeWithSeconds => {
            format!("{:02}:{:02}:{:02}", parts.hour, parts.minute, parts.second)
        }
    }
}

/// Format a UTC timestamp through an reference `localization.dateFormat` pattern
/// (formatters/format-date.ts). Tokens: `d`/`dd` day of month, `M`/`MM` month number,
/// `MMM` month short name, `MMMM` month long name, `yy` two-digit year, `yyyy` four-digit
/// year. ICU-style `'…'` spans are literal text (`''` is an escaped quote). reference implements
/// the replacement naively, so its default `dd MMM 'yy` (an *unterminated* quote) keeps the
/// apostrophe and still replaces `yy` — an unterminated quote here is likewise literal and
/// tokenization continues after it. Token runs longer than the forms above degrade
/// gracefully: `d`/`M` runs pad to two digits at `dd`/`MM`, name forms cap at `MMMM`, and
/// `y` runs other than 2/4 pass through literally.
pub fn format_date_pattern(ts: i64, pattern: &str, months: &MonthNames) -> String {
    format_date_pattern_with_time_zone(ts, pattern, months, ChartTimeZone::default())
}

pub fn format_date_pattern_with_time_zone(
    ts: i64,
    pattern: &str,
    months: &MonthNames,
    time_zone: ChartTimeZone,
) -> String {
    let parts = localized_parts(ts, time_zone);
    let (year, month, day) = (i64::from(parts.year), parts.month, parts.day);
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            if chars.get(i + 1) == Some(&'\'') {
                out.push('\'');
                i += 2;
                continue;
            }
            // Find the closing quote, skipping escaped '' pairs.
            let mut j = i + 1;
            let mut closing = None;
            while j < chars.len() {
                if chars[j] == '\'' {
                    if chars.get(j + 1) == Some(&'\'') {
                        j += 2;
                        continue;
                    }
                    closing = Some(j);
                    break;
                }
                j += 1;
            }
            match closing {
                Some(end) => {
                    // Quoted literal span ('' inside is an escaped quote).
                    let mut k = i + 1;
                    while k < end {
                        if chars[k] == '\'' {
                            out.push('\'');
                            k += 2;
                        } else {
                            out.push(chars[k]);
                            k += 1;
                        }
                    }
                    i = end + 1;
                }
                None => {
                    // Unterminated quote: literal apostrophe (reference naive-replace parity).
                    out.push('\'');
                    i += 1;
                }
            }
            continue;
        }
        // maximal-munch token run of the same letter
        let mut run = 1;
        while chars.get(i + run) == Some(&c) {
            run += 1;
        }
        match c {
            'd' => {
                if run == 1 {
                    out.push_str(&day.to_string());
                } else {
                    out.push_str(&format!("{day:02}"));
                    out.extend(std::iter::repeat_n('d', run - 2));
                }
            }
            'M' => match run {
                1 => out.push_str(&month.to_string()),
                2 => out.push_str(&format!("{month:02}")),
                3 => out.push_str(&months.short[(month - 1) as usize]),
                _ => {
                    out.push_str(&months.long[(month - 1) as usize]);
                    out.extend(std::iter::repeat_n('M', run - 4));
                }
            },
            'y' => match run {
                2 => out.push_str(&format!("{:02}", year.rem_euclid(100))),
                4 => out.push_str(&format!("{year:04}")),
                _ => out.extend(std::iter::repeat_n('y', run)),
            },
            _ => out.extend(std::iter::repeat_n(c, run)),
        }
        i += run;
    }
    out
}

/// Crosshair time label: the reference's default `dd MMM 'yy` date format, plus `HH:MM` when the
/// time is visible (`DateTimeFormatter` joins them with spaces).
pub fn format_crosshair_time(ts: i64, time_visible: bool, seconds_visible: bool) -> String {
    format_crosshair_time_with(
        ts,
        time_visible,
        seconds_visible,
        DEFAULT_DATE_FORMAT,
        english_months(),
    )
}

/// [`format_crosshair_time`] with an explicit `localization.dateFormat` pattern and
/// per-locale month names.
pub fn format_crosshair_time_with(
    ts: i64,
    time_visible: bool,
    seconds_visible: bool,
    date_format: &str,
    months: &MonthNames,
) -> String {
    format_crosshair_time_with_time_zone(
        ts,
        time_visible,
        seconds_visible,
        date_format,
        months,
        ChartTimeZone::default(),
    )
}

pub fn format_crosshair_time_with_time_zone(
    ts: i64,
    time_visible: bool,
    seconds_visible: bool,
    date_format: &str,
    months: &MonthNames,
    time_zone: ChartTimeZone,
) -> String {
    let date = format_date_pattern_with_time_zone(ts, date_format, months, time_zone);
    if !time_visible {
        return date;
    }
    let parts = localized_parts(ts, time_zone);
    let (h, m, s) = (parts.hour, parts.minute, parts.second);
    if seconds_visible {
        format!("{date}   {h:02}:{m:02}:{s:02}")
    } else {
        format!("{date}   {h:02}:{m:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2018-06-25T14:30:45Z
    const TS: i64 = 1_529_937_045;

    #[test]
    fn tick_labels() {
        assert_eq!(format_tick_label(TS, TickMarkType::Year), "2018");
        assert_eq!(format_tick_label(TS, TickMarkType::Month), "Jun");
        assert_eq!(format_tick_label(TS, TickMarkType::DayOfMonth), "25");
        assert_eq!(format_tick_label(TS, TickMarkType::Time), "14:30");
        assert_eq!(
            format_tick_label(TS, TickMarkType::TimeWithSeconds),
            "14:30:45"
        );
    }

    #[test]
    fn time_zone_changes_tick_and_crosshair_calendar_text() {
        let new_york = ChartTimeZone::parse("America/New_York").unwrap();
        // 2026-01-02 04:30 UTC = 2026-01-01 23:30 EST.
        let ts = 1_767_328_200;
        assert_eq!(
            format_tick_label_with_time_zone(ts, TickMarkType::Time, english_months(), new_york),
            "23:30"
        );
        assert_eq!(
            format_date_pattern_with_time_zone(ts, "dd MMM yyyy", english_months(), new_york),
            "01 Jan 2026"
        );
        assert_eq!(
            format_crosshair_time_with_time_zone(
                ts,
                true,
                false,
                "dd MMM yyyy",
                english_months(),
                new_york,
            ),
            "01 Jan 2026   23:30"
        );
    }

    #[test]
    fn weight_mapping() {
        // intraday weights show time when timeVisible
        assert_eq!(
            weight_to_tick_mark_type(TickMarkWeight::Hour1 as u8, true, false),
            TickMarkType::Time
        );
        assert_eq!(
            weight_to_tick_mark_type(TickMarkWeight::Hour1 as u8, false, false),
            TickMarkType::DayOfMonth
        );
        assert_eq!(
            weight_to_tick_mark_type(TickMarkWeight::Day as u8, true, true),
            TickMarkType::DayOfMonth
        );
        assert_eq!(
            weight_to_tick_mark_type(TickMarkWeight::Month as u8, true, true),
            TickMarkType::Month
        );
        assert_eq!(
            weight_to_tick_mark_type(TickMarkWeight::Year as u8, false, false),
            TickMarkType::Year
        );
        assert_eq!(
            weight_to_tick_mark_type(TickMarkWeight::Second as u8, true, true),
            TickMarkType::TimeWithSeconds
        );
    }

    #[test]
    fn crosshair_format() {
        assert_eq!(format_crosshair_time(TS, false, false), "25 Jun '18");
        assert_eq!(format_crosshair_time(TS, true, false), "25 Jun '18   14:30");
        assert_eq!(
            format_crosshair_time(TS, true, true),
            "25 Jun '18   14:30:45"
        );
    }

    #[test]
    fn date_pattern_tokens() {
        let months = MonthNames::english();
        // the reference's documented token set (localization-options.ts dateFormat).
        assert_eq!(format_date_pattern(TS, "yyyy-MM-dd", &months), "2018-06-25");
        assert_eq!(format_date_pattern(TS, "dd MMM 'yy", &months), "25 Jun '18");
        assert_eq!(format_date_pattern(TS, "d M yy", &months), "25 6 18");
        assert_eq!(
            format_date_pattern(TS, "MMMM d, yyyy", &months),
            "June 25, 2018"
        );
        // Literal quoting: a closed '…' span passes through untouched, '' escapes a quote.
        assert_eq!(
            format_date_pattern(TS, "dd 'of' MMMM", &months),
            "25 of June"
        );
        assert_eq!(format_date_pattern(TS, "dd 'MM'", &months), "25 MM");
        assert_eq!(format_date_pattern(TS, "dd ''dd''", &months), "25 '25'");
        // Non-token letters are literal; an unterminated quote is a literal apostrophe and
        // tokenization continues (reference naive-replace parity — the default pattern relies on it).
        assert_eq!(format_date_pattern(TS, "yyyy/MM/dd", &months), "2018/06/25");
        assert_eq!(format_date_pattern(TS, "dd 'MM", &months), "25 '06");
        // Degenerate runs: single y is literal, long M runs cap at the long name.
        assert_eq!(format_date_pattern(TS, "y", &months), "y");
        assert_eq!(format_date_pattern(TS, "MMMMM", &months), "JuneM");
    }

    #[test]
    fn locale_month_names_drive_tokens_and_ticks() {
        let mut months = MonthNames::english();
        months.short[5] = "Jun.".to_string();
        months.long[5] = "Juni".to_string();
        assert_eq!(
            format_date_pattern(TS, "dd MMM yyyy", &months),
            "25 Jun. 2018"
        );
        assert_eq!(format_date_pattern(TS, "MMMM", &months), "Juni");
        assert_eq!(
            format_tick_label_with(TS, TickMarkType::Month, &months),
            "Jun."
        );
        // The default stays English.
        assert_eq!(format_date_pattern(TS, "MMMM", english_months()), "June");
    }

    #[test]
    fn every_language_has_complete_builtin_date_tables() {
        for (index, language) in ChartLanguage::ALL.into_iter().enumerate() {
            assert_eq!(language as usize, index, "{language:?}");
            let months = language_months(language);
            assert_eq!(*months, MonthNames::of(language));
            assert!(
                months
                    .short
                    .iter()
                    .chain(&months.long)
                    .all(|name| !name.is_empty()),
                "{language:?}"
            );
            assert!(
                weekday_initials(language).iter().all(|day| !day.is_empty()),
                "{language:?}"
            );
        }
        assert_eq!(language_months(ChartLanguage::Russian).short[0], "Янв");
        assert_eq!(language_months(ChartLanguage::Russian).long[11], "Декабрь");
        assert_eq!(language_months(ChartLanguage::German).long[2], "März");
        assert_eq!(
            language_months(ChartLanguage::ChineseSimplified).long[10],
            "十一月"
        );
        let tag = |language| {
            format_date_pattern(
                TS,
                numeric_date_pattern(language),
                language_months(language),
            )
        };
        assert_eq!(tag(ChartLanguage::English), "6/25/2018");
        assert_eq!(tag(ChartLanguage::German), "25.6.2018");
        assert_eq!(tag(ChartLanguage::Russian), "25.6.2018");
        assert_eq!(tag(ChartLanguage::French), "25/6/2018");
        assert_eq!(tag(ChartLanguage::Japanese), "2018/6/25");
        assert_eq!(tag(ChartLanguage::Korean), "2018. 6. 25.");
        assert_eq!(
            weekday_initials(ChartLanguage::Japanese),
            ["日", "月", "火", "水", "木", "金", "土"]
        );
    }
}
