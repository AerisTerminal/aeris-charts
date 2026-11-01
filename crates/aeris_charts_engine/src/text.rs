//! The engine-owned text catalog: every user-visible string the engine generates is a
//! [`Phrase`] or a small function here, looked up by the chart's [`ChartLanguage`]. A phrase
//! carries every supported language, so a missing translation fails to compile. Persisted
//! identifiers (drawing property and enum names, indicator output names, wave-degree ids) never
//! come from this catalog, and host-supplied strings are never translated.

use aeris_charts_core::language::ChartLanguage;

/// One user-visible string in every supported language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Phrase {
    pub english: &'static str,
    pub russian: &'static str,
}

impl Phrase {
    pub(crate) const fn new(english: &'static str, russian: &'static str) -> Self {
        Self { english, russian }
    }

    pub(crate) const fn text(self, language: ChartLanguage) -> &'static str {
        match language {
            ChartLanguage::English => self.english,
            ChartLanguage::Russian => self.russian,
        }
    }
}

/// The Russian plural form of a countable noun: one (1, 21, …), few (2-4, 22-24, …) or many.
pub(crate) fn russian_plural<'a>(count: i64, one: &'a str, few: &'a str, many: &'a str) -> &'a str {
    let n = count.unsigned_abs();
    if (11..=14).contains(&(n % 100)) {
        return many;
    }
    match n % 10 {
        1 => one,
        2..=4 => few,
        _ => many,
    }
}

/// `{count} bar(s)` with the language's plural rules; the count arrives pre-formatted (it can
/// carry the typographic minus of the drawing statistics).
pub(crate) fn bars_count(language: ChartLanguage, formatted: &str, count: i64) -> String {
    match language {
        ChartLanguage::English => {
            format!("{formatted} {}", if count == 1 { "bar" } else { "bars" })
        }
        ChartLanguage::Russian => {
            format!(
                "{formatted} {}",
                russian_plural(count, "бар", "бара", "баров")
            )
        }
    }
}

/// A fractional bar count (`{:.2} bars`): a Russian fraction takes the genitive singular.
pub(crate) fn bars_fraction(language: ChartLanguage, value: f64) -> String {
    match language {
        ChartLanguage::English => format!("{value:.2} bars"),
        ChartLanguage::Russian => format!("{value:.2} бара"),
    }
}

/// `{n} fill(s)` of an aggregated execution marker.
pub(crate) fn fills_count(language: ChartLanguage, count: usize) -> String {
    match language {
        ChartLanguage::English => format!("{count} {}", if count == 1 { "fill" } else { "fills" }),
        ChartLanguage::Russian => format!(
            "{count} {}",
            russian_plural(count as i64, "сделка", "сделки", "сделок")
        ),
    }
}

/// Elapsed time as its two most significant non-zero units (`1d 4h`, `45m`, `30s`).
pub(crate) fn measure_duration(language: ChartLanguage, seconds: i64) -> String {
    let units: [&Phrase; 4] = [&UNIT_DAY, &UNIT_HOUR, &UNIT_MINUTE, &UNIT_SECOND];
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
        text.push_str(&format!("{value}{}", unit.text(language)));
        written += 1;
    }
    if written == 0 {
        return match language {
            ChartLanguage::English => "0s".to_string(),
            ChartLanguage::Russian => "0с".to_string(),
        };
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
            match language {
                ChartLanguage::English => "9999d+".to_string(),
                ChartLanguage::Russian => "9999д+".to_string(),
            }
        } else {
            format!(
                "{}{} {}{}",
                days,
                UNIT_DAY.text(language),
                secs % 86400 / 3600,
                UNIT_HOUR.text(language)
            )
        }
    }
}

/// The date pattern of the axis month tag under each bar (reference `M/d/yyyy`; the Russian
/// convention is day-first numeric).
pub(crate) const fn bar_month_tag_pattern(language: ChartLanguage) -> &'static str {
    match language {
        ChartLanguage::English => "M/d/yyyy",
        ChartLanguage::Russian => "d.M.yyyy",
    }
}

/// The one-letter weekday headers (Sunday first) a calendar chart falls back to without host
/// labels.
pub(crate) fn default_day_labels(language: ChartLanguage) -> [&'static str; 7] {
    match language {
        ChartLanguage::English => ["S", "M", "T", "W", "T", "F", "S"],
        ChartLanguage::Russian => ["В", "П", "В", "С", "Ч", "П", "С"],
    }
}

