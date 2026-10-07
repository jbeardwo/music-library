    TestCase { id: albumLayoutTest; name: "AlbumLayout"; when: false }
    property int layoutArtworkRequests: 0
    function exerciseAlbumLayout() {
        function check(ok, message) { if (!ok) throw new Error(message); }
        const albumList=albumLayoutTest.findChild(window.contentItem,"libraryPane1");
        function waitLayout() { albumLayoutTest.wait(50); albumList.forceLayout(); }
        function albumWidth(width) {
            songsPane.SplitView.preferredWidth=librarySplit.width-32-artistsPane.width-width;
        }
        function tile(index) {
            const group=albumsPane.visualIndex(index);
            albumList.positionViewAtIndex(group,ListView.Contain);albumList.forceLayout();
            const section=albumList.itemAtIndex(group);
            return section ? section.tileAt(index) : null;
        }
        try {
            window.width=1400; waitLayout();
            check(library.panes[1].rows.length>=25,"representative bounded Album page");
            const queue=JSON.stringify(view.queue), current=view.currentId, position=view.position;
            const sort=library.panes[1].sort;
            const originalTile=tile(10);
            albumsPane.selectRow(10); waitLayout();
            const selected=library.album;
            const requests=layoutArtworkRequests;
            for (const width of [148,280,544,148,544]) {
                albumWidth(width); waitLayout();
                const columns=width===148 ? 1 : width===280 ? 2 : 4;
                check(Math.round(albumsPane.width)===width && albumsPane.albumColumns===columns,"fixed tile wrapping at width " + width);
                check(tile(10)===originalTile,"resize only relayouts existing delegates");
                for (let i=0;i<Math.min(25,library.panes[1].rows.length);++i) {
                    const item=tile(i), art=albumLayoutTest.findChild(item,"albumArtwork");
                    check(item.width===120 && item.height===174 && art.width===120 && art.height===120,"tile/art dimensions remain fixed");
                    const image=albumLayoutTest.findChild(art,"coverImage");
                    check(image.smooth && image.mipmap && image.fillMode===Image.Stretch && image.width<=120 && image.height<=120,"filtered aspect-preserving image");
                    check(image.sourceSize.width===Math.ceil(120*Screen.devicePixelRatio),"decode sized to physical display");
                }
                const first=tile(0), next=tile(columns);
                check(next.y-first.y===186,"vertical wrapping uses consistent gap");
                if (columns>1) check(tile(1).x-first.x===132,"horizontal gap is 12px");
                check(library.album===selected && library.panes[1].sort===sort,"selection and sort survive resize");
                check(layoutArtworkRequests===requests,"resize requests no artwork");
            }
            const handle=albumLayoutTest.findChild(librarySplit,"librarySplitHandle");
            check(handle && handle.width===16,"standard SplitView handle");
            const before=artistsPane.width, rightBoundary=songsPane.x;
            albumLayoutTest.mouseDrag(handle,8,handle.height/2,50,0,Qt.LeftButton);waitLayout();
            check(artistsPane.width>before+20,"drag handle resizes Artists pane");
            check(Math.abs(songsPane.x-rightBoundary)<1,"left drag keeps right divider stationary");
            function handles(item) {
                let result=[];
                if(item.objectName==="librarySplitHandle") result.push(item);
                for(const child of item.children || []) result=result.concat(handles(child));
                return result;
            }
            const dividers=handles(librarySplit).sort((a,b)=>a.mapToItem(librarySplit,0,0).x-b.mapToItem(librarySplit,0,0).x);
            check(dividers.length===2,"both split handles available");
            const leftBoundary=albumsPane.x, rightBefore=songsPane.x;
            albumLayoutTest.mouseDrag(dividers[1],8,dividers[1].height/2,40,0,Qt.LeftButton);waitLayout();
            check(songsPane.x>rightBefore+20,"right divider moves independently");
            check(Math.abs(albumsPane.x-leftBoundary)<1,"right drag keeps left divider stationary");
            artistsPane.SplitView.preferredWidth=0;albumsPane.SplitView.preferredWidth=0;waitLayout();
            check(artistsPane.width>=160 && albumsPane.width>=148 && songsPane.width>=260,"usable pane minima");
            artistsPane.SplitView.preferredWidth=10000;waitLayout();
            check(albumsPane.width>=148 && songsPane.width>=260,"oversized drag cannot collapse other panes");
            artistsPane.SplitView.preferredWidth=230;waitLayout();albumWidth(280);waitLayout();
            check(JSON.stringify(view.queue)===queue && view.currentId===current && view.position===position,"resize leaves playback snapshot intact");
            check(layoutArtworkRequests===requests,"handles do not request artwork");
            window.bridge.browse_action("sort",1,"");waitLayout();
            check(library.panes[1].sort==="Year" && albumList.count===3,"three database-ordered year sections");
            check(albumList.itemAtIndex(0).rowData.groupTitle==="2002","newest year heading");
            const yearTile=tile(0), yearRequests=layoutArtworkRequests;
            for(const width of [544,148,280]) {
                albumWidth(width);waitLayout();
                const columns=albumsPane.albumColumns;
                check(tile(0)===yearTile && yearTile.width===120 && yearTile.height===174,"fixed year tiles retained through reflow");
                check(tile(columns).y-tile(0).y===186,"tiles wrap within year section");
                check(layoutArtworkRequests===yearRequests,"year reflow requests no artwork");
            }
            // Exercise a visible tile inside a section larger than the viewport.
            const visibleTile=albumList.itemAtIndex(albumsPane.visualIndex(4)).tileAt(4);
            albumList.forceActiveFocus();
            albumList.contentY=visibleTile.mapToItem(albumList.contentItem,0,0).y-80;waitLayout();
            const viewport=albumList.contentY;
            check(visibleTile.mapToItem(albumList,0,0).y>=0 && visibleTile.mapToItem(albumList,0,0).y+visibleTile.height<=albumList.height,"Ctrl test tile is already visible");
            for(const modifier of [Qt.ControlModifier,Qt.ControlModifier,Qt.NoModifier]) {
                albumLayoutTest.mouseClick(visibleTile,30,30,Qt.LeftButton,modifier);waitLayout();
                check(Math.abs(albumList.contentY-viewport)<1,"visible Album toggle/click must not reposition its oversized section");
            }
            const nextVisible=albumList.itemAtIndex(albumsPane.visualIndex(5)).tileAt(5);
            albumLayoutTest.mouseClick(nextVisible,30,30,Qt.LeftButton,Qt.ShiftModifier);
            for(let n=0;n<250 && library.pending;n++) albumLayoutTest.wait(10);
            waitLayout();check(library.panes[1].selectionCount===2 && Math.abs(albumList.contentY-viewport)<1,"visible Shift Album range retains viewport");
            const refreshAnchor=albumsPane.viewportAnchor();
            albumsPane.rememberViewport();
            for(let repeat=0;repeat<3;repeat++) {
                window.bridge.browse_action("refresh",0,"");waitLayout();
                const refreshed=albumsPane.viewportAnchor();
                check(refreshed && refreshed.id===refreshAnchor.id && Math.abs(refreshed.pixel-refreshAnchor.pixel)<1,"background refresh preserves Album viewport");
                check(library.panes[1].selectionCount===2,"background refresh preserves Album selection");
            }
            window.bridge.browse_action("sort",1,"");waitLayout();
            check(library.panes[1].sort==="Artist","Artist sections remain available");
            const sectionTile=tile(10), sectionRequests=layoutArtworkRequests;
            for (const width of [544,148,280]) {
                albumWidth(width);waitLayout();
                check(tile(10)===sectionTile && layoutArtworkRequests===sectionRequests,"section reflow retains delegates and artwork requests");
            }
            // Navigate to an Album several wrapped rows below the viewport.
            window.bridge.search_library("Tile Album 024",2);
            for(let n=0;n<200 && bridge.local_search_snapshot.busy;++n) albumLayoutTest.wait(10);
            check(bridge.local_search_snapshot.rows.length>0,"Album search finds target");
            check(window.bridge.navigate_search_result(0),"search navigation succeeds");waitLayout();
            const logical=library.panes[1].rows.findIndex(r=>r.id===library.album);
            const target=tile(logical);
            albumsPane.revealLogical(logical);waitLayout();
            const y=target.mapToItem(albumList,0,0).y;
            check(target.Accessible.selected && albumList.activeFocus && y>=-1 && y+target.height<=albumList.height+1,"wrapped search target focused and visible");
            albumLayoutTest.mouseClick(target,30,30,Qt.RightButton);albumLayoutTest.wait(30);
            check(libraryMenu.opened && contextId===library.album,"tile context menu retains exact Album");
            libraryMenu.close();
            albumLayoutTest.mouseDoubleClickSequence(target,30,30,Qt.LeftButton);
            for(let n=0;n<250 && library.pending;++n) albumLayoutTest.wait(10);
            check(!library.pending && view.queueTotal>0,"tile double-click still plays");
            window.bridge.browse_action("select",0,""); waitLayout();
            const index=library.panes[1].rows.findIndex(r=>r.id===selected);
            albumsPane.selectRow(index);albumList.forceActiveFocus();waitLayout();
            albumLayoutTest.keyClick(Qt.Key_Down);waitLayout();
            check(albumsPane.logicalIndex===Math.min(index+albumsPane.albumColumns,library.panes[1].rows.length-1),"Down moves by current column count");
            return "ok";
        } catch(e) { return String(e); }
    }

    function exerciseAlbumLayoutStress() {
        try {
            const list=albumLayoutTest.findChild(window.contentItem,"libraryPane1");
            window.width=1400;albumLayoutTest.wait(80);list.forceLayout();
            if(library.panes[1].rows.length!==200) throw new Error("200-row Album page required");
            const queueBefore=JSON.stringify(view.queue);
            for(const name of ["Genres","Albums","Songs","Playlists"]) {
                window.bridge.browse_action("view",0,name);albumLayoutTest.wait(20);
                if(library.panes.some(p=>p.rows.length>200)) throw new Error("large view exceeded bounded page");
                if(name==="Genres" && library.panes[0].rows.length) {
                    artistsPane.selectRow(0);albumLayoutTest.wait(20);
                    if(library.panes.some(p=>p.rows.length>200)) throw new Error("Genre filter exceeded bounded page");
                }
                if(name==="Songs") {
                    if(library.panes[2].rows.length!==200 || songsPane.width<librarySplit.width-1) throw new Error("large Songs page must be bounded and full width");
                    window.bridge.browse_action("next",2,"");albumLayoutTest.wait(20);
                    if(library.panes[2].rows.length!==200 || library.panes[2].page!==2) throw new Error("large Songs cursor page");
                    window.bridge.browse_action("sort",2,"");albumLayoutTest.wait(20);
                    if(library.panes[2].sort!=="Z-A" || library.panes[2].rows.length!==200) throw new Error("large reverse Songs page");
                    window.bridge.browse_action("select",2,library.panes[2].rows[0].id);
                    window.bridge.browse_action("next",2,"");albumLayoutTest.wait(20);
                    window.bridge.browse_action("select-range",2,library.panes[2].rows[10].id);
                    for(let n=0;n<250 && library.pending;n++) albumLayoutTest.wait(20);
                    if(library.pending || library.error || library.panes[2].selectionCount!==211 || library.panes[2].rows.length!==200 || library.panes[2].selectedIds.length>200) throw new Error("large reverse Song range remains bounded");
                }
                if(name==="Playlists") {
                    if(library.panes[2].rows.length) throw new Error("playlist Songs must stay blank until selection");
                    const playlist=library.panes[0].rows.find(p=>p.title==="Large selection fixture");
                    if(!playlist) throw new Error("large playlist fixture required");
                    window.bridge.browse_action("select",0,playlist.id);albumLayoutTest.wait(20);
                    if(library.panes[2].rows.length!==200) throw new Error("large playlist page bound");
                    const anchor=library.panes[2].rows[0].id;
                    window.bridge.browse_action("select",2,anchor);
                    window.bridge.browse_action("next",2,"");albumLayoutTest.wait(20);
                    window.bridge.browse_action("select-range",2,library.panes[2].rows[10].id);
                    for(let n=0;n<250 && library.pending;n++) albumLayoutTest.wait(20);
                    if(library.pending || library.error) throw new Error("large cross-page playlist range: "+library.error);
                    if(library.panes[2].selectionCount!==211 || library.panes[2].rows.length!==200 || library.panes[2].selectedIds.length>200) throw new Error("off-page stable selection and bounded projection");
                    const remaining=library.panes[2].rows[10].id;
                    window.bridge.browse_action("select-toggle",2,remaining);albumLayoutTest.wait(20);
                    if(library.panes[2].selectionCount!==210) throw new Error("large playlist Ctrl toggle");
                }
            }
            window.bridge.browse_action("view",0,"Artists");albumLayoutTest.wait(50);list.forceLayout();
            if(JSON.stringify(view.queue)!==queueBefore) throw new Error("large view navigation mutated queue");
            const queue=JSON.stringify(view.queue), selected=library.album, sort=library.panes[1].sort;
            const requests=layoutArtworkRequests, reference=list.itemAtIndex(0).tileAt(100);
            const start=Date.now();
            for(let n=0;n<60;++n) {
                songsPane.SplitView.preferredWidth=librarySplit.width-32-artistsPane.width-[148,280,544][n%3];
                albumLayoutTest.wait(1);list.forceLayout();
                list.contentY=(n%2) ? Math.max(0,list.contentHeight-list.height) : 0;
                if(list.itemAtIndex(0).tileAt(100)!==reference) throw new Error("bounded delegate recreated on resize");
            }
            if(library.panes[1].rows.length!==200 || layoutArtworkRequests!==requests) throw new Error("resize expanded data or requested artwork");
            if(JSON.stringify(view.queue)!==queue || library.album!==selected || library.panes[1].sort!==sort) throw new Error("resize changed state");
            return "ok: 60 resize/scroll iterations in " + (Date.now()-start) + " ms, 200 retained logical Albums";
        } catch(e) { return String(e); }
    }
