    TestCase { id: desktopTest; name: "DesktopLibraryAudit"; when: false }
    function desktopAudit(audio) {
        function check(value, message) { if (!value) throw new Error(message); }
        function queued() {
            for (let n = 0; n < 250 && library.pending; ++n) desktopTest.wait(20);
            check(!library.pending, "queue prepared without blocking UI");
        }
        function list(n) { return desktopTest.findChild(window.contentItem, "libraryPane" + n); }
        function click(n, i, doubleClick, button) {
            const view = list(n);
            view.forceLayout();
            view.positionViewAtIndex(i, ListView.Contain);
            desktopTest.wait(20);
            const row = view.itemAtIndex(i);
            check(row !== null, "row visible");
            if (doubleClick) desktopTest.mouseDoubleClickSequence(row, 30, row.height / 2, Qt.LeftButton);
            else desktopTest.mouseClick(row, 30, row.height / 2, button || Qt.LeftButton);
            desktopTest.wait(30);
            queued();
        }
        function playing(position) {
            for (let n = 0; n < 100 && (!view.playing || view.pending.length > 0 || (position !== undefined && view.position !== position)) && view.error === ""; ++n) desktopTest.wait(50);
            check(view.playing, "audio started: " + view.error);
        }
        function capture(path) {
            desktopTest.grabImage(window.contentItem).save(path);
        }
        try {
            desktopTest.wait(150);
            const artists = JSON.stringify(library.panes[0].rows);
            check(list(0).count > 0 && list(1).count > 0 && list(2).count > 0, "initial real library");
            click(0, 0, false);
            check(JSON.stringify(library.panes[0].rows) === artists, "Artist list intact");
            check(list(1).count > 0 && list(2).count > 0, "Artist filters");
            const albums = JSON.stringify(library.panes[1].rows);
            click(1, 0, false);
            check(JSON.stringify(library.panes[0].rows) === artists, "Album leaves Artists intact");
            click(2, 0, false);
            check(JSON.stringify(library.panes[1].rows) === albums, "Song leaves Albums intact");
            window.bridge.set_volume(0.05);
            for (let pane = 0; pane < 3; ++pane) {
                click(pane, 0, true);
                if (audio) playing();
                check(view.queueTotal > 0 && view.position === 0, "replace queue " + pane);
            }
            for (let pane = 0; pane < 3; ++pane) {
                click(pane, 0, false, Qt.RightButton);
                const before = view.queueTotal;
                libraryMenu.itemAt(1).triggered(); libraryMenu.close();
                queued();
                check(view.queueTotal > before, "append queue " + pane);
            }
            if (audio) {
                window.bridge.command("pause");
                for (let n = 0; n < 100 && view.playing; ++n) desktopTest.wait(20);
                check(!view.playing, "pause");
                window.bridge.command("play"); playing();
                window.bridge.command("next"); playing(1);
                check(view.position === 1, "next");
                window.bridge.command("previous"); playing(0);
                check(view.position === 0, "previous");
            }
            window.bridge.browse_action("select", 0, "");
            capture("/tmp/music-library-ui-library.png");
            desktopTest.mouseClick(currentTrack); desktopTest.wait(200);
            check(queueDrawer.opened && queue.count > 0, "drawer");
            capture("/tmp/music-library-ui-queue.png");
            desktopTest.keyClick(Qt.Key_Escape); desktopTest.wait(20);
            check(!queueDrawer.opened, "Escape");
            window.bridge.command("stop");
            return "ok";
        } catch (error) { window.bridge.command("stop"); return String(error); }
    }
