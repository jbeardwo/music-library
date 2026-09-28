pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

ApplicationWindow {
    id: window
    width: 1180
    height: 740
    minimumWidth: 800
    minimumHeight: 480
    visible: true
    title: window.view.realAudio ? "Music Library" : "Music Library — Demo (no audio)"
    // One dynamic context object; the Rust/QML smoke test checks this boundary.
    // qmllint disable unqualified
    readonly property var bridge: diagnostic
    // qmllint enable unqualified
    readonly property var library: window.bridge.browse_snapshot
    Component.onCompleted: { window.bridge.browse_action("refresh", 0, ""); syncQueue(); }
    property string queueSignature: ""
    ListModel { id: queueRows; dynamicRoles: true }
    function syncQueue() {
        const signature = JSON.stringify(view.queue);
        if (signature === queueSignature) return;
        queueSignature = signature;
        queueRows.clear();
        for (const row of view.queue) queueRows.append({rowData: row});
    }
    onViewChanged: syncQueue()
    readonly property var view: window.bridge.snapshot
    readonly property var matchingView: window.bridge.matching_snapshot
    readonly property var catalogView: window.bridge.catalog_snapshot
    readonly property var spotifyPlayback: window.bridge.spotify_playback_snapshot
    property string openedSpotifyAuthorization: ""
    property string spotifyChoicesSignature: ""
    property var spotifyChoices: []
    Connections {
        target: window.bridge
        function onResolver_dialog_requested() { spotifyPlaybackDialog.open(); }
    }
    onSpotifyPlaybackChanged: {
        const signature = spotifyPlayback.resolutionGeneration + JSON.stringify(spotifyPlayback.resolutionChoices);
        if (signature !== spotifyChoicesSignature) {
            spotifyChoicesSignature = signature;
            spotifyChoices = spotifyPlayback.resolutionChoices;
            spotifySongChoice.currentIndex = -1;
        }
        if (spotifyPlayback.url.length > 0 && spotifyPlayback.url !== openedSpotifyAuthorization) {
            openedSpotifyAuthorization = spotifyPlayback.url;
            Qt.openUrlExternally(spotifyPlayback.url);
        }
    }

    Dialog {
        id: spotifyPlaybackDialog
        title: "Spotify Playback — separate from catalog and local audio"
        width: Math.min(780, window.width - 30)
        anchors.centerIn: parent
        modal: false
        standardButtons: Dialog.Close
        onOpened: window.bridge.spotify_playback_action("visible", "true")
        onClosed: {
            window.bridge.spotify_playback_action("visible", "false");
            window.bridge.spotify_resolve("cancel", -1);
        }
        ColumnLayout {
            width: parent.width
            RowLayout {
                Button {
                    text: "Connect Spotify Playback"
                    enabled: window.spotifyPlayback.status !== "Authorizing"
                    onClicked: window.bridge.spotify_playback_action("connect", "")
                }
                Button {
                    text: "Cancel authorization"
                    visible: window.spotifyPlayback.status === "Authorizing"
                    onClicked: window.bridge.spotify_playback_action("cancel", "")
                }
                Label { text: "Status: " + window.spotifyPlayback.status }
            }
            Label {
                text: window.spotifyPlayback.error
                textFormat: Text.PlainText
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
            Button {
                text: "Open authorization in browser"
                visible: window.spotifyPlayback.url.length > 0
                onClicked: Qt.openUrlExternally(window.spotifyPlayback.url)
            }
            TextArea {
                visible: window.spotifyPlayback.url.length > 0
                text: window.spotifyPlayback.url
                readOnly: true
                selectByMouse: true
                wrapMode: TextEdit.WrapAnywhere
                Layout.fillWidth: true
                Layout.maximumHeight: 110
            }
            Label {
                text: "Open Spotify on this computer, then explicitly select its desktop device."
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
            RowLayout {
                ComboBox {
                    id: spotifyDevice
                    Layout.fillWidth: true
                    model: window.spotifyPlayback.devices
                    textRole: "label"
                    currentIndex: -1
                    displayText: {
                        for (let i = 0; i < window.spotifyPlayback.devices.length; ++i) {
                            if (window.spotifyPlayback.devices[i].id === window.spotifyPlayback.selected)
                                return window.spotifyPlayback.devices[i].label;
                        }
                        return "Select a Spotify device…";
                    }
                    onActivated: {
                        const device = window.spotifyPlayback.devices[currentIndex];
                        window.bridge.spotify_playback_action("device", device.id);
                    }
                }
                Button {
                    text: "Refresh devices"
                    onClicked: window.bridge.spotify_playback_action("refresh", "")
                }
            }
            Label {
                text: "Selected library Track: " + (window.spotifyPlayback.title || "Use Spotify… beside a library Track")
                textFormat: Text.PlainText
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
            Label {
                text: window.spotifyPlayback.available ? "Spotify association: available" : "No Spotify playback association"
            }
            Label {
                text: window.spotifyPlayback.songUri
                visible: window.spotifyPlayback.available
                textFormat: Text.PlainText
            }
            Button {
                text: window.spotifyPlayback.resolutionPending ? "Searching Spotify catalog…" : "Search Spotify for this Track"
                enabled: !window.spotifyPlayback.available && !window.spotifyPlayback.resolutionPending && window.spotifyPlayback.title.length > 0
                onClicked: window.bridge.spotify_resolve("search", -1)
            }
            Label {
                text: window.spotifyPlayback.resolutionMessage
                textFormat: Text.PlainText
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
            Button {
                text: "Show all Spotify results"
                visible: window.spotifyPlayback.resolutionCanShowAll || false
                enabled: !window.spotifyPlayback.resolutionPending
                onClicked: window.bridge.spotify_resolve("show-all", -1)
            }
            ComboBox {
                id: spotifySongChoice
                Layout.fillWidth: true
                model: window.spotifyChoices
                currentIndex: -1
                visible: window.spotifyChoices.length > 0
                displayText: currentIndex < 0 ? "Choose a Spotify song explicitly…" : currentText
            }
            Label {
                visible: spotifySongChoice.currentIndex >= 0
                text: spotifySongChoice.currentText
                textFormat: Text.PlainText
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
            RowLayout {
                visible: window.spotifyChoices.length > 0 || window.spotifyPlayback.resolutionPending
                Button {
                    text: "Confirm Spotify association"
                    enabled: spotifySongChoice.currentIndex >= 0 && !window.spotifyPlayback.resolutionPending
                    onClicked: window.bridge.spotify_resolve("confirm", spotifySongChoice.currentIndex)
                }
                Button {
                    text: "Cancel selection"
                    onClicked: window.bridge.spotify_resolve("cancel", -1)
                }
            }
            Label { text: window.spotifyPlayback.resolutionCounts }
            RowLayout {
                Button {
                    text: "Play / Resume"
                    enabled: window.spotifyPlayback.available && window.spotifyPlayback.selected.length > 0
                    onClicked: window.bridge.spotify_playback_action("play", "")
                }
                Button {
                    text: "Pause"
                    enabled: window.spotifyPlayback.selected.length > 0
                    onClicked: window.bridge.spotify_playback_action("pause", "")
                }
                SpinBox { id: spotifySeek; from: 0; to: 86400; editable: true }
                Button {
                    text: "Seek (seconds)"
                    enabled: window.spotifyPlayback.selected.length > 0
                    onClicked: window.bridge.spotify_playback_action("seek", String(spotifySeek.value * 1000))
                }
            }
            Label {
                text: "Observed: " + window.spotifyPlayback.observed
                textFormat: Text.PlainText
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
            Label { text: window.spotifyPlayback.requests }
        }
    }

    // Keep delegate identity and display order independent from queue completion.
    ListModel {
        id: matchingModel
        dynamicRoles: true
    }
    onMatchingViewChanged: syncMatchingRows(matchingView)
    function syncMatchingRows(rows) {
        const positions = Object.create(null);
        for (let i = 0; i < matchingModel.count; ++i)
            positions[matchingModel.get(i).albumKey] = i;
        for (let sourceIndex = 0; sourceIndex < rows.length; ++sourceIndex) {
            const row = rows[sourceIndex];
            const index = positions[row.albumId];
            const encoded = JSON.stringify(row);
            if (index === undefined) {
                positions[row.albumId] = matchingModel.count;
                matchingModel.append({
                    albumKey: row.albumId,
                    rowData: row,
                    encoded: encoded,
                    expanded: false,
                    sourceIndex: sourceIndex
                });
            } else {
                if (matchingModel.get(index).encoded !== encoded) {
                    matchingModel.setProperty(index, "rowData", row);
                    matchingModel.setProperty(index, "encoded", encoded);
                }
                if (matchingModel.get(index).sourceIndex !== sourceIndex)
                    matchingModel.setProperty(index, "sourceIndex", sourceIndex);
            }
        }
    }


    Dialog {
        id: matchingDialog
        title: "Local Album matching [" + window.bridge.matching_provider.name + "] — " + (window.bridge.matching_provider.paused ? "unavailable; retrying automatically" : window.bridge.matching_provider.processing ? "processing; " + (window.bridge.matching_provider.queued - 1) + " waiting" : window.bridge.matching_provider.queued > 0 ? window.bridge.matching_provider.queued + " waiting" : "queue idle")
        width: Math.min(window.width - 40, 900)
        height: 400
        anchors.centerIn: parent
        standardButtons: Dialog.Close
        contentItem: ListView {
            id: matchingList
            model: matchingModel
            clip: true
            delegate: ColumnLayout {
                id: matchRow
                required property var rowData
                required property int sourceIndex
                required property int index
                required property bool expanded
                width: ListView.view.width
                RowLayout {
                    Layout.fillWidth: true
                    Button {
                        text: matchRow.expanded ? "▾" : "▸"
                        onClicked: matchingModel.setProperty(matchRow.index, "expanded", !matchRow.expanded)
                        Accessible.name: "Expand or collapse local Tracks"
                    }
                    Label {
                        text: window.matchingLabel(matchRow.rowData) + (matchRow.rowData.recordingSummary ? "\n" + matchRow.rowData.recordingSummary : "")
                        textFormat: Text.PlainText
                        wrapMode: Text.Wrap
                        Layout.fillWidth: true
                    }
                    ComboBox {
                        id: artistChoice
                        Layout.preferredWidth: 260
                        visible: matchRow.rowData.artists.length > 0
                        model: matchRow.rowData.artists
                        textRole: "label"
                    }
                    Button {
                        text: "Use Artist"
                        visible: matchRow.rowData.artists.length > 0
                        enabled: !matchRow.rowData.pending && artistChoice.currentIndex >= 0
                        onClicked: window.bridge.choose_artist(matchRow.sourceIndex, artistChoice.currentIndex)
                    }
                    Button {
                        text: "Recordings…"
                        visible: !!matchRow.rowData.recordingDetails
                        onClicked: {
                            recordingDialog.details = matchRow.rowData.recordingDetails;
                            recordingDialog.open();
                        }
                    }
                    Button {
                        text: "Retry Match"
                        enabled: !matchRow.rowData.pending
                        onClicked: window.bridge.retry_match(matchRow.sourceIndex)
                    }
                }
                Repeater {
                    model: matchRow.expanded ? matchRow.rowData.tracks : []
                    delegate: RowLayout {
                        id: trackRow
                        required property var modelData
                        Layout.fillWidth: true
                        Layout.leftMargin: 24
                        Label {
                            objectName: "program-track-" + trackRow.modelData.trackId
                            Layout.fillWidth: true
                            textFormat: Text.PlainText
                            wrapMode: Text.Wrap
                            text: "Local: " + trackRow.modelData.localTitle + " → " + (trackRow.modelData.matchedTitle ? "Matched: " + trackRow.modelData.matchedTitle + " (" + trackRow.modelData.status + (trackRow.modelData.songIdentityAmbiguous ? "; provider song identity unresolved" : trackRow.modelData.recordingStatus === "NotProvided" ? "; provider song identified" : "; Recording: " + trackRow.modelData.recordingStatus) + ")" : trackRow.modelData.status + (trackRow.modelData.storedIdentity ? " [" + trackRow.modelData.storedIdentity + "]" : ""))
                        }
                        Button {
                            objectName: "choose-track-" + trackRow.modelData.trackId
                            text: trackRow.modelData.matchedTitle ? "Choose Recording" : "Choose Match"
                            visible: !!trackRow.modelData.canChoose && !matchRow.rowData.equivalent
                            onClicked: {
                                manualTrackDialog.albumKey = matchRow.rowData.albumId;
                                manualTrackDialog.trackKey = trackRow.modelData.trackId;
                                window.bridge.choose_track(matchRow.rowData.albumId, trackRow.modelData.trackId);
                                manualTrackDialog.open();
                            }
                        }
                        Button {
                            objectName: "clear-track-" + trackRow.modelData.trackId
                            text: "Clear manual match"
                            visible: !!trackRow.modelData.manual
                            onClicked: window.bridge.clear_track_choice(matchRow.rowData.albumId, trackRow.modelData.trackId)
                        }
                    }
                }
            }
        }
    }

    Dialog {
        id: manualTrackDialog
        property string albumKey: ""
        property string trackKey: ""
        objectName: "manual-track-dialog"
        title: "Choose Track within the Album"
        modal: true
        width: Math.min(window.width - 60, 750)
        anchors.centerIn: parent
        onClosed: {
            window.bridge.cancel_track_choice();
            Qt.callLater(function () {
                window.focusTrackAction(manualTrackDialog.albumKey, manualTrackDialog.trackKey);
            });
        }
        ColumnLayout {
            anchors.fill: parent
            Label {
                text: "Local: " + window.bridge.manual_snapshot.localTitle
                textFormat: Text.PlainText
            }
            Label {
                text: window.bridge.manual_snapshot.pending ? "Loading Album candidates…" : window.bridge.manual_snapshot.error
                visible: text.length > 0
                wrapMode: Text.Wrap
                Layout.fillWidth: true
                textFormat: Text.PlainText
            }
            ComboBox {
                id: manualTrackChoice
                objectName: "manual-track-choice"
                Layout.fillWidth: true
                model: window.bridge.manual_snapshot.candidates
                textRole: "label"
                currentIndex: -1
                onModelChanged: currentIndex = -1
                displayText: currentIndex < 0 ? "Select a provider Track…" : currentText
            }
            RowLayout {
                Button {
                    id: manualTrackConfirm
                    objectName: "confirm-track-choice"
                    text: "Confirm"
                    enabled: manualTrackChoice.currentIndex >= 0 && !window.bridge.manual_snapshot.pending
                    onClicked: {
                        if (window.bridge.confirm_track(manualTrackChoice.currentIndex))
                            manualTrackDialog.close();
                    }
                }
                Button {
                    text: "Cancel"
                    onClicked: manualTrackDialog.close()
                }
                Button {
                    text: "Retry Matching"
                    visible: window.bridge.matching_provider.paused
                    enabled: !window.bridge.matching_provider.probe
                    onClicked: window.bridge.retry_matching()
                }
            }
        }
    }

    function focusTrackAction(album, track) {
        if (!matchingDialog.visible)
            return;
        function visit(item) {
            if (!item)
                return false;
            if (item.visible && (item.objectName === "choose-track-" + track || item.objectName === "clear-track-" + track)) {
                item.forceActiveFocus();
                return true;
            }
            if (item.children)
                for (const child of item.children)
                    if (visit(child))
                        return true;
            return false;
        }
        for (let i = 0; i < matchingModel.count; ++i)
            if (matchingModel.get(i).albumKey === album) {
                visit(matchingList.itemAtIndex(i));
                return;
            }
    }

    Dialog {
        id: recordingDialog
        title: "Recording enrichment"
        property string details: ""
        width: Math.min(window.width - 40, 800)
        height: 450
        anchors.centerIn: parent
        standardButtons: Dialog.Close
        contentItem: ScrollView {
            TextArea {
                text: recordingDialog.details
                textFormat: TextEdit.PlainText
                readOnly: true
                selectByMouse: true
                wrapMode: TextEdit.Wrap
            }
        }
    }

    function matchingLabel(row) {
        const local = row.title + (row.localArtist ? " — " + row.localArtist : "");
        if (!row.matchedTitle)
            return local + " — " + row.status + (row.providerHistory ? "\n" + row.providerHistory : "");
        const provider = row.matchedTitle + (row.matchedArtist ? " — " + row.matchedArtist : "");
        const label = row.matchedClose ? "Matched (close): " : "Matched: ";
        return "Local: " + local + "\n" + label + provider + (row.provider ? " [" + row.provider + "]" : "");
    }

    Dialog {
        id: catalogDialog
        title: "Add Music"
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
            check(window.spotifyPlayback.status === "Disconnected", "playback OAuth independent from catalog");
            window.bridge.spotify_playback_action("track", view.rows[0].trackId);
            check(!window.spotifyPlayback.available, "no playback lookup/search without persisted association");
            check(window.spotifyPlayback.resolutionCounts === "Explicit catalog resolution: 0 token / 0 API requests", "selecting Track does not search catalog");
            check(spotifySongChoice.currentIndex === -1, "song chooser requires explicit selection");
            check(view.status === "Stopped", "Spotify selection does not control local playback");
            check(view.volume === 1 && volumeSlider.value === 100, "default volume");
            volumeSlider.value = 35;
            volumeSlider.moved();
            check(view.volume === 0.35 && view.status === "Stopped", "stopped volume");
            window.bridge.set_volume(1);
            check(view.volume === 1, "restore volume");
            check(view.rows.length === 20, "bounded initial page");
            check(window.library.panes.length === 3 && window.library.panes[2].rows.length === 45, "three pane binding");
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
            check(view.rows.length === 0 && !view.hasNext, "empty results");
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

    Timer {
        interval: 250
        repeat: true
        running: window.view.clockRunning
        onTriggered: window.bridge.refresh_clock()
    }
    palette.window: "#f6f5f3"
    palette.highlight: "#96506d"
    palette.highlightedText: "white"
    color: palette.window

    property int contextPane: 0
    property string contextId: ""
    Menu {
        id: libraryMenu
        MenuItem { text: "Play now"; enabled: !window.library.pending; onTriggered: window.bridge.browse_action("play", window.contextPane, window.contextId) }
        MenuItem { text: "Add to queue"; enabled: !window.library.pending; onTriggered: window.bridge.browse_action("append", window.contextPane, window.contextId) }
    }

    component LibraryPane: ColumnLayout {
        id: pane
        required property int paneIndex
        required property string heading
        readonly property var pageData: window.library.panes[paneIndex]
        readonly property string selectedId: paneIndex === 0 ? window.library.artist : paneIndex === 1 ? window.library.album : songId
        property string songId: ""
        property string rowsSignature: ""
        property int navigation: window.library.navigation
        onNavigationChanged: Qt.callLater(function() {
            const target = pane.paneIndex === 0 ? window.library.artist : pane.paneIndex === 1 ? window.library.album : window.library.song;
            if (pane.paneIndex === 2) pane.songId = target;
            for (let i = 0; i < pane.pageData.rows.length; ++i) {
                if (pane.pageData.rows[i].id === target) {
                    list.currentIndex = i;
                    list.positionViewAtIndex(i, ListView.Contain);
                    if ((pane.paneIndex === 2 && window.library.song.length > 0)
                        || (pane.paneIndex === 1 && window.library.song.length === 0 && window.library.album.length > 0)
                        || (pane.paneIndex === 0 && window.library.album.length === 0))
                        list.forceActiveFocus();
                    break;
                }
            }
        })
        ListModel { id: paneRows; dynamicRoles: true }
        function syncRows() {
            const signature = JSON.stringify(pageData.rows);
            if (signature === rowsSignature) return;
            rowsSignature = signature;
            paneRows.clear();
            for (const row of pageData.rows) paneRows.append({rowData: row});
            songId = "";
            list.currentIndex = -1;
            list.positionViewAtBeginning();
        }
        onPageDataChanged: syncRows()
        Component.onCompleted: syncRows()
        Layout.fillHeight: true
        spacing: 0
        function selectRow(index) {
            if (index < 0 || index >= pageData.rows.length) return;
            list.currentIndex = index;
            songId = pageData.rows[index].id;
            window.bridge.browse_action("select", paneIndex, songId);
        }
        function playRow(index) {
            if (index >= 0 && index < pageData.rows.length)
                window.bridge.browse_action("play", paneIndex, pageData.rows[index].id);
        }
        RowLayout {
            Layout.fillWidth: true
            Layout.preferredHeight: 44
            Label { text: pane.heading; font.pixelSize: 13; font.bold: true; font.letterSpacing: 1.5; Layout.fillWidth: true }
            ToolButton {
                text: "Show all"
                visible: pane.paneIndex < 2 && pane.selectedId.length > 0
                Accessible.name: "Clear " + pane.heading.toLowerCase() + " selection"
                onClicked: { window.bridge.browse_action("select", pane.paneIndex, ""); list.currentIndex = -1; }
            }
        }
        Rectangle { Layout.fillWidth: true; implicitHeight: 1; color: "#d9d6d3" }
        ListView {
            id: list
            objectName: "libraryPane" + pane.paneIndex
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: paneRows
            currentIndex: -1
            activeFocusOnTab: true
            keyNavigationEnabled: false
            boundsBehavior: Flickable.StopAtBounds
            ScrollBar.vertical: ScrollBar {}
            Keys.onDownPressed: { pane.selectRow(Math.min(count - 1, currentIndex + 1)); positionViewAtIndex(currentIndex, ListView.Contain); }
            Keys.onUpPressed: { pane.selectRow(Math.max(0, currentIndex - 1)); positionViewAtIndex(currentIndex, ListView.Contain); }
            Keys.onReturnPressed: pane.playRow(currentIndex)
            Keys.onEnterPressed: pane.playRow(currentIndex)
            Keys.onEscapePressed: {
                if (pane.paneIndex < 2) window.bridge.browse_action("select", pane.paneIndex, "");
                else pane.songId = "";
                currentIndex = -1;
            }
            delegate: Rectangle {
                id: row
                required property var rowData
                readonly property var modelData: rowData
                required property int index
                width: list.width
                height: pane.paneIndex === 0 ? 34 : 48
                color: pane.selectedId === modelData.id ? "#e8d9e0" : mouse.containsMouse ? "#eeece9" : "transparent"
                border.width: list.activeFocus && list.currentIndex === index ? 1 : 0
                border.color: "#96506d"
                Accessible.role: Accessible.ListItem
                Accessible.name: modelData.title + " " + modelData.subtitle
                Accessible.selected: pane.selectedId === modelData.id
                Column {
                    anchors.left: parent.left; anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter
                    anchors.leftMargin: 9; anchors.rightMargin: 16
                    spacing: 2
                    Label { width: parent.width; text: row.modelData.title || "Untitled"; textFormat: Text.PlainText; elide: Text.ElideRight; font.pixelSize: 14 }
                    Label {
                        width: parent.width
                        visible: pane.paneIndex > 0
                        text: pane.paneIndex === 2 ? row.modelData.subtitle + " · " + row.modelData.track.release : row.modelData.subtitle
                        textFormat: Text.PlainText; elide: Text.ElideRight; font.pixelSize: 11; color: "#68636a"
                    }
                }
                MouseArea {
                    id: mouse
                    anchors.fill: parent
                    acceptedButtons: Qt.LeftButton | Qt.RightButton
                    hoverEnabled: true
                    onClicked: event => {
                        list.forceActiveFocus();
                        if (event.button === Qt.RightButton) {
                            window.contextPane = pane.paneIndex;
                            window.contextId = row.modelData.id;
                            libraryMenu.popup();
                        } else pane.selectRow(row.index);
                    }
                    onDoubleClicked: event => { if (event.button === Qt.LeftButton) pane.playRow(row.index); }
                }
            }
            Label {
                anchors.centerIn: parent
                width: parent.width - 24
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.Wrap
                visible: list.count === 0
                text: pane.paneIndex === 2 ? "No songs in this view" : "No " + pane.heading.toLowerCase() + " in this view"
                color: "#777078"
            }
        }
        RowLayout {
            Layout.fillWidth: true
            visible: pane.pageData.more || pane.pageData.page > 1
            ToolButton { text: "‹"; Accessible.name: "Previous " + pane.heading.toLowerCase() + " page"; enabled: pane.pageData.page > 1 || pane.pageData.anchored; onClicked: window.bridge.browse_action("previous", pane.paneIndex, "") }
            Label { text: "Page " + pane.pageData.page; Layout.fillWidth: true; horizontalAlignment: Text.AlignHCenter; color: "#68636a" }
            ToolButton { text: "›"; Accessible.name: "Next " + pane.heading.toLowerCase() + " page"; enabled: pane.pageData.more; onClicked: window.bridge.browse_action("next", pane.paneIndex, "") }
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0
        RowLayout {
            Layout.fillWidth: true
            Layout.margins: 16
            Label { text: "music"; font.pixelSize: 26; font.weight: Font.Light; Layout.fillWidth: true }
            Button {
                id: query
                Layout.preferredWidth: 260
                text: "Search library…"
                Accessible.name: "Search library"
                onClicked: searchPanel.open()
            }
            Button {
                text: "Add Music"
                onClicked: {
                    if (window.bridge.matching_provider.catalogAddSupported) catalogDialog.open();
                    else addMusicHook.open();
                }
            }
            ToolButton { text: "⋯"; Accessible.name: "Settings and diagnostics"; onClicked: settingsMenu.popup() }
        }
        RowLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.leftMargin: 16; Layout.rightMargin: 16
            spacing: 16
            LibraryPane { paneIndex: 0; heading: "ARTISTS"; Layout.fillWidth: true; Layout.preferredWidth: 230 }
            Rectangle { Layout.fillHeight: true; implicitWidth: 1; color: "#dedbd8" }
            LibraryPane { paneIndex: 1; heading: "ALBUMS"; Layout.fillWidth: true; Layout.preferredWidth: 320 }
            Rectangle { Layout.fillHeight: true; implicitWidth: 1; color: "#dedbd8" }
            LibraryPane { paneIndex: 2; heading: "SONGS"; Layout.fillWidth: true; Layout.preferredWidth: 470 }
        }
        Label {
            visible: text.length > 0
            text: window.library.error || window.view.error || (window.library.pending ? "Preparing queue…" : "")
            textFormat: Text.PlainText
            color: window.library.pending ? "#68636a" : "#9d263d"
            wrapMode: Text.Wrap
            Layout.fillWidth: true; Layout.margins: visible ? 12 : 0
        }
        Rectangle {
            id: player
            Layout.fillWidth: true
            implicitHeight: 104
            color: "#ebe8e5"
            Rectangle { anchors.top: parent.top; width: parent.width; height: 1; color: "#d4cfcc" }
            RowLayout {
                anchors.fill: parent; anchors.margins: 14
                Label { text: window.view.realAudio ? "" : "DEMO · NO AUDIO"; font.pixelSize: 10; color: "#777078"; Layout.preferredWidth: 120 }
                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.maximumWidth: 650
                Button {
                    id: currentTrack
                    Layout.fillWidth: true
                    flat: true
                    Accessible.name: "Now Playing: " + window.view.currentTitle
                    onClicked: { if (queueDrawer.opened) queueDrawer.close(); else queueDrawer.open(); }
                    contentItem: Column {
                        spacing: 4
                        Label { width: parent.width; text: window.view.position < 0 ? "Choose something to play" : window.view.currentTitle; font.pixelSize: 16; textFormat: Text.PlainText; elide: Text.ElideRight; horizontalAlignment: Text.AlignHCenter }
                        Label { width: parent.width; text: window.view.position < 0 ? "Now Playing ⌃" : window.view.currentArtist + " — " + window.view.currentAlbum + "  ⌃"; color: "#68636a"; textFormat: Text.PlainText; elide: Text.ElideRight; horizontalAlignment: Text.AlignHCenter }
                    }
                }
                    RowLayout {
                        Layout.fillWidth: true
                        Label { text: window.view.elapsed; font.pixelSize: 11; Layout.preferredWidth: 42; horizontalAlignment: Text.AlignRight }
                        Slider {
                            id: seekSlider
                            Layout.fillWidth: true
                            from: 0
                            to: Math.max(1, window.view.durationMs)
                            enabled: window.view.seekAvailable
                            Accessible.name: "Seek"
                            property real preview: 0
                            property bool movedDuringPress: false
                            property string dragTrack: ""
                            Binding { restoreMode: Binding.RestoreNone; target: seekSlider; property: "value"; value: window.view.progressMs; when: !seekSlider.pressed }
                            onPressedChanged: {
                                if (pressed) { dragTrack = window.view.currentId; if (!movedDuringPress) preview = value; }
                                else { if (dragTrack === window.view.currentId) window.bridge.seek(preview); movedDuringPress = false; }
                            }
                            onMoved: {
                                movedDuringPress = true;
                                preview = value;
                                if (!pressed) window.bridge.seek(value);
                            }
                        }
                        Label { text: window.view.duration; font.pixelSize: 11; Layout.preferredWidth: 42 }
                    }
                }
                ColumnLayout {
                    RowLayout {
                        Button { id: previousButton; text: "Previous"; Accessible.name: "Previous"; enabled: window.view.canPrevious; onClicked: window.bridge.command("previous") }
                        Button { text: window.view.playing ? "Pause" : "Play"; Accessible.name: window.view.playing ? "Pause" : "Play"; enabled: window.view.position >= 0; onClicked: window.bridge.command(window.view.playing ? "pause" : "play") }
                        Button { id: nextButton; text: "Next"; Accessible.name: "Next"; enabled: window.view.canNext; onClicked: window.bridge.command("next") }
                    }
                    RowLayout {
                        Label { text: "Volume"; font.pixelSize: 11 }
                        Slider {
                            id: volumeSlider
                            from: 0; to: 100; stepSize: 1
                            Layout.preferredWidth: 130
                            Accessible.name: "Volume"
                            enabled: window.view.volumeAvailable
                            property real preview: 100
                            property bool movedDuringPress: false
                            Binding { restoreMode: Binding.RestoreNone; target: volumeSlider; property: "value"; value: window.view.volume * 100; when: !volumeSlider.pressed }
                            onMoved: { movedDuringPress = true; preview = value; if (!pressed) window.bridge.set_volume(value / 100); }
                            onPressedChanged: { if (pressed) { if (!movedDuringPress) preview = value; } else { window.bridge.set_volume(preview / 100); movedDuringPress = false; } }
                            ToolTip.visible: hovered && !enabled
                            ToolTip.text: "This playback device cannot currently change volume"
                        }
                        Label { text: Math.round(volumeSlider.value) + "%"; font.pixelSize: 11; Layout.preferredWidth: 32 }
                    }
                }
            }
        }
    }
    Popup {
        id: queueDrawer
        x: Math.max(16, (window.width - width) / 2)
        y: player.y - height
        width: Math.min(820, window.width - 32)
        height: Math.min(380, player.y - 90)
        padding: 16
        focus: true
        closePolicy: Popup.CloseOnEscape
        enter: Transition {
            ParallelAnimation {
                NumberAnimation { property: "opacity"; from: 0; to: 1; duration: 140 }
                NumberAnimation { property: "y"; from: player.y; to: player.y - queueDrawer.height; duration: 140; easing.type: Easing.OutCubic }
            }
        }
        onOpened: { window.bridge.queue_window(Math.max(0, window.view.position)); queue.positionViewAtIndex(Math.max(0, window.view.position - window.view.queueOffset), ListView.Contain); queue.forceActiveFocus(); }
        onClosed: currentTrack.forceActiveFocus()
        ColumnLayout {
            anchors.fill: parent
            RowLayout {
                Label { text: "NOW PLAYING · " + window.view.queueTotal + " songs"; font.bold: true; Layout.fillWidth: true }
                Button { id: clearButton; text: "Clear queue"; enabled: window.view.queueTotal > 0 && !window.library.pending; onClicked: window.bridge.clear_queue() }
                ToolButton { text: "✕"; Accessible.name: "Close Now Playing"; onClicked: queueDrawer.close() }
            }
            RowLayout {
                visible: window.view.queueTotal > 200
                ToolButton { text: "Previous page"; enabled: window.view.queueOffset > 0; onClicked: window.bridge.queue_window(window.view.queueOffset - 200) }
                ToolButton { text: "Current song"; onClicked: window.bridge.queue_window(window.view.position) }
                ToolButton { text: "Next page"; enabled: window.view.queueOffset + 200 < window.view.queueTotal; onClicked: window.bridge.queue_window(window.view.queueOffset + 200) }
            }
            ListView {
                id: queue
                Layout.fillWidth: true; Layout.fillHeight: true
                clip: true
                model: queueRows
                activeFocusOnTab: true
                ScrollBar.vertical: ScrollBar {}
                delegate: Rectangle {
                    id: queueRow
                    required property var rowData
                    readonly property var modelData: rowData
                    required property int index
                    width: queue.width; height: 46
                    color: modelData.current ? "#e8d9e0" : "transparent"
                    Column {
                        anchors.fill: parent; anchors.margins: 5
                        Label { width: parent.width; text: (queueRow.modelData.current ? "▶ " : "") + (window.view.queueOffset + queueRow.index + 1) + ". " + queueRow.modelData.title; textFormat: Text.PlainText; elide: Text.ElideRight }
                        Label { width: parent.width; text: queueRow.modelData.artist + " — " + queueRow.modelData.album; font.pixelSize: 11; color: "#68636a"; textFormat: Text.PlainText; elide: Text.ElideRight }
                    }
                }
                Label { anchors.centerIn: parent; visible: queue.count === 0; text: "Your queue is empty" }
            }
        }
    }
    Dialog {
        id: searchPanel
        objectName: "librarySearchPanel"
        title: "Search library"
        anchors.centerIn: parent
        width: Math.min(820, window.width - 48)
        height: Math.min(620, window.height - 80)
        modal: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        property int filter: 0
        readonly property var results: window.bridge.local_search_snapshot
        function request() {
            searchDelay.stop();
            searchResults.currentIndex = -1;
            window.bridge.search_library(searchText.text, filter);
        }
        function activate(index) {
            if (!searchDelay.running && window.bridge.navigate_search_result(index)) close();
        }
        onOpened: { searchText.forceActiveFocus(); searchText.selectAll(); request(); }
        onClosed: { searchDelay.stop(); window.bridge.close_search(); }
        Timer { id: searchDelay; interval: 150; onTriggered: searchPanel.request() }
        ColumnLayout {
            anchors.fill: parent
            TextField {
                id: searchText
                objectName: "librarySearchText"
                Layout.fillWidth: true
                placeholderText: "Artist, Album or Song"
                maximumLength: 256
                Accessible.name: "Search query"
                onTextChanged: if (searchPanel.opened) searchDelay.restart()
                Keys.onDownPressed: { searchResults.forceActiveFocus(); searchResults.currentIndex = 0; }
                onAccepted: if (searchResults.count > 0) searchPanel.activate(Math.max(0, searchResults.currentIndex))
            }
            RowLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                ColumnLayout {
                    Layout.alignment: Qt.AlignTop
                    Layout.preferredWidth: 120
                    Label { text: "FILTERS"; font.bold: true; font.pixelSize: 12 }
                    ButtonGroup { id: searchTypes }
                    Repeater {
                        model: ["All", "Artists", "Albums", "Songs", "Playlists"]
                        RadioButton {
                            required property int index
                            required property string modelData
                            text: modelData
                            ButtonGroup.group: searchTypes
                            checked: searchPanel.filter === index
                            onClicked: { searchPanel.filter = index; searchPanel.request(); }
                        }
                    }
                }
                Rectangle { Layout.fillHeight: true; implicitWidth: 1; color: "#d9d6d3" }
                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    Label {
                        text: searchPanel.results.error || (searchPanel.results.busy || searchDelay.running ? "Searching…" : "Best matches · up to 40 per type")
                        wrapMode: Text.WordWrap
                        Layout.fillWidth: true
                        color: searchPanel.results.error ? "#a03030" : "#68636a"
                    }
                    ListView {
                        id: searchResults
                        objectName: "librarySearchResults"
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        clip: true
                        enabled: !searchDelay.running
                        model: searchPanel.results.rows
                        currentIndex: -1
                        keyNavigationEnabled: true
                        ScrollBar.vertical: ScrollBar {}
                        section.property: "section"
                        section.delegate: Label {
                            required property string section
                            text: section
                            font.bold: true; font.pixelSize: 12
                            topPadding: 16; bottomPadding: 6
                        }
                        Keys.onReturnPressed: searchPanel.activate(currentIndex)
                        Keys.onEnterPressed: searchPanel.activate(currentIndex)
                        delegate: ItemDelegate {
                            required property var modelData
                            required property int index
                            width: searchResults.width
                            height: modelData.context.length > 0 ? 50 : 36
                            highlighted: searchResults.currentIndex === index
                            Accessible.name: modelData.title + " " + modelData.context
                            contentItem: Column {
                                Label { text: modelData.title; width: parent.width; elide: Text.ElideRight; textFormat: Text.PlainText }
                                Label { visible: modelData.context.length > 0; text: modelData.context; width: parent.width; elide: Text.ElideRight; font.pixelSize: 11; color: "#68636a"; textFormat: Text.PlainText }
                            }
                            onClicked: searchPanel.activate(index)
                        }
                        Label {
                            anchors.centerIn: parent
                            visible: searchResults.count === 0 && !searchPanel.results.busy
                            text: searchPanel.filter === 4 ? "No playlists yet" : searchText.text.trim().length === 0 ? "Type to search your library" : "No matching library items"
                        }
                    }
                    Label { visible: searchPanel.filter === 0; text: "PLAYLISTS · No playlists yet"; font.pixelSize: 11; color: "#68636a" }
                }
            }
        }
    }

    Dialog {
        id: outputCalibration
        title: "Output calibration"
        anchors.centerIn: parent
        modal: true
        standardButtons: Dialog.Close
        width: Math.min(480, window.width - 40)
        ColumnLayout {
            width: parent.width
            Label {
                text: "Adjust a fixed output difference between sources. This does not normalize Tracks, change relative loudness between recordings, or alter stored audio. 0 dB means no adjustment."
                wrapMode: Text.WordWrap
                Layout.fillWidth: true
            }
            Label { text: "Local output trim" }
            RowLayout {
                SpinBox {
                    from: -240; to: 0; stepSize: 5
                    value: Math.round(window.view.localTrim * 10)
                    editable: true
                    objectName: "localOutputTrim"
                    Accessible.name: "Local output trim"
                    textFromValue: (value, locale) => Number(value / 10).toLocaleString(locale, 'f', 1)
                    valueFromText: (text, locale) => Math.round(Number.fromLocaleString(locale, text) * 10)
                    validator: DoubleValidator { bottom: -24; top: 0; decimals: 1 }
                    onValueModified: window.bridge.output_trims(value / 10, window.view.spotifyTrim)
                    Layout.fillWidth: true
                }
                Label { text: window.view.localTrim.toFixed(1) + " dB"; Layout.preferredWidth: 66 }
            }
            Label { text: "Spotify output trim" }
            RowLayout {
                SpinBox {
                    from: -240; to: 0; stepSize: 5
                    value: Math.round(window.view.spotifyTrim * 10)
                    editable: true
                    Accessible.name: "Spotify output trim"
                    textFromValue: (value, locale) => Number(value / 10).toLocaleString(locale, 'f', 1)
                    valueFromText: (text, locale) => Math.round(Number.fromLocaleString(locale, text) * 10)
                    validator: DoubleValidator { bottom: -24; top: 0; decimals: 1 }
                    onValueModified: window.bridge.output_trims(window.view.localTrim, value / 10)
                    Layout.fillWidth: true
                }
                Label { text: window.view.spotifyTrim.toFixed(1) + " dB"; Layout.preferredWidth: 66 }
            }
            Label { text: "Both controls attenuate only (−24 to 0 dB). Spotify output is rounded to its device's 0–100 scale."; wrapMode: Text.WordWrap; Layout.fillWidth: true }
            Button { text: "Reset trims"; onClicked: window.bridge.output_trims(0, 0) }
        }
    }
    Menu {
        id: settingsMenu
        MenuItem { text: "Output calibration…"; onTriggered: outputCalibration.open() }
        MenuItem { text: "Playback connection…"; onTriggered: spotifyPlaybackDialog.open() }
        MenuItem { text: "Local Album matches…"; onTriggered: matchingDialog.open() }
        MenuItem { text: "Retry matching"; visible: window.bridge.matching_provider.paused; enabled: !window.bridge.matching_provider.probe; onTriggered: window.bridge.retry_matching() }
    }
    Dialog {
        id: addMusicHook
        title: "Add Music"
        anchors.centerIn: parent
        standardButtons: Dialog.Close
        Label { text: "Adding music is not available with this catalog configuration yet."; wrapMode: Text.Wrap; width: 380 }
    }
    // Existing diagnostic assertions remain available without occupying the player.
    Label { visible: false; text: "Current: " + window.view.currentTitle + " · " + window.view.status + window.view.pending + " · " + window.view.time }
    Label { visible: false; text: "State update " + window.view.revision + " · " + window.view.outcome }
}
