    TestCase { id: hopTest; when: false }
    function hopWait() { for(let i=0;i<500 && library.pending;i++) hopTest.wait(10);hopTest.wait(1800); }
    function hopCheck(label) {
        if(view.activeBackend!=="Local" || view.status!=="Playing") throw new Error(label+": "+view.activeBackend+" / "+view.status+" / "+view.error);
        console.log(label,view.activeBackend,view.currentId,view.status);
    }
    function hopLivePlayback(playlist) {
        try {
            bridge.browse_action("view",0,"Playlists");bridge.browse_action("select",0,playlist);hopWait();
            for(const r of library.panes[2].rows) {
                if(r.subtitle!=="Hop Along" || !r.track.release || r.length==="--:--") throw new Error("Incorrect canonical metadata: "+r.title+" / "+r.subtitle);
            }
            const list=hopTest.findChild(songsPane,"libraryPane2");
            const firstArtist=hopTest.findChild(list.itemAtIndex(0),"playlistArtist");
            if(!firstArtist || firstArtist.text!=="Hop Along") throw new Error("Rendered Hop Along Artist missing");
            const entry=library.panes[2].rows[0].id;
            const row=list.itemAtIndex(0);
            hopTest.mouseDoubleClickSequence(row,Math.min(row.width-20,100),row.height/2,Qt.LeftButton);hopWait();hopCheck("double-click");
            bridge.command("next");hopWait();hopCheck("Next");
            bridge.command("previous");hopWait();hopCheck("Previous");
            const waitress=library.panes[2].rows.find(r=>r.title==="Waitress");
            if(!waitress) throw new Error("Waitress missing from live playlist");
            bridge.browse_action("play",2,waitress.id);hopWait();hopCheck("Waitress direct Play");
            bridge.browse_action("play",0,playlist);hopWait();hopCheck("Play Playlist");
            bridge.clear_queue();hopWait();
            bridge.browse_action("append",0,playlist);hopWait();
            bridge.command("play");hopWait();hopCheck("Add to queue then Play");
            bridge.command("stop");return "ok";
        } catch(e) { bridge.command("stop");return String(e); }
    }