/// The display form of a persisted Elliott wave-degree id (`subminuette`..`grand_supercycle`).
/// English keeps the id itself; an unknown id passes through in every language.
pub(crate) fn wave_degree(language: ChartLanguage, id: &str) -> String {
    if language == ChartLanguage::English {
        return id.to_string();
    }
    match id {
        "subminuette" => "субминуэтка",
        "minuette" => "минуэтка",
        "minute" => "минутка",
        "minor" => "младшая",
        "intermediate" => "промежуточная",
        "primary" => "первичная",
        "cycle" => "цикловая",
        "supercycle" => "суперцикловая",
        "grand_supercycle" => "гранд-суперцикловая",
        other => other,
    }
    .to_string()
}

/// The open/high/low/close column labels of the financial legend.
pub(crate) fn ohlc_labels(language: ChartLanguage) -> [&'static str; 4] {
    match language {
        ChartLanguage::English => ["O", "H", "L", "C"],
        ChartLanguage::Russian => ["Откр", "Макс", "Мин", "Закр"],
    }
}

/// The collapsed-rows marker of legends and tooltips (`+3 more`).
pub(crate) fn more_rows(language: ChartLanguage, hidden: usize) -> String {
    match language {
        ChartLanguage::English => format!("+{hidden} more"),
        ChartLanguage::Russian => format!("ещё {hidden}"),
    }
}

/// The fallback title of an untitled general series (`Series 3`).
pub(crate) fn series_fallback(language: ChartLanguage, number: usize) -> String {
    match language {
        ChartLanguage::English => format!("Series {number}"),
        ChartLanguage::Russian => format!("Серия {number}"),
    }
}

/// The long position tool's target line.
pub(crate) fn position_target_line(
    language: ChartLanguage,
    price: &str,
    reward_percent: f64,
    ticks: &str,
    amount: &str,
) -> String {
    match language {
        ChartLanguage::English => {
            format!("Target: {price} ({reward_percent:.3}%) {ticks}, Amount: {amount}")
        }
        ChartLanguage::Russian => {
            format!("Цель: {price} ({reward_percent:.3}%) {ticks}, Сумма: {amount}")
        }
    }
}

/// The position tool's stop line.
pub(crate) fn position_stop_line(
    language: ChartLanguage,
    price: &str,
    risk_percent: f64,
    ticks: &str,
    amount: &str,
) -> String {
    match language {
        ChartLanguage::English => {
            format!("Stop: {price} ({risk_percent:.3}%) {ticks}, Amount: {amount}")
        }
        ChartLanguage::Russian => {
            format!("Стоп: {price} ({risk_percent:.3}%) {ticks}, Сумма: {amount}")
        }
    }
}

/// The position tool's open/closed P&L line.
pub(crate) fn position_pnl_line(
    language: ChartLanguage,
    closed: bool,
    pnl: &str,
    quantity: &str,
) -> String {
    let status = if closed { CLOSED } else { OPEN }.text(language);
    match language {
        ChartLanguage::English => format!("{status} P&L: {pnl}, Qty: {quantity}"),
        ChartLanguage::Russian => format!("{status} P&L: {pnl}, Кол-во: {quantity}"),
    }
}

/// The position tool's risk/reward line.
pub(crate) fn risk_reward_line(language: ChartLanguage, ratio: &str) -> String {
    match language {
        ChartLanguage::English => format!("Risk/reward ratio: {ratio}"),
        ChartLanguage::Russian => format!("Соотношение риск/прибыль: {ratio}"),
    }
}

/// The cancel-button tooltip of a partially filled working order.
pub(crate) fn cancel_remaining(language: ChartLanguage, quantity: &str) -> String {
    match language {
        ChartLanguage::English => format!("Cancel remaining {quantity}"),
        ChartLanguage::Russian => format!("Отменить оставшиеся {quantity}"),
    }
}

/// The stop-limit trigger marker.
pub(crate) fn trigger_quantity(language: ChartLanguage, quantity: &str) -> String {
    match language {
        ChartLanguage::English => format!("Trigger {quantity}"),
        ChartLanguage::Russian => format!("Триггер {quantity}"),
    }
}

/// The Info Line's pixel-distance row.
pub(crate) fn distance_px(language: ChartLanguage, px: i64) -> String {
    match language {
        ChartLanguage::English => format!("distance: {px} px"),
        ChartLanguage::Russian => format!("расстояние: {px} пикс."),
    }
}

