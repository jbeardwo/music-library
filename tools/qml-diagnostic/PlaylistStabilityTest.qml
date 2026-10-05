    TestCase { id: stabilityTest; when: false }
    property var stabilityBaseline: ({})
    property var stabilityDelegate: null
    function preparePlaylistStability() {
        try {
            bridge.browse_action("view",0,"Playlists");
            bridge.browse_action("select",0,library.panes[0].rows[0].id);
            bridge.browse_action("playlist-sort",2,"title");
            stabilityTest.wait(300);
            const list=stabilityTest.findChild(songsPane,"libraryPane2");
            const entry=library.panes[2].rows[80].id;
            bridge.browse_action("select",2,entry);
            list.positionViewAtIndex(80,ListView.Beginning);
            stabilityTest.wait(100);
            songsPane.resizeColumns(1,songsPane.tableColumns.slice(),50);
            stabilityDelegate=list.itemAtIndex(80);
            stabilityBaseline={id:entry,y:list.contentY,model:list.model,sort:library.playlistSort,
                widths:JSON.stringify(window.playlistColumnWidths),epoch:library.panes[2].epoch};
            return "ok";
        } catch(e) {return String(e);}
    }
    function checkPlaylistStability() {
        try {
            stabilityTest.wait(400);
            const list=stabilityTest.findChild(songsPane,"libraryPane2");
            if(list.model!==stabilityBaseline.model) return "model replaced";
            if(list.itemAtIndex(80)!==stabilityDelegate) return "delegate recreated";
            if(Math.abs(list.contentY-stabilityBaseline.y)>0.1) return "viewport jumped";
            if(library.song!==stabilityBaseline.id) return "selected entry lost";
            if(library.playlistSort!==stabilityBaseline.sort) return "sort changed";
            if(JSON.stringify(window.playlistColumnWidths)!==stabilityBaseline.widths) return "width changed";
            return "ok";
        } catch(e) {return String(e);}
    }
    function checkPlaylistStructuralUpdate() {
        stabilityTest.wait(400);
        const rows=library.panes[2].rows;
        if(rows.some(r=>r.id==="stability-20")) return "removed entry still present";
        if(!rows.some(r=>r.id===stabilityBaseline.id)) return "stable entry disappeared";
        if(library.song!==stabilityBaseline.id) return "structural update lost selection";
        return "ok";
    }
    function checkPlaylistEnrichmentDisplayed() {
        const title=stabilityTest.findChild(stabilityDelegate,"songTitle");
        const length=stabilityTest.findChild(stabilityDelegate,"playlistLength");
        return title && title.text==="Enriched title" && length && length.text==="03:00" ? "ok" : "enriched cells did not update";
    }

    function checkPlaylistCanonicalMetadata(artist,album,genre,length) {
        const artistCell=stabilityTest.findChild(stabilityDelegate,"playlistArtist");
        const albumCell=stabilityTest.findChild(stabilityDelegate,"playlistAlbum");
        const lengthCell=stabilityTest.findChild(stabilityDelegate,"playlistLength");
        const row=library.panes[2].rows.find(r=>r.id===stabilityBaseline.id);
        return artistCell && artistCell.text===artist && albumCell && albumCell.text===album && lengthCell && lengthCell.text===length && row.genres===genre ? "ok" : "canonical metadata did not patch";
    }
