//! Derived comparisons only: never rewrite observations or infer edition identity.

/// Unicode lowercase plus whitespace folding; punctuation and version qualifiers survive.
/// This deliberately does not perform transliteration, fuzzy matching or Unicode compatibility folding.
pub fn normalize(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Album-title typography only: curly apostrophes and Unicode hyphens. Keep
/// punctuation rather than deleting it;
/// en/em dashes, minus signs and Artist-name identity comparisons stay distinct.
pub fn normalize_album_title(value: &str) -> String {
    normalize(&title_typography(presentation_title(value)))
}
/// Typography only, also usable by bounded provider discovery. Case and
/// semantic/internal punctuation are retained; comparisons fold case separately.
pub fn title_typography(value: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    value
        .nfc()
        .map(|c| match c {
            '\u{2010}' | '\u{2011}' => '-',
            '\u{2018}' | '\u{2019}' | '\u{02bc}' | '\u{ff07}' => '\'',
            _ => c,
        })
        .collect()
}

/// Textual containment evidence only. The caller must establish Artist/program
/// context and unique correspondence; this is never a global identity rule.
/// Unicode letters and numbers delimit components equally in every script.
pub fn shared_title_span(left: &str, right: &str) -> Option<String> {
    let a = normalize_album_title(left);
    let b = normalize_album_title(right);
    if a == b {
        return None;
    }
    let (short, long) = if a.len() < b.len() {
        (&a, &b)
    } else {
        (&b, &a)
    };
    if short.chars().filter(|c| c.is_alphanumeric()).count() < 2 {
        return None;
    }
    long.match_indices(short).find_map(|(start, _)| {
        let end = start + short.len();
        let boundary =
            |c: char| !c.is_alphanumeric() && !unicode_normalization::char::is_combining_mark(c);
        (long[..start].chars().next_back().is_none_or(boundary)
            && long[end..].chars().next().is_none_or(boundary))
        .then(|| short.clone())
    })
}

/// Candidate layout, not a declaration that either part is an alias. Only the
/// shared program policy can corroborate this interpretation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AlternateTitleLayout {
    SpacedHyphen,
    SpacedEnDash,
    SpacedEmDash,
    WhitespaceBlocks,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct AlternateTitleComponents {
    pub layout: AlternateTitleLayout,
    pub components: [String; 2],
}
/// Recognize possible separately expressed representations. Never split inside
/// a word, at a script transition, or inside semantic parenthetical annotations.
pub fn alternate_title_components(title: &str) -> Option<AlternateTitleComponents> {
    let title = normalize_album_title(title);
    if title.contains(['(', ')', '[', ']']) {
        return None;
    }
    for (separator, layout) in [
        (" - ", AlternateTitleLayout::SpacedHyphen),
        (" – ", AlternateTitleLayout::SpacedEnDash),
        (" — ", AlternateTitleLayout::SpacedEmDash),
    ] {
        if let Some((left, right)) = title.split_once(separator) {
            if right.contains(separator)
                || !left.chars().any(char::is_alphanumeric)
                || !right.chars().any(char::is_alphanumeric)
            {
                return None;
            }
            return Some(AlternateTitleComponents {
                layout,
                components: [left.into(), right.into()],
            });
        }
    }
    // Whitespace is required in addition to two internally unmixed alphabet
    // blocks. This proposes a layout only; neither scripts nor spaces confer
    // equivalence. Whole-program corroboration below remains mandatory.
    let alphabet = |word: &str| {
        let mut latin = false;
        let mut other = false;
        for c in word.chars().filter(|c| c.is_alphabetic()) {
            if c.is_ascii_alphabetic()
                || ('\u{00c0}'..='\u{024f}').contains(&c)
                || ('\u{1e00}'..='\u{1eff}').contains(&c)
            {
                latin = true;
            } else {
                other = true;
            }
        }
        match (latin, other) {
            (true, false) => Some(false),
            (false, true) => Some(true),
            _ => None,
        }
    };
    let words = title.split_whitespace().collect::<Vec<_>>();
    let alphabets = words
        .iter()
        .map(|w| alphabet(w))
        .collect::<Option<Vec<_>>>()?;
    let boundaries = (1..words.len())
        .filter(|&i| alphabets[i] != alphabets[i - 1])
        .collect::<Vec<_>>();
    let [at] = boundaries.as_slice() else {
        return None;
    };
    Some(AlternateTitleComponents {
        layout: AlternateTitleLayout::WhitespaceBlocks,
        components: [words[..*at].join(" "), words[*at..].join(" ")],
    })
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct AlternateComponentMatch {
    pub layout: AlternateTitleLayout,
    pub formatted_left: bool,
    pub component_index: usize,
    pub component: String,
}
/// Exact COMPLETE component equality. A one-character component is valid syntax;
/// a one-character substring is not. This alone never establishes correspondence.
pub fn exact_alternate_component(left: &str, right: &str) -> Option<AlternateComponentMatch> {
    let a = normalize_album_title(left);
    let b = normalize_album_title(right);
    if a == b {
        return None;
    }
    for (formatted_left, formatted, whole) in [(true, &a, &b), (false, &b, &a)] {
        if let Some(parts) = alternate_title_components(formatted) {
            if let Some(component_index) = parts.components.iter().position(|part| part == whole) {
                return Some(AlternateComponentMatch {
                    layout: parts.layout,
                    formatted_left,
                    component_index,
                    component: whole.clone(),
                });
            }
        }
    }
    None
}

/// Negative evidence only: containment without a whole textual boundary is
/// not a representation component. Never use this to infer correspondence.
pub fn arbitrary_title_fragment(left: &str, right: &str) -> bool {
    let a = normalize_album_title(left);
    let b = normalize_album_title(right);
    if a == b {
        return false;
    }
    let (short, long) = if a.len() < b.len() {
        (&a, &b)
    } else {
        (&b, &a)
    };
    !short.is_empty()
        && long.contains(short)
        && exact_alternate_component(left, right).is_none()
        && shared_title_span(left, right).is_none()
}

pub fn usable(value: &str) -> bool {
    !matches!(
        normalize(value).as_str(),
        "" | "unknown album" | "unknown artist" | "unknown track"
    )
}

/// Local tags have ordered names but no parsed join phrases. Preserve that boundary:
/// multiple names use the application's ordinary comma-separated credit display.
pub fn credit(names: &[String]) -> Option<String> {
    (!names.is_empty() && names.iter().all(|name| usable(name)))
        .then(|| normalize(&names.join(", ")))
}

/// Derived, positioned title evidence. Missing positions are not guessed.
pub struct TrackEvidence {
    pub disc: u32,
    pub position: u32,
    pub title: String,
}

/// Require an exact positional/title overlap and no contradictory overlapping positions
/// within one edition. Missing positions provide neither support nor contradiction.
pub fn tracks_support_album(local: &[TrackEvidence], edition: &[TrackEvidence]) -> bool {
    let mut supported = false;
    for track in local {
        let mut at_position = edition
            .iter()
            .filter(|other| other.disc == track.disc && other.position == track.position);
        match (at_position.next(), at_position.next()) {
            (None, _) => (),
            (Some(other), None) if other.title == track.title => supported = true,
            _ => return false,
        }
    }
    supported
}

/// Canonical Unicode and ordinary typography, for bounded Album comparison only.
/// Words (including version qualifiers), ampersands and plus signs survive.
pub fn album_representation_title(value: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    let canonical: String = value
        .nfc()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' | '\u{02bc}' | '\u{ff07}' => '\'',
            '\u{2010}' | '\u{2011}' | '\u{2013}' | '\u{2014}' => '-',
            _ => c,
        })
        .collect();
    canonical
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '&' || c == '+' {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Remove one entire balanced presentational wrapper. Internal brackets and
/// nested/semantic qualifiers remain meaningful. This is supporting evidence.
pub fn presentation_title(value: &str) -> &str {
    let value = value.trim();
    let pair = match value.chars().next() {
        Some('[') => Some(('[', ']')),
        Some('(') => Some(('(', ')')),
        Some('{') => Some(('{', '}')),
        _ => None,
    };
    if let Some((open, close)) = pair
        && value.ends_with(close)
    {
        let inner = &value[open.len_utf8()..value.len() - close.len_utf8()];
        if !inner.trim().is_empty() && !inner.contains(['[', ']', '(', ')', '{', '}']) {
            return inner.trim();
        }
    }
    value
}
/// Only an already established provider Artist credit can explain this prefix.
/// This never establishes Artist identity or removes a semantic version word.
pub fn artist_album_title<'a>(title: &'a str, artist: &str) -> Option<&'a str> {
    let prefix = format!("{} on ", artist.trim());
    let head = title.get(..prefix.len())?;
    if normalize(head) == normalize(&prefix) {
        title.get(prefix.len()..).filter(|v| !v.trim().is_empty())
    } else {
        None
    }
}

