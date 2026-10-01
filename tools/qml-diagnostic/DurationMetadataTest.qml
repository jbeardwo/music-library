    TestCase { id: durationTest; when: false }
    function waitDuration() {
        for(let n=0;n<500 && (library.pending || library.playlistDetails.pending);n++) durationTest.wait(10);
        durationTest.wait(30);
    }
    function addDurationPlaylist() {
        bridge.add_music_action("playlist","1:1,1:1,2:1");waitDuration();
        if(library.error) return library.error;
        if(library.panes[2].rows.length!==3 || library.panes[2].rows[0].length!=="≈ 02:03" || library.panes[2].rows[2].length!=="--:--") return "duration row rendering";
        if(library.playlistDetails.duration!=="≈ 04:06 (partial; 1 unknown)") return "approximate partial duplicate total: "+library.playlistDetails.duration;
        if(window.view.queueTotal!==0) return "metadata started playback";
        const list=durationTest.findChild(songsPane,"libraryPane2");list.forceLayout();
        if(durationTest.findChild(list.itemAtIndex(0),"playlistLength").text!=="≈ 02:03") return "actual Length cell";
        durationTest.grabImage(window.contentItem).save("/tmp/playlist-duration-without-playback.png");
        return "ok";
    }
    function checkExactDurationPlaylist() {
        waitDuration();
        return library.panes[2].rows[0].length==="02:04" && library.playlistDetails.duration==="04:09" && window.view.queueTotal===0 ? "ok" : "exact duration enrichment";
    }
