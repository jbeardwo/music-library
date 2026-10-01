    TestCase { id: scrollingTest; name: "ContinuousScrolling"; when: false }
    function exerciseContinuousScrolling() {
        function check(v,m) { if (!v) throw new Error(m); }
        function wait() { scrollingTest.wait(100); }
        function done() { for(let n=0;n<300 && library.pending;n++) scrollingTest.wait(20); check(!library.pending && !library.error,"operation finished: " + library.error); wait(); }
        function list(p) { return scrollingTest.findChild(window.contentItem,"libraryPane"+p); }
        function down(p) { const l=list(p); l.forceLayout(); l.positionViewAtEnd(); wait(); l.forceLayout(); }
        function action(a,p,id) { bridge.browse_action(a,p,id||""); wait(); }
        function view(name) { action("view",0,name); }
        function bounded(p) { check(library.panes[p].rows.length<=600 && list(p).count<=600,"bounded QML window"); }
        try {
            wait(); view("Songs");
            let rows=library.panes[2].rows;
            check(rows.length===200,"initial chunk");
            const first=rows[0].id;
            action("select-toggle",2,first);
            const seen=new Set(rows.map(r=>r.id));
            const ordered=rows.map(r=>r.id);
            let previous=rows[rows.length-1].title;
            for(let n=0;n<5;n++) {
                const old=library.panes[2].rows;
                down(2); rows=library.panes[2].rows;
                check(rows.length>old.length || rows[0].id!==old[0].id,"scroll automatically fetches later rows n="+n+" old="+old.length+" rows="+rows.length+" y="+list(2).contentY+" origin="+list(2).originY+" h="+list(2).contentHeight+" viewh="+list(2).height+" adjusting="+songsPane.adjustingWindow);
                check(rows.every((r,i)=>i===0 || rows[i-1].title<r.title),"continuous deterministic order");
                for(const r of rows) if(!seen.has(r.id)) { check(r.title>previous,"chunks ordered without overlap errors"); previous=r.title; seen.add(r.id);ordered.push(r.id); }
                bounded(2);
            }
            check(seen.size===1003 && !library.panes[2].more,"all rows exposed without omissions");
            check(!rows.some(r=>r.id===first) && library.panes[2].selectionCount===1,"Ctrl selection survives eviction");
            action("append",2,""); done();
            check(window.view.queueTotal===1 && window.view.queue[0].trackId===first,"context expansion includes offscreen selection");
            action("picker-open",2,"");
            const offscreenDestination=library.playlistChoices.find(p=>p.name==="Playlist 001");
            action("picker-add",2,offscreenDestination.id);done();
            view("Playlists");action("select",0,offscreenDestination.id);
            check(library.panes[2].rows.length===1 && library.panes[2].rows[0].track.trackId===first,"Add to Playlist includes offscreen logical selection");
            view("Songs");rows=library.panes[2].rows;
            const endpoint=rows[20].id, endpointIndex=ordered.indexOf(endpoint);
            action("select-range",2,endpoint); done();
            check(library.panes[2].selectionCount===endpointIndex+1,"Shift range spans discarded chunks outside QML");
            // Anchor a partially visible row and explicitly request the predecessor window.
            const l=list(2);l.positionViewAtIndex(100,ListView.Beginning);wait();
            const anchor=songsPane.viewportAnchor();
            action("scroll-backward",2);
            const restored=songsPane.viewportAnchor();
            check(anchor && restored && anchor.id===restored.id && Math.abs(anchor.pixel-restored.pixel)<2,"window shift preserves exact visible row pixel");
            bounded(2);
            for(let n=0;n<5 && library.panes[2].before;n++) { l.positionViewAtBeginning();wait(); }
            check(library.panes[2].rows[0].id===first && !library.panes[2].before,"upward scrolling restores discarded rows");
            down(2); const saved=songsPane.viewportAnchor();
            view("Artists"); view("Songs");
            const returned=songsPane.viewportAnchor();
            check(saved && returned && saved.id===returned.id && Math.abs(saved.pixel-returned.pixel)<2,"independent per-view scroll state");
            action("sort",2);
            check(library.panes[2].rows.length===200 && !library.panes[2].before && library.panes[2].rows[0].title==="Song 1002","sort resets lazy window");
            const flickList=list(2);
            flickList.positionViewAtIndex(180,ListView.Beginning);
            flickList.flick(0,-2000);
            for(let n=0;n<50 && library.panes[2].rows.length===200;n++) scrollingTest.wait(10);
            check(library.panes[2].rows.length>200 && flickList.flicking,"fetch preserves kinetic scrolling");
            const movingY=flickList.contentY;scrollingTest.wait(30);
            check(flickList.contentY>movingY,"kinetic scrolling continues in the same direction");
            flickList.cancelFlick();wait();
            down(2);
            const oldStart=library.panes[2].rows[0].id;
            flickList.positionViewAtIndex(580,ListView.Beginning);flickList.flick(0,-2000);
            for(let n=0;n<50 && library.panes[2].rows[0].id===oldStart;n++) scrollingTest.wait(10);
            check(library.panes[2].rows[0].id!==oldStart && flickList.flicking,"kinetic scrolling survives window eviction");
            flickList.cancelFlick();wait();bounded(2);
            check(library.panes[2].rows.every((r,i,a)=>i===0 || a[i-1].title>r.title),"reverse order across fetch");
            view("Artists");action("select",0,library.panes[0].rows[0].id);
            action("sort",2);action("sort",2);
            check(library.panes[2].sort==="Album","album ordering");
            down(2);down(2);down(2);bounded(2);
            rows=library.panes[2].rows;
            check(rows.some((r,i)=>i>0 && r.albumId!==rows[i-1].albumId),"loaded window crosses canonical album boundary");
            check(new Set(rows.map(r=>r.albumId)).size===2,"same-title albums remain distinct groups");
            function visibleHeaders() {
                const found=[];
                function scan(item) { for(const c of item.children||[]) { if(c.objectName==="songAlbumHeader" && c.visible && c.height>0) found.push(c.section);scan(c); } }
                scan(list(2).contentItem); return found;
            }
            const songs=list(2);
            songsPane.adjustingWindow=true;
            songs.positionViewAtBeginning();songs.forceLayout();wait();
            check(visibleHeaders().filter(id=>id===rows[0].albumId).length===1,"one continuation header at materialized window start");
            const boundary=rows.findIndex(r=>r.albumId!==rows[0].albumId);
            songs.positionViewAtIndex(boundary,ListView.Beginning);songs.forceLayout();wait();
            check(visibleHeaders().filter(id=>id===rows[boundary].albumId).length===1,"one canonical header at boundary across fetched chunks");
            scrollingTest.grabImage(window.contentItem).save("/tmp/library-continuous-headers.png");
            songsPane.adjustingWindow=false;
            action("select",1,library.panes[1].rows[0].id);
            check(library.panes[2].rows.length===200 && !library.panes[2].before,"filter resets window and drops stale chunks");
            view("Playlists");
            check(library.panes[0].rows.length===200,"playlist chunk");down(0);check(library.panes[0].rows.length===205,"playlists load automatically");
            const playlist=library.panes[0].rows.find(r=>r.title==="Playlist 000").id;
            action("select",0,playlist);done();
            let entries=library.panes[2].rows;const duplicate=entries[0].id;
            action("select-toggle",2,duplicate);
            const entryIds=new Set(entries.map(r=>r.id));
            for(let n=0;n<5;n++) {down(2);entries=library.panes[2].rows;entries.forEach(r=>entryIds.add(r.id));bounded(2);}
            check(entryIds.size===1003 && entries.every(r=>r.track.trackId===first),"duplicates preserve distinct entry identity and exact order size="+entryIds.size+" start="+entries[0].number+" end="+entries[entries.length-1].number+" same="+entries.every(r=>r.track.trackId===first));
            check(entries[0].number === "404" && entries[entries.length-1].number === "1003", "absolute numbering across discarded chunks");
            check(entries.every((r,i)=>Number(r.number)===404+i), "deep duplicate entries retain sequential positions");
            check(library.panes[2].selectionCount===1,"offscreen duplicate selection preserved");
            const entry=entries[20].id;
            action("select-toggle",2,entry);check(library.panes[2].selectionCount===2,"duplicate selections remain distinct");
            action("append",2,"");done();check(window.view.queueTotal===3,"both selected duplicate entries expanded");
            action("play",2,entry);done();check(window.view.position===423,"playlist playback starts exact duplicate entry");
            scrollingTest.grabImage(window.contentItem).save("/tmp/library-continuous-playlist.png");
            // A mixed playlist in reverse title order proves global, rather than
            // QML-window sorting; the final occurrence duplicates Song 0500.
            action("select",0,library.panes[0].rows.find(r=>r.title==="Playlist 002").id);done();
            function whole() {
                const collected=[];const seenIds=new Set();
                for(let n=0;n<8;n++) {
                    for(const r of library.panes[2].rows) if(!seenIds.has(r.id)) {seenIds.add(r.id);collected.push(r);}
                    bounded(2);if(!library.panes[2].more) break;
                    down(2);done();
                }
                return collected;
            }
            const canonical=whole();check(canonical.length===1004,"mixed canonical entries complete");
            function sortKey(r,key) {
                if(key==="position") return Number(r.number);
                if(key==="length") {const n=Number(r.title.substr(5));return n%17===0?-1:n*1000;}
                return (key==="title"?r.title:key==="artist"?r.subtitle:r.track.release).toLowerCase();
            }
            for(const column of [["Title","title"],["Artist","artist"],["Album","album"],["Length","length"],["#","position"]]) {
                for(const descending of [false,true]) {
                    scrollingTest.findChild(songsPane,"playlistHeader"+column[0]).clicked();done();
                    check(library.playlistSort===column[1] && library.playlistDescending===descending,"global header state");
                    check(library.panes[2].rows.length===200 && !library.panes[2].before,"sort resets bounded window to beginning");
                    const displayed=whole();
                    const expected=canonical.slice().sort((a,b)=>{const ka=sortKey(a,column[1]),kb=sortKey(b,column[1]);const comparison=ka<kb?-1:ka>kb?1:Number(a.number)-Number(b.number);return descending?-comparison:comparison;});
                    check(displayed.map(r=>r.id).join()===expected.map(r=>r.id).join(),"global deterministic "+column[1]+" "+descending);
                    check(displayed.every(r=>canonical[Number(r.number)-1].id===r.id),"absolute canonical # survives sorted chunk eviction");
                    check(displayed.filter(r=>r.title==="Song 0500").length===2,"duplicate occurrences preserved under every sort");
                    const chosen=displayed.find(r=>r.number==="1004");
                    action("select",2,chosen.id);action("context",2,chosen.id);
                    check(library.panes[2].selectionCount===1 && library.song===chosen.id,"exact duplicate selected offscreen");
                    if(column[1]==="title" && !descending) {action("play",2,chosen.id);done();check(window.view.position===1003,"sorted playback retains exact canonical duplicate position");}
                }
            }
            scrollingTest.findChild(songsPane,"playlistHeader#").clicked();done();
            check(library.playlistReorderAllowed && library.panes[2].rows[0].id===canonical[0].id,"restore canonical order and manual reorder after global sorts");
            scrollingTest.findChild(songsPane,"playlistHeaderTitle").clicked();done();
            const rangeAnchor=library.panes[2].rows[0].id;
            action("select",2,rangeAnchor);down(2);done();down(2);done();down(2);done();
            const rangeEnd=library.panes[2].rows[20].id;
            action("select-range",2,rangeEnd);done();
            const titleOrder=canonical.slice().sort((a,b)=>a.title<b.title?-1:a.title>b.title?1:Number(a.number)-Number(b.number));
            check(library.panes[2].selectionCount===titleOrder.findIndex(r=>r.id===rangeEnd)+1,"sorted Shift range crosses evicted chunks");
            scrollingTest.findChild(songsPane,"playlistHeader#").clicked();done();
            view("Songs");down(2);scrollingTest.grabImage(window.contentItem).save("/tmp/library-continuous-songs.png");
            // Main Songs headers sort the full logical library, retaining
            // stable Track identities and the same bounded viewport protocol.
            action("sort",2); // Restore alphabetical ascending after earlier legacy tests.
            for(const column of [["Artist","artist"],["Album","album"],["Genre","genre"],["Song","song"]]) {
                for(const descending of [false,true]) {
                    scrollingTest.findChild(songsPane,"songsHeader"+column[0]).clicked();done();
                    check(library.songsColumn===column[1] && library.songsDescending===descending,"Songs header direction");
                    check(library.panes[2].rows.length===200 && !library.panes[2].before,"Songs sort resets window");
                    const displayed=whole();check(displayed.length===1003,"Songs global sort exhausts all logical tracks");
                    function key(r) {return (column[1]==="song"?r.title:column[1]==="artist"?r.subtitle:column[1]==="album"?r.track.release:r.genres).toLowerCase();}
                    check(displayed.every((r,i)=>i===0 || (descending?key(displayed[i-1])>key(r) || key(displayed[i-1])===key(r) && displayed[i-1].id>r.id:key(displayed[i-1])<key(r) || key(displayed[i-1])===key(r) && displayed[i-1].id<r.id)),"Songs globally deterministic ordering");
                }
            }
            scrollingTest.findChild(songsPane,"songsHeaderSong").clicked();done();
            const mainSongs=list(2);mainSongs.forceLayout();
            const firstRow=mainSongs.itemAtIndex(0);
            for(const cell of ["songTitle","songsArtist","songsAlbum","songsGenre"]) check(scrollingTest.findChild(firstRow,cell).visible,"four Songs cells "+cell);
            check(firstRow.height===30 && !songsPane.albumSongs,"compact flat Songs table");
            check(!scrollingTest.findChild(songsPane,"paneSortControl").visible,"redundant Songs A-Z control hidden");
            songsPane.selectRow(0,Qt.NoModifier);wait();songsPane.selectRow(2,Qt.ControlModifier);wait();
            check(library.panes[2].selectionCount===2,"table Ctrl selection");
            const selected=library.panes[2].rows[2].id;
            scrollingTest.mouseClick(mainSongs.itemAtIndex(2),mainSongs.width-50,15,Qt.RightButton,Qt.NoModifier);wait();
            check(window.contextId===selected && library.panes[2].selectionCount===2,"any table cell right-click targets exact track and keeps selection");
            libraryMenu.close();
            songsPane.selectRow(0,Qt.NoModifier);wait();down(2);done();down(2);done();down(2);done();
            const mainEndpoint=library.panes[2].rows[20];
            action("select-range",2,mainEndpoint.id);done();
            check(library.panes[2].selectionCount===Number(mainEndpoint.title.substr(5))+1,"Songs Shift selection follows displayed order across chunks");
            scrollingTest.grabImage(window.contentItem).save("/tmp/songs-details-table.png");
            view("Artists");check(!songsPane.detailsTable,"embedded Artist Songs stays a normal list");
            view("Genres");check(!songsPane.detailsTable,"embedded Genre Songs stays a normal list");
            view("Albums");check(!songsPane.detailsTable,"embedded Album Songs stays a normal list");
            view("Songs");
            return "ok";
        } catch(e) { return String(e); }
    }

    function cacheSongsBeforeRefresh() {
        bridge.browse_action("view",0,"Songs");
        const id=library.panes[2].rows[0].id;
        bridge.browse_action("view",0,"Artists");
        return id;
    }
    function exerciseInactiveRefresh(removed) {
        bridge.browse_action("view",0,"Songs");
        scrollingTest.wait(100);
        return !library.error && library.panes[2].rows.every(r=>r.id!==removed) ? "ok" : "stale inactive window after library edit";
    }
