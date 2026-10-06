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
            const playing = view.currentId;
            const queue = JSON.stringify(view.queue);
            const associations = [];
            for (let i = 1; i < Math.min(6, library.panes[2].rows.length); ++i) {
                list.positionViewAtIndex(i, ListView.Center); hopTest.wait(50); list.forceLayout();
                const selected = library.panes[2].rows[i];
                hopTest.mouseClick(list.itemAtIndex(i), 20, 20, Qt.RightButton); hopTest.wait(30);
                const action = hopTest.findChild(libraryMenu, "songSpotifyConnection");
                if (!action || !action.visible || contextTrackId !== selected.track.trackId) throw new Error("canonical Playlist Track not captured");
                action.triggered(); libraryMenu.close(); hopTest.wait(50);
                if (!spotifyPlaybackDialog.visible || spotifyPlayback.trackId !== selected.track.trackId || spotifyPlayback.title.indexOf(selected.title) < 0) throw new Error("wrong diagnostic Track");
                if (spotifyPlayback.songUri !== hopAssociations[selected.track.trackId]) throw new Error("persisted Spotify association mismatch");
                if (view.currentId !== playing || JSON.stringify(view.queue) !== queue) throw new Error("diagnostic changed playback");
                hopCheck("inspect " + selected.title);
                associations.push(selected.title + ": " + spotifyPlayback.trackId + " / " + spotifyPlayback.songUri);
                spotifyPlaybackDialog.close(); hopTest.wait(30);
            }
            console.log("Hop Along canonical diagnostics", JSON.stringify(associations));
            bridge.browse_action("view", 0, "Songs"); hopTest.wait(50);
            const localIndex = library.panes[2].rows.findIndex(r => r.id === hopUnconnected);
            if (localIndex < 0) throw new Error("unconnected local row missing");
            list.positionViewAtIndex(localIndex, ListView.Center); hopTest.wait(50); list.forceLayout();
            hopTest.mouseClick(list.itemAtIndex(localIndex), 20, 20, Qt.RightButton); hopTest.wait(30);
            hopTest.findChild(libraryMenu, "songSpotifyConnection").triggered(); libraryMenu.close(); hopTest.wait(50);
            if (spotifyPlayback.trackId !== hopUnconnected || spotifyPlayback.available || spotifyPlayback.title.length === 0 || spotifyPlayback.resolutionPending) throw new Error("unconnected local diagnostic state");
            if (view.currentId !== playing || JSON.stringify(view.queue) !== queue) throw new Error("unconnected inspection changed playback");
            if (!hopTest.findChild(spotifyPlaybackDialog, "spotifyConnectionSearch").enabled) throw new Error("unconnected search unavailable");
            console.log("Unconnected local diagnostic", spotifyPlayback.title, spotifyPlayback.trackId);
            spotifyPlaybackDialog.close(); hopTest.wait(30);
            bridge.browse_action("view", 0, "Playlists"); hopTest.wait(50);
            settingsMenu.itemAt(1).triggered(); hopTest.wait(30);
            if (spotifyPlayback.trackId !== playing) throw new Error("Now Playing diagnostic target");
            spotifyPlaybackDialog.close(); hopTest.wait(30);
            list.positionViewAtBeginning(); hopTest.wait(30);
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
