    // Injected only by the Rust UI test; exercise the shipped delegates and menus.
    TestCase {
        id: uiTest
        name: "LibraryPlayer"
        when: false
    }
    function exerciseLibraryUi() {
        function check(value, message) { if (!value) throw new Error(message); }
        function queued() {
            for (let n = 0; n < 250 && library.pending; ++n) uiTest.wait(20);
            check(!library.pending, "queue prepared without blocking UI");
        }
        function list(n) { return uiTest.findChild(window.contentItem, "libraryPane" + n); }
        function click(n, index, button) {
            const view = list(n);
            view.forceLayout();
            const row = view.itemAtIndex(index);
            check(row !== null, "visible row " + n + ":" + index);
            uiTest.mouseClick(row, 30, row.height / 2, button || Qt.LeftButton);
            uiTest.wait(15);
        }
        function doubleClick(n, index) {
            const view = list(n);
            view.forceLayout();
            const row = view.itemAtIndex(index);
            uiTest.mouseDoubleClickSequence(row, 30, row.height / 2, Qt.LeftButton);
            uiTest.wait(15);
            queued();
        }
        try {
            uiTest.wait(80);
            const artistCount = list(0).count;
            const albumCount = list(1).count;
            const songCount = list(2).count;
            check(artistCount > 0 && albumCount > 0 && songCount === 45, "initial panes");
            function firstTile() { list(1).forceLayout(); return uiTest.findChild(list(1).itemAtIndex(0), "albumTile0"); }
            let tile = firstTile();
            check(uiTest.findChild(tile,"albumTitle").text === library.panes[1].rows[0].title, "tile uses human Album name");
            check(uiTest.findChild(tile,"albumSecondary").text === library.panes[1].rows[0].subtitle, "global tile shows Artist");
            check(uiTest.findChild(tile,"artPlaceholder").visible, "missing artwork shows shared placeholder");
            click(0, 0);
            tile = firstTile();
            check(uiTest.findChild(tile,"albumSecondary").text === "2015", "Artist tile shows year");
            check(library.artist.length > 0 && list(0).count === artistCount, "Artist selection keeps Artist list");
            const filteredAlbums = list(1).count;
            const filteredSongs = list(2).count;
            click(2, 0);
            check(list(1).count === filteredAlbums && list(0).count === artistCount && list(2).count === filteredSongs, "Song selection does not navigate");
            list(0).forceActiveFocus();
            uiTest.keyClick(Qt.Key_Escape);
            uiTest.wait(15);
            check(library.artist === "" && list(2).count === songCount, "clear Artist");
            click(1, 0);
            check(library.album.length > 0 && list(0).count === artistCount, "Album selection keeps Artists");
            check(library.panes[2].rows[0].title === "00 Sourceless", "Album order");
            list(1).forceActiveFocus();
            uiTest.keyClick(Qt.Key_Escape);
            uiTest.wait(15);
            check(library.album === "", "clear Album");
            click(2, 2);
            list(2).forceActiveFocus();
            uiTest.keyClick(Qt.Key_Down);
            check(list(2).currentIndex === 3, "keyboard Down");
            uiTest.keyClick(Qt.Key_Up);
            check(list(2).currentIndex === 2, "keyboard Up");
            uiTest.keyClick(Qt.Key_Return);
            queued();
            check(view.position === 2 && view.queueTotal === 45 && view.status === "Playing", "Enter starts Song");
            doubleClick(2, 3);
            check(view.queueTotal === 45 && view.position === 3 && view.currentTitle === "03 Multiple available sources", "Song double-click replaces");
            for (let pane = 0; pane < 3; ++pane) {
                click(pane, 0, Qt.RightButton);
                check(libraryMenu.opened, "context menu opens " + pane);
                const before = view.queueTotal;
                libraryMenu.itemAt(1).triggered();
                libraryMenu.close();
                queued();
                check(view.queueTotal === before + (pane === 0 ? filteredSongs : pane === 1 ? songCount : 1), "context append " + pane);
            }
            doubleClick(1, 0);
            doubleClick(2, 2);
            check(view.queueTotal === 45 && view.position === 2 && view.currentId === library.panes[2].rows[2].id, "Album Track 3 preserves full program");
            click(1, 0, Qt.RightButton);
            libraryMenu.itemAt(0).triggered(); libraryMenu.close(); queued();
            check(view.queueTotal === 45 && view.position === 0, "Album double-click queues full library program");
            doubleClick(0, 0);
            check(view.queueTotal > 0 && view.queueTotal < 45 && view.position === 0, "Artist double-click replaces queue");
            const artistProgram = library.panes[2].rows.map(row => row.id);
            doubleClick(2, 1);
            check(view.queueTotal === artistProgram.length && view.position === 1 && view.currentId === artistProgram[1], "Artist Song starts within full program");
            click(2, 1, Qt.RightButton);
            libraryMenu.itemAt(0).triggered(); libraryMenu.close(); queued();
            check(view.queueTotal === artistProgram.length && view.position === 1, "Song context Play keeps program");
            for (let i = 0; i < artistProgram.length; ++i) check(view.queue[i].trackId === artistProgram[i], "Artist queue matches Songs order");
            window.bridge.browse_action("select", 0, "");
            doubleClick(2, 2);
            check(view.currentArtist.length > 0 && view.currentAlbum.length > 0, "current metadata");
            uiTest.mouseClick(currentTrack);
            uiTest.wait(180);
            check(queueDrawer.opened && queue.count === 45 && view.position === 2, "queue drawer opens");
            check(queueDrawer.y + queueDrawer.height <= player.y + 1, "drawer stays above player controls");
            uiTest.keyClick(Qt.Key_Escape);
            uiTest.wait(20);
            check(!queueDrawer.opened, "Escape closes drawer");
            uiTest.mouseClick(currentTrack);
            uiTest.wait(180);
            uiTest.mouseClick(currentTrack);
            uiTest.wait(20);
            check(!queueDrawer.opened, "current Track toggles drawer");
            window.bridge.command("pause");
            check(!view.playing, "pause");
            window.bridge.command("play");
            check(view.playing, "resume");
            for (let i = 0; i < 5; ++i) {
                window.bridge.browse_action("append", 1, library.panes[1].rows[0].id);
                queued();
            }
            check(view.queueTotal === 270 && queue.count === 200, "large queue presentation bounded");
            window.bridge.queue_window(200);
            check(view.queueOffset === 200 && queue.count === 70, "last queue page");
            window.bridge.queue_window(0);
            check(queue.count === 200, "queue page back");
            // Stable identity drives red text, independently of selection and title.
            doubleClick(2, 4);
            click(2, 4);
            function titleColor(index) {
                list(2).forceLayout();
                return String(uiTest.findChild(list(2).itemAtIndex(index), "songTitle").color);
            }
            check(library.panes[2].rows[4].title === library.panes[2].rows[5].title, "duplicate title fixture");
            check(titleColor(4) === "#c6283e" && titleColor(5) !== "#c6283e", "red uses Track identity");
            check(String(list(2).itemAtIndex(4).color) === "#e8d9e0", "selection coexists with red");
            window.bridge.command("next");
            check(titleColor(4) !== "#c6283e" && titleColor(5) === "#c6283e", "Next updates red");
            window.bridge.command("previous");
            check(titleColor(4) === "#c6283e" && titleColor(5) !== "#c6283e", "Previous updates red");
            const album = library.panes[1].rows[0].id;
            window.bridge.browse_action("select", 1, album);
            doubleClick(2, 4);
            const original = JSON.stringify(view.queue);
            const originalId = view.currentId;
            window.bridge.browse_action("sort", 2, "");
            check(library.panes[2].sort === "A-Z", "Album Songs sort cycles");
            check(JSON.stringify(view.queue) === original && view.currentId === originalId, "sort preserves snapshot");
            window.bridge.browse_action("select", 0, library.panes[0].rows[0].id);
            check(JSON.stringify(view.queue) === original, "Artist browsing preserves snapshot");
            window.bridge.browse_action("select", 1, album);
            check(JSON.stringify(view.queue) === original, "Album browsing preserves snapshot");
            const sorted = library.panes[2].rows.map(r => r.id);
            doubleClick(2, 5);
            check(view.position === 5 && view.currentId === sorted[5], "replay starts exact sorted Track");
            check(JSON.stringify(view.queue.map(r => r.trackId)) === JSON.stringify(sorted), "replay snapshots new order");
            window.bridge.browse_action("play", 1, album); queued();
            check(view.position === 0 && JSON.stringify(view.queue.map(r => r.trackId)) === JSON.stringify(sorted), "Album Play uses Album Songs mode");
            const artist = library.panes[0].rows[0].id;
            window.bridge.browse_action("select", 0, artist);
            window.bridge.browse_action("sort", 2, "");
            check(library.panes[2].sort === "Album", "Artist Songs cycles to Album");
            const artistSorted = library.panes[2].rows.map(r => r.id);
            window.bridge.browse_action("play", 0, artist); queued();
            check(JSON.stringify(view.queue.map(r => r.trackId)) === JSON.stringify(artistSorted), "Artist Play uses Artist Songs mode");
            window.bridge.browse_action("append", 1, album); queued();
            check(view.queueTotal === artistSorted.length + sorted.length && view.position === 0, "sorted append preserves current program");
            check(JSON.stringify(view.queue.slice(artistSorted.length).map(r => r.trackId)) === JSON.stringify(sorted), "Album append uses applicable order");
            window.bridge.browse_action("select", 0, "");
            window.bridge.browse_action("sort", 1, "");
            check(library.panes[1].sort === "Year", "Albums Year");
            window.bridge.browse_action("sort", 1, "");
            check(library.panes[1].sort === "Artist", "Albums Artist sections");
            window.bridge.browse_action("select", 0, artist);
            check(library.panes[1].sort === "Year", "grouped mode normalizes on Artist selection");
            window.bridge.browse_action("sort", 1, "");
            check(library.panes[1].sort === "A-Z", "Artist Albums title");
            const snapshot = JSON.stringify(view.queue);
            window.bridge.browse_action("sort", 0, "");
            check(library.panes[0].sort === "Z-A", "Artists reverse");
            window.bridge.browse_action("sort", 0, "");
            check(library.panes[0].sort === "A-Z" && JSON.stringify(view.queue) === snapshot, "Artist sorting leaves queue intact");
            window.bridge.browse_action("select", 0, "");
            return "ok";
        } catch (error) { return String(error); }
    }
    function exercisePlayerControls() {
        uiTest.wait(30);
        if (!seekSlider.enabled || !volumeSlider.enabled) return "disabled";
        uiTest.mouseClick(seekSlider, seekSlider.width / 2, seekSlider.height / 2);
        volumeSlider.value = 35;
        volumeSlider.moved();
        return "ok";
    }
    function seekControlEnabled() { return seekSlider.enabled; }

    function exercisePagedProgram() {
        window.bridge.browse_action("select", 0, "");
        window.bridge.browse_action("refresh", 0, "");
        window.bridge.browse_action("next", 2, "");
        uiTest.wait(30);
        const list = uiTest.findChild(window.contentItem, "libraryPane2");
        list.forceLayout();
        const selected = library.panes[2].rows[3].id;
        const row = list.itemAtIndex(3);
        uiTest.mouseDoubleClickSequence(row, 30, row.height / 2, Qt.LeftButton);
        for (let n = 0; n < 250 && library.pending; ++n) uiTest.wait(20);
        if (library.pending || view.queueTotal !== 496 || view.position !== 203 || view.currentId !== selected)
            return "later-page program/start mismatch";
        if (library.panes[2].rows.length !== 200 || view.queueOffset !== 200 || view.queue.length !== 200 || !view.queue[3].current)
            return "bounded drawer/current occurrence mismatch";
        return "ok";
    }
