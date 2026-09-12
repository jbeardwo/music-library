//! Read-only: pass files from ONE local Release, or one directory containing it.
use music_library::{
    filesystem::{LoftyMetadataExtractor, MetadataExtractor},
    provenance::EditionProvenance,
};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.first().is_some_and(|a| a == "--database") {
        if args.len() != 3 {
            return Err("usage: local_provenance --database LIBRARY.sqlite RELEASE_ID".into());
        }
        let release = music_library::domain::ReleaseId(
            args[2].to_str().ok_or("Release ID must be UTF-8")?.into(),
        );
        let evidence = music_library::edition_storage::read_only(&args[1], &release)?;
        println!(
            "Durable database evidence (no file reads, network, migration or identity writes):\n{evidence:#?}"
        );
        let acceptance =
            music_library::provenance_acceptance::inspect_database(&args[1], &evidence.album_id)?;
        println!(
            "Acceptance diagnostics: managed = retractable local acceptance; independent = protected canonical association. Assessments describe current observations, not new writes. Other categories remain observation-only.\n{acceptance:#?}"
        );
        return Ok(());
    }
    let mut extractor = LoftyMetadataExtractor;
    let mut paths = Vec::new();
    for arg in std::env::args_os().skip(1) {
        let path = PathBuf::from(arg);
        if path.is_dir() {
            for entry in walkdir::WalkDir::new(path).follow_links(false) {
                let entry = entry?;
                if entry.file_type().is_file() && extractor.supports(entry.path()) {
                    paths.push(entry.into_path());
                }
            }
        } else {
            paths.push(path);
        }
    }
    if paths.is_empty() {
        return Err("usage: local_provenance FILE... | ONE_RELEASE_DIRECTORY".into());
    }
    paths.sort();
    let mut files = Vec::new();
    for path in paths {
        let metadata = extractor.read(&path)?;
        println!(
            "File: {}\nLocal Album: {:?}, Album artists: {:?}\nLocal Track: {:?}, Track artists: {:?}, duration_ms: {:?}\n{:#?}",
            path.display(),
            metadata.release_title,
            metadata.release_artists,
            metadata.track_title,
            metadata.track_artists,
            metadata.duration_ms,
            metadata.provenance
        );
        files.push(vec![metadata.provenance]);
    }
    let summary = EditionProvenance::new(files);
    println!(
        "Aggregate (one explicitly supplied local Release):\nCompleteness: {:?}\nAlbum/edition: {:#?}\nPer-file Recording/occurrence: {:#?}\nObservations only: no identities attached, no metadata changed.",
        summary.completeness, summary.album_and_edition, summary.per_track
    );
    Ok(())
}
