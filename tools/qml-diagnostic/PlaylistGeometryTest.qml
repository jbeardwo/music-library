    TestCase { id: geometryTest; when: false }
    property bool geometryWatching: false
    property var geometryBaseline: ({})
    property var geometryChanges: []
    function playlistGeometry() {
        const button = geometryTest.findChild(albumsPane, "playlistAddTracks");
        const details = geometryTest.findChild(albumsPane, "playlistDetails");
        const busy = geometryTest.findChild(albumsPane,"playlistDetailsBusy");
        const p = button.mapToItem(window.contentItem,0,0);
        return {y:p.y, localY:button.y, height:button.height, implicitHeight:button.implicitHeight,
            parentY:button.parent.y,parentHeight:button.parent.height,parentImplicitHeight:button.parent.implicitHeight,
            detailsY:details.y,detailsHeight:details.height,detailsImplicitHeight:details.implicitHeight,
            margin:details.Layout.margins,buttonVisible:button.visible,parentVisible:button.parent.visible,
            busyVisible:busy ? busy.visible : false,busyHeight:busy ? busy.height : 0,
            pending:library.playlistDetails.pending,loading:library.pending,rows:library.panes[2].rows.length};
    }
    Connections {
        target: geometryTest.findChild(albumsPane,"playlistAddTracks")
        function onYChanged() {
            if(!window.geometryWatching) return;
            const current=window.playlistGeometry();
            if(Math.abs(current.y-window.geometryBaseline.y)>0.1) {
                window.geometryChanges.push(current);
                console.log("PLAYLIST GEOMETRY CHANGE",JSON.stringify(current),"BASELINE",JSON.stringify(window.geometryBaseline));
            }
        }
    }
    function auditPlaylistGeometry(milliseconds, playing) {
        try {
            for(let n=0;n<500 && (library.playlistDetails.pending || !library.panes[2].rows.length);n++) geometryTest.wait(20);
            geometryTest.wait(100);
            if(playing) {
                bridge.browse_action("play",2,library.panes[2].rows[0].id);
                for(let n=0;n<500 && library.pending;n++) geometryTest.wait(20);
                geometryTest.wait(100);
                if(view.status!=="Playing") return "playback did not start: "+view.status+" "+view.error;
            }
            geometryBaseline=playlistGeometry();geometryChanges=[];geometryWatching=true;
            let sawBusy=false,sawReady=false;
            for(let n=0;n<milliseconds/20;n++) {
                // A same-name rename refreshes the real aggregate without changing metadata or order.
                if(n%100===0) bridge.browse_action("playlist-rename",0,"Geometry audit");
                if(playing && n%12===0) bridge.refresh_clock();
                geometryTest.wait(20);
                const current=playlistGeometry();
                sawBusy=sawBusy || current.pending; sawReady=sawReady || !current.pending;

                if(current.y!==geometryBaseline.y || current.height!==geometryBaseline.height || current.detailsY!==geometryBaseline.detailsY || current.detailsHeight!==geometryBaseline.detailsHeight)
                    geometryChanges.push(current);
            }
            geometryWatching=false;
            geometryTest.grabImage(window.contentItem).save(playing?"/tmp/playlist-geometry-playing.png":"/tmp/playlist-geometry-idle.png");
            if(!sawBusy || !sawReady) return "loading transitions were not exercised";
            console.log("PLAYLIST GEOMETRY STABLE",playing?"playing":"idle",milliseconds,"ms",JSON.stringify(geometryBaseline));
            if(geometryChanges.length) return "movement: "+JSON.stringify(geometryChanges[0]);
            return "ok";
        } catch(e) { geometryWatching=false; return String(e); }
    }