/// One alternate established-Artist query for an otherwise undiscovered series
/// title. Evaluation still requires systematic Live-program corroboration.
pub fn artist_album_discovery_variant(artist: &str, title: &str) -> Option<String> {
    let title = title.trim();
    let base = title
        .rsplit_once(' ')
        .filter(|(_, suffix)| suffix.eq_ignore_ascii_case("live"))
        .map_or(title, |(base, _)| base);
    (base.chars().count() >= 4
        && base.chars().count() <= 40
        && base.chars().all(char::is_alphabetic)
        && !matches!(
            normalize(base).as_str(),
            "live" | "demo" | "remaster" | "acoustic"
        )
        && !artist.trim().is_empty())
    .then(|| format!("{} on {} Live", artist.trim(), base))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn established_series_discovery_has_one_variant() {
        for title in ["Audiotree", "Audiotree Live", "Audiotree LIVE"] {
            assert_eq!(
                artist_album_discovery_variant("CHON", title).as_deref(),
                Some("CHON on Audiotree Live")
            );
        }
        for title in [
            "Live",
            "Demo",
            "Remaster",
            "Acoustic",
            "Ordinary Album",
            "Audiotree (Demo)",
        ] {
            assert!(artist_album_discovery_variant("Artist", title).is_none());
        }
        assert!(artist_album_discovery_variant("", "Audiotree").is_none());
    }
    #[test]
    fn apostrophe_typography_is_equivalent_without_losing_version_qualifiers() {
        for title in ["X'ed Out", "X’ed Out", "X‘ed Out", "Xʼed Out", "X＇ed Out"] {
            assert_eq!(normalize_album_title(title), "x'ed out");
            assert_eq!(title_typography(title), "X'ed Out");
            assert_eq!(album_representation_title(title), "x ed out");
        }
        assert_ne!(
            normalize_album_title("X’ed Out (Live)"),
            normalize_album_title("X'ed Out")
        );
    }
    #[test]
    fn comparison_preserves_qualifiers_and_original_text() {
        let original = "  ÉCHO\t (Live)  ";
        assert_eq!(normalize(original), "écho (live)");
        assert_eq!(original, "  ÉCHO\t (Live)  ");
        for title in [
            "Echo (Live)",
            "Echo [Remix]",
            "Echo - Edit",
            "Echo: Acoustic",
            "Echo Remaster",
        ] {
            assert_ne!(normalize(title), normalize("Echo"));
        }
        assert_ne!(normalize("Artist A & B"), normalize("Artist A B"));
        assert!(!usable(" Unknown   Album "));
        assert!(credit(&["Unknown Artist".into()]).is_none());
    }
}

