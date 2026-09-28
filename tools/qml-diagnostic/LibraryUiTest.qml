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
            click(0, 0);
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
            check(view.queueTotal === 1 && view.status === "Playing", "Enter starts Song");
            doubleClick(2, 3);
            check(view.queueTotal === 1 && view.currentTitle === "03 Multiple available sources", "Song double-click replaces");
            for (let pane = 0; pane < 3; ++pane) {
                click(pane, 0, Qt.RightButton);
                check(libraryMenu.opened, "context menu opens " + pane);
                const before = view.queueTotal;
                libraryMenu.itemAt(1).triggered();
                libraryMenu.close();
                queued();
                check(view.queueTotal > before, "context append " + pane);
            }
            doubleClick(1, 0);
            check(view.queueTotal === 45 && view.position === 0, "Album double-click queues full library program");
            doubleClick(0, 0);
            check(view.queueTotal > 0 && view.queueTotal < 45 && view.position === 0, "Artist double-click replaces queue");
            window.bridge.browse_action("select", 0, "");
            doubleClick(2, 2);
            check(view.currentArtist.length > 0 && view.currentAlbum.length > 0, "current metadata");
            uiTest.mouseClick(currentTrack);
            uiTest.wait(180);
            check(queueDrawer.opened && queue.count === 1, "queue drawer opens");
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
            check(view.queueTotal === 226 && queue.count === 200, "large queue presentation bounded");
            window.bridge.queue_window(200);
            check(view.queueOffset === 200 && queue.count === 26, "last queue page");
            window.bridge.queue_window(0);
            check(queue.count === 200, "queue page back");
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
