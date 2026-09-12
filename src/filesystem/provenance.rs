//! Lofty/Picard interpretation belongs at the file adapter, not generic matching.
use crate::{domain::ExternalIdentity, provenance::*};
use lofty::tag::{ItemKey, Tag};

/// Picard's Album-group and Recording identifiers are MBIDs. Only canonical
/// hyphenated, non-nil UUID strings qualify; malformed raw tags remain observable.
pub(crate) fn validate(o: &Observation) -> crate::provenance_acceptance::Validation {
    use crate::provenance_acceptance::Validation;
    if o.identity.provider != "musicbrainz"
        || !matches!(
            (o.scope, o.semantics, o.identity.kind.as_str()),
            (Scope::Album, Semantics::AlbumIdentity, "release_group")
                | (Scope::Recording, Semantics::RecordingIdentity, "recording")
        )
    {
        return Validation::Unsupported;
    }
    match uuid::Uuid::parse_str(&o.identity.external_id) {
        Ok(id) if !id.is_nil() && id.hyphenated().to_string() == o.identity.external_id => {
            Validation::Valid
        }
        _ => Validation::Invalid,
    }
}

/// Lofty 0.25.1 translates ID3 UFID, Vorbis MUSICBRAINZ_TRACKID and MP4
/// MusicBrainz Track Id into *RecordingId*. Release Track Id is *TrackId*.
/// See https://docs.rs/lofty/0.25.1/lofty/tag/enum.ItemKey.html .
pub(super) fn extract(tags: &[Tag]) -> FileProvenance {
    use Scope::*;
    use Semantics::*;
    let mut out = FileProvenance::default();
    for tag in tags {
        let format = format!("{:?}", tag.tag_type());
        out.formats.push(format.clone());
        for (key, provider, kind, scope, semantics) in [
            (
                ItemKey::MusicBrainzArtistId,
                "musicbrainz",
                "artist",
                TrackArtist,
                ArtistIdentity,
            ),
            (
                ItemKey::MusicBrainzReleaseArtistId,
                "musicbrainz",
                "artist",
                AlbumArtist,
                ArtistIdentity,
            ),
            (
                ItemKey::MusicBrainzReleaseGroupId,
                "musicbrainz",
                "release_group",
                Album,
                AlbumIdentity,
            ),
            (
                ItemKey::MusicBrainzReleaseId,
                "musicbrainz",
                "release",
                Edition,
                EditionIdentity,
            ),
            (
                ItemKey::MusicBrainzRecordingId,
                "musicbrainz",
                "recording",
                Recording,
                RecordingIdentity,
            ),
            (
                ItemKey::MusicBrainzTrackId,
                "musicbrainz",
                "track",
                Occurrence,
                OccurrenceIdentity,
            ),
            (ItemKey::Isrc, "isrc", "recording", Recording, Isrc),
        ] {
            for value in tag.get_strings(key).filter(|v| !v.trim().is_empty()) {
                out.observations.push(Observation {
                    identity: ExternalIdentity {
                        provider: provider.into(),
                        kind: kind.into(),
                        external_id: value.into(),
                    },
                    scope,
                    semantics,
                    origin: Origin::EmbeddedTag {
                        format: format.clone(),
                        field: format!("{key:?}"),
                    },
                });
            }
        }
        for (key, values) in [
            (ItemKey::DiscNumber, &mut out.positions.disc),
            (ItemKey::DiscTotal, &mut out.positions.discs),
            (ItemKey::TrackNumber, &mut out.positions.track),
            (ItemKey::TrackTotal, &mut out.positions.tracks),
        ] {
            values.extend(tag.get_strings(key).map(str::to_owned));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use lofty::{
        TextEncoding,
        id3::v2::{ExtendedTextFrame, Frame, Id3v2Tag, UniqueFileIdentifierFrame},
        mp4::{Atom, AtomData, AtomIdent, Ilst},
        ogg::tag::VorbisComments,
    };

    fn check(tag: Tag) {
        let p = extract(&[tag]);
        for (scope, semantics, value) in [
            (
                Scope::Recording,
                Semantics::RecordingIdentity,
                "recording-id",
            ),
            (
                Scope::Occurrence,
                Semantics::OccurrenceIdentity,
                "occurrence-id",
            ),
            (Scope::Album, Semantics::AlbumIdentity, "group-id"),
            (Scope::Edition, Semantics::EditionIdentity, "edition-id"),
        ] {
            assert!(p.observations.iter().any(|o| o.scope == scope
                && o.semantics == semantics
                && o.identity.external_id == value));
        }
    }
    #[test]
    fn id3_ufid_is_recording_not_release_track() {
        let mut t = Id3v2Tag::new();
        t.insert(Frame::UniqueFileIdentifier(UniqueFileIdentifierFrame::new(
            "http://musicbrainz.org",
            b"recording-id".to_vec(),
        )));
        for (key, value) in [
            ("MusicBrainz Release Track Id", "occurrence-id"),
            ("MusicBrainz Album Id", "edition-id"),
            ("MusicBrainz Release Group Id", "group-id"),
        ] {
            t.insert(Frame::UserText(ExtendedTextFrame::new(
                TextEncoding::UTF8,
                key,
                value,
            )));
        }
        check(t.into());
    }
    #[test]
    fn vorbis_picard_trackid_is_recording_and_releasetrackid_is_occurrence() {
        let mut t = VorbisComments::new();
        for (key, value) in [
            ("MUSICBRAINZ_TRACKID", "recording-id"),
            ("MUSICBRAINZ_RELEASETRACKID", "occurrence-id"),
            ("MUSICBRAINZ_ALBUMID", "edition-id"),
            ("MUSICBRAINZ_RELEASEGROUPID", "group-id"),
        ] {
            t.insert(key.into(), value.into());
        }
        check(t.into());
    }
    #[test]
    fn mp4_freeform_picard_identifiers_keep_their_semantics() {
        let mut t = Ilst::new();
        for (key, value) in [
            ("MusicBrainz Track Id", "recording-id"),
            ("MusicBrainz Release Track Id", "occurrence-id"),
            ("MusicBrainz Album Id", "edition-id"),
            ("MusicBrainz Release Group Id", "group-id"),
        ] {
            t.insert(Atom::new(
                AtomIdent::Freeform {
                    mean: "com.apple.iTunes".into(),
                    name: key.into(),
                },
                AtomData::UTF8(value.into()),
            ));
        }
        check(t.into());
    }
    #[test]
    fn explicit_totals_artist_scopes_and_isrc_survive_extraction() {
        let mut t = VorbisComments::new();
        for (key, value) in [
            ("TRACKNUMBER", "1"),
            ("TRACKTOTAL", "1"),
            ("DISCNUMBER", "1"),
            ("DISCTOTAL", "1"),
            ("MUSICBRAINZ_ARTISTID", "track-artist"),
            ("MUSICBRAINZ_ALBUMARTISTID", "album-artist"),
            ("ISRC", "AAA123"),
        ] {
            t.insert(key.into(), value.into());
        }
        let p = extract(&[t.into()]);
        assert_eq!(
            crate::provenance::completeness([&p.positions]),
            crate::edition::Completeness::TrustedComplete
        );
        for scope in [Scope::TrackArtist, Scope::AlbumArtist, Scope::Recording] {
            assert!(p.observations.iter().any(|o| o.scope == scope));
        }
        assert!(
            p.observations
                .iter()
                .any(|o| o.semantics == Semantics::Isrc)
        );
    }
}