/// Literal ASCII title span in a mixed-script observation. Search evidence only;
/// punctuation is retained and no text is translated or transliterated.
pub fn literal_search_component(title: &str) -> Option<String> {
    if !title.chars().any(|c| c.is_alphabetic() && !c.is_ascii()) {
        return None;
    }
    title
        .split(|c: char| !c.is_ascii())
        .map(|s| {
            s.trim_matches(|c: char| {
                c.is_whitespace() || matches!(c, '(' | ')' | '[' | ']' | '/' | '-')
            })
        })
        .filter(|s| s.chars().filter(|c| c.is_ascii_alphanumeric()).count() >= 3)
        .max_by_key(|s| s.chars().filter(|c| c.is_ascii_alphanumeric()).count())
        .map(str::to_owned)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextualTitleClass {
    Punctuation,
    TokenBoundary,
    TinyTypo,
    AlbumVersion,
    ArticleInProgram,
}
impl ContextualTitleClass {
    pub fn label(self) -> &'static str {
        match self {
            Self::Punctuation => "contextual punctuation equivalence",
            Self::TokenBoundary => "contextual token-boundary equivalence",
            Self::TinyTypo => "contextual tiny typo",
            Self::AlbumVersion => "weak Album Version annotation",
            Self::ArticleInProgram => "contextual article difference under strong program evidence",
        }
    }
}
/// Shared contextual policy. Callers must prove Artist, independent program
/// anchors, a unique counterpart and position; this never establishes identity.
pub fn contextual_title_class(left: &str, right: &str) -> Option<ContextualTitleClass> {
    let annotation_base = |s: &str| {
        let normalized = normalize_album_title(s);
        normalized
            .strip_suffix(" (album version)")
            .or_else(|| normalized.strip_suffix(" - album version"))
            .map(str::to_owned)
            .unwrap_or(normalized)
    };
    let a = normalize_album_title(left);
    let b = normalize_album_title(right);
    if a != b && annotation_base(left) == annotation_base(right) {
        return Some(ContextualTitleClass::AlbumVersion);
    }
    if crate::album_matching::qualifiers(left) != crate::album_matching::qualifiers(right) {
        return None;
    }
    // Periods in abbreviations can join letters; other punctuation retains a
    // token boundary. A hyphen must never silently concatenate musical words.
    let periods = |s: &str| crate::album_program::comparison_title(&s.replace('.', ""));
    if periods(&a) == periods(&b) && !periods(&a).is_empty() {
        return Some(ContextualTitleClass::Punctuation);
    }
    let a = crate::album_program::comparison_title(left);
    let b = crate::album_program::comparison_title(right);
    let compact = |s: &str| s.split_whitespace().collect::<String>();
    if a != b
        && compact(&a) == compact(&b)
        && compact(&a).chars().count() >= 4
        && a.split_whitespace()
            .chain(b.split_whitespace())
            .all(|w| w.chars().count() >= 2)
    {
        return Some(ContextualTitleClass::TokenBoundary);
    }
    let aw: Vec<_> = a.split_whitespace().collect();
    let bw: Vec<_> = b.split_whitespace().collect();
    let article_difference = |long: &[&str], short: &[&str]| {
        short.len() >= 6
            && long.len() == short.len() + 1
            && long.iter().enumerate().any(|(i, w)| {
                i > 0
                    && *w == "the"
                    && long
                        .iter()
                        .enumerate()
                        .filter(|(j, _)| *j != i)
                        .map(|(_, w)| *w)
                        .eq(short.iter().copied())
            })
    };
    if article_difference(&aw, &bw) || article_difference(&bw, &aw) {
        return Some(ContextualTitleClass::ArticleInProgram);
    }
    if left.contains(['\'', '’']) || right.contains(['\'', '’']) {
        return None;
    }
    if arbitrary_title_fragment(left, right)
        && [left, right].iter().any(|s| {
            s.split_whitespace().any(|w| {
                w.chars().any(|c| c.is_ascii_alphabetic())
                    && w.chars().any(|c| c.is_alphabetic() && !c.is_ascii())
            })
        })
    {
        return None;
    }
    if crate::album_matching::edit_close(&a, &b, 1, 4) {
        return Some(ContextualTitleClass::TinyTypo);
    }
    let ac: Vec<_> = a.chars().collect();
    let bc: Vec<_> = b.chars().collect();
    if ac.len() >= 8 && ac.len() == bc.len() {
        let differing: Vec<_> = (0..ac.len()).filter(|i| ac[*i] != bc[*i]).collect();
        if let [i, j] = differing.as_slice()
            && *j == *i + 1
            && ac[*i] == bc[*j]
            && ac[*j] == bc[*i]
        {
            return Some(ContextualTitleClass::TinyTypo);
        }
    }
    None
}
