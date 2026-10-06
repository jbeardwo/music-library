    Rectangle { anchors.fill: parent; z: -1; color: window.color }
    TestCase { id: viewsTest; name: "LibraryViews"; when: false }
    function exerciseLibraryViews() {
        function check(value, message) { if (!value) throw new Error(message); }
        function wait() { viewsTest.wait(30); }
        function page(name) {
            const nav = viewsTest.findChild(window.contentItem, "libraryView" + name);
            check(nav && nav.visible && nav.enabled, name + " navigation clickable");
            viewsTest.mouseClick(nav); wait();
            check(library.view === name, name + " navigation switched");
        }
        function select(pane, index) {
            if (pane === 1) { albumsPane.selectRow(index); }
            else if (pane === 0) { artistsPane.selectRow(index); }
            else { songsPane.selectRow(index); }
            wait();
        }
        function sort(pane) { window.bridge.browse_action("sort", pane, ""); wait(); }
        function headings(left, albums) {
            check(artistsPane.visible === left && albumsPane.visible === albums && songsPane.visible, "visible panes for " + library.view);
        }
        function numbers(shown) {
            check(library.panes[2].rows.every(r => shown ? !!r.number : !r.number), "number presentation in " + library.panes[2].sort);
            const list = viewsTest.findChild(window.contentItem, "libraryPane2"); list.forceLayout();
            const label = viewsTest.findChild(list.itemAtIndex(0), "trackNumber");
            check(label && label.visible === shown, "actual number label");
            if (shown) {
                const title=viewsTest.findChild(list.itemAtIndex(0), "songTitle");
                const numberPosition=label.mapToItem(list,0,0), titlePosition=title.mapToItem(list,0,0);
                check(numberPosition.y === titlePosition.y && numberPosition.x + label.width < titlePosition.x, "track number immediately left of title");
            }
        }
        function connection(index) {
            const list = viewsTest.findChild(window.contentItem, "libraryPane2");
            list.positionViewAtIndex(index, ListView.Center); wait(); list.forceLayout();
            const rowData = library.panes[2].rows[index];
            const id = rowData.track ? rowData.track.trackId : rowData.id;
            const before = JSON.stringify([view.currentId, view.queue, view.status, view.position]);
            viewsTest.mouseClick(list.itemAtIndex(index), 20, 20, Qt.RightButton); wait();
            check(libraryMenu.visible, "Song right-click menu opens");
            const action = viewsTest.findChild(libraryMenu, "songSpotifyConnection");
            check(action && action.visible && action.enabled, "Spotify action in " + library.view);
            check(contextTrackId === id, "clicked canonical Track captured");
            action.triggered(); libraryMenu.close(); wait();
            check(spotifyPlaybackDialog.visible && spotifyPlayback.trackId === id, "diagnostic targets clicked Track in " + library.view);
            check(JSON.stringify([view.currentId, view.queue, view.status, view.position]) === before, "inspection preserves playback");
            check(spotifyPlayback.title.indexOf(rowData.title) >= 0, "persisted selected metadata");
            if (!spotifyPlayback.available) {
                check(viewsTest.findChild(spotifyPlaybackDialog, "spotifyConnectionSearch").enabled, "unconnected Track permits existing search workflow");
                check(!spotifyPlayback.resolutionPending, "opening does not initiate catalog lookup");
            }
            spotifyPlaybackDialog.close(); wait();
            list.positionViewAtBeginning(); wait(); list.forceLayout();
        }
        try {
            wait();
            check(library.view === "Artists", "Artists default"); headings(true,true);
            window.width=800; wait();
            for(const name of ["Artists","Genres","Albums","Songs","Playlists"]) page(name);
            const lastNav=viewsTest.findChild(window.contentItem,"libraryViewPlaylists");
            const navRight=lastNav.mapToItem(window.contentItem,lastNav.width,0).x;
            check(navRight <= query.mapToItem(window.contentItem,0,0).x, "navigation and search fit minimum window");
            window.width=1180; page("Artists");
            select(0,0); connection(0);
            const artist = library.artist, artistSort = library.panes[1].sort;
            page("Genres"); headings(true,true);
            check(library.panes.every(p=>p.page===1), "new view starts on page one");
            check(artistsPane.heading === "GENRES", "Genres heading");
            const rockIndex = library.panes[0].rows.findIndex(r => r.title === "Rock");
            check(rockIndex >= 0, "persisted genre present"); select(0,rockIndex); connection(0);
            check(library.genre === "Rock", "genre selection");
            check(library.panes[1].rows.length === 2 && library.panes[2].rows.length === 3, "genre membership and deduplication");
            const additional = library.panes[1].rows.findIndex(r => r.title === "Additional");
            select(1,additional);
            check(library.panes[2].rows.length === 1 && library.panes[2].rows[0].title === "Alpha", "Album within Genre excludes Jazz and unsaved Rock");
            numbers(true); connection(0);
            check(library.panes[2].rows[0].number === "1.02", "multi-disc numbers disambiguate disc one too");
            window.bridge.browse_action("play",2,library.panes[2].rows[0].id);
            page("Songs"); page("Genres");
            for(let i=0;i<250 && library.pending;++i) viewsTest.wait(20);
            check(view.queueTotal === 1, "explicit Play snapshots Genre intersection");
            const queue = JSON.stringify(view.queue), current = view.currentId;
            sort(2); numbers(false); sort(2); numbers(false);
            check(library.panes[2].sort === "Z-A", "Genre Z-A available");
            page("Albums"); headings(false,true);
            check(library.panes[1].rows.length === 3 && library.panes[2].rows.length === 48, "all saved Albums and Songs");
            sort(1);
            check(library.panes[1].sort === "Year", "Year sort");
            check(JSON.stringify(library.panes[1].rows.map(r => r.year)) === JSON.stringify(["2024","2019",""]), "year query order with missing last");
            const albums = viewsTest.findChild(window.contentItem,"libraryPane1"); albums.forceLayout();
            check(albums.count === 3, "year sections");
            for(let i=0;i<3;++i) {
                const section = albums.itemAtIndex(i);
                check(section.rowData.groupTitle === ["2024","2019","Unknown"][i], "year section heading");
                const tile = section.tileAt(i);
                check(tile && tile.width === 120 && tile.height === 174, "fixed year tiles");
            }
            select(1,0);
            check(library.panes[2].rows.length === 2, "Album selection filters Songs");
            numbers(true);
            check(JSON.stringify(library.panes[2].rows.map(r => r.number)) === '["1.02","2.01"]', "disc/track order");
            sort(2); numbers(false); sort(2);
            check(JSON.stringify(library.panes[2].rows.map(r => r.title)) === '["Zulu","Alpha"]', "exact reverse alphabetic order");
            page("Songs"); headings(false,false);
            check(library.panes[2].rows.length === 48, "full library bounded Songs page");
            check(songsPane.width >= librarySplit.width - 1, "Songs full width");
            const ascending = library.panes[2].rows.map(r => r.id);
            numbers(false); sort(2); numbers(false);
            check(JSON.stringify(library.panes[2].rows.map(r => r.id)) === JSON.stringify(ascending.slice().reverse()), "Z-A reverses titles and ID ties");
            connection(0); select(2,3); const selectedSong=library.song;
            page("Playlists"); headings(true,true);
            check(albumsPane.heading === "DETAILS", "playlist Details pane");
            check(artistsPane.heading === "PLAYLISTS", "Playlists heading");
            check(library.panes[0].rows.length === 0 && library.panes[2].rows.length === 0, "playlist placeholder and blank Songs");
            page("Artists"); headings(true,true);
            check(library.artist === artist && library.panes[1].sort === artistSort, "Artist state restored");
            page("Albums"); check(library.panes[1].sort === "Year" && library.album && library.panes[2].sort === "Z-A", "Album state restored");
            page("Genres"); check(library.genre === "Rock" && library.album && library.panes[2].sort === "Z-A", "Genre state restored");
            page("Songs"); check(library.panes[2].sort === "Z-A" && library.song === selectedSong && songsPane.songId === selectedSong, "Song state restored");
            check(JSON.stringify(view.queue) === queue && view.currentId === current, "navigation, selections and sorts preserve active queue");
            check(library.panes.every(p => p.rows.length <= 200), "all panes bounded");
            window.bridge.search_library("Additional",2);
            for(let i=0;i<200 && window.bridge.local_search_snapshot.busy;++i) viewsTest.wait(10);
            check(window.bridge.navigate_search_result(0) && library.view === "Artists" && library.album, "search returns to Artist hierarchy from Songs");
            check(JSON.stringify(view.queue) === queue, "search navigation preserves queue");
            page("Songs");
            viewsTest.grabImage(window.contentItem).save("/tmp/library-views-songs.png"); wait();
            page("Albums");
            viewsTest.grabImage(window.contentItem).save("/tmp/library-views-years.png"); wait();
            page("Playlists");
            viewsTest.grabImage(window.contentItem).save("/tmp/library-views-playlists.png"); wait();
            page("Songs");
            const track = library.panes[2].rows[0].id;
            window.bridge.browse_action("picker-open",2,track);
            window.bridge.browse_action("playlist-create",0,"Test playlist"); wait();
            const playlist = library.playlistChoices.find(p => p.name === "Test playlist");
            check(playlist,"created playlist in chooser");
            window.bridge.browse_action("picker-add",2,playlist.id);
            for(let i=0;i<100 && library.pending;i++) wait();
            window.bridge.browse_action("picker-add",2,playlist.id);
            for(let i=0;i<100 && library.pending;i++) wait();
            check(duplicatePlaylistDialog.visible,"single duplicate confirmation");
            window.bridge.browse_action("picker-yes",0,"");
            for(let i=0;i<100 && library.pending;i++) wait();
            page("Playlists");
            window.bridge.browse_action("select",0,playlist.id); wait();
            check(library.panes[2].rows.length===2,"duplicate entries displayed");
            const entries=library.panes[2].rows.map(r=>r.id);
            check(entries[0]!==entries[1],"independent entry identities");
            window.bridge.browse_action("playlist-up",2,entries[1]); wait();
            check(library.panes[2].rows[0].id===entries[1],"move persists in view");
            window.bridge.browse_action("sort",2,""); wait();
            check(library.panes[2].rows[0].id===entries[1] && library.panes[2].sort==="","no playlist sorting");
            page("Artists"); page("Playlists");
            check(library.artist===playlist.id && library.panes[2].rows.length===2,"playlist session selection retained"); connection(0);
            window.bridge.browse_action("playlist-rename",0,"Renamed playlist"); wait();
            check(library.panes[0].rows.some(p=>p.title==="Renamed playlist"),"renamed playlist displayed");
            window.bridge.browse_action("play",2,entries[0]);
            for(let i=0;i<100 && library.pending;i++) wait();
            check(view.queueTotal===2 && view.position===1,"playlist duplicate exact queue start");
            window.bridge.browse_action("playlist-remove",2,entries[1]); wait();
            check(library.panes[2].rows.length===1 && view.queueTotal===2,"entry removal preserves queue snapshot");
            window.bridge.browse_action("playlist-delete",0,playlist.id); wait();
            check(library.panes[2].rows.length===0 && view.queueTotal===2,"delete playlist preserves queue");
            return "ok";
        } catch(e) { return e.message; }
    }
    function exerciseRealLibraryViews() {
        function check(value,message) { if(!value) throw new Error(message); }
        function page(name) { viewsTest.mouseClick(viewsTest.findChild(window.contentItem,"libraryView"+name)); viewsTest.wait(60); check(library.view===name,"navigation "+name); }
        function capture(name) { viewsTest.grabImage(window.contentItem).save("/tmp/library-views-real-"+name+".png"); }
        try {
            viewsTest.wait(100);
            check(library.view==="Artists","default Artists"); capture("artists");
            page("Genres");
            check(library.panes[0].rows.length>0,"real local genre tags");
            const genre=library.panes[0].rows[0].title;
            artistsPane.selectRow(0); viewsTest.wait(100);
            check(library.panes[1].rows.length>0 && library.panes[2].rows.length>0,"real Genre Album and Song matches");
            capture("genres");
            albumsPane.selectRow(0); viewsTest.wait(60);
            check(library.panes[2].rows.length>0,"Album inside real Genre");
            capture("genre-album");
            page("Albums"); window.bridge.browse_action("sort",1,""); viewsTest.wait(100); capture("years");
            albumsPane.selectRow(0); viewsTest.wait(60); capture("numbers");
            window.bridge.browse_action("sort",2,""); viewsTest.wait(60); capture("alphabetical");
            page("Songs"); window.bridge.browse_action("sort",2,""); viewsTest.wait(100); capture("reverse-songs");
            check(library.panes[2].rows.length<=200,"real library remains bounded");
            page("Playlists"); check(library.panes[2].rows.length===0,"real playlist Songs blank"); capture("playlists");
            return "ok: real genre "+genre;
        } catch(e) { return e.message; }
    }
