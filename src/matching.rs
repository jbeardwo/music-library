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

#[cfg(test)]
mod tests {
    use super::*;
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
