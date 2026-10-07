    TestCase { id: headersTest; name: "SongAlbumHeaders"; when: false }
    function exerciseSongAlbumHeaders() {
        function check(v,m) { if(!v) throw new Error(m); }
        function wait() { headersTest.wait(40); }
        function done() { for(let i=0;i<250 && library.pending;i++) wait();check(!library.pending && !library.error,"operation finishes "+library.error); }
        const list=headersTest.findChild(window.contentItem,"libraryPane2");
        function headers() { const found=[]; function scan(item) { for(const child of item.children || []) { if(child.objectName==="songAlbumHeader" && child.height>0 && child.visible) found.push(child); scan(child); } } scan(list.contentItem); return found; }
        function row(i) {list.forceLayout();return list.itemAtIndex(i);}
        function click(i,modifier) {headersTest.mouseClick(row(i),20,20,Qt.LeftButton,modifier||Qt.NoModifier);wait();done();}
        function connection(index) {
            const list = headersTest.findChild(window.contentItem, "libraryPane2");
            list.positionViewAtIndex(index, ListView.Center); wait(); list.forceLayout();
            const rowData = library.panes[2].rows[index];
            const id = rowData.track ? rowData.track.trackId : rowData.id;
            const before = JSON.stringify([view.currentId, view.queue, view.status, view.position]);
            headersTest.mouseClick(list.itemAtIndex(index), 20, 20, Qt.RightButton); wait();
            check(libraryMenu.visible, "Song right-click menu opens");
            const action = headersTest.findChild(libraryMenu, "songSpotifyConnection");
            check(action && action.visible && action.enabled, "Spotify action in " + library.view);
            check(contextTrackId === id, "clicked canonical Track captured");
            action.triggered(); libraryMenu.close(); wait();
            check(spotifyPlaybackDialog.visible && spotifyPlayback.trackId === id, "diagnostic targets clicked Track in " + library.view);
            check(JSON.stringify([view.currentId, view.queue, view.status, view.position]) === before, "inspection preserves playback");
            check(spotifyPlayback.title.indexOf(rowData.title) >= 0, "persisted selected metadata");
            if (!spotifyPlayback.available) {
                check(headersTest.findChild(spotifyPlaybackDialog, "spotifyConnectionSearch").enabled, "unconnected Track permits existing search workflow");
                check(!spotifyPlayback.resolutionPending, "opening does not initiate catalog lookup");
            }
            spotifyPlaybackDialog.close(); wait();
            list.positionViewAtBeginning(); wait(); list.forceLayout();
        }
        try {
            wait();bridge.browse_action("view",0,"Artists");wait();
            bridge.browse_action("select",0,library.panes[0].rows[0].id);wait();
            // Artist Songs: A-Z -> Z-A -> Album.
            bridge.browse_action("sort",2,"");bridge.browse_action("sort",2,"");wait();list.forceLayout();
            check(library.panes[2].sort==="Album" && list.count===200,"Album Songs remain a bounded logical Song list");
            check(headers().length===1 && headers()[0].text==="Same album name","one first-page Album header, no lookahead header: "+headers().length+" "+headers().map(h=>h.text)+" keys "+library.panes[2].rows[0].albumId+" section "+list.section.property);
            check(headers()[0].section===library.panes[2].rows[0].albumId,"header uses canonical Album identity");
            const before=library.panes[2].selectionCount;
            headersTest.mouseClick(headers()[0],20,10);wait();
            check(library.panes[2].selectionCount===before,"header is not a selectable Song");
            check(headersTest.findChild(row(0),"trackNumber").text==="1.01","multi-disc number unchanged");
            const subtitle=headersTest.findChild(row(0),"songSubtitle");
            check(subtitle && subtitle.text.indexOf("Same album name")<0,"album name omitted from each Song row");
            bridge.browse_action("next",2,"");wait();list.forceLayout();
            const rows=library.panes[2].rows;
            check(rows.length===200 && rows[0].albumId!==rows[1].albumId,"page starts with one continuation then next canonical Album");
            check(headers().length===2 && headers().every(h=>h.text==="Same album name"),"same-title Albums have separate headers, one per page group: "+headers().map(h=>h.section+":"+h.text+":"+h.visible));
            check(new Set(headers().map(h=>h.section)).size===2,"canonical section IDs remain distinct");
            check(headersTest.findChild(row(0),"trackNumber").text==="2.101","continued disc.Track numbering");
            click(0);click(1,Qt.ControlModifier);check(library.panes[2].selectionCount===2,"Ctrl selection across headers");
            click(2,Qt.ShiftModifier);check(library.panes[2].selectionCount===2,"Shift range uses Song IDs, not header positions");
            headersTest.mouseClick(row(1),20,20,Qt.RightButton);wait();
            check(libraryMenu.visible && library.panes[2].selectionCount===2,"selected context retains range across header");
            libraryMenu.itemAt(1).triggered();libraryMenu.close();done();
            check(view.queueTotal===2 && view.queue[0].trackId===rows[1].id && view.queue[1].trackId===rows[2].id,"queue actions use logical Song selection");
            headersTest.mouseDoubleClickSequence(row(1),20,20,Qt.LeftButton);wait();done();
            check(view.queueTotal===402 && view.position===201 && view.currentId===rows[1].id,"playback starts exact Song across Album boundary");
            bridge.browse_action("picker-open",2,rows[1].id);
            bridge.browse_action("playlist-create",0,"Header selection");wait();
            const dest=library.playlistChoices.find(p=>p.name==="Header selection");
            bridge.browse_action("picker-add",0,dest.id);done();
            for(let attempt=0;attempt<10 && library.panes[2].page<3;attempt++) {bridge.browse_action("next",2,"");wait();}
            check(library.panes[2].page>=3,"third Song page is reachable");list.forceLayout();
            check(library.panes[2].rows.length===2 && headers().length===1,"last page retains one continuation header");
            bridge.browse_action("previous",2,"");wait();list.forceLayout();
            check(headers().length===2,"return to boundary page has neither missing nor duplicated headers"); connection(180);
            for(let n=0;n<2;n++) {
                bridge.browse_action("sort",2,"");wait();list.forceLayout();
                connection(150);
                check(!headers().length && library.panes[2].rows.every(r=>!r.albumId),"alphabetical Songs have no Album headers");
                check(!headersTest.findChild(row(0),"trackNumber").visible,"alphabetical Track-number behavior unchanged");
                check(headersTest.findChild(row(0),"songSubtitle").text.indexOf("Same album name")>=0,"flat Songs retain album context");
            }
            bridge.browse_action("view",0,"Playlists");bridge.browse_action("select",0,dest.id);wait();
            check(library.panes[2].rows.length===1 && library.panes[2].rows[0].track.trackId===rows[1].id && !headers().length,"Add to Playlist from headed rows, persisted order without headers");
            return "ok";
        } catch(e) {return String(e);}
    }
