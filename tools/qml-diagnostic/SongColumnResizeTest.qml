    TestCase { id: columnResizeTest; when: false }
    property bool columnResizeWatching: false
    property int columnResizeNotifications: 0
    Connections {
        target: bridge
        function onBrowse_changed() { if(window.columnResizeWatching) window.columnResizeNotifications++; }
        function onMusic_changed() { if(window.columnResizeWatching) window.columnResizeNotifications++; }
    }
    function resizeTableDone() {
        for(let n=0;n<1000 && (library.pending || library.playlistDetails.pending);n++) columnResizeTest.wait(10);
        columnResizeTest.wait(80);
        if(library.pending || library.playlistDetails.pending || library.error) throw new Error("table operation: "+library.error);
    }
    function resizeTableCheck(v,message) { if(!v) throw new Error(message); }
    function resizeTableReveal(edge) {
        const list=columnResizeTest.findChild(songsPane,"libraryPane2");
        list.contentX=Math.min(Math.max(0,list.contentWidth-list.width),Math.max(0,edge-list.width/2));
        columnResizeTest.wait(20);
    }
    function resizeTableDrag(index,delta,bodyFraction) {
        resizeTableReveal(songsPane.columnEdge(index));
        const body=bodyFraction!==undefined;
        const handle=columnResizeTest.findChild(songsPane,(songsPane.songsTable?(body?"songsBodyColumnDivider":"songsColumnDivider"):(body?"playlistBodyColumnDivider":"playlistColumnDivider"))+index);
        resizeTableCheck(handle && handle.width>=10 && handle.cursorShape===Qt.SplitHCursor,"usable resize target "+index);
        const y=handle.height*(body?bodyFraction:0.5);
        const p=handle.mapToItem(window.contentItem,handle.width/2,y);
        columnResizeTest.mousePress(handle,handle.width/2,y,Qt.LeftButton);
        const steps=4;
        for(let step=1;step<=steps;step++) {
            columnResizeTest.mouseMove(window.contentItem,p.x+delta*step/steps,p.y,5);
            columnResizeTest.wait(5);
        }
        columnResizeTest.mouseRelease(window.contentItem,p.x+delta,p.y,Qt.LeftButton);
        columnResizeTest.wait(20);
    }
    function resizeTableAlignment() {
        const list=columnResizeTest.findChild(songsPane,"libraryPane2");list.forceLayout();
        const row=list.itemAtIndex(0);
        resizeTableCheck(row!==null,"materialized first row");
        const names=songsPane.songsTable?["Song","Artist","Album","Genre"]:["#","Title","Artist","Album","Length"];
        const cells=songsPane.songsTable?["songTitle","songsArtist","songsAlbum","songsGenre"]:["trackNumber","songTitle","playlistArtist","playlistAlbum","playlistLength"];
        for(let i=0;i<names.length;i++) {
            const header=columnResizeTest.findChild(songsPane,(songsPane.songsTable?"songsHeader":"playlistHeader")+names[i]);
            const cell=columnResizeTest.findChild(row,cells[i]);
            resizeTableCheck(header && cell && Math.abs(header.width-cell.width)<0.1,"header/cell widths "+names[i]);
            resizeTableCheck(Math.abs(header.mapToItem(window.contentItem,0,0).x-cell.mapToItem(window.contentItem,0,0).x)<0.1,"header/cell positions "+names[i]);
            resizeTableCheck(header.width>=songsPane.columnMinimums[i],"minimum width "+names[i]);
        }
    }
    function exerciseCurrentTableResize() {
        resizeTableDone();
        const corrupt=songsPane.columnMinimums.map((w,i) => [-100,NaN,Infinity,0,1e12][i]);
        if(songsPane.songsTable) window.songsColumnWidths=corrupt; else window.playlistColumnWidths=corrupt;
        columnResizeTest.wait(40);
        for(let i=0;i<songsPane.tableColumns.length;i++) resizeTableCheck(Number.isFinite(songsPane.tableColumns[i]) && songsPane.tableColumns[i]>=songsPane.columnMinimums[i] && songsPane.tableColumns[i]<=songsPane.columnMaximums[i],"invalid session width clamped "+i);
        if(songsPane.songsTable) window.songsColumnWidths=[]; else window.playlistColumnWidths=[];
        columnResizeTest.wait(40);
        const list=columnResizeTest.findChild(songsPane,"libraryPane2");
        list.positionViewAtBeginning();columnResizeTest.wait(80);
        const epoch=library.panes[2].epoch,ids=library.panes[2].rows.map(r=>r.id).join();
        const sort=songsPane.songsTable?library.songsColumn+library.songsDescending:library.playlistSort+library.playlistDescending;
        const queue=JSON.stringify(view.queue),position=view.position,status=view.status;
        columnResizeWatching=true;columnResizeNotifications=0;
        for(let index=0;index<songsPane.tableColumns.length;index++) {
            let old=songsPane.tableColumns.slice();
            // Exercise both clamp edges with actual pointer drags.
            resizeTableDrag(index,songsPane.columnMinimums[index]-old[index]-15);
            resizeTableCheck(Math.abs(songsPane.tableColumns[index]-songsPane.columnMinimums[index])<0.1,"left minimum clamp "+index);
            old=songsPane.tableColumns.slice();
            const total=songsPane.columnsWidth;
            const growth=Math.min(50,songsPane.columnMaximums[index]-old[index]-8);
            resizeTableDrag(index,growth);
            resizeTableCheck(Math.abs(songsPane.tableColumns[index]-old[index]-growth)<0.1,"independent growth "+index);
            resizeTableCheck(Math.abs(songsPane.columnsWidth-total-growth)<0.1,"content width grows "+index);
            for(let other=0;other<old.length;other++) if(other!==index)
                resizeTableCheck(songsPane.tableColumns[other]===old[other],"other column unchanged "+other);
            old=songsPane.tableColumns.slice();resizeTableDrag(index,-12);
            resizeTableCheck(Math.abs(songsPane.tableColumns[index]-old[index]+12)<0.1,"responsive shrink "+index);
            for(let other=0;other<old.length;other++) if(other!==index)
                resizeTableCheck(songsPane.tableColumns[other]===old[other],"shrink does not redistribute "+other);
            for(const fraction of [0.1,0.5,0.9]) {
                old=songsPane.tableColumns.slice();resizeTableDrag(index,6,fraction);
                resizeTableCheck(Math.abs(songsPane.tableColumns[index]-old[index]-6)<0.1,"body divider grows at "+fraction+" / "+index);
                resizeTableDrag(index,-6,fraction);
                resizeTableCheck(Math.abs(songsPane.tableColumns[index]-old[index])<0.1,"body divider shrinks at "+fraction+" / "+index);
                resizeTableAlignment();
            }
        }
        // Oversized drags remain finite and the last divider stays in the
        // horizontal Flickable's range, including after shrinking at the end.
        for(let index=0;index<songsPane.tableColumns.length;index++) {
            const initial=songsPane.tableColumns.slice();
            songsPane.resizeColumns(index,initial,1e12);
            columnResizeTest.wait(20);
            resizeTableCheck(songsPane.tableColumns[index]===songsPane.columnMaximums[index],"practical maximum "+index);
            const tail=list.ScrollBar.horizontal;
            tail.position=1-tail.size;columnResizeTest.wait(30);
            resizeTableCheck(list.contentWidth>=songsPane.columnsWidth,"scroll content encompasses columns");
            resizeTableCheck(Math.abs(list.contentX-(list.contentWidth-list.width))<1,"scrollbar reaches last column");
            songsPane.resizeColumns(index,songsPane.tableColumns.slice(),-1e12);columnResizeTest.wait(30);
            resizeTableCheck(list.contentX>=0 && list.contentX<=Math.max(0,list.contentWidth-list.width)+0.1,"horizontal offset clamped after shrink");
            tail.position=0;columnResizeTest.wait(30);
            resizeTableCheck(Math.abs(list.contentX)<1,"leftmost column recoverable");
            resizeTableAlignment();
        }
        columnResizeWatching=false;
        resizeTableCheck(columnResizeNotifications===0,"resize performs no browse/catalog notifications");
        resizeTableCheck(library.panes[2].epoch===epoch && library.panes[2].rows.map(r=>r.id).join()===ids,"resize does not reload/reorder model");
        resizeTableCheck((songsPane.songsTable?library.songsColumn+library.songsDescending:library.playlistSort+library.playlistDescending)===sort,"drag does not sort");
        resizeTableCheck(JSON.stringify(view.queue)===queue && view.position===position && view.status===status,"queue/playback unchanged");
        resizeTableDrag(0,400);
        const oldWidth=window.width;window.width=800;columnResizeTest.wait(100);
        resizeTableCheck(list.contentWidth>list.width,"narrow pane scrolls horizontally instead of crushing columns");
        resizeTableReveal(songsPane.columnsWidth);resizeTableAlignment();
        const row=list.itemAtIndex(0);
        const last=columnResizeTest.findChild(row,songsPane.songsTable?"songsGenre":"playlistLength");
        columnResizeTest.mouseClick(last,last.width/2,last.height/2,Qt.LeftButton);columnResizeTest.wait(50);
        resizeTableCheck(library.song===library.panes[2].rows[0].id,"scrolled last cell targets entire exact row");
        window.width=oldWidth;columnResizeTest.wait(100);list.contentX=0;
        resizeTableAlignment();
        if(songsPane.playlistTable) {
            const hash=columnResizeTest.findChild(songsPane,"playlistHeader#");
            resizeTableCheck(hash.contentItem.text.startsWith("#") && !hash.contentItem.truncated,"# visible without ellipsis");
        }
        columnResizeTest.grabImage(window.contentItem).save(songsPane.songsTable?"/tmp/songs-resizable-columns.png":"/tmp/playlist-resizable-columns.png");
        return "ok";
    }
    function exerciseSongColumnResize() {
        try {
            bridge.browse_action("view",0,"Songs");resizeTableDone();
            exerciseCurrentTableResize();
            const songWidths=JSON.stringify(window.songsColumnWidths);
            bridge.browse_action("view",0,"Playlists");resizeTableDone();
            bridge.browse_action("select",0,library.panes[0].rows[0].id);resizeTableDone();
            exerciseCurrentTableResize();
            const playlistWidths=JSON.stringify(window.playlistColumnWidths);
            for(const spec of [["Songs",["Song","Artist","Album","Genre"],["song","artist","album","genre"]],["Playlists",["Title","Artist","Album","Length","#"],["title","artist","album","length","position"]]]) {
                bridge.browse_action("view",0,spec[0]);resizeTableDone();
                resizeTableCheck(JSON.stringify(spec[0]==="Songs"?window.songsColumnWidths:window.playlistColumnWidths)===(spec[0]==="Songs"?songWidths:playlistWidths),"independent session widths restored");
                for(let i=0;i<spec[1].length;i++) {
                    const header=columnResizeTest.findChild(songsPane,(songsPane.songsTable?"songsHeader":"playlistHeader")+spec[1][i]);
                    resizeTableReveal(header.x+header.width/2);
                    const active=songsPane.songsTable?library.songsColumn===spec[2][i]:library.playlistSort===spec[2][i];
                    const direction=songsPane.songsTable?library.songsDescending:library.playlistDescending;
                    columnResizeTest.mouseClick(header,Math.min(header.width/2,header.width-12),14,Qt.LeftButton);resizeTableDone();
                    resizeTableCheck((songsPane.songsTable?library.songsColumn:library.playlistSort)===spec[2][i],"header sorts "+spec[1][i]);
                    resizeTableCheck((songsPane.songsTable?library.songsDescending:library.playlistDescending)===(active?!direction:false),"header direction "+spec[1][i]);
                    columnResizeTest.mouseClick(header,Math.min(header.width/2,header.width-12),14,Qt.LeftButton);resizeTableDone();
                    resizeTableCheck((songsPane.songsTable?library.songsDescending:library.playlistDescending)===(active?direction:true),"header reverses "+spec[1][i]);
                }
            }
            const hash=columnResizeTest.findChild(songsPane,"playlistHeader#");
            if(library.playlistDescending) {hash.clicked();resizeTableDone();}
            resizeTableCheck(library.playlistReorderAllowed,"canonical reorder restored");
            return "ok";
        } catch(e) {columnResizeWatching=false;return String(e);}
    }
