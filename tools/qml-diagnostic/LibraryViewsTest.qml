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
        try {
            wait();
            check(library.view === "Artists", "Artists default"); headings(true,true);
            window.width=800; wait();
            for(const name of ["Artists","Genres","Albums","Songs","Playlists"]) page(name);
            const lastNav=viewsTest.findChild(window.contentItem,"libraryViewPlaylists");
            const navRight=lastNav.mapToItem(window.contentItem,lastNav.width,0).x;
            check(navRight <= query.mapToItem(window.contentItem,0,0).x, "navigation and search fit minimum window");
            window.width=1180; page("Artists");
            select(0,0);
            const artist = library.artist, artistSort = library.panes[1].sort;
            page("Genres"); headings(true,true);
            check(library.panes.every(p=>p.page===1), "new view starts on page one");
            check(artistsPane.heading === "GENRES", "Genres heading");
            const rockIndex = library.panes[0].rows.findIndex(r => r.title === "Rock");
            check(rockIndex >= 0, "persisted genre present"); select(0,rockIndex);
            check(library.genre === "Rock", "genre selection");
            check(library.panes[1].rows.length === 2 && library.panes[2].rows.length === 3, "genre membership and deduplication");
            const additional = library.panes[1].rows.findIndex(r => r.title === "Additional");
            select(1,additional);
            check(library.panes[2].rows.length === 1 && library.panes[2].rows[0].title === "Alpha", "Album within Genre excludes Jazz and unsaved Rock");
            numbers(true);
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
            select(2,3); const selectedSong=library.song;
            page("Playlists"); headings(true,false);
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
