    TestCase { id: preferenceTest; name: "Preferences"; when: false }
    function exercisePreferencesUi() {
        function check(ok,message) { if(!ok) throw new Error(message); }
        function waitDone() { for(let n=0;n<300 && library.pending;++n) preferenceTest.wait(20); preferenceTest.wait(60); check(!library.pending,"queue completes"); check(library.error==="","browse error: "+library.error); }
        function action(name,pane,id) { bridge.browse_action(name,pane,id); waitDone(); }
        try {
            preferenceTest.wait(80);
            const featured=library.panes[0].rows.find(r=>r.title==="Featured Person");
            check(!!featured,"secondary Artist appears");
            window.contextPane=0; window.contextId=featured.id;
            libraryMenu.open(); preferenceTest.wait(30);
            const hide=libraryMenu.contentData.find(r=>r.objectName==="hideArtistAction");
            check(hide.text==="Hide this artist","Artist menu wording"); hide.triggered(); libraryMenu.close(); waitDone();
            check(!library.panes[0].rows.some(r=>r.id===featured.id),"hidden from Artists");
            check(library.panes[2].rows.some(r=>r.subtitle.indexOf("Featured Person")>=0),"secondary credit preserved");
            searchPanel.open(); searchText.text="Featured Person"; searchPanel.request();
            for(let n=0;n<300 && searchPanel.results.busy;++n) preferenceTest.wait(10);
            const hit=searchPanel.results.rows.findIndex(r=>r.id===featured.id);
            check(hit>=0,"hidden Artist searchable");check(bridge.navigate_search_result(hit),"hidden Artist search navigation");searchPanel.close();waitDone();
            check(library.artist===featured.id,"hidden Artist reached normally");
            action("select",0,"");check(!library.panes[0].rows.some(r=>r.id===featured.id),"return to passive Artists omits hidden Artist");
            action("preference-hidden",0,""); preferenceManager.hiddenArtists=true;preferenceManager.open();preferenceTest.wait(30);
            check(library.preferenceRows.some(r=>r.id===featured.id),"Hidden Artists manager");
            action("preference-context",0,featured.id);check(library.contextHidden,"hidden context exposes Unhide");
            const restore=preferenceTest.findChild(preferenceManager.contentItem,"preferenceRestore-"+featured.id);
            check(!!restore,"manager Unhide button exists");preferenceTest.mouseClick(restore,20,15);waitDone();preferenceManager.close();
            check(library.panes[0].rows.some(r=>r.id===featured.id),"Unhide restores immediately");
            const album=library.panes[1].rows[0];const songs=library.panes[2].rows.slice();const ignored=songs[1];
            action("preference-ignore",2,ignored.id);
            check(library.panes[2].rows.find(r=>r.id===ignored.id).ignored,"Song greyed");
            action("context-play",1,album.id);
            check(!bridge.snapshot.queue.some(r=>r.trackId===ignored.id),"Play Album excludes ignored Song");
            action("append",2,ignored.id);
            check(bridge.snapshot.queue.some(r=>r.trackId===ignored.id),"explicit enqueue allows ignored Song");
            action("play",2,ignored.id);
            if (bridge.snapshot.realAudio) { for(let n=0;n<150 && bridge.snapshot.status!=="Playing";++n) preferenceTest.wait(20); check(bridge.snapshot.status==="Playing","explicit ignored Song plays through audio engine"); }
            check(bridge.snapshot.currentId===ignored.id,"explicit row Play selects ignored Song");
            action("preference-ignore",1,album.id);check(library.panes[1].rows.find(r=>r.id===album.id).ignored,"fully ignored Album greyed");
            action("preference-unignore",2,ignored.id);check(!library.panes[1].rows.find(r=>r.id===album.id).ignored,"one Unignore clears full Album appearance");
            const main=library.panes[0].rows.find(r=>r.title==="Main Artist");
            action("preference-ignore",0,main.id);check(library.panes[0].rows.find(r=>r.id===main.id).ignored,"fully ignored Artist greyed");
            action("preference-ignored",2,"");preferenceManager.hiddenArtists=false;preferenceManager.open();preferenceTest.wait(30);
            check(library.preferenceRows.length===3,"Ignored Songs manager");
            check(library.preferenceRows.every(r=>r.title && r.album && r.artist),"Song Artist Album displayed");
            action("preference-unignore",2,ignored.id);check(library.preferenceRows.length===2,"targeted manager Unignore");
            check(!library.panes[0].rows.find(r=>r.id===main.id).ignored,"Artist partial state normal");
            action("preference-unignore",0,main.id);check(library.preferenceRows.length===0,"bulk Unignore Artist");preferenceManager.close();
            preferenceTest.grabImage(window.contentItem).save("/tmp/music-library-preferences-ui.png");
            return "ok";
        } catch(e) { return String(e); }
    }
