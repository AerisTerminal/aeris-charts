//! The chart's display language: which built-in text the engine paints and which built-in
//! month names it installs. Host data (titles, labels, category names) is never translated.

/// A display language with a complete built-in text catalog. English is the default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ChartLanguage {
    #[default]
    English,
    Russian,
}

impl ChartLanguage {
    /// The ISO 639-1 code of the language.
    pub const fn code(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Russian => "ru",
        }
    }

    /// The language of a BCP 47 (or POSIX-style) locale tag, read from its primary subtag
    /// case-insensitively: `ru`, `ru-RU` and `RU_ru` are Russian. Any other tag, including an
    /// empty or malformed one, falls back to English.
    pub fn from_tag(tag: &str) -> Self {
        let primary = tag.trim().split(['-', '_']).next().unwrap_or_default();
        if primary.eq_ignore_ascii_case("ru") {
            Self::Russian
        } else {
            Self::English
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ChartLanguage;

    #[test]
    fn tags_resolve_by_primary_subtag() {
        for tag in ["ru", "ru-RU", "RU_ru", " ru-Cyrl-RU ", "RU"] {
            assert_eq!(
                ChartLanguage::from_tag(tag),
                ChartLanguage::Russian,
                "{tag}"
            );
        }
        for tag in ["en", "en-US", "", "-", "rus", "uk-UA", "de", "r"] {
            assert_eq!(
                ChartLanguage::from_tag(tag),
                ChartLanguage::English,
                "{tag}"
            );
        }
        assert_eq!(ChartLanguage::default(), ChartLanguage::English);
        assert_eq!(ChartLanguage::Russian.code(), "ru");
        assert_eq!(ChartLanguage::English.code(), "en");
    }
}