/// The gantt task span summary.
pub(crate) fn gantt_task_span(
    language: ChartLanguage,
    start: &str,
    end: &str,
    percent: f64,
) -> String {
    match language {
        ChartLanguage::English => format!("{start} – {end} · {percent}% complete"),
        ChartLanguage::Russian => format!("{start} – {end} · выполнено {percent}%"),
    }
}

/// A gantt milestone with its date.
pub(crate) fn gantt_milestone(language: ChartLanguage, date: &str) -> String {
    match language {
        ChartLanguage::English => format!("Milestone {date}"),
        ChartLanguage::Russian => format!("Веха {date}"),
    }
}

/// The mosaic accessibility label of one cell.
pub(crate) fn mosaic_cell(
    language: ChartLanguage,
    column: &str,
    segment: &str,
    share: f64,
) -> String {
    match language {
        ChartLanguage::English => format!("{column}, {segment}, {share:.1}% of column"),
        ChartLanguage::Russian => format!("{column}, {segment}, {share:.1}% столбца"),
    }
}

/// The prompt an empty line-text drawing shows while selected.
pub(crate) fn line_text_placeholder(language: ChartLanguage) -> &'static str {
    LINE_TEXT_PLACEHOLDER.text(language)
}

/// The prompt an empty annotation shows.
pub(crate) fn annotation_placeholder(language: ChartLanguage) -> &'static str {
    ANNOTATION_PLACEHOLDER.text(language)
}

/// The label of one anchor of a drawing kind in its property schema.
pub(crate) fn anchor_label(
    language: ChartLanguage,
    kind: crate::DrawingKind,
    index: usize,
) -> String {
    use crate::DrawingKind;
    match (kind, index) {
        (DrawingKind::LongPosition | DrawingKind::ShortPosition, 0) => {
            ENTRY.text(language).to_string()
        }
        (DrawingKind::LongPosition | DrawingKind::ShortPosition, 1) => {
            TARGET.text(language).to_string()
        }
        (DrawingKind::LongPosition | DrawingKind::ShortPosition, _) => {
            STOP.text(language).to_string()
        }
        _ if kind.anchor_count() == 1 => POINT.text(language).to_string(),
        _ => match language {
            ChartLanguage::English => format!("Point {}", index + 1),
            ChartLanguage::Russian => format!("Точка {}", index + 1),
        },
    }
}

const UNIT_DAY: Phrase = Phrase::new("d", "д");
const UNIT_HOUR: Phrase = Phrase::new("h", "ч");
const UNIT_MINUTE: Phrase = Phrase::new("m", "м");
const UNIT_SECOND: Phrase = Phrase::new("s", "с");

const OPEN: Phrase = Phrase::new("Open", "Открыта");
const CLOSED: Phrase = Phrase::new("Closed", "Закрыта");
const ENTRY: Phrase = Phrase::new("Entry", "Вход");
const TARGET: Phrase = Phrase::new("Target", "Цель");
const STOP: Phrase = Phrase::new("Stop", "Стоп");
const POINT: Phrase = Phrase::new("Point", "Точка");
const LINE_TEXT_PLACEHOLDER: Phrase = Phrase::new("+ Add text", "+ Добавить текст");
const ANNOTATION_PLACEHOLDER: Phrase = Phrase::new("Add text", "Добавить текст");

pub(crate) const BUY: Phrase = Phrase::new("Buy", "Покупка");
pub(crate) const SELL: Phrase = Phrase::new("Sell", "Продажа");
pub(crate) const MARKET: Phrase = Phrase::new("Market", "Рынок");
pub(crate) const LIMIT: Phrase = Phrase::new("Limit", "Лимит");
pub(crate) const STOP_ORDER: Phrase = Phrase::new("Stop", "Стоп");
pub(crate) const STOP_LIMIT: Phrase = Phrase::new("Stop Limit", "Стоп-лимит");
pub(crate) const INACTIVE: Phrase = Phrase::new("Inactive", "Неактивен");
pub(crate) const REPLAY: Phrase = Phrase::new("Replay", "Повтор");
pub(crate) const CLOSE_POSITION: Phrase = Phrase::new("Close position", "Закрыть позицию");
pub(crate) const CANCEL_TAKE_PROFIT: Phrase =
    Phrase::new("Cancel take profit", "Отменить тейк-профит");
