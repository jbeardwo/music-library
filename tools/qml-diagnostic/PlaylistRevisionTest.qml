    TestCase {id: revisionTest;when:false}
    property string revisionPlaylist: ""
    property string revisionEntry: ""
    property var revisionWidths: []
    function revisionWait() {for(let n=0;n<500 && (library.pending || library.playlistDetails.pending);n++) revisionTest.wait(10);revisionTest.wait(100);}
    function revisionOpen(id) {
        revisionPlaylist=id;bridge.browse_action("view",0,"Playlists");bridge.browse_action("select",0,id);revisionWait();
        revisionEntry=library.panes[2].rows[40].id;
        bridge.browse_action("select",2,revisionEntry);
        const list=revisionTest.findChild(songsPane,"libraryPane2");list.positionViewAtIndex(40,ListView.Beginning);revisionTest.wait(100);
        songsPane.resizeColumns(1,songsPane.tableColumns.slice(),50);revisionWidths=window.playlistColumnWidths.slice();
        bridge.browse_action("view",0,"Songs");revisionWait();return "ok";
    }
    function revisionAddFromSongs(song) {
        bridge.browse_action("picker-open",2,song);bridge.browse_action("picker-add",2,revisionPlaylist);revisionWait();return "ok";
    }
    function revisionReturn(count) {
        bridge.browse_action("view",0,"Playlists");
        for(let n=0;n<500 && library.panes[2].rows.length!==count;n++) revisionTest.wait(10);revisionWait();
        if(library.artist!==revisionPlaylist) return "selected playlist lost";
        if(library.panes[2].rows.length!==count) return "stale contents: "+library.panes[2].rows.length+" expected "+count;
        if(count>0 && library.song!==revisionEntry) return "entry selection lost";
        if(count===0 && library.song) return "deleted entry still selected";
        if(JSON.stringify(window.playlistColumnWidths)!==JSON.stringify(revisionWidths)) return "widths lost";
        const list=revisionTest.findChild(songsPane,"libraryPane2");
        const item=list.itemAtIndex(library.panes[2].rows.findIndex(r=>r.id===revisionEntry));
        if(count>0 && (!item || item.mapToItem(list,0,0).y< -1 || item.mapToItem(list,0,0).y>list.height)) return "viewport anchor lost";
        return "ok";
    }
    function revisionHide() {bridge.browse_action("view",0,"Songs");revisionWait();return "ok";}
    function revisionNoReload() {
        const list=revisionTest.findChild(songsPane,"libraryPane2"),model=list.model,y=list.contentY,epoch=library.panes[2].epoch;
        bridge.browse_action("view",0,"Playlists");revisionWait();
        return list.model===model && list.contentY===y && library.panes[2].epoch===epoch ? "ok" : "unchanged playlist reloaded";
    }
