use music_library::{
    Library,
    artwork::{MAX_EDGE, Provider},
    domain::*,
};
use std::{io::Cursor, path::PathBuf};
struct Art {
    bytes: Vec<u8>,
    calls: usize,
    fail_mb: bool,
}
impl Provider for Art {
    fn fetch(&mut self, id: &ExternalIdentity) -> Option<(String, Vec<u8>)> {
        self.calls += 1;
        if self.fail_mb && id.provider == "musicbrainz" {
            None
        } else {
            Some((format!("https://art/{}", id.provider), self.bytes.clone()))
        }
    }
}
fn png(w: u32, h: u32) -> Vec<u8> {
    let mut output = Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(w, h)
        .write_to(&mut output, image::ImageFormat::Png)
        .unwrap();
    output.into_inner()
}
fn fixture() -> (tempfile::TempDir, Library, String, TrackId) {
    let temp = tempfile::tempdir().unwrap();
    let mut l = Library::open(temp.path().join("db")).unwrap();
    let r = l
        .create_catalog_release(&CatalogReleaseInput {
            title: "Partial".into(),
            year: Some(2015),
            artists: vec![],
            tracks: vec![CatalogTrackInput {
                title: "Song".into(),
                disc_number: Some(1),
                track_number: Some(1),
                artists: vec![],
            }],
        })
        .unwrap();
    l.add_to_library(&r.track_ids[0]).unwrap();
    let a = l.album_for_release(&r.release_id).unwrap().album_id;
    let db = rusqlite::Connection::open(temp.path().join("db")).unwrap();
    for (provider, kind) in [("musicbrainz", "release_group"), ("spotify", "album")] {
        db.execute(
            "INSERT INTO album_external_identity VALUES (?1,?2,?3,'known')",
            rusqlite::params![a.as_ref(), provider, kind],
        )
        .unwrap();
    }
    (temp, l, a.0, r.track_ids[0].clone())
}
#[test]
fn provider_cache_resize_reuse_missing_file_offline_and_malformed() {
    let (tmp, l, id, track) = fixture();
    let mut r = l.artwork_resolver(tmp.path().join("cache")).unwrap();
    let mut art = Art {
        bytes: png(1000, 600),
        calls: 0,
        fail_mb: true,
    };
    let keys = vec![id.clone(), format!("track:{}", track.as_ref())];
    let rows = r.resolve(&keys, &mut art).unwrap();
    assert_eq!(art.calls, 2);
    assert_eq!(rows[0].1, rows[1].1);
    let path = rows[0].1.clone().unwrap();
    assert_eq!(image::image_dimensions(&path).unwrap(), (MAX_EDGE, 300));
    assert_eq!(r.resolve(&keys, &mut art).unwrap()[0].1, Some(path.clone()));
    assert_eq!(art.calls, 2);
    std::fs::remove_file(path).unwrap();
    assert!(r.resolve(&keys, &mut art).unwrap()[0].1.is_some());
    assert_eq!(art.calls, 4);
    let cached = r.resolve(&keys, &mut art).unwrap()[0].1.clone().unwrap();
    let bytes = std::fs::read(&cached).unwrap();
    std::fs::write(&cached, &bytes[..33]).unwrap(); // intact PNG dimensions, missing pixels
    assert!(r.resolve(&keys, &mut art).unwrap()[0].1.is_some());
    assert_eq!(
        art.calls, 6,
        "corrupt cache is reconstructed instead of reused"
    );
    let db = rusqlite::Connection::open(tmp.path().join("db")).unwrap();
    assert_eq!(
        db.query_row("SELECT origin FROM album_artwork", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "spotify"
    );
    db.execute("DELETE FROM album_artwork", []).unwrap();
    art.bytes = b"malformed".to_vec();
    assert!(r.resolve(&keys, &mut art).unwrap()[0].1.is_none());
    let calls = art.calls;
    assert!(r.resolve(&keys, &mut art).unwrap()[0].1.is_none());
    assert_eq!(
        calls, art.calls,
        "negative cache suppresses repeated browsing requests"
    );
}
fn local(tmp: &tempfile::TempDir, track: &TrackId, embedded: bool) -> PathBuf {
    use lofty::{
        config::WriteOptions,
        picture::{Picture, PictureType},
        prelude::TagExt,
        tag::{Tag, TagType},
    };
    let path = tmp.path().join("song.wav");
    let mut wav = Vec::new();
    wav.extend(b"RIFF");
    wav.extend(38_u32.to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16_u32.to_le_bytes());
    wav.extend(1_u16.to_le_bytes());
    wav.extend(1_u16.to_le_bytes());
    wav.extend(8000_u32.to_le_bytes());
    wav.extend(16000_u32.to_le_bytes());
    wav.extend(2_u16.to_le_bytes());
    wav.extend(16_u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend(2_u32.to_le_bytes());
    wav.extend([0, 0]);
    std::fs::write(&path, wav).unwrap();
    if embedded {
        let mut tag = Tag::new(TagType::Id3v2);
        let mut pic = Picture::from_reader(&mut Cursor::new(png(120, 60))).unwrap();
        pic.set_pic_type(PictureType::CoverFront);
        tag.push_picture(pic);
        tag.save_to_path(&path, WriteOptions::default()).unwrap();
    }
    let db = rusqlite::Connection::open(tmp.path().join("db")).unwrap();
    db.execute_batch("INSERT INTO discovery_root(id,kind,location) VALUES ('root','local_filesystem',X'2F'); INSERT INTO playable_source(id,kind) VALUES ('source','local_file');").unwrap();
    db.execute(
        "INSERT INTO track_source(track_id,source_id) VALUES (?1,'source')",
        [track.as_ref()],
    )
    .unwrap();
    #[cfg(unix)]
    let bytes = {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    };
    #[cfg(windows)]
    let bytes = {
        use std::os::windows::ffi::OsStrExt;
        path.as_os_str()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    };
    db.execute("INSERT INTO local_file_observation(source_id,root_id,path,size_bytes,modified_ns,available) VALUES ('source','root',?1,1,1,1)",[bytes]).unwrap();
    path
}
#[test]
fn embedded_then_sidecar_precedes_providers_without_upscaling() {
    for embedded in [false, true] {
        let (tmp, l, id, track) = fixture();
        local(&tmp, &track, embedded);
        std::fs::write(tmp.path().join("cover.jpg"), b"broken").unwrap();
        std::fs::write(tmp.path().join("Folder.PNG"), png(200, 100)).unwrap();
        let mut art = Art {
            bytes: png(500, 500),
            calls: 0,
            fail_mb: false,
        };
        let mut resolver = l.artwork_resolver(tmp.path().join("cache")).unwrap();
        let rows = resolver.resolve(&[id], &mut art).unwrap();
        let image = rows[0].1.as_ref().unwrap();
        assert_eq!(
            image::image_dimensions(image).unwrap(),
            if embedded { (120, 60) } else { (200, 100) }
        );
        assert_eq!(art.calls, 0);
    }
}
#[test]
fn asynchronous_provider_work_does_not_hold_a_database_transaction() {
    let (tmp, l, id, track) = fixture();
    let mut resolver = l.artwork_resolver(tmp.path().join("cache")).unwrap();
    let (start_tx, start_rx) = std::sync::mpsc::channel();
    let (end_tx, end_rx) = std::sync::mpsc::channel();
    struct Gate(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>);
    impl Provider for Gate {
        fn fetch(&mut self, _: &ExternalIdentity) -> Option<(String, Vec<u8>)> {
            self.0.send(()).unwrap();
            self.1.recv().unwrap();
            Some(("https://art".into(), png(20, 20)))
        }
    }
    let album = id.clone();
    let worker = std::thread::spawn(move || {
        resolver
            .resolve(&[id], &mut Gate(start_tx, end_rx))
            .unwrap()
    });
    start_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    let db = rusqlite::Connection::open(tmp.path().join("db")).unwrap();
    db.execute(
        "UPDATE album_application_metadata SET title='Still responsive'",
        [],
    )
    .unwrap();
    local(&tmp, &track, true);
    let mut local_resolver = l.artwork_resolver(tmp.path().join("cache")).unwrap();
    let mut art = Art {
        bytes: png(500, 500),
        calls: 0,
        fail_mb: false,
    };
    let local_art = local_resolver.resolve(&[album], &mut art).unwrap()[0]
        .1
        .clone();
    assert_eq!(art.calls, 0);
    end_tx.send(()).unwrap();
    assert_eq!(
        worker.join().unwrap()[0].1,
        local_art,
        "late provider reply cannot overwrite new embedded art"
    );
}

#[test]
fn no_art_and_spotify_only_and_local_upgrade_invalidate_cached_provider() {
    let (tmp, l, id, track) = fixture();
    let db = rusqlite::Connection::open(tmp.path().join("db")).unwrap();
    db.execute(
        "DELETE FROM album_external_identity WHERE provider='musicbrainz'",
        [],
    )
    .unwrap();
    let mut r = l.artwork_resolver(tmp.path().join("cache")).unwrap();
    let mut art = Art {
        bytes: png(500, 500),
        calls: 0,
        fail_mb: false,
    };
    assert!(
        r.resolve(std::slice::from_ref(&id), &mut art).unwrap()[0]
            .1
            .is_some()
    );
    assert_eq!(art.calls, 1);
    local(&tmp, &track, true);
    let result = r.resolve(std::slice::from_ref(&id), &mut art).unwrap();
    assert_eq!(
        image::image_dimensions(result[0].1.as_ref().unwrap()).unwrap(),
        (120, 60)
    );
    assert_eq!(
        art.calls, 1,
        "new saved local art outranks cached provider art"
    );
    db.execute("DELETE FROM album_external_identity", [])
        .unwrap();
    db.execute("UPDATE local_file_observation SET available=0", [])
        .unwrap();
    assert!(r.resolve(&[id], &mut art).unwrap()[0].1.is_none());
    assert_eq!(
        art.calls, 1,
        "Album without candidates stays visible without network"
    );
}
