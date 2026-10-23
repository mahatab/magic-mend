//! Output check: scans drafts for contractions and em dashes (spec, "Output check in the app").

use regex::Regex;
use serde::Serialize;
use std::sync::LazyLock;

/// Straight and curly apostrophes.
const APOS: &str = "['\u{2019}\u{2018}\u{02BC}]";
pub const EM_DASH: char = '\u{2014}';

static CONTRACTION_RE: LazyLock<Regex> = LazyLock::new(|| {
    let pattern = format!(
        r"(?i)\b(?:\w+n{a}t|\w+{a}(?:re|ve|ll|m|d)|(?:it|that|there|let|he|she|what){a}s)\b",
        a = APOS
    );
    Regex::new(&pattern).expect("contraction regex")
});

#[derive(Debug, Clone, Copy)]
pub struct Rules {
    pub no_contractions: bool,
    pub no_em_dashes: bool,
}

impl Default for Rules {
    fn default() -> Self {
        Rules { no_contractions: true, no_em_dashes: true }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Findings {
    pub contractions: Vec<String>,
    pub em_dashes: usize,
}

impl Findings {
    pub fn is_clean(&self) -> bool {
        self.contractions.is_empty() && self.em_dashes == 0
    }
}

pub fn check(text: &str, rules: Rules) -> Findings {
    let mut f = Findings::default();
    if rules.no_contractions {
        f.contractions = CONTRACTION_RE.find_iter(text).map(|m| m.as_str().to_string()).collect();
    }
    if rules.no_em_dashes {
        f.em_dashes = text.chars().filter(|c| *c == EM_DASH).count();
    }
    f
}

/// Local em dash repair: replace each em dash with a comma and one space, then remove double spaces.
pub fn fix_em_dashes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch == EM_DASH {
            // Drop any space we already wrote before the dash so "word — word" becomes "word, word".
            while out.ends_with(' ') {
                out.pop();
            }
            out.push_str(", ");
        } else {
            out.push(ch);
        }
    }
    collapse_double_spaces(&out)
        .replace(", ,", ",")
        .replace(",,", ",")
        .replace(", .", ".")
        .replace(", \n", ",\n")
}

fn collapse_double_spaces(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for ch in s.chars() {
        if ch == ' ' {
            if !prev_space {
                out.push(ch);
            }
            prev_space = true;
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    out
}

/// Plain words naming what is wrong, used in the one fix request sent back to the engine.
pub fn describe(findings: &[Findings]) -> String {
    let mut words: Vec<String> = findings.iter().flat_map(|f| f.contractions.clone()).collect();
    words.sort();
    words.dedup();
    let dashes: usize = findings.iter().map(|f| f.em_dashes).sum();
    let mut parts = Vec::new();
    if !words.is_empty() {
        parts.push(format!("contractions ({})", words.join(", ")));
    }
    if dashes > 0 {
        parts.push("em dashes".to_string());
    }
    parts.join(" and ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(t: &str) -> Vec<String> {
        check(t, Rules::default()).contractions
    }

    #[test]
    fn flags_spec_contractions_straight_and_curly() {
        for t in [
            "I don't know", "I don\u{2019}t know", "you're", "we've", "I'll", "I'm", "I'd go",
            "it's fine", "that's it", "there's one", "let's go", "he's here", "she's here",
            "what's up", "can't", "won't", "they\u{2019}re", "It\u{2019}s", "LET'S",
        ] {
            assert_eq!(c(t).len(), 1, "should flag: {t}");
        }
    }

    #[test]
    fn does_not_flag_possessives_or_names() {
        for t in [
            "the host's place", "John's car", "O'Neill", "rock 'n' roll", "Chi's apartment",
            "Assalamu Alaikum", "I do not know", "it is fine", "the 90's", "Mahatab's draft",
        ] {
            assert!(c(t).is_empty(), "should not flag: {t} -> {:?}", c(t));
        }
    }

    #[test]
    fn flags_em_dash_only() {
        let f = check("one \u{2014} two \u{2014}three - four \u{2013} five", Rules::default());
        assert_eq!(f.em_dashes, 2);
        assert!(f.contractions.is_empty());
    }

    #[test]
    fn rules_can_be_turned_off() {
        let f = check("don't \u{2014}", Rules { no_contractions: false, no_em_dashes: false });
        assert!(f.is_clean());
    }

    #[test]
    fn fixes_em_dashes() {
        assert_eq!(fix_em_dashes("I am here \u{2014} call me"), "I am here, call me");
        assert_eq!(fix_em_dashes("I am here\u{2014}call me"), "I am here, call me");
        assert_eq!(fix_em_dashes("a  b"), "a b");
        assert_eq!(fix_em_dashes("end\u{2014}."), "end.");
        assert!(check(&fix_em_dashes("x \u{2014} y \u{2014} z"), Rules::default()).is_clean());
    }

    #[test]
    fn describes_findings() {
        let f1 = check("don't do it \u{2014} ok", Rules::default());
        let f2 = check("I'm here, don't", Rules::default());
        assert_eq!(describe(&[f1, f2]), "contractions (I'm, don't) and em dashes");
    }
}
