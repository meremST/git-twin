//! Text matching used by gitk-style find and diff search.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MatchKind {
    #[default]
    Exact,
    IgnCase,
    Regexp,
}

impl MatchKind {
    pub const ALL: [MatchKind; 3] = [MatchKind::Exact, MatchKind::IgnCase, MatchKind::Regexp];

    pub fn label(self) -> &'static str {
        match self {
            MatchKind::Exact => "Exact",
            MatchKind::IgnCase => "IgnCase",
            MatchKind::Regexp => "Regexp",
        }
    }
}

#[derive(Clone, Debug)]
pub enum Matcher {
    Exact(String),
    IgnCase(String),
    Regex(regex::Regex),
}

impl Matcher {
    pub fn new(kind: MatchKind, pattern: &str) -> Result<Matcher, String> {
        Ok(match kind {
            MatchKind::Exact => Matcher::Exact(pattern.to_string()),
            MatchKind::IgnCase => Matcher::IgnCase(pattern.to_lowercase()),
            MatchKind::Regexp => Matcher::Regex(
                regex::RegexBuilder::new(pattern)
                    .size_limit(1 << 20)
                    .build()
                    .map_err(|e| format!("invalid regular expression: {e}"))?,
            ),
        })
    }

    pub fn is_match(&self, s: &str) -> bool {
        match self {
            Matcher::Exact(p) => s.contains(p.as_str()),
            Matcher::IgnCase(p) => s.to_lowercase().contains(p.as_str()),
            Matcher::Regex(r) => r.is_match(s),
        }
    }

    /// Byte ranges of all matches in `s` (for highlighting).
    pub fn find_all(&self, s: &str) -> Vec<(usize, usize)> {
        match self {
            Matcher::Exact(p) if !p.is_empty() => {
                s.match_indices(p.as_str()).map(|(i, m)| (i, i + m.len())).collect()
            }
            Matcher::IgnCase(p) if !p.is_empty() => {
                let lower = s.to_lowercase();
                if lower.len() != s.len() {
                    return Vec::new();
                }
                lower
                    .match_indices(p.as_str())
                    .map(|(i, m)| (i, i + m.len()))
                    .collect()
            }
            Matcher::Regex(r) => r
                .find_iter(s)
                .filter(|m| m.start() != m.end())
                .map(|m| (m.start(), m.end()))
                .collect(),
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds() {
        assert!(Matcher::new(MatchKind::Exact, "Foo").unwrap().is_match("a Foo b"));
        assert!(!Matcher::new(MatchKind::Exact, "foo").unwrap().is_match("a Foo b"));
        assert!(Matcher::new(MatchKind::IgnCase, "foo").unwrap().is_match("a FOO b"));
        assert!(Matcher::new(MatchKind::Regexp, "^fix(es)?:").unwrap().is_match("fixes: x"));
        assert!(Matcher::new(MatchKind::Regexp, "(").is_err());
        let m = Matcher::new(MatchKind::Exact, "ab").unwrap();
        assert_eq!(m.find_all("xabyab"), vec![(1, 3), (4, 6)]);
    }
}
