    TestCase {id:connectionsTest;when:false}
    function exerciseConnections(rootUrl) {
        function check(v,m){if(!v)throw new Error(m);}
        function wait(){connectionsTest.wait(40);}
        try {
            check(applicationSection==="Music" && library.view==="Artists","default Music Artists");
            for(const name of ["Artists","Genres","Albums","Songs","Playlists"]){const b=connectionsTest.findChild(window.contentItem,"libraryView"+name);check(b&&b.visible,"Music page "+name);b.clicked();wait();check(library.view===name,"Music navigation "+name);}
            bridge.browse_action("view",0,"Artists");wait();
            connectionsTest.findChild(window.contentItem,"sectionConnections").clicked();wait();
            check(applicationSection==="Connections"&&!librarySplit.visible,"top-level switch");
            check(connectionsPage.providers.map(p=>p.key).join(",")==="local,musicbrainz,spotify","providers");
            connectionsPage.act("add-root",rootUrl);wait();check(connectionsPage.roots.length===1,"authoritative root action");
            check(connectionGroups.count===1,"local missing group");
            check(connectionGroups.get(0).matched===43&&connectionGroups.get(0).total===45&&connectionGroups.get(0).relevant===2,"local matched total");
            for(const provider of ["local","musicbrainz","spotify"]){
                connectionsTest.findChild(window.contentItem,"connectionProvider"+provider).clicked();wait();check(connectionsPage.provider===provider,"provider switch");
                if(provider==="spotify"){check(connectionsTest.findChild(connectionsPage,"connectionAuthorization").text==="Authorization: "+spotifyPlayback.status,"existing authorization state");check(connectionsTest.findChild(connectionsPage,"connectionDevice")!==null,"existing device selector");}
                connectionList.forceLayout();wait();
                const group=connectionList.itemAtIndex(0);check(group!==null,"Album rendered");
                const before=connectionGroups.get(0).relevant;
                group.expanded=true;group.refreshTracks();wait();check(group.tracks.length===before,"only unresolved expanded");
                const track=group.tracks[0].id;
                const match=connectionsTest.findChild(group,"connectionAlbumMatch");check(match!==null,"Album Match");
                connectionsTest.mouseMove(match,match.width/2,match.height/2);wait();check(group.highlighted,"hover over action");
                const mark=connectionsTest.findChild(group,"connectionAlbumMark");connectionsTest.mouseMove(mark,mark.width/2,mark.height/2);wait();check(group.highlighted,"hover over unavailable action");mark.clicked();wait();check(connectionMarkDialog.visible&&connectionMarkDialog.count===before,"Album marking confirms current unresolved snapshot");connectionMarkDialog.close();
                connectionsPage.act("mark-track",track);wait();check(connectionGroups.get(0).relevant===before-1,"immediate mark");check(connectionList.itemAtIndex(0).expanded,"expanded state retained during targeted refresh");
                connectionsTest.findChild(window.contentItem,"markedUnavailable").clicked();wait();check(connectionGroups.count===1&&connectionGroups.get(0).relevant===1,"marked tab");
                if(provider==="spotify"){connectionsPage.matchTrack(connectionGroups.get(0).albumId,track);wait();connectionReviewDialog.manualMatch();wait();check(connectionsTest.findChild(connectionReviewDialog,"reviewManualSearch").enabled&&!spotifyPlaybackDialog.visible,"inline marked Track manual search");connectionReviewDialog.close();}
                connectionsPage.act("clear",track);wait();check(connectionGroups.count===0,"immediate clear");
                connectionsTest.findChild(window.contentItem,"missedConnections").clicked();wait();check(connectionGroups.get(0).relevant===before,"restored unresolved");
                const album=connectionGroups.get(0).albumId;
                connectionsPage.matchAlbum(album);wait();
                check(connectionReviewDialog.visible,"direct shared Album review");
                check(!spotifyAlbumDialog.visible&&!connectionAlbumDialog.visible,"no intermediate chooser");
                connectionReviewDialog.manualMatch();wait();
                check(connectionReviewDialog.manualMode&&!spotifyAlbumDialog.visible&&!spotifyAdvancedMapping.visible&&!connectionAlbumDialog.visible&&!musicbrainzConnection.visible,"Album manual selection remains inline");
                connectionReviewDialog.close();wait();
                connectionsPage.matchTrack(album,track);wait();check(connectionReviewDialog.visible,"direct shared Track review");
                if(provider!=="local"){
                    connectionReviewDialog.manualMatch();wait();
                    check(connectionReviewDialog.manualMode&&!spotifyPlaybackDialog.visible&&!musicbrainzConnection.visible,"Track manual selection remains inline");
                }
                connectionReviewDialog.close();wait();
            }
            check(!connectionsPage.headerInfo.error,"persisted provider header");
            const scroll=connectionsTest.findChild(connectionList,"connectionsScrollbar");check(scroll&&scroll.interactive,"draggable scrollbar");
            connectionsTest.findChild(window.contentItem,"sectionMusic").clicked();wait();check(applicationSection==="Music"&&library.view==="Artists","Music restored");
            connectionsTest.findChild(window.contentItem,"sectionConnections").clicked();wait();check(connectionsPage.provider==="spotify","provider preserved across sections");
            return "ok";
        }catch(e){return String(e)+" "+e.stack;}
    }
    function exerciseConnectionsLarge() {
        try {
            window.showConnections("spotify");connectionsTest.wait(50);
            for(let n=0;n<20;n++) {
                connectionList.positionViewAtEnd();connectionList.forceLayout();
                connectionsPage.nextWindow();connectionsTest.wait(10);
                if(connectionGroups.count>300)throw new Error("unbounded model");
                const ids=[];for(let i=0;i<connectionGroups.count;i++)ids.push(connectionGroups.get(i).albumId);
                if(new Set(ids).size!==ids.length)throw new Error("duplicate Album cursor");
            }
            for(let n=0;n<5;n++){connectionsPage.previousWindow();connectionsTest.wait(10);if(connectionGroups.count>300)throw new Error("unbounded reverse model");}
            const scroll=connectionsTest.findChild(connectionList,"connectionsScrollbar");
            if(!scroll.visible||!scroll.interactive||scroll.size>=1)throw new Error("visible scrollbar");
            const old=connectionList.contentY;
            const thumb=(scroll.position+scroll.size/2)*scroll.height;
            connectionsTest.mousePress(scroll,scroll.width/2,thumb);
            connectionsTest.mouseMove(scroll,scroll.width/2,Math.min(scroll.height-4,thumb+100),30);
            connectionsTest.mouseRelease(scroll,scroll.width/2,Math.min(scroll.height-4,thumb+100));connectionsTest.wait(30);
            if(connectionList.contentY===old)throw new Error("scrollbar drag did not move");
            // Review deep in the bounded window, with two Albums expanded.
            const selected=Math.floor(connectionGroups.count/2);
            const album=connectionGroups.get(selected).albumId;
            const neighbor=connectionGroups.get(selected+1).albumId;
            connectionsPage.expanded["spotifyfalse"+album]=true;
            connectionsPage.expanded["spotifyfalse"+neighbor]=true;
            connectionList.positionViewAtIndex(selected,ListView.Center);connectionList.forceLayout();connectionsTest.wait(30);
            const selectedRow=connectionList.itemAtIndex(selected);if(selectedRow)selectedRow.expanded=true;
            const neighborRow=connectionList.itemAtIndex(selected+1);if(neighborRow)neighborRow.expanded=true;
            connectionList.forceLayout();connectionsTest.wait(30);
            const before=connectionList.contentY;
            connectionsPage.matchAlbum(album);connectionsTest.wait(30);
            if(!connectionReviewDialog.visible)throw new Error("deep Album review");
            connectionReviewDialog.action("mark",0);connectionsTest.wait(80);connectionList.forceLayout();
            if(connectionReviewDialog.visible)throw new Error("mark did not close review");
            if(!connectionsPage.expanded["spotifyfalse"+neighbor])throw new Error("unrelated expansion lost");
            if(Math.abs(connectionList.contentY-before)>250)throw new Error("review jumped away from surrounding position");
            if(connectionsPage.provider!=="spotify"||connectionsPage.marked)throw new Error("review changed provider or lower tab");
            if(connectionGroups.count>300)throw new Error("review grew bounded model");
            if(connectionReviewDialog.visible||spotifyAlbumDialog.visible)throw new Error("automatic next item");
            return "ok";
        }catch(e){return String(e)+" "+e.stack;}
    }
    function liveConnectionGap(album,track) {
        try {
            window.showConnections("spotify");connectionsTest.wait(30);
            let row=null;for(let i=0;i<connectionGroups.count;i++)if(connectionGroups.get(i).albumId===album)row=connectionGroups.get(i);
            if(!row||row.relevant!==1)throw new Error("real gap not shown");
            connectionsPage.act("mark-track",track);
            connectionsPage.marked=true;connectionsPage.reload(true);connectionsTest.wait(30);
            const marked=connectionsPage.readConnection("tracks",album);
            if(!marked.some(t=>t.id===track))throw new Error("real unavailable Track not shown");
            window.openSpotifyConnection(track);connectionsTest.wait(30);
            return "ok";
        }catch(e){return String(e);}
    }
    function liveConnectionRestored(album,track) {
        connectionsTest.wait(50);
        if(connectionsPage.readConnection("tracks",album).some(t=>t.id===track))return "marked Track was not cleared";
        connectionsPage.marked=false;connectionsPage.reload(true);
        for(let i=0;i<connectionGroups.count;i++)if(connectionGroups.get(i).albumId===album)return "resolved Album remains";
        return "ok";
    }
