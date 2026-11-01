//! The chart's display language: which built-in text the engine paints and which built-in
//! month names it installs. Host data (titles, labels, category names) is never translated.

/// A display language with a complete built-in text catalog. English is the default. The
/// variant order is the column order of the engine's compiled text catalog.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ChartLanguage {
    #[default]
    English,
    German,
    Spanish,
    French,
    Portuguese,
    Russian,
    Hindi,
    ChineseSimplified,
    Japanese,
    Korean,
}

/// A CLDR plural category of an integer count.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PluralCategory {
    One,
    Few,
    Many,
    Other,
}

impl ChartLanguage {
    /// Every language, in catalog column order.
    pub const ALL: [Self; 10] = [
        Self::English,
        Self::German,
        Self::Spanish,
        Self::French,
        Self::Portuguese,
        Self::Russian,
        Self::Hindi,
        Self::ChineseSimplified,
        Self::Japanese,
        Self::Korean,
    ];

    /// The ISO 639-1 code of the language.
    pub const fn code(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::German => "de",
            Self::Spanish => "es",
            Self::French => "fr",
            Self::Portuguese => "pt",
            Self::Russian => "ru",
            Self::Hindi => "hi",
            Self::ChineseSimplified => "zh",
            Self::Japanese => "ja",
            Self::Korean => "ko",
        }
    }

    /// The language of a BCP 47 (or POSIX-style) locale tag, read from its primary subtag
    /// case-insensitively: `ru`, `ru-RU` and `RU_ru` are Russian, and every Chinese tag
    /// (`zh`, `zh-CN`, `zh-Hant-TW`) selects the Simplified Chinese catalog. Any other tag,
    /// including an empty or malformed one, falls back to English.
    pub fn from_tag(tag: &str) -> Self {
        let primary = tag
            .trim()
            .split(['-', '_', '.', '@'])
            .next()
            .unwrap_or_default();
        Self::ALL
            .into_iter()
            .find(|language| language.code().eq_ignore_ascii_case(primary))
            .unwrap_or_default()
    }

    /// The CLDR plural category of a non-negative integer count in this language (callers
    /// pass the magnitude of a signed count, which is the CLDR operand `n`).
    pub const fn plural_category(self, count: u64) -> PluralCategory {
        match self {
            Self::English | Self::German | Self::Spanish => {
                if count == 1 {
                    PluralCategory::One
                } else {
                    PluralCategory::Other
                }
            }
            Self::French | Self::Portuguese | Self::Hindi => {
                if count <= 1 {
                    PluralCategory::One
                } else {
                    PluralCategory::Other
                }
            }
            Self::Russian => {
                let (units, tens) = (count % 10, count % 100);
                if units == 1 && tens != 11 {
                    PluralCategory::One
                } else if matches!(units, 2..=4) && !matches!(tens, 12..=14) {
                    PluralCategory::Few
                } else {
                    PluralCategory::Many
                }
            }
            Self::ChineseSimplified | Self::Japanese | Self::Korean => PluralCategory::Other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ChartLanguage, PluralCategory};

    #[test]
    fn tags_resolve_by_primary_subtag() {
        for tag in ["ru", "ru-RU", "RU_ru", " ru-Cyrl-RU ", "RU", "ru_RU.UTF-8"] {
            assert_eq!(
                ChartLanguage::from_tag(tag),
                ChartLanguage::Russian,
                "{tag}"
            );
        }
        for tag in [
            "en", "en-US", "", "-", "rus", "uk-UA", "r", "zz", "de!", "chinese",
        ] {
            assert_eq!(
                ChartLanguage::from_tag(tag),
                ChartLanguage::English,
                "{tag}"
            );
        }
        let regional = [
            (ChartLanguage::German, ["de", "de-DE", "DE_at"]),
            (ChartLanguage::Spanish, ["es", "es-419", "ES_mx"]),
            (ChartLanguage::French, ["fr", "fr-CA", "FR_be"]),
            (ChartLanguage::Portuguese, ["pt", "pt-BR", "pt-PT"]),
            (ChartLanguage::Hindi, ["hi", "hi-IN", "HI_in"]),
            (
                ChartLanguage::ChineseSimplified,
                ["zh", "zh-CN", "zh-Hant-TW"],
            ),
            (ChartLanguage::Japanese, ["ja", "ja-JP", "JA_jp"]),
            (ChartLanguage::Korean, ["ko", "ko-KR", "KO_kr"]),
        ];
        for (language, tags) in regional {
            for tag in tags {
                assert_eq!(ChartLanguage::from_tag(tag), language, "{tag}");
            }
        }
        for language in ChartLanguage::ALL {
            assert_eq!(ChartLanguage::from_tag(language.code()), language);
        }
        assert_eq!(ChartLanguage::default(), ChartLanguage::English);
        assert_eq!(ChartLanguage::Russian.code(), "ru");
        assert_eq!(ChartLanguage::English.code(), "en");
    }

    #[test]
    fn every_language_follows_its_cldr_integer_plural_rule() {
        use PluralCategory::{Few, Many, One, Other};
        let counts = [0, 1, 2, 5, 11, 21, 22, 25];
        let expected = |language| match language {
            ChartLanguage::English | ChartLanguage::German | ChartLanguage::Spanish => {
                [Other, One, Other, Other, Other, Other, Other, Other]
            }
            ChartLanguage::French | ChartLanguage::Portuguese | ChartLanguage::Hindi => {
                [One, One, Other, Other, Other, Other, Other, Other]
            }
            ChartLanguage::Russian => [Many, One, Few, Many, Many, One, Few, Many],
            ChartLanguage::ChineseSimplified | ChartLanguage::Japanese | ChartLanguage::Korean => {
                [Other; 8]
            }
        };
        for language in ChartLanguage::ALL {
            let categories = counts.map(|count| language.plural_category(count));
            assert_eq!(categories, expected(language), "{language:?}");
        }
        let russian = |count| ChartLanguage::Russian.plural_category(count);
        for count in [101, 1001, 31] {
            assert_eq!(russian(count), One, "{count}");
        }
        for count in [3, 4, 34, 102] {
            assert_eq!(russian(count), Few, "{count}");
        }
        for count in [12, 13, 14, 19, 111, 112, 114] {
            assert_eq!(russian(count), Many, "{count}");
        }
    }
}
