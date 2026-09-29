    TestCase { id: musicTest; name: "AddMusic"; when: false }
    function exerciseAddMusic() {
        function check(v, label) { if (!v) throw new Error(label); }
        function waitResults() {
            for (let n = 0; n < 500 && addMusicPanel.view.busy; ++n) musicTest.wait(10);
            musicTest.wait(20);
            check(!addMusicPanel.view.busy, "catalog request completed");
        }
        function search(text, filter) {
            musicQuery.text = text; addMusicPanel.filter = filter || 0;
            addMusicPanel.request(); waitResults(); return addMusicPanel.view.hits;
        }
        function open(index) {
            musicResults.forceLayout(); musicResults.positionViewAtIndex(index, ListView.Center); musicTest.wait(20);
            const row=musicResults.itemAtIndex(index); check(row !== null, "result delegate exists");
            musicTest.mouseClick(row, 30, row.height / 2); waitResults();
        }
        try {
            const queue=JSON.stringify(view.queue);
            const playback=JSON.stringify(window.spotifyPlayback);
            addMusicPanel.open(); musicTest.wait(40);
            musicQuery.text="slow"; addMusicPanel.request(); musicTest.wait(40);
            check(addMusicPanel.view.busy, "slow provider is pending");
            musicQuery.text="tricot"; addMusicPanel.request();
            musicTest.wait(20); check(musicQuery.text === "tricot", "typing remains responsive");
            waitResults();
            check(addMusicPanel.view.hits.every(r => r.title !== "slow"), "stale search rejected");
            let hits=addMusicPanel.view.hits;
            check(hits.some(r=>r.section === "ARTISTS") && hits.some(r=>r.section === "ALBUMS") && hits.some(r=>r.section === "SONGS"), "grouped results survive unavailable provider");
            open(hits.findIndex(r=>r.section === "ARTISTS"));
            check(addMusicPanel.view.heading === "tricot" && !addMusicPanel.view.detail, "Artist browses catalog");
            check(library.panes[2].rows.length === 0, "browsing Artist does not add music");
            open(0);
            check(addMusicPanel.view.detail && addMusicPanel.view.tracks.length === 2, "Album detail has complete ordered program");
            check(addMusicPanel.view.tracks[0].position === "1.1" && addMusicPanel.view.tracks[1].position === "2.1", "disc ordering");
            window.bridge.add_music_action("song", "2:1"); musicTest.wait(30);
            check(addMusicPanel.view.membership === "1 of 2 Tracks in library", "partial Album membership");
            check(!addMusicPanel.view.tracks[0].saved && addMusicPanel.view.tracks[1].saved, "only selected Song saved");
            check(library.panes[2].rows.length === 1, "main library updated");
            window.bridge.add_music_action("song", "2:1");
            check(library.panes[2].rows.length === 1, "repeated Song add is idempotent");
            window.bridge.add_music_action("back", "");
            check(addMusicPanel.view.hits[0].membership === "1 of 2 Tracks in library", "Artist Album result reflects partial membership");
            window.bridge.add_music_action("back", "");
            hits=search("Hatsumimi",3);
            check(hits.length === 1 && hits[0].section === "SONGS", "Song filter");
            open(0);
            check(addMusicPanel.view.song && addMusicPanel.view.tracks.length === 1, "Song preview selects only occurrence");
            window.bridge.add_music_action("song", "1:1");
            check(addMusicPanel.view.tracks[0].saved, "Song preview reflects saved state");
            hits=search("T H E",2); check(hits.every(r=>r.section === "ALBUMS"), "Album filter");
            open(0); check(addMusicPanel.view.complete, "Album is fully saved");
            window.bridge.add_music_action("album", "");
            check(library.panes[2].rows.length === 2, "Album re-add creates no duplicate membership");
            hits=search("tricot",1); check(hits.every(r=>r.section === "ARTISTS"), "Artist filter");
            search("tricot",0); open(addMusicPanel.view.hits.findIndex(r=>r.section === "ALBUMS"));
            window.bridge.add_music_action("album", "");
            check(addMusicPanel.view.complete && library.panes[2].rows.length === 2, "Add Album");
            check(JSON.stringify(view.queue) === queue && JSON.stringify(window.spotifyPlayback) === playback, "Add never starts playback or alters queue");
            musicQuery.text="slow"; addMusicPanel.request(); musicTest.wait(20);
            musicTest.keyClick(Qt.Key_Escape); musicTest.wait(300);
            check(!addMusicPanel.opened, "Escape dismisses pending search");
            addMusicPanel.open(); hits=search("tricot",0);
            check(hits.some(r=>r.section === "SONGS" && r.membership === "In library"), "search Song is already saved");
            musicTest.grabImage(window.contentItem).save("/tmp/add-music-results.png");
            open(hits.findIndex(r=>r.section === "ALBUMS"));
            musicTest.grabImage(window.contentItem).save("/tmp/add-music-album.png");
            addMusicPanel.close();
            return "ok";
        } catch(e) { return String(e); }
    }
