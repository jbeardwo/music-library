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
    function exerciseCatalogPlaylist() {
        function check(v, label) { if (!v) throw new Error(label); }
        function waitDone() {
            for (let n=0; n<600 && (addMusicPanel.view.busy || library.pending || (library.playlistDetails || {}).pending); ++n) musicTest.wait(10);
            musicTest.wait(30);
            check(!addMusicPanel.view.busy && !library.pending, "operation completed");
        }
        try {
            window.bridge.browse_action("playlist-create",0,"Catalog destination");
            window.bridge.browse_action("view",0,"Playlists");
            const destination=library.panes[0].rows.find(r=>r.title === "Catalog destination").id;
            window.bridge.browse_action("select",0,destination);
            musicTest.wait(40);
            const addControl=musicTest.findChild(albumsPane,"playlistAddTracks");
            check(addControl.text === "Add Tracks from Catalog" && addControl.enabled, "single Catalog control in Details");
            check(!musicTest.findChild(window.contentItem,"libraryTracksDialog"), "no From Library picker");
            check(artistsPane.x < songsPane.x && songsPane.x < albumsPane.x, "Playlists Songs Details column order");
            addControl.clicked(); musicTest.wait(20);
            check(addMusicPanel.opened && addMusicPanel.playlistMode, "direct catalog workflow opens");
            musicQuery.text="tricot"; addMusicPanel.request(); waitDone();
            window.bridge.add_music_action("open",String(addMusicPanel.view.hits.findIndex(r=>r.section === "ALBUMS"))); waitDone();
            check(addMusicPanel.view.tracks.length === 2, "bounded catalog track selection");
            addMusicPanel.selectedTracks=["2:1","1:1"];
            window.bridge.add_music_action("playlist",addMusicPanel.selectedTracks.join(",")); waitDone();
            check(library.panes[2].rows.length === 2, "catalog-only rows rendered");
            check(library.playlistDetails.name === "Catalog destination" && library.playlistDetails.count === "2", "catalog-only entries count in Details");
            check(library.playlistDetails.duration === "Unknown", "unknown catalog durations are honest");
            check(library.panes[2].rows[0].title === "Second Song", "selected order retained");
            const songs=musicTest.findChild(songsPane,"libraryPane2"); songs.forceLayout();
            const rendered=songs.itemAtIndex(0);
            check(musicTest.findChild(rendered,"songTitle").text === "Second Song", "canonical title rendered");
            const subtitle=musicTest.findChild(rendered,"playlistArtist").text;
            check(subtitle.indexOf("tricot") >= 0 && !library.panes[2].rows[0].track.available, "missing Track credit uses Album fallback and unavailable source stays unavailable");
            check(library.panes[2].rows.map(r=>r.number).join(",") === "1,2", "playlist positions replace album numbers");
            check(addMusicPanel.view.tracks.every(r=>!r.saved), "playlist add does not save");
            window.bridge.add_music_action("playlist","1:1,2:1");
            check(library.duplicateMessage.length > 0, "one aggregate duplicate confirmation");
            window.bridge.browse_action("picker-no",0,""); waitDone();
            check(library.panes[2].rows.length === 2, "No skips duplicates");
            window.bridge.add_music_action("playlist","1:1");
            window.bridge.browse_action("picker-yes",0,""); waitDone();
            check(library.panes[2].rows.length === 3, "Yes preserves duplicate copies");
            check(library.playlistDetails.count === "3", "Details count includes duplicates");
            check(library.panes[2].rows.map(r=>r.number).join(",") === "1,2,3", "duplicates have distinct playlist positions");
            addMusicPanel.close(); addMusicPanel.playlistMode=false;
            const entry=library.panes[2].rows[2].id;
            window.bridge.browse_action("append",2,entry); waitDone();
            check(view.queue.length === 1, "catalog-only entry queued");
            window.bridge.clear_queue();
            window.bridge.browse_action("view",0,"Songs");
            check(library.panes[2].rows.length === 0, "catalog-only entries excluded from Songs");
            window.bridge.add_music_action("destination","");
            return "ok";
        } catch(e) { return String(e); }
    }

    function exerciseLibraryContextPlaylist() {
        function check(v, label) { if (!v) throw new Error(label); }
        try {
            window.bridge.browse_action("view",0,"Playlists");
            const destination=library.panes[0].rows.find(r=>r.title === "Catalog destination").id;
            window.bridge.browse_action("select",0,destination);
            window.bridge.browse_action("view",0,"Songs");
            musicTest.wait(30);
            const song=library.panes[2].rows[0].id;
            window.bridge.browse_action("select",2,song);
            window.bridge.browse_action("picker-open",2,song);
            window.bridge.browse_action("picker-add",0,destination);
            for(let n=0;n<500 && library.pending;n++) musicTest.wait(10);
            window.bridge.browse_action("view",0,"Playlists");
            check(library.view === "Playlists", "return to selected playlist");
            check(library.duplicateMessage.length>0,"Library route shares duplicate policy");
            window.bridge.browse_action("picker-yes",0,"");
            for(let n=0;n<500 && library.pending;n++) musicTest.wait(10);
            check(library.panes[2].rows.length===4,"Library selection inserted through shared batch");
            return "ok";
        } catch(e) { return String(e); }
    }

    function exercisePlaylistDetailsKnown() {
        function check(v,m) { if(!v) throw new Error(m); }
        function waitDetails() {
            for(let n=0;n<500 && (library.pending || (library.playlistDetails || {}).pending);n++) musicTest.wait(10);
            musicTest.wait(20);
        }
        try {
            waitDetails();
            check(library.playlistDetails.count === "4" && library.playlistDetails.duration === "00:05", "known durations count duplicate occurrences");
            check(musicTest.findChild(albumsPane,"playlistDetailsName").text === "Catalog destination", "name label renders");
            check(musicTest.findChild(albumsPane,"playlistDetailsCount").text === "4 tracks", "count label renders");
            const original=library.panes[2].rows.map(r=>r.id);
            window.bridge.browse_action("playlist-down",2,original[0]);
            check(!library.playlistDetails.pending, "reorder reuses cached aggregate");
            check(library.playlistDetails.count === "4" && library.playlistDetails.duration === "00:05", "reorder leaves statistics unchanged");
            waitDetails();
            check(library.panes[2].rows[1].id === original[0] && library.panes[2].rows.map(r=>r.number).join(",") === "1,2,3,4", "reorder updates position numbers and preserves identity");
            window.bridge.browse_action("playlist-rename",0,"Renamed destination");
            check(library.playlistDetails.name === "Renamed destination", "rename updates name immediately");
            waitDetails();
            window.bridge.browse_action("playlist-remove",2,library.panes[2].rows[2].id); waitDetails();
            check(library.playlistDetails.count === "3" && library.playlistDetails.duration === "00:04", "remove updates count and duration");
            check(library.panes[2].rows.map(r=>r.number).join(",") === "1,2,3", "removal closes numbering gaps");
            return "ok";
        } catch(e) { return String(e); }
    }
    function exercisePlaylistDetailsPartialAndClear() {
        function check(v,m) { if(!v) throw new Error(m); }
        function waitDetails() {
            for(let n=0;n<500 && (library.pending || (library.playlistDetails || {}).pending);n++) musicTest.wait(10);
            musicTest.wait(20);
        }
        try {
            waitDetails();
            check(library.playlistDetails.duration === "00:02 (partial; 2 unknown)", "missing durations identified as partial");
            const destination=library.panes[0].rows.find(r=>r.title === "Renamed destination").id;
            window.bridge.browse_action("select",0,"");
            check(library.panes[2].rows.length === 0 && !library.playlistDetails.name, "no selection clears Songs and Details");
            check(musicTest.findChild(albumsPane,"playlistDetailsName").text === "", "cleared name label");
            check(!musicTest.findChild(albumsPane,"playlistAddTracks").visible, "Details empty without selection");
            window.bridge.browse_action("playlist-create",0,"Empty statistics");
            const empty=library.panes[0].rows.find(r=>r.title === "Empty statistics").id;
            window.bridge.browse_action("select",0,empty); waitDetails();
            check(library.playlistDetails.name === "Empty statistics" && library.playlistDetails.count === "0" && library.playlistDetails.duration === "00:00", "select another playlist updates Details");
            window.bridge.browse_action("playlist-delete",0,empty); waitDetails();
            check(!library.playlistDetails.name && library.panes[2].rows.length === 0, "delete clears selected Details and Songs");
            window.bridge.browse_action("select",0,destination); waitDetails();
            check(library.playlistDetails.count === "3", "restored playlist selection");
            musicTest.grabImage(window.contentItem).save("/tmp/playlist-details-polish.png");
            return "ok";
        } catch(e) { return String(e); }
    }

    function exercisePlaylistTable() {
        function check(v,m) {if(!v) throw new Error(m);}
        function done() {for(let n=0;n<500 && (library.pending || library.playlistDetails.pending);n++) musicTest.wait(10);musicTest.wait(30);check(!library.pending && !library.error,"table operation: "+library.error);}
        function click(label) {musicTest.findChild(songsPane,"playlistHeader"+label).clicked();done();}
        try {
            done();
            check(library.playlistSort === "position" && !library.playlistDescending && library.playlistReorderAllowed,"default canonical sort permits reorder");
            const canonical=library.panes[2].rows.map(r=>r.id);
            check(musicTest.findChild(songsPane,"playlistTableHeader").visible,"Playlist details header shown");
            const songs=musicTest.findChild(songsPane,"libraryPane2");songs.forceLayout();
            const row=songs.itemAtIndex(0);
            for(const name of ["trackNumber","songTitle","playlistArtist","playlistAlbum","playlistLength"]) check(musicTest.findChild(row,name).visible,"five compact cells: "+name);
            check(row.height===30,"compact playlist row");
            for(const pair of [["Title","title"],["Artist","artist"],["Album","album"],["Length","length"],["#","position"]]) {
                click(pair[0]);
                check(library.playlistSort===pair[1] && !library.playlistDescending,"header ascending: "+pair[0]);
                const ascending=library.panes[2].rows.map(r=>r.id);
                check(new Set(ascending).size===canonical.length,"occurrences stay distinct");
                check(library.panes[2].rows.every(r=>canonical[Number(r.number)-1]===r.id),"canonical # under "+pair[0]);
                if(pair[1]==="title") check(library.panes[2].rows[0].title==="Hatsumimi","Title uses canonical title");
                if(pair[1]==="length") check(library.panes[2].rows[0].length==="00:01" && library.panes[2].rows[3].length==="00:02","Length numeric ascending");
                click(pair[0]);
                check(library.playlistDescending && !library.playlistReorderAllowed,"second click descending disables reorder");
                check(library.panes[2].rows.map(r=>r.id).join()===ascending.reverse().join(),"deterministic descending: "+pair[0]);
                bridge.browse_action("playlist-down",2,canonical[0]);done();
                check(library.panes[2].rows.map(r=>r.id).join()===ascending.join(),"backend rejects reorder under alternate sort");
            }
            click("#");
            check(library.panes[2].rows.map(r=>r.id).join()===canonical.join() && library.playlistReorderAllowed,"return restores canonical order and reorder");
            click("Title");
            const sorted=library.panes[2].rows.map(r=>r.id);
            const duplicate=library.panes[2].rows[1];
            bridge.browse_action("select",2,duplicate.id);done();
            bridge.browse_action("context",2,duplicate.id);done();
            check(library.panes[2].selectedIds[0]===duplicate.id,"exact duplicate context identity");
            bridge.browse_action("play",2,duplicate.id);done();
            check(window.view.position===Number(duplicate.number)-1,"sorted playback starts canonical duplicate position");
            bridge.browse_action("view",0,"Songs");done();
            check(musicTest.findChild(songsPane,"songsTableHeader").visible,"main Songs has its own table header");
            bridge.browse_action("view",0,"Playlists");done();
            check(library.playlistSort==="title" && library.panes[2].rows.map(r=>r.id).join()===sorted.join(),"session Playlist sort retained");
            click("#");
            bridge.browse_action("select",2,"");done();
            musicTest.grabImage(window.contentItem).save("/tmp/playlist-table.png");
            return "ok";
        } catch(e) {return String(e);}
    }
