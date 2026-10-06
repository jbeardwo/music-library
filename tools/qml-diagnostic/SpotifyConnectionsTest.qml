    TestCase { id: reviewTest; when: false }
    function reviewWait() { reviewTest.wait(80); }
    function reviewCheck(v,m) { if(!v) throw new Error(m); }
    function reviewOpen() {
        try {
            const entry=reviewTest.findChild(settingsMenu,"spotifyConnectionsReview");
            reviewCheck(entry && entry.text==="Spotify Connections","management entry point");entry.triggered();reviewWait();
            reviewCheck(library.view==="Spotify Connections","review active");
            reviewCheck(!artistsPane.visible && !albumsPane.visible && songsPane.detailsTable,"shared management table");
            reviewCheck(library.panes[2].rows.length===200,"bounded page");
            const list=reviewTest.findChild(songsPane,"libraryPane2");list.forceLayout();
            const row=list.itemAtIndex(0),id=library.panes[2].rows[0].id;
            const before=JSON.stringify([view.currentId,view.queue,view.status]);
            reviewTest.mouseDoubleClickSequence(row,30,15,Qt.LeftButton);reviewWait();
            reviewCheck(spotifyPlayback.trackId===id,"double click canonical diagnostic");
            reviewCheck(JSON.stringify([view.currentId,view.queue,view.status])===before,"current playback unaffected");spotifyPlaybackDialog.close();reviewWait();
            reviewTest.mouseClick(row,30,15,Qt.RightButton);reviewWait();
            reviewTest.findChild(libraryMenu,"songSpotifyConnection").triggered();libraryMenu.close();reviewWait();
            reviewCheck(spotifyPlayback.trackId===id,"context canonical diagnostic");spotifyPlaybackDialog.close();reviewWait();
            reviewTest.findChild(songsPane,"songsHeaderReason").clicked();reviewWait();
            reviewCheck(library.songsColumn==="reason","reason sorting");
            reviewTest.findChild(songsPane,"songsHeaderSong").clicked();reviewWait();
            window.reviewColumnWidths=[260,160,220,300];reviewWait();
            list.contentY=450;reviewWait();
            return "ok";
        } catch(e){ return String(e); }
    }
    property var reviewAnchor: null
    property string reviewSort: ""
    property string reviewFilter: ""
    property string reviewWidths: ""
    property string reviewQueue: ""
    function reviewSnapshot() {
        reviewAnchor=songsPane.viewportAnchor();
        reviewSort=library.songsColumn+library.songsDescending;
        reviewFilter=library.reviewAlbum;
        reviewWidths=JSON.stringify(window.reviewColumnWidths);
        reviewQueue=JSON.stringify([view.currentId,view.queue,view.status]);
        return reviewAnchor ? reviewAnchor.id : "";
    }
    function reviewAfter(removed,count) {
        try {
            for(let n=0;n<100 && library.unresolvedCount!==String(count);n++) reviewTest.wait(10);
            reviewWait();
            reviewCheck(!library.panes[2].rows.some(r=>removed.indexOf(r.id)>=0),"resolved rows removed");
            reviewCheck(library.unresolvedCount===String(count),"updated count "+library.unresolvedCount);
            reviewCheck(reviewSort===library.songsColumn+library.songsDescending,"sort retained");
            reviewCheck(reviewFilter===library.reviewAlbum,"filter retained");
            reviewCheck(reviewWidths===JSON.stringify(window.reviewColumnWidths),"widths retained");
            reviewCheck(reviewQueue===JSON.stringify([view.currentId,view.queue,view.status]),"queue retained");
            const anchor=songsPane.viewportAnchor();
            reviewCheck(anchor && anchor.id===reviewAnchor.id && Math.abs(anchor.pixel-reviewAnchor.pixel)<1,"viewport retained");
            return "ok";
        } catch(e){ return String(e); }
    }
    function reviewFilterAlbum(id) { bridge.browse_action("review-album",2,id);reviewWait();reviewTest.findChild(songsPane,"libraryPane2").contentY=450;reviewWait();return library.reviewAlbum===id; }
    function reviewEmpty() {
        bridge.browse_action("review-album",2,"");reviewWait();
        const label=reviewTest.findChild(songsPane,"libraryEmptyState2");
        return library.panes[2].rows.length===0 && label && label.text==="All eligible Library Tracks are connected or reviewed.";
    }
    function reviewLiveOpen(track,album) {
        try {
            bridge.browse_action("view",2,"Spotify Connections");
            bridge.browse_action("review-album",2,album);reviewWait();
            const index=library.panes[2].rows.findIndex(r=>r.id===track);
            reviewCheck(index>=0,"real unresolved Track appears");
            const list=reviewTest.findChild(songsPane,"libraryPane2");list.positionViewAtIndex(index,ListView.Contain);list.forceLayout();reviewWait();
            const row=list.itemAtIndex(index);reviewCheck(row!==null,"real row materialized");
            reviewTest.mouseDoubleClickSequence(row,30,15,Qt.LeftButton);reviewWait();
            reviewCheck(spotifyPlayback.trackId===track,"real review diagnostic canonical Track");
            return "ok";
        } catch(e){return String(e);}
    }
    function reviewLiveRetry(track,connected,expected) {
        try {
            const before=JSON.stringify([view.currentId,view.queue,view.status]);
            reviewTest.findChild(spotifyPlaybackDialog,"spotifyAlbumReevaluate").clicked();
            for(let n=0;n<1500 && spotifyPlayback.albumPending;n++) reviewTest.wait(20);
            reviewWait();
            reviewCheck(!spotifyPlayback.albumPending,"bounded live retry completed");
            reviewCheck(spotifyPlayback.trackId===track && spotifyPlayback.available===connected,"conservative live decision "+spotifyPlayback.albumExplanation);
            reviewCheck(spotifyPlayback.albumExplanation.indexOf(expected)>=0,"structured evidence "+spotifyPlayback.albumExplanation);
            reviewCheck(JSON.stringify([view.currentId,view.queue,view.status])===before,"live queue unchanged");
            reviewCheck(library.panes[2].rows.some(r=>r.id===track)!==connected,"review row follows association");
            spotifyPlaybackDialog.close();reviewWait();return "ok";
        } catch(e){return String(e);}
    }
    function reviewLiveSearch() {
        reviewTest.findChild(spotifyPlaybackDialog,"spotifyConnectionSearch").clicked();
        for(let n=0;n<1500 && spotifyPlayback.resolutionPending;n++) reviewTest.wait(20);
        return !spotifyPlayback.resolutionPending;
    }
    function reviewLiveConfirm(index,track) {
        try {
            const before=JSON.stringify([view.currentId,view.queue,view.status]);
            bridge.spotify_resolve("confirm",index);reviewWait();
            reviewCheck(spotifyPlayback.available,"live manual confirmation persisted "+spotifyPlayback.resolutionMessage);
            reviewCheck(!library.panes[2].rows.some(r=>r.id===track),"live manual row disappears");
            reviewCheck(before===JSON.stringify([view.currentId,view.queue,view.status]),"live manual queue unchanged");
            spotifyPlaybackDialog.close();return "ok";
        }catch(e){return String(e);}
    }
    function reviewLarge() {
        try {
            const list=reviewTest.findChild(songsPane,"libraryPane2");
            for(let n=0;n<20;n++) {
                bridge.browse_action("scroll-forward",2,"");reviewWait();
                reviewCheck(library.panes[2].rows.length<=600,"bounded 200k review window");
                reviewCheck(new Set(library.panes[2].rows.map(r=>r.id)).size===library.panes[2].rows.length,"unique canonical IDs");
            }
            reviewTest.findChild(songsPane,"songsHeaderReason").clicked();reviewWait();
            for(let n=0;n<20;n++) { bridge.browse_action("scroll-forward",2,"");reviewWait();reviewCheck(library.panes[2].rows.length<=600,"bounded reason scroll"); }
            reviewCheck(list.count<=600,"QML materialization bounded");
            return "ok";
        }catch(e){return String(e);}
    }

    function reviewLifecycle(track) {
        try {
            reviewSnapshot();
            bridge.browse_action("spotify-mark",2,track);reviewWait();
            reviewCheck(reviewAfter(track,449)==="ok","mark preserves review context");
            bridge.browse_action("review-mode",2,"marked");reviewWait();
            reviewCheck(library.reviewMarked && library.panes[2].rows.length===1 && library.panes[2].rows[0].id===track,"marked view canonical Track");
            for(let n=0;n<100 && library.markedCount!=="1";n++) reviewTest.wait(10);
            reviewCheck(library.markedCount==="1","marked count");
            window.openSpotifyConnection(track);reviewWait();
            reviewCheck(spotifyPlayback.manuallyExcluded,"manual decision visible");
            reviewCheck(!reviewTest.findChild(spotifyPlaybackDialog,"spotifyConnectionSearch").enabled,"marked search disabled");
            reviewTest.findChild(spotifyPlaybackDialog,"diagnosticSpotifyCheck").clicked();reviewWait();
            reviewCheck(!spotifyPlayback.manuallyExcluded,"diagnostic clears mark");
            reviewCheck(library.panes[2].rows.length===0,"marked row immediately removed");
            spotifyPlaybackDialog.close();
            bridge.browse_action("review-mode",2,"unresolved");reviewWait();
            reviewCheck(library.panes[2].rows.some(r=>r.id===track),"cleared row eligible again");
            reviewCheck(reviewWidths===JSON.stringify(window.reviewColumnWidths) && reviewSort===library.songsColumn+library.songsDescending,"mode keeps widths and sort");
            reviewCheck(reviewQueue===JSON.stringify([view.currentId,view.queue,view.status]),"mark/check queue unchanged");
            reviewTest.findChild(songsPane,"libraryPane2").contentY=450;reviewWait();
            return "ok";
        }catch(e){return String(e);}
    }

    function reviewIdle() { for(let n=0;n<500 && library.reviewLocalActive;n++) reviewTest.wait(10); return !library.reviewLocalActive; }
    function reviewBulkMark() {
        try {
            const rows=library.panes[2].rows.slice(0,3);
            const state=JSON.stringify([window.reviewColumnWidths,library.songsColumn,library.songsDescending,view.currentId,view.queue,view.status]);
            bridge.browse_action("select",2,rows[0].id);
            bridge.browse_action("select-toggle",2,rows[1].id);
            bridge.browse_action("select-toggle",2,rows[2].id);reviewWait();
            reviewCheck(library.panes[2].selectionCount===3,"three selected Tracks");
            window.openSongContext(2,rows[1]);reviewWait();
            reviewTest.findChild(libraryMenu,"spotifyMarkUnavailable").triggered();libraryMenu.close();reviewWait();
            reviewCheck(rows.every(r=>!library.panes[2].rows.some(t=>t.id===r.id)),"all selected rows removed");
            bridge.browse_action("review-mode",2,"marked");reviewWait();
            reviewCheck(rows.every(r=>library.panes[2].rows.some(t=>t.id===r.id)),"all selected Tracks in marked view");
            bridge.browse_action("review-mode",2,"unresolved");reviewWait();
            const remaining=library.panes[2].rows.slice(0,3);
            bridge.browse_action("select",2,remaining[0].id);
            bridge.browse_action("select-toggle",2,remaining[1].id);
            window.openSongContext(2,remaining[2]);reviewWait();
            reviewTest.findChild(libraryMenu,"spotifyMarkUnavailable").triggered();libraryMenu.close();reviewWait();
            reviewCheck(library.panes[2].rows.some(r=>r.id===remaining[0].id) && library.panes[2].rows.some(r=>r.id===remaining[1].id),"outside-selection click preserves other Tracks");
            reviewCheck(!library.panes[2].rows.some(r=>r.id===remaining[2].id),"outside-selection click marks only clicked Track");
            reviewCheck(state===JSON.stringify([window.reviewColumnWidths,library.songsColumn,library.songsDescending,view.currentId,view.queue,view.status]),"bulk action preserves table and playback state");
            return "ok";
        } catch(e) {return String(e);}
    }
