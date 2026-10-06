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

#[cfg(test)]
mod tests {
    use super::*;
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