pub(crate) const CANCEL_STOP_LOSS: Phrase = Phrase::new("Cancel stop loss", "Отменить стоп-лосс");
pub(crate) const CANCEL_ORDER: Phrase = Phrase::new("Cancel order", "Отменить ордер");
pub(crate) const AVG_PRICE: Phrase = Phrase::new("avg", "ср.");

pub(crate) const BID: Phrase = Phrase::new("Bid", "Бид");
pub(crate) const ASK: Phrase = Phrase::new("Ask", "Аск");

pub(crate) const ASSET: Phrase = Phrase::new("Asset", "Актив");
pub(crate) const OPTIONS_VOLUME: Phrase = Phrase::new("Options Volume", "Объём опционов");
pub(crate) const CALLS: Phrase = Phrase::new("Calls", "Коллы");
pub(crate) const PUTS: Phrase = Phrase::new("Puts", "Путы");
pub(crate) const DELTA: Phrase = Phrase::new("Delta", "Дельта");

pub(crate) const NO_VALUE: Phrase = Phrase::new("No value", "Нет значения");
pub(crate) const RANGE: Phrase = Phrase::new("range", "диапазон");
pub(crate) const TOOLTIP_SIZE: Phrase = Phrase::new("size", "размер");
pub(crate) const TOOLTIP_TARGET: Phrase = Phrase::new("target", "цель");
pub(crate) const TOOLTIP_COMPARATIVE: Phrase = Phrase::new("comparative", "сравнение");
pub(crate) const TOOLTIP_BANDS: Phrase = Phrase::new("bands", "диапазоны");
pub(crate) const TOOLTIP_RANK: Phrase = Phrase::new("rank", "ранг");

/// The display name of a waterfall bar's role (increase/decrease/subtotal/total).
pub(crate) fn waterfall_mark(
    language: ChartLanguage,
    mark: crate::GeneralWaterfallMark,
) -> &'static str {
    use crate::GeneralWaterfallMark as Mark;
    match (language, mark) {
        (ChartLanguage::English, _) => mark.name(),
        (ChartLanguage::Russian, Mark::Increase) => "рост",
        (ChartLanguage::Russian, Mark::Decrease) => "снижение",
        (ChartLanguage::Russian, Mark::Subtotal) => "промежуточный итог",
        (ChartLanguage::Russian, Mark::Total) => "итог",
    }
}

/// The drawing settings labels, one phrase per property label the schema can emit.
pub(crate) mod drawing_labels {
    use super::Phrase;

