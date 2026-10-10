//! Mutations are restricted to an explicitly supplied disposable database.
use music_library::{Library, filesystem::LoftyMetadataExtractor, local_ingestion::Request};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("disposable database path required")?;
    let mut l = Library::open(path)?;
    if let Some(folder) = std::env::args().nth(2) {
        println!(
            "{:#?}",
            l.ingest_local(
                &Request::Folder(folder.into()),
                &mut LoftyMetadataExtractor,
                &mut |_| {}
            )?
        );
    }
    Ok(())
}
