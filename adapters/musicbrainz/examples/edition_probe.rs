//! Explicit, read-only audit; no automatic matching or identity writes.
use music_library::{
    catalog::CatalogProvider,
    domain::{ExternalIdentity, ReleaseId},
    edition::{EditionProvider, compare},
    edition_storage,
};
use music_library_musicbrainz::MusicBrainz;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: edition_probe DATABASE APPLICATION_RELEASE_ID".into());
    }
    let local = edition_storage::read_only(&args[0], &ReleaseId(args[1].clone()))?;
    let groups: Vec<_> = local
        .grouping_identities
        .iter()
        .filter(|i| i.provider == "musicbrainz" && i.kind == "release_group")
        .collect();
    let [group] = groups.as_slice() else {
        return Err("this MusicBrainz discovery probe requires one known Release Group; the generic comparator does not".into());
    };
    let mut provider = MusicBrainz::new();
    let page = provider.releases(group, 0)?;
    let complete = page.next_offset.is_none() && page.items.len() <= 3;
    let mut candidates = vec![];
    for candidate in page.items.iter().take(3) {
        let id: &ExternalIdentity = &candidate.identity;
        candidates.push(provider.edition(id)?);
    }
    println!("Local evidence: {local:#?}\nCandidates: {candidates:#?}");
    let start = std::time::Instant::now();
    let report = compare(&local, &candidates, complete);
    let elapsed = start.elapsed();
    println!(
        "Overall: {:?}; candidate discovery complete: {}",
        report.assessment, report.candidates_complete
    );
    for candidate in &report.candidates {
        println!(
            "{}: {:?}; {} supported Tracks; {:?}",
            candidate.identity.external_id,
            candidate.assessment,
            candidate.supported_tracks,
            candidate.findings
        );
        for track in &candidate.tracks {
            println!(
                "  Local {} -> provider sequence {:?}: {:?}",
                track.track_id.as_ref(),
                track.candidate_index.map(|i| i + 1),
                track.findings
            );
        }
    }
    println!(
        "Local comparison time: {elapsed:?}\nRead-only; no identities attached. At most one browse plus three detail operations."
    );
    Ok(())
}
