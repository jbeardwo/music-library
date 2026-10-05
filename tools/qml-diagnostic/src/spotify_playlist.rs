//! UI glue for the read-only playlist importer, using the connected user worker.
use crate::{Bridge, spotify_playback::Command, string};
use music_library::{
    Library,
    playlist_import::{Conflict, Decision, Outcome, Plan},
};
use music_library_spotify::{
    playback::{Error as AuthError, Playback},
    playlists::{Error, Summary},
};
use qmetaobject::{QPointer, QVariantList, QVariantMap};
use std::path::PathBuf;

#[derive(Default)]
pub struct State {
    busy: bool,
    message: String,
    playlists: Vec<Summary>,
    resume: Option<(String, String)>,
    needs_auth: bool,
    staged: Option<Plan>,
    conflicts: Vec<Conflict>,
}
pub enum Reply {
    Browse(Vec<Summary>),
    Conflict {
        plan: Plan,
        conflicts: Vec<Conflict>,
    },
    Imported {
        outcome: Outcome,
        name: String,
        unsupported: usize,
        unavailable: usize,
    },
}
type Completion = Result<Reply, (String, bool)>;
pub struct Job {
    pub action: String,
    pub input: String,
    pub path: PathBuf,
    pub staged: Option<Plan>,
    pub decision: Option<Decision>,
    pub progress: Box<dyn Fn(String) + Send>,
    pub finish: Box<dyn Fn(Completion) + Send>,
}
impl Job {
    pub fn run(self, spotify: &mut Playback) {
        let result = (|| {
            if self.action == "browse" {
                return spotify
                    .account_playlists()
                    .map(Reply::Browse)
                    .map_err(auth_error);
            }
            let mut library = Library::open(&self.path).map_err(|e| (e.to_string(), false))?;
            let plan = if let Some(plan) = self.staged {
                plan
            } else {
                let id = music_library_spotify::playlists::parse_playlist(&self.input)
                    .map_err(auth_error)?;
                spotify
                    .fetch_playlist(&id, &mut |count| {
                        (self.progress)(format!("Loading Spotify playlist… {count} items"))
                    })
                    .map_err(auth_error)?
            };
            let conflicts = library
                .playlist_import_conflicts(&plan)
                .map_err(|e| (e.to_string(), false))?;
            if self.decision.is_none() && !conflicts.is_empty() {
                return Ok(Reply::Conflict { plan, conflicts });
            }
            (self.progress)(format!("Importing {} Tracks…", plan.items.len()));
            let outcome = library
                .resolve_playlist_import(&plan, self.decision.as_ref().unwrap_or(&Decision::Create))
                .map_err(|e| (e.to_string(), false))?;
            Ok(Reply::Imported {
                outcome,
                name: match &self.decision {
                    Some(Decision::Rename(name)) => name.trim().to_owned(),
                    _ => plan.name,
                },
                unsupported: plan.unsupported,
                unavailable: plan.unavailable,
            })
        })();
        (self.finish)(result);
    }
}
fn auth_error(error: Error) -> (String, bool) {
    let needs = matches!(
        error,
        Error::Authorization(
            AuthError::AuthorizationRequired
                | AuthError::ReauthorizationRequired
                | AuthError::InsufficientScope
        )
    );
    (error.to_string(), needs)
}
impl Bridge {
    pub fn spotify_playlist_value(&self) -> QVariantMap {
        let state = &self.spotify_playlist_state;
        let mut value = QVariantMap::default();
        value.insert("busy".into(), state.busy.into());
        value.insert("message".into(), string(&state.message));
        value.insert("needsAuth".into(), state.needs_auth.into());
        value.insert("conflict".into(), (!state.conflicts.is_empty()).into());
        value.insert("canOverwrite".into(), (state.conflicts.len() == 1).into());
        value.insert(
            "incomingName".into(),
            string(
                state
                    .staged
                    .as_ref()
                    .map(|p| p.name.as_str())
                    .unwrap_or_default(),
            ),
        );
        let rows: QVariantList = state
            .playlists
            .iter()
            .map(|p| {
                let mut row = QVariantMap::default();
                row.insert("id".into(), string(&p.id));
                row.insert("name".into(), string(&p.name));
                row.insert("owner".into(), string(&p.owner));
                row.insert(
                    "count".into(),
                    string(p.count.map(|n| format!("{n} items")).unwrap_or_default()),
                );
                qmetaobject::QVariant::from(row)
            })
            .collect();
        value.insert("playlists".into(), rows.into());
        value
    }
    pub fn spotify_playlist_action_impl(&mut self, action: String, input: String) {
        if action == "cancel" && !self.spotify_playlist_state.busy {
            self.spotify_playlist_state.staged = None;
            self.spotify_playlist_state.conflicts.clear();
            self.spotify_playlist_state.message =
                "Import cancelled. Existing playlists were kept.".into();
            self.spotify_playlist_changed();
            return;
        }
        if action == "connect" {
            if self.spotify_playlist_state.busy {
                return;
            }
            self.spotify_playback_action("connect".into(), String::new());
            return;
        }
        if self.spotify_playlist_state.busy
            || !matches!(
                action.as_str(),
                "browse" | "import" | "rename" | "overwrite"
            )
        {
            return;
        }
        if action == "import"
            && let Err(e) = music_library_spotify::playlists::parse_playlist(&input)
        {
            self.spotify_playlist_state.message = e.to_string();
            self.spotify_playlist_changed();
            return;
        }
        let decision = match action.as_str() {
            "rename" if self.spotify_playlist_state.staged.is_some() => {
                Some(Decision::Rename(input.clone()))
            }
            "overwrite" if self.spotify_playlist_state.conflicts.len() == 1 => Some(
                Decision::Overwrite(self.spotify_playlist_state.conflicts[0].playlist_id.clone()),
            ),
            "rename" | "overwrite" => return,
            _ => None,
        };
        let staged = if decision.is_some() {
            self.spotify_playlist_state.staged.clone()
        } else {
            None
        };
        if action == "rename" && (input.trim().is_empty() || input.chars().any(char::is_control)) {
            self.spotify_playlist_state.message = "Enter a non-empty usable playlist title.".into();
            self.spotify_playlist_changed();
            return;
        }
        if matches!(action.as_str(), "browse" | "import") {
            self.spotify_playlist_state.staged = None;
            self.spotify_playlist_state.conflicts.clear();
        }
        let path = match self.session.library.playlist_import_path() {
            Ok(p) => p,
            Err(e) => {
                self.spotify_playlist_state.message = e.to_string();
                self.spotify_playlist_changed();
                return;
            }
        };
        if self.spotify_playback_worker.is_none() {
            self.spotify_playback_action("visible".into(), "false".into());
        }
        let pointer = QPointer::from(&*self);
        let progress_pointer = pointer.clone();
        let progress = qmetaobject::queued_callback(move |message: String| {
            if let Some(p) = progress_pointer.as_pinned() {
                let mut b = p.borrow_mut();
                b.spotify_playlist_state.message = message;
                b.spotify_playlist_changed();
            }
        });
        let request = (action.clone(), input.clone());
        let finish = qmetaobject::queued_callback(move |result: Result<Reply, (String, bool)>| {
            if let Some(p) = pointer.as_pinned() {
                let mut b = p.borrow_mut();
                b.spotify_playlist_state.busy = false;
                match result {
                    Ok(Reply::Conflict { plan, conflicts }) => {
                        b.spotify_playlist_state.message = if conflicts.len() == 1 {
                            format!(
                                "A playlist named “{}” already exists{}.\nOverwrite replaces its contents and discards local edits.",
                                conflicts[0].name,
                                if conflicts[0].same_source {
                                    " with this Spotify source"
                                } else {
                                    ""
                                }
                            )
                        } else {
                            "Multiple playlists match this title or Spotify source. Rename incoming to keep them unchanged.".into()
                        };
                        b.spotify_playlist_state.staged = Some(plan);
                        b.spotify_playlist_state.conflicts = conflicts;
                    }
                    Ok(Reply::Browse(rows)) => {
                        b.spotify_playlist_state.message =
                            format!("{} Spotify playlists", rows.len());
                        b.spotify_playlist_state.playlists = rows;
                        b.spotify_playlist_state.resume = None;
                        b.spotify_playlist_state.needs_auth = false;
                    }
                    Ok(Reply::Imported {
                        outcome,
                        name,
                        unsupported,
                        unavailable,
                    }) => {
                        b.spotify_playlist_state.staged = None;
                        b.spotify_playlist_state.conflicts.clear();
                        b.spotify_playlist_state.resume = None;
                        b.spotify_playlist_state.needs_auth = false;
                        let id = match outcome {
                            Outcome::Imported {
                                playlist_id,
                                entries,
                            } => {
                                let mut lines =
                                    vec![format!("Imported “{name}”"), format!("{entries} Tracks")];
                                if unsupported > 0 {
                                    lines.push(format!("{unsupported} unsupported items skipped"));
                                }
                                if unavailable > 0 {
                                    lines.push(format!("{unavailable} unavailable items skipped"));
                                }
                                b.spotify_playlist_state.message = lines.join("\n");
                                playlist_id
                            }
                            Outcome::AlreadyImported { playlist_id } => {
                                b.spotify_playlist_state.message="This Spotify playlist is already imported. Local edits have been kept.".into();
                                playlist_id
                            }
                        };
                        b.browse_action_impl("view", 0, "Playlists".into());
                        b.browse_action_impl("refresh", 0, String::new());
                        b.browse_action_impl("select", 0, id.clone());
                        if b.auto_match {
                            b.reconcile_spotify_playlist(&id);
                        }
                    }
                    Err((message, needs_auth)) => {
                        b.spotify_playlist_state.message = message;
                        b.spotify_playlist_state.needs_auth = needs_auth;
                        b.spotify_playlist_state.resume = needs_auth.then(|| request.clone());
                    }
                }
                b.spotify_playlist_changed();
            }
        });
        let job = Job {
            action,
            input,
            path,
            staged,
            decision,
            progress: Box::new(progress),
            finish: Box::new(finish),
        };
        self.spotify_playlist_state.busy = self
            .spotify_playback_worker
            .as_ref()
            .is_some_and(|w| w.send(Command::Playlist(Box::new(job))));
        self.spotify_playlist_state.message = if self.spotify_playlist_state.busy {
            "Loading Spotify playlist…"
        } else {
            "Spotify worker is unavailable. Check Spotify connection settings and retry."
        }
        .into();
        self.spotify_playlist_state.needs_auth = false;
        self.spotify_playlist_changed();
    }
    pub fn resume_spotify_playlist(&mut self, authorized: bool) {
        if authorized && let Some((action, input)) = self.spotify_playlist_state.resume.take() {
            self.spotify_playlist_action_impl(action, input);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{sample, session::Session, spotify_playback::Worker};
    use qmetaobject::prelude::*;
    fn call(engine: &mut QmlEngine, name: &str) -> String {
        engine
            .invoke_method(name.into(), &[])
            .to_qstring()
            .to_string()
    }
    #[test]
    fn spotify_conflict_cancel_overwrite_rename_and_access_error_keep_dialog_usable() {
        let (temp, mut library) = sample::create().unwrap();
        let existing = library.create_playlist("Collision").unwrap();
        let queued = library
            .search(&music_library::domain::SearchRequest {
                limit: 200,
                ..Default::default()
            })
            .unwrap()[0]
            .track_id
            .clone();
        library.append_playlist_track(&existing, &queued).unwrap();
        let mut bridge = Bridge::new(Session::new(library));
        bridge.session.playback.enqueue(queued.clone());
        let (worker, commands) = Worker::fake();
        bridge.spotify_playback_worker = Some(worker);
        let bridge = QObjectBox::new(bridge);
        let mut engine = QmlEngine::new();
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        let extra = r#"
        TestCase {id:conflictTest;when:false}
        function flushConflict() {conflictTest.wait(100);return "ok";}
        function chooseRename() {spotifyIncomingTitle.text="Renamed incoming";spotifyRenameDialog.accept();return "ok";}
        "#;
        let qml = include_str!("../Main.qml")
            .replacen("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
            .replacen(
                "    function ready() {",
                &format!("{extra}\n    function ready() {{"),
                1,
            );
        engine.load_data(qml.into());
        assert!(engine.invoke_method("ready".into(), &[]).to_bool());
        let plan = Plan {
            provider: "spotify".into(),
            external_id: "3cEYpjA9oz9GiPac4AsH4n".into(),
            source_url: "https://open.spotify.com/playlist/3cEYpjA9oz9GiPac4AsH4n".into(),
            version: Some("v2".into()),
            owner: "Other owner".into(),
            name: "Collision".into(),
            items: vec![],
            unsupported: 0,
            unavailable: 0,
        };
        let next = || {
            let Command::Playlist(job) = commands
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap()
            else {
                panic!("playlist job")
            };
            job
        };
        for action in ["cancel", "overwrite", "rename"] {
            bridge
                .pinned()
                .borrow_mut()
                .spotify_playlist_action_impl("import".into(), plan.external_id.clone());
            let job = next();
            assert!(job.staged.is_none());
            let conflicts = bridge
                .pinned()
                .borrow()
                .session
                .library
                .playlist_import_conflicts(&plan)
                .unwrap();
            (job.finish)(Ok(Reply::Conflict {
                plan: plan.clone(),
                conflicts,
            }));
            call(&mut engine, "flushConflict");
            assert!(
                !bridge
                    .pinned()
                    .borrow()
                    .spotify_playlist_state
                    .conflicts
                    .is_empty()
            );
            if action == "rename" {
                assert_eq!(call(&mut engine, "chooseRename"), "ok");
            } else {
                bridge
                    .pinned()
                    .borrow_mut()
                    .spotify_playlist_action_impl(action.into(), String::new());
            }
            if action == "cancel" {
                assert!(commands.try_recv().is_err());
                assert!(
                    bridge
                        .pinned()
                        .borrow()
                        .spotify_playlist_state
                        .staged
                        .is_none()
                );
                continue;
            }
            let job = next();
            assert!(job.staged.is_some());
            let mut spotify =
                Playback::new("test-client".into(), temp.path().join("auth.json")).unwrap();
            job.run(&mut spotify);
            assert_eq!(spotify.snapshot.api_requests, 0);
            assert_eq!(spotify.snapshot.token_requests, 0);
            call(&mut engine, "flushConflict");
            assert!(
                bridge
                    .pinned()
                    .borrow()
                    .spotify_playlist_state
                    .conflicts
                    .is_empty()
            );
        }
        let pin = bridge.pinned();
        let b = pin.borrow();
        assert!(
            b.session
                .library
                .playlist_details(&existing)
                .unwrap()
                .is_some()
        );
        assert_eq!(
            b.session
                .library
                .playlist_import_conflicts(&plan)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(b.session.playback.state().queue, vec![queued]);
        assert!(!auth_error(Error::AccessDenied).1);
        assert!(auth_error(Error::Authorization(AuthError::ReauthorizationRequired)).1);
        bridge
            .pinned()
            .borrow_mut()
            .spotify_playlist_action_impl("import".into(), plan.external_id.clone());
        (next().finish)(Err(auth_error(Error::AccessDenied)));
        call(&mut engine, "flushConflict");
        assert!(!bridge.pinned().borrow().spotify_playlist_state.needs_auth);
        bridge
            .pinned()
            .borrow_mut()
            .spotify_playlist_action_impl("import".into(), plan.external_id);
        assert_eq!(next().action, "import");
    }
    #[test]
    fn spotify_playlist_dialog_browse_reconnect_import_and_queue_independence() {
        let (_temp, library) = sample::create().unwrap();
        let mut bridge = Bridge::new(Session::new(library));
        let (worker, commands) = Worker::fake();
        bridge.spotify_playback_worker = Some(worker);
        let track = bridge.session.rows[0].track_id.clone();
        bridge.session.playback.enqueue(track.clone());
        let saved = bridge
            .session
            .library
            .browse(&music_library::browse::Request {
                limit: 200,
                ..Default::default()
            })
            .unwrap()
            .len();
        let bridge = QObjectBox::new(bridge);
        let mut engine = QmlEngine::new();
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        let extra = r#"
        TestCase { id: playlistImportTest; when: false }
        function startSpotifyImportTest() {
            window.bridge.browse_action("view",0,"Playlists");
            const plus=playlistImportTest.findChild(window.contentItem,"playlistAddButton");
            const menu=playlistImportTest.findChild(plus,"playlistAddMenu");
            plus.clicked();playlistImportTest.wait(20);
            if(!menu.opened || menu.itemAt(0).text!=="New" || menu.itemAt(1).text!=="From Spotify") return "add chooser";
            menu.itemAt(0).triggered();menu.close();playlistImportTest.wait(20);
            if(!playlistNameDialog.opened || playlistNameDialog.rename) return "existing New flow";
            playlistNameDialog.reject();
            plus.clicked();menu.itemAt(1).triggered();menu.close();
            return spotifyPlaylistDialog.opened ? "ok" : "existing Spotify flow";
        }
        function flushSpotifyImportTest() { playlistImportTest.wait(100); return "ok"; }
        function browseSpotifyImportTest() { playlistImportTest.wait(100); if (spotifyPlaylistDialog.state.playlists.length !== 1 || spotifyPlaylistDialog.state.busy) return "browse state"; spotifyPlaylistFilter.text="owner"; spotifyPlaylistLink.text="bad link"; window.bridge.spotify_playlist_action("import",spotifyPlaylistLink.text); if (spotifyPlaylistDialog.state.message.indexOf("valid Spotify") < 0) return "malformed link"; window.bridge.spotify_playlist_action("import","3cEYpjA9oz9GiPac4AsH4n"); return "ok"; }
        function authSpotifyImportTest() { playlistImportTest.wait(100); if (!spotifyPlaylistDialog.state.needsAuth || spotifyPlaylistDialog.state.busy) return "scope prompt"; window.bridge.spotify_playlist_action("connect",""); return "ok"; }
        function importedSpotifyImportTest() { playlistImportTest.wait(300); if (spotifyPlaylistDialog.state.busy || spotifyPlaylistDialog.state.message.indexOf("3 Tracks") < 0) return "result"; if (window.library.panes[2].rows.length !== 3) return "songs missing"; const rows=window.library.panes[2].rows; if (rows[0].id===rows[2].id || rows[0].track.trackId!==rows[2].track.trackId) return "duplicate identity"; if (rows[0].length !== "03:00") return "duration: " + JSON.stringify(rows[0]); spotifyPlaylistDialog.close(); return "ok"; }
        "#;
        let qml = include_str!("../Main.qml")
            .replacen("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
            .replacen(
                "    function ready() {",
                &format!("{extra}\n    function ready() {{"),
                1,
            );
        engine.load_data(qml.into());
        assert!(engine.invoke_method("ready".into(), &[]).to_bool());
        assert_eq!(call(&mut engine, "startSpotifyImportTest"), "ok");
        let Command::Playlist(job) = commands
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap()
        else {
            panic!("browse command")
        };
        assert_eq!(job.action, "browse");
        (job.finish)(Ok(Reply::Browse(vec![Summary {
            id: "3cEYpjA9oz9GiPac4AsH4n".into(),
            name: "Recommendations".into(),
            owner: "Owner".into(),
            count: Some(3),
        }])));
        assert_eq!(call(&mut engine, "browseSpotifyImportTest"), "ok");
        let Command::Playlist(job) = commands
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap()
        else {
            panic!("import command")
        };
        (job.finish)(Err((
            "Reconnect Spotify to authorize playlist access".into(),
            true,
        )));
        assert_eq!(call(&mut engine, "authSpotifyImportTest"), "ok");
        assert!(matches!(
            commands
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap(),
            Command::Connect
        ));
        {
            let pinned = bridge.pinned();
            let mut b = pinned.borrow_mut();
            b.spotify_playback_state.authorization =
                music_library_spotify::playback::AuthorizationState::Authorizing;
            b.apply_player_update(crate::spotify_playback::Update {
                snapshot: music_library_spotify::playback::Snapshot {
                    authorization: music_library_spotify::playback::AuthorizationState::Connected,
                    ..Default::default()
                },
                application_command: false,
                completion: None,
                generation: 0,
                volume_ack: None,
            });
        }
        let Command::Playlist(job) = commands
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap()
        else {
            panic!("resumed import command")
        };
        let identity = music_library::domain::ExternalIdentity {
            provider: "spotify".into(),
            kind: "track".into(),
            external_id: "1234567890123456789012".into(),
        };
        let item = music_library::playlist_import::Item {
            identity: identity.clone(),
            title: "Imported song".into(),
            credits: vec![],
            release_identity: music_library::domain::ExternalIdentity {
                kind: "album".into(),
                ..identity
            },
            release_title: "Imported edition".into(),
            release_credits: vec![],
            year: None,
            disc: Some(1),
            number: Some(1),
            duration: Some(music_library::catalog::Duration {
                milliseconds: 180000,
                approximate: false,
            }),
        };
        let plan = music_library::playlist_import::Plan {
            provider: "spotify".into(),
            external_id: "3cEYpjA9oz9GiPac4AsH4n".into(),
            source_url: "https://open.spotify.com/playlist/3cEYpjA9oz9GiPac4AsH4n".into(),
            version: Some("v1".into()),
            owner: "Owner".into(),
            name: "Recommendations".into(),
            items: vec![
                item.clone(),
                music_library::playlist_import::Item {
                    identity: music_library::domain::ExternalIdentity {
                        external_id: "1234567890123456789013".into(),
                        ..item.identity.clone()
                    },
                    ..item.clone()
                },
                item,
            ],
            unsupported: 0,
            unavailable: 0,
        };
        let outcome = Library::open(&job.path)
            .unwrap()
            .import_playlist_snapshot(&plan)
            .unwrap();
        let Outcome::Imported {
            ref playlist_id, ..
        } = outcome
        else {
            panic!()
        };
        let imported_id = playlist_id.clone();
        (job.progress)("Importing 3 Tracks…".into());
        (job.finish)(Ok(Reply::Imported {
            outcome,
            name: plan.name.clone(),
            unsupported: 0,
            unavailable: 0,
        }));
        assert_eq!(call(&mut engine, "importedSpotifyImportTest"), "ok");
        let pinned = bridge.pinned();
        let b = pinned.borrow();
        assert_eq!(b.session.playback.state().queue, vec![track]);
        assert_eq!(
            b.session
                .library
                .browse(&music_library::browse::Request {
                    limit: 200,
                    ..Default::default()
                })
                .unwrap()
                .len(),
            saved
        );
        {
            let mut b = pinned.borrow_mut();
            b.browse_action_impl("playlist-sort", 2, "title".into());
            b.browse_action_impl("context-play", 0, imported_id);
        }
        assert_eq!(call(&mut engine, "flushSpotifyImportTest"), "ok");
        let b = pinned.borrow();
        let queue = &b.session.playback.state().queue;
        assert_eq!(queue.len(), 3);
        assert_eq!(queue[0], queue[2]);
        assert_ne!(queue[0], queue[1]);
    }
}
#[cfg(test)]
mod live_tests {
    use super::*;
    use qmetaobject::prelude::*;
    #[test]
    #[ignore = "real connected Spotify account and MUSIC_LIBRARY_SPOTIFY_TEST_PLAYLIST"]
    fn live_spotify_playlist_import_qml() {
        let link = std::env::var("MUSIC_LIBRARY_SPOTIFY_TEST_PLAYLIST")
            .expect("Set a real accessible playlist link");
        let external = music_library_spotify::playlists::parse_playlist(&link).unwrap();
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("playlist.sqlite");
        let bridge = QObjectBox::new(Bridge::new(crate::session::Session::new(
            Library::open(&path).unwrap(),
        )));
        let mut engine = QmlEngine::new();
        engine.set_object_property("diagnostic".into(), bridge.pinned());
        let extra = r#"
        TestCase { id: livePlaylistTest; when: false }
        function livePlaylistScrollStability() {
            const list=livePlaylistTest.findChild(songsPane,"libraryPane2");
            bridge.browse_action("select",2,library.panes[2].rows[80].id);
            list.positionViewAtIndex(80,ListView.Beginning);livePlaylistTest.wait(100);
            const y=list.contentY,model=list.model,delegate=list.itemAtIndex(80),entry=library.song;
            bridge.spotify_playback_action("visible","true");
            for(let n=0;n<150;n++) {
                bridge.refresh_clock();livePlaylistTest.wait(200);
                if(list.model!==model || list.itemAtIndex(80)!==delegate || Math.abs(list.contentY-y)>0.1 || library.song!==entry) return "viewport/model changed during live polling";
            }
            bridge.spotify_playback_action("visible","false");
            console.log("Live mally viewport and delegates stable for 30 seconds with Spotify polling and clock updates");
            return "ok";
        }
        function livePlaylistReimport(link,mode) {
            spotifyPlaylistDialog.open();
            for(let n=0;n<6000 && spotifyPlaylistDialog.state.busy;n++) livePlaylistTest.wait(10);
            bridge.spotify_playlist_action("import",link);
            for(let n=0;n<6000 && spotifyPlaylistDialog.state.busy;n++) livePlaylistTest.wait(10);
            if(!spotifyPlaylistDialog.state.conflict) return "re-import conflict missing: "+spotifyPlaylistDialog.state.message;
            bridge.spotify_playlist_action(mode,mode==="rename" ? "mally Spotify validation" : "");
            for(let n=0;n<6000 && spotifyPlaylistDialog.state.busy;n++) livePlaylistTest.wait(10);
            livePlaylistTest.wait(200);spotifyPlaylistDialog.close();
            return spotifyPlaylistDialog.state.message.startsWith("Imported “") ? "ok" : spotifyPlaylistDialog.state.message;
        }
        function liveSpotifyPlaylistImport(link) {
            window.bridge.browse_action("view",0,"Playlists");
            spotifyPlaylistDialog.open();
            for (let n=0;n<6000 && spotifyPlaylistDialog.state.busy;n++) livePlaylistTest.wait(10);
            if (spotifyPlaylistDialog.state.needsAuth) return spotifyPlaylistDialog.state.message;
            console.log("Spotify account browse:", spotifyPlaylistDialog.state.message, "rows:", spotifyPlaylistDialog.state.playlists.length);
            if (spotifyPlaylistDialog.state.message.indexOf(" Spotify playlists") < 0) return "Account browse: " + spotifyPlaylistDialog.state.message;
            spotifyPlaylistLink.text=link;
            window.bridge.spotify_playlist_action("import",spotifyPlaylistLink.text);
            for (let n=0;n<6000 && spotifyPlaylistDialog.state.busy;n++) livePlaylistTest.wait(10);
            for (let n=0;n<6000 && (window.library.pending || window.library.playlistDetails.pending);n++) livePlaylistTest.wait(10);
            livePlaylistTest.wait(200);
            if (spotifyPlaylistDialog.state.message.indexOf("Imported “") !== 0) return spotifyPlaylistDialog.state.message;
            livePlaylistTest.grabImage(window.contentItem).save("/tmp/spotify-playlist-import-live.png");
            spotifyPlaylistDialog.close();
            livePlaylistTest.wait(100);
            livePlaylistTest.grabImage(window.contentItem).save("/tmp/spotify-playlist-table-live.png");
            return "ok";
        }
        "#;
        let qml = include_str!("../Main.qml")
            .replacen("import QtQuick\n", "import QtQuick\nimport QtTest\n", 1)
            .replacen(
                "    function ready() {",
                &format!("{extra}\n    function ready() {{"),
                1,
            );
        engine.load_data(qml.into());
        assert!(engine.invoke_method("ready".into(), &[]).to_bool());
        assert_eq!(
            engine
                .invoke_method("liveSpotifyPlaylistImport".into(), &[string(link.clone())])
                .to_qstring()
                .to_string(),
            "ok"
        );
        let pinned = bridge.pinned();
        let b = pinned.borrow();
        let playlist = b
            .session
            .library
            .imported_playlist("spotify", &external)
            .unwrap()
            .unwrap();
        assert_eq!(
            engine
                .invoke_method("livePlaylistScrollStability".into(), &[])
                .to_qstring()
                .to_string(),
            "ok"
        );
        let original_count = b
            .session
            .library
            .playlist_details(&playlist)
            .unwrap()
            .unwrap()
            .entry_count;
        let entry = b
            .session
            .library
            .playlist_entries(&playlist, None, 1)
            .unwrap()[0]
            .id
            .clone();
        bridge
            .pinned()
            .borrow_mut()
            .session
            .library
            .remove_playlist_entry(&playlist, &entry)
            .unwrap();
        assert_eq!(
            engine
                .invoke_method(
                    "livePlaylistReimport".into(),
                    &[string(link.clone()), string("overwrite")]
                )
                .to_qstring()
                .to_string(),
            "ok"
        );
        assert_eq!(
            b.session
                .library
                .playlist_details(&playlist)
                .unwrap()
                .unwrap()
                .entry_count,
            original_count
        );
        assert_eq!(
            engine
                .invoke_method(
                    "livePlaylistReimport".into(),
                    &[string(link), string("rename")]
                )
                .to_qstring()
                .to_string(),
            "ok"
        );
        assert_eq!(
            b.session
                .library
                .playlist_import_conflicts(&Plan {
                    provider: "spotify".into(),
                    external_id: external.clone(),
                    source_url: String::new(),
                    version: None,
                    owner: String::new(),
                    name: "mally".into(),
                    items: vec![],
                    unsupported: 0,
                    unavailable: 0
                })
                .unwrap()
                .len(),
            2
        );
        let details = b
            .session
            .library
            .playlist_details(&playlist)
            .unwrap()
            .unwrap();
        assert_eq!(details.unknown_duration_count, 0);
        assert!(b.session.playback.state().queue.is_empty());
        let db = rusqlite::Connection::open(&path).unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM library_membership", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        for pane in [
            music_library::browse::Pane::Songs,
            music_library::browse::Pane::Artists,
            music_library::browse::Pane::Albums,
            music_library::browse::Pane::Genres,
        ] {
            assert!(
                b.session
                    .library
                    .browse(&music_library::browse::Request {
                        pane,
                        limit: 200,
                        ..Default::default()
                    })
                    .unwrap()
                    .is_empty()
            );
        }
        let queue = b
            .session
            .library
            .library_queue_reader()
            .unwrap()
            .read_playlist(&playlist, None)
            .unwrap()
            .0;
        assert_eq!(queue.len() as u64, details.entry_count);
        let unique: std::collections::HashSet<_> =
            queue.iter().map(|r| r.track_id.clone()).collect();
        let restored = Library::open(&path).unwrap();
        assert_eq!(
            restored.playlist_details(&playlist).unwrap(),
            Some(details.clone())
        );
        println!(
            "Live QML imported {:?}: {} entries ({} repeated occurrences), {} ms duration; all Library panes empty, unchanged queue, offline metadata verified. Screenshots /tmp/spotify-playlist-import-live.png and /tmp/spotify-playlist-table-live.png",
            details.name,
            details.entry_count,
            queue.len() - unique.len(),
            details.known_duration_ms
        );
    }
}
