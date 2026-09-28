    function exerciseLocalSearch(capture) {
        function check(value, message) { if (!value) throw new Error(message); }
        function search(text, filter) {
            if (!searchPanel.opened) searchPanel.open();
            searchPanel.filter = filter || 0;
            searchText.text = text;
            searchPanel.request();
            for (let n = 0; n < 500 && searchPanel.results.busy; ++n) uiTest.wait(10);
            uiTest.wait(30);
            check(!searchPanel.results.busy && searchPanel.results.error === "", "local search completed: " + searchPanel.results.error);
            return searchPanel.results.rows;
        }
        function choose(index) {
            searchResults.positionViewAtIndex(index, ListView.Center);
            uiTest.wait(80);
            const item = searchResults.itemAtIndex(index);
            check(item !== null, "search result visible");
            uiTest.mouseClick(item, 30, item.height / 2);
            uiTest.wait(60);
            check(!searchPanel.opened, "result closes search");
        }
        try {
            if (capture) {
                for (const query of ["hop", "hop along", "painted", "waitress", "hella", "bygones", "toe"]) {
                    const found=search(query);
                    check(found.length>0, "real-library query " + query);
                    console.log("Local search audit", query, found.length, "results", JSON.stringify(found.slice(0,3)));
                }
            }
            const before = JSON.stringify(view.queue);
            const position = view.position;
            const requests = window.spotifyPlayback.resolutionCounts;
            window.bridge.browse_action("select", 0, library.panes[0].rows[0].id);
            let rows = search("hop");
            check(rows.some(r => r.section === "ARTISTS" && r.title === "Hop Along"), "global Artist match");
            check(rows.some(r => r.section === "ALBUMS" && r.title === "Painted Shut"), "contextual Album");
            check(rows.some(r => r.section === "SONGS" && r.context.indexOf("Hop Along") >= 0), "contextual Song");
            if (capture) uiTest.grabImage(window.contentItem).save("/tmp/music-library-search-results.png");
            const artistIndex = rows.findIndex(r => r.section === "ARTISTS");
            const artist = rows[artistIndex].id;
            choose(artistIndex);
            check(library.artist === artist && library.album === "" && library.song === "", "Artist navigation state");
            check(library.panes[0].rows.some(r => r.id === artist), "Artist target positioned");
            rows = search("painted");
            const albumIndex = rows.findIndex(r => r.section === "ALBUMS");
            const album = rows[albumIndex].id;
            choose(albumIndex);
            check(library.artist === artist && library.album === album && library.song === "", "Album navigation state: " + library.artist + "/" + artist + " " + library.album + "/" + album);
            check(library.panes[1].rows.some(r => r.id === album), "Album target positioned");
            rows = search("waitress", 3);
            check(rows.every(r => r.section === "SONGS"), "Song type filter");
            const song = rows[0].id;
            choose(0);
            check(library.artist === artist && library.album === album && library.song === song, "Song navigation state");
            check(library.panes[2].rows.some(r => r.id === song), "Song target positioned");
            const songs = uiTest.findChild(window.contentItem, "libraryPane2");
            check(songs.currentIndex === library.panes[2].rows.findIndex(r => r.id === song) && songs.activeFocus, "Song focused and visible");
            if (capture) uiTest.grabImage(window.contentItem).save("/tmp/music-library-search-navigation.png");
            check(JSON.stringify(view.queue) === before && view.position === position, "search navigation never changes playback");
            check(window.spotifyPlayback.resolutionCounts === requests, "search performs no provider resolution");
            check(search("hop", 4).length === 0, "deferred playlists");
            uiTest.keyClick(Qt.Key_Escape); uiTest.wait(40);
            check(!searchPanel.opened, "Escape dismisses search");
            // A burst keeps only the last query; earlier replies cannot overwrite it.
            searchPanel.open();
            window.bridge.search_library("a", 0);
            window.bridge.search_library("painted", 0);
            window.bridge.search_library("waitress", 3);
            for (let n = 0; n < 500 && searchPanel.results.busy; ++n) uiTest.wait(10);
            check(searchPanel.results.rows.every(r => r.title === "Waitress"), "latest query wins");
            searchPanel.close();
            return "ok";
        } catch (error) { return String(error); }
    }

    function exerciseDistantSearch() {
        function find(text, filter) {
            searchPanel.open();
            searchText.text=text; searchPanel.filter=filter; searchPanel.request();
            for(let n=0;n<500 && searchPanel.results.busy;++n) uiTest.wait(10);
            if(searchPanel.results.rows.length === 0) throw new Error("missing distant result " + text);
            const id=searchPanel.results.rows[0].id;
            searchPanel.activate(0); uiTest.wait(60);
            if(searchPanel.opened) throw new Error("distant navigation failed");
            return id;
        }
        try {
            const before=JSON.stringify(view.queue);
            window.bridge.browse_action("select",0,"");
            window.bridge.browse_action("refresh",0,"");
            if(library.panes[0].rows.some(r=>r.title==="ZZZ Artist 210") || library.panes[1].rows.some(r=>r.title==="ZZZ Album 210"))
                throw new Error("fixture target must be beyond initial page");
            const artist=find("ZZZ Artist 210",1);
            if(library.artist!==artist || library.album!=="" || library.song!=="") throw new Error("distant Artist state");
            const album=find("ZZZ Album 210",2);
            if(library.album!==album || library.song!=="") throw new Error("distant Album state");
            const song=find("ZZZ target",3);
            const list=uiTest.findChild(window.contentItem,"libraryPane2");
            if(library.album!==album || library.song!==song || list.currentIndex<0 || library.panes[2].rows[list.currentIndex].id!==song)
                throw new Error("distant Song state");
            if(library.panes[2].rows.length>200 || !list.activeFocus) throw new Error("navigation bounded/focused");
            if(JSON.stringify(view.queue)!==before) throw new Error("search altered playback");
            return "ok";
        } catch(error) {return String(error);}
    }
