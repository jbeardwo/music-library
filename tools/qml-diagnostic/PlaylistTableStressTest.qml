    TestCase { id: tableStress; name: "PlaylistTableStress"; when: false }
    function exercisePlaylistTableStress() {
        function check(v,m) {if(!v) throw new Error(m);}
        function done() {for(let n=0;n<1000 && library.pending;n++) tableStress.wait(10);tableStress.wait(50);check(!library.pending && !library.error,"stress operation: "+library.error);}
        function key(r,column) {return column==="position"?Number(r.number):column==="length"?r.durationMs:(column==="title"?r.title:column==="artist"?r.subtitle:r.track.release).toLowerCase();}
        try {
            bridge.browse_action("view",0,"Playlists");done();
            bridge.browse_action("select",0,library.panes[0].rows.find(r=>r.title==="! Table stress").id);done();
            const list=tableStress.findChild(songsPane,"libraryPane2");
            check(library.playlistSort==="position" && library.playlistReorderAllowed,"default order");
            check(window.exerciseCurrentTableResize()==="ok","200k Playlist column resizing");
            for(const column of [["Title","title"],["Artist","artist"],["Album","album"],["Length","length"],["#","position"]]) {
                for(const descending of [false,true]) {
                    tableStress.findChild(songsPane,"playlistHeader"+column[0]).clicked();done();
                    const first=library.panes[2].rows[0].id;
                    const encountered=new Set();
                    for(let n=0;n<8;n++) {
                        const rows=library.panes[2].rows;
                        check(rows.length<=600 && list.count<=600,"200k bounded table");
                        check(rows.every((r,i)=>i===0 || (descending?key(rows[i-1],column[1])>=key(r,column[1]):key(rows[i-1],column[1])<=key(r,column[1]))),"200k sorted chunks");
                        check(new Set(rows.map(r=>r.id)).size===rows.length,"distinct entries in loaded window");
                        check(rows.every(r=>Number(r.number)>=1 && Number(r.number)<=200002),"absolute persisted positions");
                        rows.forEach(r=>encountered.add(r.id));
                        list.forceLayout();list.positionViewAtEnd();
                        for(let poll=0;poll<100 && library.panes[2].rows.slice(-1)[0].id===rows.slice(-1)[0].id;poll++) tableStress.wait(10);
                        done();
                    }
                    check(encountered.size>600,"global fetch beyond current window");
                    for(let n=0;n<15 && library.panes[2].before;n++) {list.positionViewAtBeginning();tableStress.wait(80);done();}
                    check(!library.panes[2].before && library.panes[2].rows[0].id===first,"deterministic reverse fetch");
                }
            }
            tableStress.findChild(songsPane,"playlistHeader#").clicked();done();
            check(library.playlistReorderAllowed,"canonical reorder restored");
            tableStress.grabImage(window.contentItem).save("/tmp/playlist-table-200k.png");
            return "ok";
        } catch(e) {return String(e);}
    }
