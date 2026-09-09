pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

ApplicationWindow {
    id: window
    width: 1050
    height: 840
    minimumWidth: 800
    minimumHeight: 700
    visible: true
    title: window.view.realAudio ? "Music Library — GstPlay local audio diagnostic" : "Music Library — fake engine (NO AUDIO)"
    // One dynamic context object; the Rust/QML smoke test checks this boundary.
    // qmllint disable unqualified
    readonly property var bridge: diagnostic
    // qmllint enable unqualified
    readonly property var view: window.bridge.snapshot
    readonly property var catalogView: window.bridge.catalog_snapshot

    Dialog {
        id: catalogDialog
        title: "MusicBrainz catalog (diagnostic)"
        width: Math.min(window.width - 40, 980)
        height: 560
        property bool showEditions: false
        anchors.centerIn: parent
        standardButtons: Dialog.Close
        contentItem: ColumnLayout {
            RowLayout {
                TextField {
                    id: catalogQuery
                    placeholderText: "Album / artist search"
                    Layout.fillWidth: true
                    enabled: !window.catalogView.catalogPending
                    onAccepted: {
                        catalogDialog.showEditions = false;
                        window.bridge.catalog_action("search", text);
                    }
                }
                Button {
                    text: "Search Albums"
                    enabled: !window.catalogView.catalogPending
                    onClicked: {
                        catalogDialog.showEditions = false;
                        window.bridge.catalog_action("search", catalogQuery.text);
                    }
                }
            }
            ListView {
                id: albumResults
                Layout.fillWidth: true
                Layout.preferredHeight: 170
                clip: true
                model: window.catalogView.catalogGroups
                delegate: RowLayout {
                    id: albumRow
                    required property var modelData
                    required property int index
                    width: albumResults.width
                    Label {
                        text: albumRow.modelData.label
                        textFormat: Text.PlainText
                        Layout.fillWidth: true
                        elide: Text.ElideRight
                    }
                    Button {
                        text: "Add Album"
                        enabled: !window.catalogView.catalogPending
                        onClicked: window.addAlbum(albumRow.index)
                    }
                    Button {
                        text: "Editions…"
                        enabled: !window.catalogView.catalogPending
                        onClicked: window.chooseEditions(albumRow.index)
                    }
                }
            }
            Button {
                text: "Next Album page"
                enabled: !window.catalogView.catalogPending && window.catalogView.catalogMoreGroups
                onClicked: {
                    catalogDialog.showEditions = false;
                    window.bridge.catalog_action("more_groups", "");
                }
            }
            ColumnLayout {
                id: editionSection
                visible: catalogDialog.showEditions
                Layout.fillWidth: true
                ComboBox {
                    id: editions
                    Layout.fillWidth: true
                    model: window.catalogView.catalogEditions
                    textRole: "label"
                    currentIndex: -1
                    enabled: !window.catalogView.catalogPending
                    onModelChanged: currentIndex = -1
                }
                Label {
                    text: editions.currentIndex < 0 ? "Choose an edition explicitly." : editions.currentText
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                }
                RowLayout {
                    Button {
                        text: "Add this edition"
                        enabled: !window.catalogView.catalogPending && editions.currentIndex >= 0
                        onClicked: window.bridge.catalog_action("add", String(editions.currentIndex))
                    }
                    Button {
                        text: "Next edition page"
                        enabled: !window.catalogView.catalogPending && window.catalogView.catalogMoreEditions
                        onClicked: window.bridge.catalog_action("more_editions", "")
                    }
                }
            }
            BusyIndicator {
                running: window.catalogView.catalogPending
                implicitWidth: 30
                implicitHeight: 30
            }
            Label {
                text: window.catalogView.catalogStatus
                Layout.fillWidth: true
                wrapMode: Text.Wrap
            }
        }
    }

    function addAlbum(index) {
        catalogDialog.showEditions = false;
        window.bridge.catalog_action("add_album", String(index));
    }
    function chooseEditions(index) {
        catalogDialog.showEditions = true;
        window.bridge.catalog_action("editions", String(index));
    }

    function ready() {
        return true;
    }
    function smokeTest() {
        function check(condition, message) {
            if (!condition)
                throw new Error(message);
        }
        try {
            check(view.volume === 1 && volumeSlider.value === 100, "default volume");
            volumeSlider.value = 35;
            volumeSlider.moved();
            check(view.volume === 0.35 && view.status === "Stopped", "stopped volume");
            window.bridge.set_volume(1);
            check(view.volume === 1, "restore volume");
            check(view.rows.length === 20, "bounded initial page");
            check(results.count === 20, "list model binding");
            const firstId = view.rows[0].trackId;
            window.bridge.page_next();
            check(view.page === 2 && view.rows[0].trackId !== firstId, "cursor next");
            window.bridge.page_previous();
            check(view.page === 1 && view.rows[0].trackId === firstId, "cursor previous");
            window.bridge.search("Available");
            window.bridge.queue_page();
            check(queue.count === view.queue.length, "queue model binding");
            window.bridge.command("play");
            check(view.status === "Playing", "play notification");
            window.bridge.command("pause");
            check(view.status === "Paused", "pause notification");
            window.bridge.set_volume(0.6);
            check(view.volume === 0.6 && view.status === "Paused", "paused volume");
            window.bridge.set_volume(-1);
            check(view.volume === 0.6 && view.error.length > 0, "invalid volume");
            window.bridge.set_volume(1);
            window.bridge.command("play");
            check(view.status === "Playing", "resume notification");
            window.bridge.toggle_failure();
            window.bridge.command("stop");
            check(view.status === "Failed" && view.error.length > 0 && !view.failureArmed, "failure notification");
            window.bridge.command("play");
            check(view.status === "Playing" && view.error === "", "stop then retry recovery");
            window.bridge.command("next");
            check(view.position === 1, "next queue entry");
            window.bridge.command("previous");
            check(view.position === 0, "previous queue entry");
            window.bridge.search("Sourceless");
            window.bridge.play_row(view.rows[0].trackId);
            check(view.status === "Stopped" && view.error.length > 0, "sourceless error");
            window.bridge.search("Unavailable");
            window.bridge.play_row(view.rows[0].trackId);
            check(view.status === "Stopped" && view.error.length > 0, "unavailable error");
            window.bridge.search("nothingmatches");
            check(view.rows.length === 0 && results.count === 0 && !view.hasNext, "empty results");
            clearButton.clicked();
            check(view.queue.length === 0 && queue.count === 0 && view.position === -1, "clear queue binding");
            check(!previousButton.enabled && !nextButton.enabled, "empty movement disabled");
            window.bridge.search("");
            const missingId = view.rows[0].trackId;
            const playableId = view.rows[2].trackId;
            window.bridge.enqueue_row(playableId);
            window.bridge.enqueue_row(missingId);
            window.bridge.enqueue_row(playableId);
            check(view.queue.length === 3 && queue.count === 3, "individual append and duplicate");
            check(view.queue[0].trackId === view.queue[2].trackId, "duplicate IDs preserved");
            check(!previousButton.enabled && nextButton.enabled, "first movement bounds");
            window.bridge.command("play");
            nextButton.clicked();
            check(view.position === 1 && view.status === "Stopped" && view.error.length > 0, "unplayable neighbor selected");
            check(previousButton.enabled && nextButton.enabled, "unplayable movement enabled");
            nextButton.clicked();
            check(view.position === 2 && view.status === "Playing" && view.error === "", "next past unplayable");
            check(view.queue.length === 3 && queue.count === 3 && view.queue[0].trackId === playableId, "played entries retained in snapshot");
            check(!view.queue[0].current && view.queue[2].current, "current duplicate distinguished by index");
            check(previousButton.enabled && !nextButton.enabled, "last movement bounds");
            previousButton.clicked();
            check(view.position === 1 && view.error.length > 0, "previous selects unplayable");
            previousButton.clicked();
            check(view.position === 0 && view.status === "Playing", "previous past unplayable");
            clearButton.clicked();
            window.bridge.enqueue_row(playableId);
            window.bridge.enqueue_row(playableId);
            window.bridge.enqueue_row(playableId);
            window.bridge.command("play");
            nextButton.clicked();
            window.bridge.toggle_failure();
            window.bridge.command("pause");
            check(view.status === "Failed" && previousButton.enabled && nextButton.enabled, "Failed does not disable navigation");
            nextButton.clicked();
            check(view.status === "Playing" && view.position === 2, "next from Failed");
            window.bridge.toggle_failure();
            window.bridge.command("pause");
            previousButton.clicked();
            check(view.status === "Playing" && view.position === 1, "previous from Failed");
            window.bridge.toggle_failure();
            clearButton.clicked();
            check(view.status === "Failed" && view.queue.length === 3 && view.error.length > 0, "failed clear is explicit");
            clearButton.clicked();
            check(view.status === "Stopped" && view.position === -1 && queue.count === 0 && view.source === "—", "clear after failure");
            check(!previousButton.enabled && !nextButton.enabled && !clearButton.enabled, "cleared button bounds");
            return "ok";
        } catch (error) {
            return String(error);
        }
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 12
        spacing: 8
        Label {
            text: window.view.realAudio ? "DIAGNOSTIC ONLY · Real local audio · Temporary library from supplied folder" : "DIAGNOSTIC ONLY · Synthetic 45-Track library · Fake engine · No audio or real files"
            Layout.fillWidth: true
            wrapMode: Text.Wrap
        }
        RowLayout {
            Button {
                text: "Catalog…"
                onClicked: catalogDialog.open()
            }
            TextField {
                id: query
                Layout.fillWidth: true
                placeholderText: "Search title, Artist, Release"
                onAccepted: window.bridge.search(text)
            }
            Button {
                text: "Search / reset page"
                onClicked: window.bridge.search(query.text)
            }
        }
        RowLayout {
            Label {
                text: "Results for “" + window.view.searchText + "” · Page " + window.view.page
                Layout.fillWidth: true
            }
            Button {
                text: "Previous page"
                enabled: window.view.page > 1
                onClicked: window.bridge.page_previous()
            }
            Button {
                text: "Next page"
                enabled: window.view.hasNext
                onClicked: window.bridge.page_next()
            }
            Button {
                text: "Queue this page (stops)"
                enabled: window.view.rows.length > 0
                onClicked: window.bridge.queue_page()
            }
        }
        ListView {
            id: results
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.minimumHeight: 160
            clip: true
            model: window.view.rows
            ScrollBar.vertical: ScrollBar {}
            delegate: RowLayout {
                id: resultRow
                required property var modelData
                width: results.width - 18
                height: 54
                Label {
                    text: resultRow.modelData.title + "\n" + resultRow.modelData.artist + " · " + resultRow.modelData.release
                    textFormat: Text.PlainText
                    elide: Text.ElideRight
                    Layout.fillWidth: true
                }
                Label {
                    text: resultRow.modelData.available ? "Available" : "Unavailable"
                }
                Button {
                    text: "Add to queue"
                    onClicked: window.bridge.enqueue_row(resultRow.modelData.trackId)
                }
                Button {
                    text: "Replace queue & play"
                    onClicked: window.bridge.play_row(resultRow.modelData.trackId)
                }
            }
            Label {
                anchors.centerIn: parent
                visible: results.count === 0
                text: "No matching library Tracks"
            }
        }
        Label {
            text: "Current: " + window.view.currentTitle + " · " + window.view.status + window.view.pending + " · " + window.view.time + " · Queue position " + (window.view.position < 0 ? "—" : (window.view.position + 1) + "/" + window.view.queue.length)
            textFormat: Text.PlainText
            font.bold: true
            Layout.fillWidth: true
            elide: Text.ElideRight
        }
        Label {
            text: "Track ID: " + window.view.currentId + " · Source: " + window.view.source
            textFormat: Text.PlainText
            Layout.fillWidth: true
            elide: Text.ElideRight
        }
        RowLayout {
            Button {
                id: previousButton
                text: "Previous"
                enabled: window.view.canPrevious
                onClicked: window.bridge.command("previous")
            }
            Button {
                text: "Play / resume"
                onClicked: window.bridge.command("play")
            }
            Button {
                text: "Pause"
                onClicked: window.bridge.command("pause")
            }
            Button {
                text: "Stop"
                onClicked: window.bridge.command("stop")
            }
            Button {
                id: nextButton
                text: "Next"
                enabled: window.view.canNext
                onClicked: window.bridge.command("next")
            }
            Button {
                visible: !window.view.realAudio
                text: window.view.failureArmed ? "Failure ARMED — cancel" : "Fail next engine call"
                onClicked: window.bridge.toggle_failure()
            }
        }
        RowLayout {
            Label {
                text: "Volume"
            }
            Slider {
                id: volumeSlider
                from: 0
                to: 100
                stepSize: 1
                value: window.view.volume * 100
                onMoved: window.bridge.set_volume(value / 100)
                Layout.fillWidth: true
            }
            Label {
                text: Math.round(window.view.volume * 100) + "%"
            }
        }
        RowLayout {
            Label {
                text: "Queue · all entries retained · current selection marked ▶ · no wrapping"
                Layout.fillWidth: true
                wrapMode: Text.Wrap
            }
            Button {
                id: clearButton
                text: "Clear queue"
                enabled: window.view.queue.length > 0
                onClicked: window.bridge.clear_queue()
            }
        }
        ListView {
            id: queue
            Layout.fillWidth: true
            Layout.preferredHeight: 105
            clip: true
            model: window.view.queue
            ScrollBar.vertical: ScrollBar {}
            delegate: Label {
                required property var modelData
                required property int index
                width: queue.width - 18
                height: 26
                text: (modelData.current ? "▶ " : "   ") + (index + 1) + ". " + modelData.title
                textFormat: Text.PlainText
                font.bold: modelData.current
                elide: Text.ElideRight
            }
        }
        Label {
            text: window.view.error ? "Error: " + window.view.error : "No command error"
            textFormat: Text.PlainText
            color: window.view.error ? "#b00020" : "#333333"
            Layout.fillWidth: true
            wrapMode: Text.Wrap
        }
        Label {
            text: "State update " + window.view.revision + " · " + window.view.outcome
            textFormat: Text.PlainText
        }
        Label {
            visible: !window.view.realAudio
            text: "Last 12 engine calls (Failed means output is unknown; recovery stops before restarting):"
        }
        ScrollView {
            visible: !window.view.realAudio
            Layout.fillWidth: true
            Layout.preferredHeight: 110
            TextArea {
                text: window.view.engineCalls
                readOnly: true
                textFormat: TextEdit.PlainText
                selectByMouse: true
            }
        }
    }
}