    pub(crate) const ACCOUNT_SIZE: Phrase = Phrase::new("Account size", "Размер счёта");
    pub(crate) const RISK_PERCENT: Phrase = Phrase::new("Risk %", "Риск %");
    pub(crate) const DEVIATION: Phrase = Phrase::new("Deviation", "Отклонение");
    pub(crate) const ROWS: Phrase = Phrase::new("Rows", "Строки");
    pub(crate) const VALUE_AREA_PERCENT: Phrase = Phrase::new("Value area %", "Зона стоимости %");
    pub(crate) const LINE: Phrase = Phrase::new("Line", "Линия");
    pub(crate) const WIDTH: Phrase = Phrase::new("Width", "Толщина");
    pub(crate) const LINE_STYLE: Phrase = Phrase::new("Line style", "Стиль линии");
    pub(crate) const LINE_START: Phrase = Phrase::new("Line start", "Начало линии");
    pub(crate) const LINE_END: Phrase = Phrase::new("Line end", "Конец линии");
    pub(crate) const EXTEND_LEFT: Phrase = Phrase::new("Extend left", "Продлить влево");
    pub(crate) const EXTEND_RIGHT: Phrase = Phrase::new("Extend right", "Продлить вправо");
    pub(crate) const MIDDLE_POINT: Phrase = Phrase::new("Middle point", "Средняя точка");
    pub(crate) const PRICE_LABELS: Phrase = Phrase::new("Price labels", "Метки цены");
    pub(crate) const STATS: Phrase = Phrase::new("Stats", "Статистика");
    pub(crate) const STATS_POSITION: Phrase = Phrase::new("Stats position", "Положение статистики");
    pub(crate) const ALWAYS_SHOW_STATS: Phrase =
        Phrase::new("Always show stats", "Всегда показывать статистику");
    pub(crate) const PROFIT: Phrase = Phrase::new("Profit", "Прибыль");
    pub(crate) const STOP: Phrase = Phrase::new("Stop", "Стоп");
    pub(crate) const FIT_BOX: Phrase = Phrase::new(
        "Fit box to traded range",
        "Вписать рамку в торгуемый диапазон",
    );
    pub(crate) const WIDTH_PERCENT: Phrase = Phrase::new("Width %", "Ширина %");
    pub(crate) const VALUE_AREA: Phrase = Phrase::new("Value area", "Зона стоимости");
    pub(crate) const VOLUME: Phrase = Phrase::new("Volume", "Объём");
    pub(crate) const DELTA_PROFILE: Phrase = Phrase::new("Delta profile", "Дельта-профиль");
    pub(crate) const POSITIVE_DELTA: Phrase = Phrase::new("Positive delta", "Положительная дельта");
    pub(crate) const NEGATIVE_DELTA: Phrase = Phrase::new("Negative delta", "Отрицательная дельта");
    pub(crate) const POINT_OF_CONTROL: Phrase = Phrase::new("Point of control", "Точка контроля");
    pub(crate) const POC_COLOR: Phrase = Phrase::new("POC color", "Цвет POC");
    pub(crate) const VALUE_AREA_LINES: Phrase =
        Phrase::new("Value area lines", "Линии зоны стоимости");
    pub(crate) const EXTEND_LEVELS_RIGHT: Phrase =
        Phrase::new("Extend levels right", "Продлить уровни вправо");
    pub(crate) const BORDER: Phrase = Phrase::new("Border", "Граница");
    pub(crate) const MIDDLE_LINE: Phrase = Phrase::new("Middle line", "Средняя линия");
    pub(crate) const MIDDLE_LINE_COLOR: Phrase =
        Phrase::new("Middle line color", "Цвет средней линии");
    pub(crate) const MIDDLE_LINE_WIDTH: Phrase =
        Phrase::new("Middle line width", "Толщина средней линии");
    pub(crate) const MIDDLE_LINE_STYLE: Phrase =
        Phrase::new("Middle line style", "Стиль средней линии");
    pub(crate) const BACKGROUND: Phrase = Phrase::new("Background", "Фон");
    pub(crate) const BACKGROUND_COLOR: Phrase = Phrase::new("Background color", "Цвет фона");
    pub(crate) const PRICE_AND_TIME_LABELS: Phrase =
        Phrase::new("Price and time labels", "Метки цены и времени");
    pub(crate) const AXIS_HIGHLIGHT: Phrase = Phrase::new("Axis highlight", "Подсветка оси");
    pub(crate) const LABEL_BACKGROUND: Phrase = Phrase::new("Label background", "Фон метки");
    pub(crate) const LABEL_TEXT: Phrase = Phrase::new("Label text", "Текст метки");
    pub(crate) const PRICE_LABEL_TEXT: Phrase = Phrase::new("Price label text", "Текст метки цены");
    pub(crate) const TREND_LINE: Phrase = Phrase::new("Trend line", "Линия тренда");
    pub(crate) const LEVELS: Phrase = Phrase::new("Levels", "Уровни");
    pub(crate) const FANS: Phrase = Phrase::new("Fans", "Веера");
    pub(crate) const ARCS: Phrase = Phrase::new("Arcs", "Дуги");
    pub(crate) const REVERSE: Phrase = Phrase::new("Reverse", "Обратный порядок");
    pub(crate) const PRICES: Phrase = Phrase::new("Prices", "Цены");
    pub(crate) const VALUES: Phrase = Phrase::new("Values", "Значения");
    pub(crate) const PERCENTS: Phrase = Phrase::new("Percents", "Проценты");
    pub(crate) const FIB_LOG_SCALE: Phrase = Phrase::new(
        "Fib levels based on log scale",
        "Уровни Фибоначчи по лог. шкале",
    );
    pub(crate) const LABELS: Phrase = Phrase::new("Labels", "Подписи");
    pub(crate) const LABELS_VERTICAL: Phrase =
        Phrase::new("Labels vertical", "Подписи по вертикали");
    pub(crate) const DEGREE: Phrase = Phrase::new("Degree", "Степень");
    pub(crate) const SIZE: Phrase = Phrase::new("Size", "Размер");
    pub(crate) const MODE: Phrase = Phrase::new("Mode", "Режим");
    pub(crate) const FLIP_HORIZONTALLY: Phrase =
        Phrase::new("Flip horizontally", "Отразить по горизонтали");
    pub(crate) const FLIP_VERTICALLY: Phrase =
        Phrase::new("Flip vertically", "Отразить по вертикали");
    pub(crate) const TEXT_COLOR: Phrase = Phrase::new("Text color", "Цвет текста");
    pub(crate) const FONT_SIZE: Phrase = Phrase::new("Font size", "Размер шрифта");
    pub(crate) const BOLD: Phrase = Phrase::new("Bold", "Жирный");
    pub(crate) const ITALIC: Phrase = Phrase::new("Italic", "Курсив");
    pub(crate) const TEXT: Phrase = Phrase::new("Text", "Текст");
    pub(crate) const VERTICAL_ALIGNMENT: Phrase =
        Phrase::new("Vertical alignment", "Выравнивание по вертикали");
    pub(crate) const HORIZONTAL_ALIGNMENT: Phrase =
        Phrase::new("Horizontal alignment", "Выравнивание по горизонтали");
    pub(crate) const BORDER_WIDTH: Phrase = Phrase::new("Border width", "Толщина границы");
    pub(crate) const HORIZONTAL_POSITION: Phrase =
        Phrase::new("Horizontal position", "Положение по горизонтали");
    pub(crate) const VERTICAL_POSITION: Phrase =
        Phrase::new("Vertical position", "Положение по вертикали");
    pub(crate) const INTERVALS: Phrase = Phrase::new("Intervals", "Интервалы");
    pub(crate) const NAME: Phrase = Phrase::new("Name", "Имя");
    pub(crate) const VISIBLE: Phrase = Phrase::new("Visible", "Видимость");
    pub(crate) const LOCKED: Phrase = Phrase::new("Locked", "Заблокировано");
    pub(crate) const GROUP: Phrase = Phrase::new("Group", "Группа");
    pub(crate) const ORDER: Phrase = Phrase::new("Order", "Порядок");
    pub(crate) const MAGNET: Phrase = Phrase::new("Magnet", "Магнит");
    pub(crate) const PRICE_SCALE: Phrase = Phrase::new("Price scale", "Ценовая шкала");
    pub(crate) const SOURCE: Phrase = Phrase::new("Source", "Источник");
    pub(crate) const ICON: Phrase = Phrase::new("Icon", "Значок");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn russian_plurals_follow_the_one_few_many_rule() {
        let form = |n| russian_plural(n, "бар", "бара", "баров");
        for n in [1, 21, 31, 101] {
            assert_eq!(form(n), "бар", "{n}");
        }
        for n in [2, 3, 4, 22, 54] {
            assert_eq!(form(n), "бара", "{n}");
        }
        for n in [0, 5, 11, 12, 14, 25, 111] {
            assert_eq!(form(n), "баров", "{n}");
        }
        assert_eq!(bars_count(ChartLanguage::Russian, "1", 1), "1 бар");
        assert_eq!(bars_count(ChartLanguage::Russian, "3", 3), "3 бара");
        assert_eq!(bars_count(ChartLanguage::Russian, "12", 12), "12 баров");
        assert_eq!(bars_count(ChartLanguage::English, "1", 1), "1 bar");
        assert_eq!(bars_count(ChartLanguage::English, "2", 2), "2 bars");
    }

    #[test]
    fn durations_and_countdowns_use_the_language_units() {
        assert_eq!(measure_duration(ChartLanguage::English, 90_061), "1d 1h");
        assert_eq!(measure_duration(ChartLanguage::Russian, 90_061), "1д 1ч");
        assert_eq!(measure_duration(ChartLanguage::Russian, 0), "0с");
        assert_eq!(countdown(ChartLanguage::English, 91_800.0), "1d 1h");
        assert_eq!(countdown(ChartLanguage::Russian, 91_800.0), "1д 1ч");
        assert_eq!(countdown(ChartLanguage::Russian, 59.0), "00:59");
    }

    #[test]
    fn trading_phrases_localize() {
        assert_eq!(BUY.text(ChartLanguage::Russian), "Покупка");
        assert_eq!(SELL.text(ChartLanguage::Russian), "Продажа");
        assert_eq!(STOP_LIMIT.text(ChartLanguage::Russian), "Стоп-лимит");
        assert_eq!(
            risk_reward_line(ChartLanguage::Russian, "2.50"),
            "Соотношение риск/прибыль: 2.50"
        );
        assert_eq!(
            CLOSE_POSITION.text(ChartLanguage::Russian),
            "Закрыть позицию"
        );
    }
}
