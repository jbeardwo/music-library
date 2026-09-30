    TestCase { id: removalTest; name: "LibraryRemoval"; when: false }
    function exerciseRemovalUi() {
        function check(ok, message) { if (!ok) throw new Error(message); }
        function waitDone() {
            for (let n=0; n<250 && library.pending; ++n) removalTest.wait(20);
            check(!library.pending, "background removal completes");
            check(library.error === "", "removal error: " + library.error);
            removalTest.wait(250);
        }
        function openMenu(pane, index) {
            const view=removalTest.findChild(window.contentItem, "libraryPane" + pane);
            view.forceLayout();
            const row=view.itemAtIndex(index);
            check(!!row, "row exists");
            if (pane===1) {
                const tile=removalTest.findChild(row,"albumTile0");
                removalTest.mouseClick(tile,30,30,Qt.RightButton);
            } else removalTest.mouseClick(row,30,row.height/2,Qt.RightButton);
            removalTest.wait(20);
            check(libraryMenu.visible,"right click opens context menu pane="+pane+" dialog="+removalDialog.visible);
            const item=libraryMenu.itemAt(libraryMenu.count - 1);
            check(item.text==="Remove from library","separated removal menu action");
            item.triggered(); libraryMenu.close(); waitDone(); removalTest.wait(250);
            check(removalDialog.visible,"confirmation opens");
            check(cancelRemoval.activeFocus,"Cancel is safe default: " + window.activeFocusItem + " text=" + (window.activeFocusItem ? window.activeFocusItem.text : "none"));
        }
        try {
            removalTest.wait(80);
            // A queued snapshot is independent of the library panes.
            bridge.browse_action("append",2,library.panes[2].rows[0].id); waitDone();
            const queueBefore=JSON.stringify(bridge.snapshot.queue);
            const initialSongs=library.panes[2].rows.length;
            const removedSong=library.panes[2].rows[0].id;
            openMenu(2,0);
            const local=!!library.removal.local;
            check(suppressRescan.visible===local,"checkbox only for local sources");
            check(suppressRescan.checked,"suppression defaults checked");
            check(library.removal.message.indexOf(library.panes[2].rows[0].title)>=0,"Song identified");
            removalTest.keyClick(Qt.Key_Return); removalTest.wait(250);
            check(!removalDialog.visible,"Enter activates safe Cancel");
            check(library.panes[2].rows.length===initialSongs,"Cancel preserves membership");
            openMenu(2,0);
            removalTest.mouseClick(confirmRemoval, 20, 15); waitDone();
            check(!library.panes[2].rows.some(function(row) { return row.id===removedSong; }),"Song pane refreshes");
            check(JSON.stringify(bridge.snapshot.queue)===queueBefore,"active queue unchanged");
            openMenu(1,0);
            check(library.removal.message.indexOf("saved Tracks")>=0,"Album explains bulk removal");
            check(suppressRescan.checked,"Album defaults checked");
            bridge.browse_action("remove-cancel",0,""); removalTest.wait(250);
            openMenu(0,0);
            check(library.removal.message.indexOf("saved Tracks")>=0,"Artist explains bulk removal");
            removalTest.mouseClick(confirmRemoval, 20, 15); waitDone();
            check(library.artist==="" && library.album==="" && library.song==="","stale selections cleared");
            if(library.panes[1].rows.length>0) {
                openMenu(1,0); suppressRescan.checked=false; removalTest.mouseClick(confirmRemoval,20,15); waitDone();
            }
            return "ok";
        } catch(e) { return String(e); }
    }
