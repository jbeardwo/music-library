pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Window
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Qt.labs.platform as PlatformDialogs

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
    // Independent table widths live with the window for the current session.
    property var songsColumnWidths: []
    property var reviewColumnWidths: []
    property var playlistColumnWidths: []
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

    function openSpotifyConnection(trackId) {
        if (trackId) window.bridge.spotify_playback_action("track", trackId);
        spotifyPlaybackDialog.open();
    }

    Dialog {
        id: artistEquivalenceDialog
        objectName: "artistEquivalenceConfirmation"
        title: "Treat these as the same artist?"
        modal: true
        anchors.centerIn: parent
        width: Math.min(550, window.width - 30)
        standardButtons: Dialog.Ok | Dialog.Cancel
        property var proposal: JSON.parse(window.spotifyPlayback.artistProposalJson || "null")
        onAccepted: window.bridge.spotify_artist_equivalence("confirm", -1)
        onRejected: window.bridge.spotify_artist_equivalence("cancel", -1)
        contentItem: Label {
            text: artistEquivalenceDialog.proposal ? "Local: " + artistEquivalenceDialog.proposal.local_name + "\nArtist ID: " + artistEquivalenceDialog.proposal.local_id + "\n\nSpotify: " + artistEquivalenceDialog.proposal.candidate_name + "\nSpotify Artist ID: " + artistEquivalenceDialog.proposal.candidate_identity.external_id + "\n\nThis will allow tracks and albums credited to these Artist identities to be considered compatible during matching. Names and credits stay unchanged. Other matching evidence is still required.\n\nAfter confirmation, only this Album will be re-evaluated." : ""
            textFormat: Text.PlainText; wrapMode: Text.Wrap
        }
    }
    Dialog {
        id: spotifyPlaybackDialog
        title: "Spotify connection"
        width: Math.min(1040, window.width - 30)
        height: Math.min(900, window.height - 30)
        anchors.centerIn: parent
        modal: false
        standardButtons: Dialog.Close
        onOpened: window.bridge.spotify_playback_action("visible", "true")
        onClosed: {
            spotifySearchArtist.text = "";
            window.bridge.spotify_playback_action("visible", "false");
            window.bridge.spotify_resolve("cancel", -1);
        }
        contentItem: ScrollView {
            contentWidth: availableWidth
            clip: true
            ColumnLayout {
                width: spotifyPlaybackDialog.availableWidth
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
                Label { text: "Track ID: " + (window.spotifyPlayback.trackId || ""); textFormat: Text.PlainText; Layout.fillWidth: true; wrapMode: Text.WrapAnywhere }
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
                Label { text: "Manually marked as not on Spotify"; visible: window.spotifyPlayback.manuallyExcluded || false }
                Button { objectName: "diagnosticSpotifyMark"; text: "Mark as not on Spotify"; ToolTip.visible: hovered; ToolTip.text: "Skips Spotify reconciliation until you choose Check Spotify again."; visible: !window.spotifyPlayback.available && !window.spotifyPlayback.manuallyExcluded; onClicked: window.bridge.browse_action("spotify-mark",2,window.spotifyPlayback.trackId) }
                Button { objectName: "diagnosticSpotifyCheck"; text: "Check Spotify again"; visible: window.spotifyPlayback.manuallyExcluded || false; onClicked: window.bridge.browse_action("spotify-check",2,window.spotifyPlayback.trackId) }
                TextField {
                    id: spotifySearchArtist
                    objectName: "spotifySearchArtist"
                    placeholderText: "Search Artist name (optional; does not change metadata)"
                    Layout.fillWidth: true
                    onVisibleChanged: if (!visible) text = ""
                }
                Button {
                    objectName: "spotifyConnectionSearch"
                    text: window.spotifyPlayback.resolutionPending ? "Searching Spotify catalog…" : "Search Spotify for this Track"
                    enabled: !window.spotifyPlayback.available && !window.spotifyPlayback.manuallyExcluded && !window.spotifyPlayback.resolutionPending && window.spotifyPlayback.title.length > 0
                    onClicked: window.bridge.spotify_resolve(spotifySearchArtist.text.trim().length > 0 ? "search-artist:" + spotifySearchArtist.text.trim() : "search", -1)
                }
                Button {
                    text: "Search Spotify again (refresh provider results)"
                    enabled: !window.spotifyPlayback.available && !window.spotifyPlayback.manuallyExcluded && !window.spotifyPlayback.resolutionPending
                    onClicked: window.bridge.spotify_resolve("refresh", -1)
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
                ColumnLayout {
                    id: spotifyComparison
                    objectName: "spotifyComparison"
                    Layout.fillWidth: true
                    property var trace: JSON.parse(window.spotifyPlayback.comparisonJson || '{"candidates":[],"programs":[],"album_candidates":[],"candidate_count":0}')
                    property var evaluation: trace.candidates.length > 0 ? trace.candidates[Math.max(0, spotifySongChoice.currentIndex)] : null
                    Label {
                        objectName: "spotifyCandidateCount"
                        visible: spotifyComparison.trace.candidate_count > 0
                        text: spotifyComparison.trace.candidate_count + (spotifyComparison.trace.candidate_count === 1 ? " candidate found" : " candidates found") + (spotifyComparison.trace.more_candidates ? " · more results exist" : " · complete bounded page")
                    }
                    Label { visible: spotifyComparison.evaluation !== null; text: "KNOWN TRACK / ALBUM                         SPOTIFY CANDIDATE"; font.bold: true }
                    Repeater {
                        model: spotifyComparison.evaluation ? spotifyComparison.evaluation.fields : []
                        delegate: ColumnLayout {
                            required property var modelData
                            Layout.fillWidth: true
                            objectName: "spotifyComparisonField" + modelData.label
                            RowLayout {
                                Layout.fillWidth: true
                                Label { text: modelData.label; Layout.preferredWidth: 105; font.bold: true }
                                Label { objectName: "spotifyLocalValue"; text: modelData.local; textFormat: Text.PlainText; wrapMode: Text.WrapAnywhere; Layout.fillWidth: true; Layout.preferredWidth: 1 }
                                Label { objectName: "spotifyCandidateValue"; text: modelData.candidate; textFormat: Text.PlainText; wrapMode: Text.WrapAnywhere; Layout.fillWidth: true; Layout.preferredWidth: 1 }
                                Label {
                                    text: modelData.status === "equivalent" ? "✓" : modelData.status === "normalized" ? "≈" : modelData.status === "conflict" ? "Conflict" : modelData.status === "unknown" ? "—" : "Warning"
                                    color: modelData.status === "conflict" ? "#b54747" : modelData.status === "warning" ? "#986927" : palette.text
                                    Layout.preferredWidth: 65
                                }
                            }
                            Label { text: modelData.evidence; textFormat: Text.PlainText; wrapMode: Text.Wrap; Layout.fillWidth: true; opacity: 0.75; font.pixelSize: 11 }
                        }
                    }
                    CheckBox { id: spotifyProvenance; text: "Evidence sources and local observations"; visible: spotifyComparison.evaluation !== null }
                    Label { visible: spotifyProvenance.checked; text: spotifyComparison.evaluation ? spotifyComparison.evaluation.provenance : ""; textFormat: Text.PlainText; wrapMode: Text.Wrap; Layout.fillWidth: true }
                    Label { visible: spotifyComparison.evaluation !== null; text: "PRIMARY BLOCKER"; font.bold: true }
                    Label {
                        objectName: "spotifyPrimaryBlocker"
                        text: spotifyComparison.evaluation ? (spotifyComparison.evaluation.primary_blocker || (spotifyComparison.evaluation.association_persisted ? "None: trusted association is saved." : "None: candidate accepted. See the operation result above for persistence status.")) : ""
                        textFormat: Text.PlainText; wrapMode: Text.Wrap; Layout.fillWidth: true
                    }
                    Label { visible: spotifyComparison.evaluation !== null; text: "OTHER DIFFERENCES / WARNINGS"; font.bold: true }
                    Label {
                        text: spotifyComparison.evaluation ? (spotifyComparison.evaluation.warnings.join("\n") || "None") : ""
                        textFormat: Text.PlainText; wrapMode: Text.Wrap; Layout.fillWidth: true
                    }
                    Label { visible: spotifyComparison.evaluation !== null; text: "DECISION"; font.bold: true }
                    Label {
                        text: spotifyComparison.evaluation ? spotifyComparison.evaluation.decision + "\n" + spotifyComparison.evaluation.requirements : ""
                        textFormat: Text.PlainText; wrapMode: Text.Wrap; Layout.fillWidth: true
                    }
                    Button {
                        objectName: "spotifyArtistsSame"
                        text: "Mark artists as same…"
                        visible: spotifyComparison.evaluation !== null && spotifyComparison.evaluation.artist_equivalence_available
                        enabled: spotifySongChoice.currentIndex >= 0 && !window.spotifyPlayback.resolutionPending
                        onClicked: {
                            window.bridge.spotify_artist_equivalence("prepare", spotifySongChoice.currentIndex);
                            if (JSON.parse(window.spotifyPlayback.artistProposalJson || "null")) artistEquivalenceDialog.open();
                        }
                    }
                    Label { visible: spotifyComparison.trace.latest_review !== undefined; text: "LAST PERSISTED RECONCILIATION RESULT"; font.bold: true }
                    Label {
                        text: spotifyComparison.trace.latest_review ? spotifyComparison.trace.latest_review.reason : ""
                        textFormat: Text.PlainText; wrapMode: Text.Wrap; Layout.fillWidth: true
                    }
                    Label { visible: spotifyComparison.trace.album_candidates.length > 0; text: "ALBUM ACCEPTANCE EVIDENCE"; font.bold: true }
                    Repeater {
                        model: spotifyComparison.trace.album_candidates
                        delegate: Label {
                            required property var modelData
                            text: modelData.title + " · " + modelData.id + "\n" + modelData.evidence + "\nDecision: " + modelData.decision + "\nPrimary blocker: " + (modelData.primary_blocker || "None") + "\nOther warnings: " + (modelData.warnings.join("; ") || "None") + "\n" + modelData.requirements
                            textFormat: Text.PlainText; wrapMode: Text.Wrap; Layout.fillWidth: true
                        }
                    }
                    CheckBox { id: spotifyProgramExpanded; text: "Program details"; visible: spotifyComparison.trace.programs.length > 0 }
                    Repeater {
                        model: spotifyProgramExpanded.checked ? spotifyComparison.trace.programs : []
                        delegate: ColumnLayout {
                            required property var modelData
                            Layout.fillWidth: true
                            Label { text: modelData.album + " · anchors: " + modelData.anchors + " · provider tracks: " + modelData.provider_count + " · complete: " + modelData.complete + " · duplicate positions: " + modelData.duplicate_positions; wrapMode: Text.Wrap; Layout.fillWidth: true }
                            Label { text: "LOCAL ALBUM                         SPOTIFY ALBUM"; font.bold: true }
                            Repeater {
                                model: modelData.rows
                                delegate: RowLayout {
                                    required property var modelData
                                    objectName: "spotifyProgramRow"
                                    Layout.fillWidth: true
                                    Label { objectName: "spotifyProgramLocal"; text: modelData.local; textFormat: Text.PlainText; wrapMode: Text.Wrap; Layout.fillWidth: true; Layout.preferredWidth: 1 }
                                    Label { objectName: "spotifyProgramCandidate"; text: modelData.candidate; textFormat: Text.PlainText; wrapMode: Text.Wrap; Layout.fillWidth: true; Layout.preferredWidth: 1 }
                                    Label { text: modelData.decision; wrapMode: Text.Wrap; Layout.preferredWidth: 150 }
                                }
                            }
                            Label { visible: modelData.truncated; text: "First 200 established Tracks shown; full decision evidence retained below." }
                        }
                    }
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
                        text: spotifyComparison.evaluation && spotifyComparison.evaluation.association_persisted ? "Connected to Spotify" : "Confirm Spotify association"
                        enabled: spotifySongChoice.currentIndex >= 0 && !window.spotifyPlayback.resolutionPending && !(spotifyComparison.evaluation && spotifyComparison.evaluation.association_persisted)
                        onClicked: window.bridge.spotify_resolve("confirm", spotifySongChoice.currentIndex)
                    }
                    Button {
                        text: "Cancel selection"
                        onClicked: window.bridge.spotify_resolve("cancel", -1)
                    }
                }
                Button {
                    objectName: "spotifyAlbumReevaluate"
                    text: window.spotifyPlayback.albumPending ? "Evaluating Album…" : "Re-evaluate Album matching"
                    enabled: window.spotifyPlayback.title.length > 0 && !window.spotifyPlayback.albumPending && !window.spotifyPlayback.resolutionPending && !window.spotifyPlayback.available
                    onClicked: window.bridge.spotify_album_retry()
                }
                Button {
                    text: "Search Spotify again for this Album"
                    enabled: !window.spotifyPlayback.available && !window.spotifyPlayback.manuallyExcluded && !window.spotifyPlayback.albumPending && !window.spotifyPlayback.resolutionPending
                    onClicked: window.bridge.spotify_album_refresh()
                }
                ScrollView {
                    contentWidth: availableWidth
                    clip: true
                    Layout.fillWidth: true
                    Layout.preferredHeight: 200
                    TextArea {
                        objectName: "spotifyAlbumEvidence"
                        text: window.spotifyPlayback.albumExplanation || ""
                        textFormat: TextEdit.PlainText
                        readOnly: true
                        selectByMouse: true
                        wrapMode: TextEdit.Wrap
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

    Menu {
        id: addMusicChooser
        objectName: "addMusicChooser"
        MenuItem { text: "From catalog"; onTriggered: { window.bridge.add_music_action("destination", ""); addMusicPanel.playlistMode = false; addMusicPanel.open(); } }
        MenuItem { text: "From file"; onTriggered: localImportPanel.open() }
    }
    Dialog {
        id: localImportPanel
        objectName: "localImportPanel"
        title: "From file"
        anchors.centerIn: parent
        modal: true
        width: Math.min(460, window.width - 40)
        standardButtons: Dialog.Close
        readonly property var state: window.bridge.local_import_snapshot
        contentItem: ColumnLayout {
            RowLayout {
                Button { objectName: "chooseLocalFiles"; text: "Choose files"; enabled: !localImportPanel.state.busy && !window.library.pending; onClicked: localFilesPicker.openFresh() }
                Button { objectName: "chooseLocalFolder"; text: "Choose folder"; enabled: !localImportPanel.state.busy && !window.library.pending; onClicked: localFolderPicker.openFresh() }
            }
            BusyIndicator { running: localImportPanel.state.busy; visible: running; Layout.alignment: Qt.AlignHCenter }
            Label { objectName: "localImportStatus"; text: localImportPanel.state.status || ""; visible: text.length > 0; textFormat: Text.PlainText; wrapMode: Text.WordWrap; Layout.fillWidth: true }
        }
    }
    // Platform dialogs use the standard Qt Widgets fallback (including extended
    // selection) when the OS has no native dialog, rather than the Quick fallback.
    PlatformDialogs.FileDialog {
        id: localFilesPicker
        objectName: "localFilesPicker"
        title: "Choose music files"
        parentWindow: window
        modality: Qt.WindowModal
        function openFresh() {
            files = [];
            currentFiles = [];
            open();
        }
        fileMode: PlatformDialogs.FileDialog.OpenFiles
        nameFilters: ["Audio files (*.aac *.aiff *.ape *.flac *.m4a *.mp3 *.mp4 *.ogg *.opus *.wav *.wv)", "All files (*)"]
        onAccepted: {
            const urls = [];
            for (let i = 0; i < files.length; ++i) urls.push(files[i].toString());
            close();
            window.bridge.local_import("files", urls);
        }
        onRejected: { close(); files = []; currentFiles = []; }
    }
    PlatformDialogs.FolderDialog {
        id: localFolderPicker
        objectName: "localFolderPicker"
        title: "Choose a library folder"
        parentWindow: window
        modality: Qt.WindowModal
        function openFresh() { folder = ""; currentFolder = ""; open(); }
        onAccepted: {
            const url = folder.toString();
            close();
            window.bridge.local_import("folder", [url]);
        }
        onRejected: { close(); folder = ""; currentFolder = ""; }
    }
    Timer {
        interval: 1
        repeat: true
        running: window.bridge.local_import_snapshot.scheduling
        onTriggered: window.bridge.local_import_schedule()
    }

    Dialog {
        id: addMusicPanel
        objectName: "addMusicPanel"
        title: playlistMode ? "Add Tracks from Catalog" : "Add Music"
        property bool playlistMode: false
        property var selectedTracks: []
        anchors.centerIn: parent
        width: Math.min(900, window.width - 48)
        height: Math.min(650, window.height - 80)
        modal: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        standardButtons: Dialog.Close
        property int filter: 0
        readonly property var view: window.bridge.music_snapshot
        function request() {
            musicDelay.stop();
            selectedTracks = [];
            window.bridge.add_music_action("search", filter + ":" + musicQuery.text);
        }
        onOpened: { selectedTracks = []; musicQuery.forceActiveFocus(); musicQuery.selectAll(); request(); }
        onClosed: { musicDelay.stop(); window.bridge.add_music_action("close", ""); }
        Timer { id: musicDelay; interval: 300; onTriggered: addMusicPanel.request() }
        contentItem: ColumnLayout {
            TextField {
                id: musicQuery
                objectName: "addMusicQuery"
                Layout.fillWidth: true
                placeholderText: "Find an Artist, Album or Song to add"
                maximumLength: 256
                Accessible.name: "Search catalogs"
                onTextChanged: if (addMusicPanel.opened) {
                    window.bridge.add_music_action("invalidate", "");
                    musicDelay.restart();
                }
                onAccepted: addMusicPanel.request()
                Keys.onDownPressed: event => { musicResults.forceActiveFocus(); musicResults.currentIndex = 0; }
            }
            RowLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                ColumnLayout {
                    Layout.alignment: Qt.AlignTop
                    Layout.preferredWidth: 115
                    Label { text: "FILTERS"; font.bold: true; font.pixelSize: 12 }
                    ButtonGroup { id: musicTypes }
                    Repeater {
                        model: ["All", "Artists", "Albums", "Songs"]
                        RadioButton {
                            required property int index
                            required property string modelData
                            text: modelData
                            ButtonGroup.group: musicTypes
                            checked: addMusicPanel.filter === index
                            onClicked: { addMusicPanel.filter = index; addMusicPanel.request(); }
                        }
                    }
                }
                Rectangle { Layout.fillHeight: true; implicitWidth: 1; color: "#d9d6d3" }
                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    RowLayout {
                        Button {
                            text: "Back"
                            visible: addMusicPanel.view.back
                            onClicked: { addMusicPanel.selectedTracks = []; musicDelay.stop(); window.bridge.add_music_action("back", ""); }
                        }
                        Label {
                            text: addMusicPanel.view.heading || "CATALOG RESULTS"
                            textFormat: Text.PlainText
                            font.bold: true
                            Layout.fillWidth: true
                            elide: Text.ElideRight
                        }
                        BusyIndicator { running: addMusicPanel.view.busy; visible: running; implicitWidth: 28; implicitHeight: 28 }
                    }
                    Label {
                        visible: addMusicPanel.view.detail
                        text: addMusicPanel.view.artist + (addMusicPanel.view.date ? " · " + addMusicPanel.view.date : "")
                        textFormat: Text.PlainText
                        Layout.fillWidth: true
                        wrapMode: Text.Wrap
                    }
                    RowLayout {
                        visible: !addMusicPanel.playlistMode && addMusicPanel.view.detail && !addMusicPanel.view.song
                        Button {
                            objectName: "addCatalogAlbum"
                            text: addMusicPanel.view.complete ? "In library" : "Add Album"
                            enabled: !addMusicPanel.view.complete
                            onClicked: window.bridge.add_music_action("album", "")
                        }
                        Label { text: addMusicPanel.view.membership }
                    }
                    ListView {
                        id: musicResults
                        objectName: "addMusicResults"
                        visible: !addMusicPanel.view.detail
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        clip: true
                        model: addMusicPanel.view.hits
                        section.property: "section"
                        section.delegate: Label {
                            required property string section
                            text: section
                            font.bold: true
                            font.pixelSize: 12
                            topPadding: 12
                            bottomPadding: 6
                        }
                        delegate: ItemDelegate {
                            required property var modelData
                            required property int index
                            width: musicResults.width
                            height: 60
                            highlighted: musicResults.currentIndex === index
                            enabled: !musicDelay.running
                            contentItem: Column {
                                Label { width: parent.width; text: modelData.title + (modelData.membership ? "    ·    " + modelData.membership : ""); textFormat: Text.PlainText; elide: Text.ElideRight }
                                Label { width: parent.width; text: modelData.context; textFormat: Text.PlainText; elide: Text.ElideRight; opacity: 0.7 }
                            }
                            onClicked: { addMusicPanel.selectedTracks = []; window.bridge.add_music_action("open", String(index)); }
                        }
                        Keys.onReturnPressed: if (currentIndex >= 0 && !musicDelay.running) window.bridge.add_music_action("open", String(currentIndex))
                        ScrollBar.vertical: ScrollBar {}
                    }
                    ListView {
                        id: musicTracks
                        objectName: "addMusicTracks"
                        visible: addMusicPanel.view.detail
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        clip: true
                        model: addMusicPanel.view.tracks
                        delegate: RowLayout {
                            required property var modelData
                            width: musicTracks.width
                            height: 62
                            Label { text: modelData.position; Layout.preferredWidth: 40 }
                            ColumnLayout {
                                Layout.fillWidth: true
                                Label { text: modelData.title; textFormat: Text.PlainText; Layout.fillWidth: true; elide: Text.ElideRight }
                                Label { text: modelData.artist; textFormat: Text.PlainText; Layout.fillWidth: true; elide: Text.ElideRight; opacity: 0.7 }
                            }
                            CheckBox {
                                visible: addMusicPanel.playlistMode
                                checked: addMusicPanel.selectedTracks.indexOf(modelData.key) >= 0
                                onClicked: {
                                    let keys = addMusicPanel.selectedTracks.slice();
                                    const i = keys.indexOf(modelData.key);
                                    if (checked && i < 0) keys.push(modelData.key);
                                    if (!checked && i >= 0) keys.splice(i, 1);
                                    addMusicPanel.selectedTracks = keys;
                                }
                            }
                            Button {
                                visible: !addMusicPanel.playlistMode
                                text: modelData.saved ? "In library" : "Add Song"
                                enabled: !modelData.saved
                                onClicked: window.bridge.add_music_action("song", modelData.key)
                            }
                        }
                        ScrollBar.vertical: ScrollBar {}
                    }
                    Label {
                        visible: !addMusicPanel.view.busy && !addMusicPanel.view.detail && musicResults.count === 0
                        text: musicQuery.text.trim() ? "No results. Try another Artist, Album or Song." : "Search external catalogs to add music to your library."
                        Layout.fillWidth: true
                        wrapMode: Text.Wrap
                    }
                    Label {
                        visible: addMusicPanel.view.detail && addMusicPanel.view.song && musicTracks.count === 0
                        text: "This Song could not be located in the Album’s track list. Go back and choose another result."
                        Layout.fillWidth: true
                        wrapMode: Text.Wrap
                    }
                    Button {
                        visible: addMusicPanel.view.more && !addMusicPanel.view.detail
                        enabled: !addMusicPanel.view.busy
                        text: "Next Albums"
                        onClicked: window.bridge.add_music_action("more", "")
                    }
                }
            }
            Button {
                objectName: "addCatalogTracksToPlaylist"
                visible: addMusicPanel.playlistMode && addMusicPanel.view.detail
                text: "Add selected tracks"
                enabled: addMusicPanel.selectedTracks.length > 0 && !window.library.pending
                onClicked: { window.bridge.add_music_action("playlist", addMusicPanel.selectedTracks.join(",")); }
            }
            Label { text: addMusicPanel.view.status; visible: text.length > 0; Layout.fillWidth: true; wrapMode: Text.Wrap }
        }
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

    property var metadataState: JSON.parse(bridge.metadata_snapshot)
    function openMetadata(kind, canonicalId) {
        artistsPane.rememberViewport(); albumsPane.rememberViewport(); songsPane.rememberViewport();
        bridge.metadata_action("open", kind, canonicalId, "");
        metadataDialog.reload();
        metadataDialog.open();
    }
    Dialog {
        id: metadataDialog
        objectName: "metadataDialog"
        modal: true
        width: Math.min(920, window.width - 40)
        height: Math.min(800, window.height - 40)
        anchors.centerIn: parent
        title: inspection && inspection.target.album !== undefined ? "Album Metadata" : "Track Metadata"
        property var inspection: window.metadataState.inspection
        property var edits: ({})
        property var selectedFiles: ({})
        property bool writeFiles: false
        function reload() { edits = ({}); selectedFiles = ({}); writeFiles = false; }
        function setEdit(key,value) { const copy=Object.assign({},edits); copy[key]=value; edits=copy; }
        function save() {
            const changes=Object.keys(edits).map(key=>({field:key,value:edits[key]}));
            const files=writeFiles ? Object.keys(selectedFiles).filter(key=>selectedFiles[key]) : [];
            if(writeFiles && files.length===0) { localWarning.text="Select the files to update."; return; }
            localWarning.text="";
            bridge.metadata_action("save","","",JSON.stringify({changes:changes,files:files}));
        }
        closePolicy: window.metadataState.busy ? Popup.NoAutoClose : Popup.CloseOnEscape
        contentItem: ColumnLayout {
            spacing: 8
            Button { text: "Edit shared Album metadata…"; visible: !!metadataDialog.inspection && metadataDialog.inspection.target.track !== undefined; enabled: !window.metadataState.busy && Object.keys(metadataDialog.edits).length===0; onClicked: window.openMetadata("album",metadataDialog.inspection.album_id) }
            Label { text: metadataDialog.inspection ? metadataDialog.inspection.track_count + " Tracks · " + metadataDialog.inspection.files.length + " attached local files" : ""; Layout.fillWidth: true }
            ScrollView {
                Layout.fillWidth: true; Layout.fillHeight: true
                contentWidth: availableWidth
                ColumnLayout {
                    width: parent.width
                    spacing: 12
                    Label { text: "Library values"; font.bold: true }
                    Repeater {
                        model: metadataDialog.inspection ? metadataDialog.inspection.fields : []
                        delegate: ColumnLayout {
                            required property var modelData
                            Layout.fillWidth: true
                            RowLayout {
                                Layout.fillWidth: true
                                Label { text: modelData.label; Layout.preferredWidth: 170; wrapMode: Text.Wrap }
                                TextField {
                                    objectName: "metadataField_" + modelData.key
                                    Layout.fillWidth: true
                                    enabled: modelData.editable && !window.metadataState.busy
                                    placeholderText: "—"
                                    text: metadataDialog.edits[modelData.key] === undefined || metadataDialog.edits[modelData.key] === null ? modelData.value : metadataDialog.edits[modelData.key]
                                    onTextEdited: metadataDialog.setEdit(modelData.key,text)
                                }
                                Button {
                                    text: "Use automatic value"
                                    visible: modelData.editable
                                    enabled: !window.metadataState.busy && (modelData.overridden || metadataDialog.edits[modelData.key] !== undefined)
                                    onClicked: metadataDialog.setEdit(modelData.key,null)
                                }
                            }
                            Label { text: metadataDialog.edits[modelData.key] === null ? "Will clear override on Save" : metadataDialog.edits[modelData.key] !== undefined ? "User override (pending Save)" : modelData.effective_source; opacity: 0.7 }
                        }
                    }
                    CheckBox {
                        objectName: "metadataWriteFiles"
                        text: "Also update local file metadata" + (metadataDialog.inspection && metadataDialog.inspection.target.album !== undefined ? " for affected Tracks" : "")
                        checked: metadataDialog.writeFiles
                        enabled: !window.metadataState.busy && !!metadataDialog.inspection && metadataDialog.inspection.files.length > 0
                        onToggled: metadataDialog.writeFiles=checked
                    }
                    ColumnLayout {
                        visible: metadataDialog.writeFiles
                        Layout.fillWidth: true
                        Label { text: "Select affected files. Only changed fields will be written. Release type cannot be written to local tags."; wrapMode: Text.Wrap; Layout.fillWidth: true }
                        RowLayout {
                            Button {
                                objectName: "metadataSelectAllFiles"
                                text: "Select all"
                                enabled: !window.metadataState.busy
                                onClicked: { const files={}; for(const file of metadataDialog.inspection.files) files[file.source_id]=true; metadataDialog.selectedFiles=files; }
                            }
                            Button { objectName: "metadataClearFiles"; text: "Clear selection"; enabled: !window.metadataState.busy; onClicked: metadataDialog.selectedFiles=({}) }
                        }
                        Repeater {
                            model: metadataDialog.inspection ? metadataDialog.inspection.files : []
                            delegate: CheckBox {
                                required property var modelData
                                Layout.fillWidth: true
                                text: modelData.path + (modelData.available ? "" : " (unavailable)")
                                checked: !!metadataDialog.selectedFiles[modelData.source_id]
                                enabled: !window.metadataState.busy
                                onToggled: {const copy=Object.assign({},metadataDialog.selectedFiles); copy[modelData.source_id]=checked; metadataDialog.selectedFiles=copy;}
                            }
                        }
                        Label { text: "Update local tags for " + Object.keys(metadataDialog.selectedFiles).filter(key=>metadataDialog.selectedFiles[key]).length + " files" }
                    }
                    Label { text: "Source evidence · read-only"; font.bold: true }
                    Label { text: metadataDialog.inspection && metadataDialog.inspection.target.album !== undefined ? "Tracks are ordered by disc and album position, with each Track’s sources together. Shared Album and Release evidence follows." : "Each file/provider object is shown separately. Differences remain evidence; editing does not reassign identities."; wrapMode: Text.Wrap; Layout.fillWidth: true }
                    Repeater {
                        model: metadataDialog.inspection ? metadataDialog.inspection.track_evidence : []
                        delegate: GroupBox {
                            required property var modelData
                            objectName: "metadataTrackGroup_" + modelData.track_id
                            title: "Disc " + (modelData.disc_number || "—") + " · Track " + (modelData.track_number || "—") + " · " + modelData.title
                            Layout.fillWidth: true
                            ColumnLayout {
                                width: parent.width
                                Repeater {
                                    model: modelData.evidence
                                    delegate: Frame {
                                        required property var modelData
                                        Layout.fillWidth: true
                                        ColumnLayout {
                                            width: parent.width
                                            Label { text: modelData.source; font.bold: true }
                                            Label { text: modelData.label; wrapMode: Text.Wrap; Layout.fillWidth: true; opacity: 0.7 }
                                            Label { text: Object.keys(modelData.values).map(key=>key + ": " + modelData.values[key]).join("\n") || "—"; wrapMode: Text.Wrap; Layout.fillWidth: true; textFormat: Text.PlainText }
                                            Button {
                                                objectName: "metadataConnectSpotify_" + modelData.track_id + "_" + modelData.candidate_id
                                                text: "Connect this Spotify candidate"
                                                visible: !!modelData.candidate_id
                                                enabled: !window.metadataState.busy && Object.keys(metadataDialog.edits).length===0
                                                onClicked: window.bridge.metadata_action("connect-spotify","track",modelData.track_id,modelData.candidate_id)
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Label { visible: !!metadataDialog.inspection && metadataDialog.inspection.target.album !== undefined; text: "Shared Album / exact Release evidence"; font.bold: true }
                    Repeater {
                        model: metadataDialog.inspection ? metadataDialog.inspection.evidence : []
                        delegate: Frame {
                            required property var modelData
                            Layout.fillWidth: true
                            ColumnLayout {
                                width: parent.width
                                Label { text: modelData.source; font.bold: true }
                                Label { text: modelData.label; wrapMode: Text.Wrap; Layout.fillWidth: true; opacity: 0.7 }
                                Label { text: Object.keys(modelData.values).map(key=>key + ": " + modelData.values[key]).join("\n") || "—"; wrapMode: Text.Wrap; Layout.fillWidth: true; textFormat: Text.PlainText }
                                Button {
                                    objectName: "metadataConnectSpotify_" + modelData.track_id + "_" + modelData.candidate_id
                                    text: "Connect this Spotify candidate"
                                    visible: !!modelData.candidate_id
                                    enabled: !window.metadataState.busy && Object.keys(metadataDialog.edits).length===0
                                    onClicked: window.bridge.metadata_action("connect-spotify","track",modelData.track_id,modelData.candidate_id)
                                }
                            }
                        }
                    }
                    Label { id:localWarning; color: "#c65b52"; wrapMode: Text.Wrap; Layout.fillWidth: true }
                    GroupBox {
                        title: "Advanced · attached provider identities (read-only)"
                        Layout.fillWidth: true
                        Label { width: parent.width; text: metadataDialog.inspection ? metadataDialog.inspection.identities.join("\n") || "—" : "—"; wrapMode: Text.Wrap; textFormat: Text.PlainText }
                    }
                }
            }
            Label { objectName: "metadataResult"; text: window.metadataState.message; wrapMode: Text.Wrap; Layout.fillWidth: true; textFormat: Text.PlainText }
            RowLayout {
                Layout.alignment: Qt.AlignRight
                Button { text: "Cancel"; enabled: !window.metadataState.busy; onClicked: metadataDialog.close() }
                Button { objectName: "metadataSave"; text: metadataDialog.inspection && metadataDialog.inspection.target.album !== undefined ? "Save Album Metadata" : "Save"; enabled: !!metadataDialog.inspection && !window.metadataState.busy && Object.keys(metadataDialog.edits).length>0; onClicked: metadataDialog.save() }
            }
        }
        Connections {
            target: bridge
            function onMetadata_changed() { if ((window.metadataState.renameMatches || []).length) metadataRenameDialog.open(); else metadataRenameDialog.close(); if(!window.metadataState.busy && window.metadataState.message.startsWith("Library metadata saved")) metadataDialog.reload();}
        }
    }

    Dialog {
        id: metadataRenameDialog
        objectName: "metadataRenameDialog"
        title: "An Album with this name already exists"
        modal: true
        anchors.centerIn: parent
        width: Math.min(620, window.width - 40)
        property var matches: window.metadataState.renameMatches || []
        property var chosen: matches[renameChoice.currentIndex] || null
        onRejected: bridge.metadata_action("rename-cancel", "", "", "")
        contentItem: ColumnLayout {
            Label { text: "Move the " + (metadataDialog.inspection ? metadataDialog.inspection.track_count : 0) + " affected Tracks to an existing Album?"; wrapMode: Text.Wrap; Layout.fillWidth: true }
            ComboBox {
                id: renameChoice
                objectName: "metadataRenameChoice"
                Layout.fillWidth: true
                model: metadataRenameDialog.matches.map(m => m.title + " · " + m.artist + " · " + (m.year || "Unknown year") + " · " + m.track_count + " Tracks · " + m.release_count + " Releases")
            }
            Label { text: "Moving uses the destination Album’s shared metadata. Track titles, individual overrides and exact Releases are preserved. Selected local files are updated only after saving."; wrapMode: Text.Wrap; Layout.fillWidth: true }
            Label { text: metadataRenameDialog.chosen ? metadataRenameDialog.chosen.blocked_reason : ""; wrapMode: Text.Wrap; Layout.fillWidth: true }
            RowLayout {
                Button { text: "Cancel"; objectName: "metadataRenameCancel"; onClicked: { bridge.metadata_action("rename-cancel", "", "", ""); metadataRenameDialog.close(); } }
                Button { text: "Keep separate"; objectName: "metadataRenameKeep"; onClicked: { metadataRenameDialog.close(); bridge.metadata_action("rename-keep", "", "", ""); } }
                Button { text: "Move Tracks"; objectName: "metadataRenameMove"; enabled: !!metadataRenameDialog.chosen && !metadataRenameDialog.chosen.blocked_reason; onClicked: { const id = metadataRenameDialog.chosen.album_id; metadataRenameDialog.close(); bridge.metadata_action("rename-move", "", id, ""); } }
            }
        }
    }
    Dialog {
        id: preferenceManager; objectName: "preferenceManager"
        property bool hiddenArtists: false
        title: hiddenArtists ? "Hidden Artists" : "Ignored Songs"
        modal: true; anchors.centerIn: parent
        width: Math.min(820,window.width-40); height: Math.min(600,window.height-60)
        standardButtons: Dialog.Close
        contentItem: ColumnLayout {
            Label { text: preferenceManager.hiddenArtists ? "Artist" : "Song · Artist · Album" }
            ListView {
                Layout.fillWidth: true; Layout.fillHeight: true; clip: true
                model: window.library.preferenceRows || []
                delegate: RowLayout {
                    required property var modelData
                    width: ListView.view.width
                    Label { text: preferenceManager.hiddenArtists ? modelData.title : [modelData.title,modelData.artist,modelData.album].join(" · "); textFormat: Text.PlainText; elide: Text.ElideRight; Layout.fillWidth: true }
                    Button { objectName: "preferenceRestore-" + modelData.id; text: preferenceManager.hiddenArtists ? "Unhide" : "Unignore"; onClicked: window.bridge.browse_action(preferenceManager.hiddenArtists ? "preference-unhide" : "preference-unignore",preferenceManager.hiddenArtists ? 0 : 2,modelData.id) }
                }
                ScrollBar.vertical: ScrollBar {}
            }
            RowLayout {
                Button { text: "First page"; onClicked: window.bridge.browse_action("preference-first",0,"") }
                Button { text: "Next page"; enabled: window.library.preferenceMore; onClicked: window.bridge.browse_action("preference-next",0,"") }
            }
        }
    }
    property int contextPane: 0
    property string contextId: ""
    property string contextTrackId: ""
    function openSongContext(paneIndex, rowData) {
        window.bridge.browse_action("context", paneIndex, rowData.id);
        contextPane = paneIndex;
        contextId = rowData.id;
        // Playlist row IDs identify entries; their Track relationship is canonical.
        contextTrackId = paneIndex !== 2 ? "" : window.library.view === "Playlists"
            ? (rowData.track ? rowData.track.trackId : "") : rowData.id;
        window.bridge.browse_action("preference-context",paneIndex,paneIndex === 2 ? contextTrackId : contextId);
        libraryMenu.popup();
    }
    Dialog {
        id: spotifyPlaylistDialog
        objectName: "spotifyPlaylistDialog"
        title: "Import from Spotify"
        anchors.centerIn: parent
        modal: true
        width: Math.min(640, window.width - 40)
        height: Math.min(600, window.height - 40)
        standardButtons: Dialog.Close
        readonly property var state: window.bridge.spotify_playlist_snapshot
        onOpened: window.bridge.spotify_playlist_action("browse", "")
        ColumnLayout {
            anchors.fill: parent
            RowLayout {
                Layout.fillWidth: true
                TextField { id: spotifyPlaylistLink; objectName: "spotifyPlaylistLink"; Layout.fillWidth: true; placeholderText: "Spotify playlist link, URI or ID"; enabled: !spotifyPlaylistDialog.state.busy }
                Button { text: "Import"; enabled: !spotifyPlaylistDialog.state.busy && spotifyPlaylistLink.text.trim().length > 0; onClicked: window.bridge.spotify_playlist_action("import", spotifyPlaylistLink.text) }
            }
            RowLayout {
                Button { text: "Browse account playlists"; enabled: !spotifyPlaylistDialog.state.busy; onClicked: window.bridge.spotify_playlist_action("browse", "") }
                Button { text: "Reconnect Spotify"; visible: spotifyPlaylistDialog.state.needsAuth; enabled: window.spotifyPlayback.status !== "Authorizing"; onClicked: window.bridge.spotify_playlist_action("connect", "") }
                BusyIndicator { running: spotifyPlaylistDialog.state.busy || window.spotifyPlayback.status === "Authorizing"; Layout.preferredWidth: 24; Layout.preferredHeight: 24 }
            }
            Label { Layout.fillWidth: true; text: spotifyPlaylistDialog.state.message; textFormat: Text.PlainText; wrapMode: Text.Wrap }
            RowLayout {
                visible: spotifyPlaylistDialog.state.conflict
                Button { text: "Rename incoming"; enabled: !spotifyPlaylistDialog.state.busy; onClicked: { spotifyIncomingTitle.text=spotifyPlaylistDialog.state.incomingName; spotifyRenameDialog.open(); } }
                Button { text: "Overwrite existing"; enabled: !spotifyPlaylistDialog.state.busy && spotifyPlaylistDialog.state.canOverwrite; onClicked: window.bridge.spotify_playlist_action("overwrite","") }
                Button { text: "Cancel"; enabled: !spotifyPlaylistDialog.state.busy; onClicked: window.bridge.spotify_playlist_action("cancel","") }
            }
            TextField { id: spotifyPlaylistFilter; Layout.fillWidth: true; placeholderText: "Filter playlists by name or owner" }
            ListView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                model: spotifyPlaylistDialog.state.playlists.filter(p => (p.name + " " + p.owner).toLowerCase().indexOf(spotifyPlaylistFilter.text.toLowerCase()) >= 0)
                ScrollBar.vertical: ScrollBar {}
                delegate: ItemDelegate {
                    required property var modelData
                    width: ListView.view.width
                    text: modelData.name + (modelData.owner ? " — " + modelData.owner : "") + (modelData.count ? " · " + modelData.count : "")
                    enabled: !spotifyPlaylistDialog.state.busy
                    onClicked: window.bridge.spotify_playlist_action("import", modelData.id)
                }
            }
        }
    }
    Dialog {
        id: spotifyRenameDialog; objectName: "spotifyRenameDialog"
        title: "Rename incoming playlist"; anchors.centerIn: parent; modal: true
        standardButtons: Dialog.Ok | Dialog.Cancel
        TextField { id: spotifyIncomingTitle; objectName: "spotifyIncomingTitle"; width: 320; placeholderText: "New local playlist title" }
        onAccepted: window.bridge.spotify_playlist_action("rename",spotifyIncomingTitle.text)
    }
    Dialog {
        id: playlistNameDialog
        property bool rename: false
        title: rename ? "Rename playlist" : "Create playlist"
        anchors.centerIn: parent
        modal: true
        standardButtons: Dialog.Ok | Dialog.Cancel
        TextField { id: playlistName; placeholderText: "Playlist name" }
        onAccepted: { if (playlistName.text.trim().length) window.bridge.browse_action(rename ? "playlist-rename" : "playlist-create", 0, playlistName.text); }
    }
    Dialog {
        id: addPlaylistDialog
        title: "Add to Playlist"
        anchors.centerIn: parent
        modal: true
        standardButtons: Dialog.Close
        ColumnLayout {
            ListView {
                Layout.preferredWidth: 300; Layout.preferredHeight: 240
                clip: true
                model: window.library.playlistChoices || []
                delegate: ItemDelegate { required property var modelData; width: ListView.view.width; text: modelData.name; onClicked: { window.bridge.browse_action("picker-add",2,modelData.id); addPlaylistDialog.close(); } }
                Label { anchors.centerIn: parent; visible: parent.count === 0; text: "No playlists yet" }
            }
            RowLayout {
                Button { text: "Previous"; enabled: window.library.playlistChoicesPrevious; onClicked: window.bridge.browse_action("picker-previous",0,"") }
                Button { text: "Next"; enabled: window.library.playlistChoicesMore; onClicked: window.bridge.browse_action("picker-next",0,"") }
                Button { text: "Create playlist…"; onClicked: { playlistNameDialog.rename = false; playlistName.text = ""; playlistNameDialog.open(); } }
            }
        }
    }
    Dialog {
        id: duplicatePlaylistDialog
        objectName: "duplicatePlaylistDialog"
        title: "Duplicate songs"
        width: Math.min(480, window.width-60)
        anchors.centerIn: parent
        modal: true
        visible: !!window.library.duplicateMessage
        standardButtons: Dialog.Yes | Dialog.No
        Label { text: window.library.duplicateMessage || ""; wrapMode: Text.Wrap; width: Math.min(420, window.width-80); textFormat: Text.PlainText }
        onAccepted: window.bridge.browse_action("picker-yes",0,"")
        onRejected: window.bridge.browse_action("picker-no",0,"")
        closePolicy: Popup.NoAutoClose
    }
    Menu {
        id: libraryMenu
        onAboutToShow: window.bridge.browse_action("preference-context",window.contextPane,window.contextPane === 2 ? window.contextTrackId : window.contextId)
        MenuItem { objectName: "hideArtistAction"; text: window.library.contextHidden ? "Unhide this artist" : "Hide this artist"; visible: window.contextPane === 0 && window.library.view === "Artists"; onTriggered: window.bridge.browse_action(window.library.contextHidden ? "preference-unhide" : "preference-hide",0,window.contextId) }
        MenuItem { objectName: "ignoreAction"; text: (window.library.contextIgnored ? "Unignore " : "Ignore ") + ["artist","album","song"][window.contextPane]; visible: window.contextPane === 2 || (window.library.view !== "Playlists" && (window.contextPane === 1 || window.library.view === "Artists")); onTriggered: window.bridge.browse_action(window.library.contextIgnored ? "preference-unignore" : "preference-ignore",window.contextPane,window.contextPane === 2 ? window.contextTrackId : window.contextId) }
        MenuItem { text: "Play now"; enabled: !window.library.pending; onTriggered: window.bridge.browse_action("context-play", window.contextPane, window.contextId) }
        MenuItem { text: "Add to queue"; enabled: !window.library.pending; onTriggered: window.bridge.browse_action("append", window.contextPane, window.contextId) }
        MenuItem { text: "Add to Playlist…"; enabled: !window.library.pending; onTriggered: { window.bridge.browse_action("picker-open",window.contextPane,window.contextId); addPlaylistDialog.open(); } }
        MenuItem { text: "Rename playlist…"; visible: window.library.view === "Playlists" && window.contextPane === 0; enabled: !window.library.pending && window.library.panes[0].selectionCount === 1; onTriggered: { playlistNameDialog.rename = true; playlistName.text = (window.library.panes[0].rows.find(r => r.id === window.contextId) || {}).title || ""; playlistNameDialog.open(); } }
        MenuItem { text: "Resolve local Tracks"; visible: window.library.view === "Playlists" && window.contextPane === 0; enabled: !window.library.pending; onTriggered: window.bridge.browse_action("playlist-reconcile", 0, window.contextId) }
        MenuItem { text: "Delete playlist"; enabled: !window.library.pending; visible: window.library.view === "Playlists" && window.contextPane === 0; onTriggered: window.bridge.browse_action("playlist-delete", 0, window.contextId) }
        MenuItem { text: "Save to Library"; visible: window.library.view === "Playlists" && window.contextPane === 2; enabled: !window.library.pending && window.library.panes[2].selectionCount === 1; onTriggered: window.bridge.browse_action("save-playlist-track", 2, window.contextId) }
        MenuItem { text: "Remove entry from playlist"; enabled: !window.library.pending; visible: window.library.view === "Playlists" && window.contextPane === 2; onTriggered: window.bridge.browse_action("playlist-remove", 2, window.contextId) }
        MenuItem { text: "Move up"; enabled: !window.library.pending && window.library.playlistReorderAllowed && window.library.panes[2].selectionCount === 1; visible: window.library.view === "Playlists" && window.contextPane === 2; onTriggered: window.bridge.browse_action("playlist-up", 2, window.contextId) }
        MenuItem { text: "Move down"; enabled: !window.library.pending && window.library.playlistReorderAllowed && window.library.panes[2].selectionCount === 1; visible: window.library.view === "Playlists" && window.contextPane === 2; onTriggered: window.bridge.browse_action("playlist-down", 2, window.contextId) }
        MenuItem { objectName: "reviewThisAlbum"; text: "Review this Album"; visible: window.library.view === "Spotify Connections" && window.contextPane === 2; onTriggered: { const row = window.library.panes[2].rows.find(r => r.id === window.contextTrackId); if(row) window.bridge.browse_action("review-album",2,row.albumId); } }
        MenuItem { objectName: "spotifyMarkUnavailable"; text: "Mark as not on Spotify"; visible: window.library.view === "Spotify Connections" && !window.library.reviewMarked && window.contextPane === 2; onTriggered: window.bridge.browse_action("spotify-mark-selection",2,window.contextTrackId) }
        MenuItem { objectName: "spotifyCheckAgain"; text: "Check Spotify again"; visible: window.library.view === "Spotify Connections" && window.library.reviewMarked && window.contextPane === 2; onTriggered: window.bridge.browse_action("spotify-check",2,window.contextTrackId) }
        MenuItem { id: trackMetadataMenuAction; objectName: "trackMetadataAction"; text: "Metadata…"; visible: window.contextPane === 2; enabled: !window.library.pending && window.contextTrackId.length > 0; onTriggered: window.openMetadata("track",window.contextTrackId) }
        MenuItem { id: albumMetadataMenuAction; objectName: "albumMetadataAction"; text: "Metadata…"; visible: window.contextPane === 1 && window.library.view !== "Playlists"; enabled: !window.library.pending && window.contextId.length > 0; onTriggered: window.openMetadata("album",window.contextId) }
        MenuItem { objectName: "songSpotifyConnection"; text: "Spotify connection…"; visible: window.contextPane === 2; enabled: !window.library.pending && window.contextTrackId.length > 0; onTriggered: window.openSpotifyConnection(window.contextTrackId) }
        MenuSeparator {}
        MenuItem { text: "Remove from library"; visible: window.library.view !== "Playlists"; enabled: !window.library.pending && !window.bridge.local_import_snapshot.busy; onTriggered: window.bridge.browse_action("remove-preview", window.contextPane, window.contextId) }

    }

    Dialog {
        id: removalDialog
        objectName: "removalDialog"
        focus: true
        title: "Remove from library"
        modal: true
        anchors.centerIn: parent
        width: Math.min(480, window.width - 40)
        visible: !!window.library.removal.message
        closePolicy: Popup.CloseOnEscape
        onRejected: window.bridge.browse_action("remove-cancel", 0, "")
        onOpened: { suppressRescan.checked = true; removalFocus.start(); }
        contentItem: Column {
            spacing: 14
            Label { width: parent.width; text: window.library.removal.message || ""; textFormat: Text.PlainText; wrapMode: Text.WordWrap }
            CheckBox { id: suppressRescan; objectName: "suppressRescan"; visible: !!window.library.removal.local; checked: true; text: "Do not automatically rescan" }
            Label { visible: !!window.library.removal.local; text: "Your music files will not be deleted."; textFormat: Text.PlainText }
        }
        footer: Item {
            implicitHeight: removalButtons.implicitHeight + 20
            Row {
                id: removalButtons
                anchors.right: parent.right
                anchors.rightMargin: 12
                anchors.verticalCenter: parent.verticalCenter
                spacing: 12
                Button {
                    id: cancelRemoval
                    focus: true
                    text: "Cancel"
                    onClicked: window.bridge.browse_action("remove-cancel", 0, "")
                    Keys.onReturnPressed: clicked()
                    Keys.onEnterPressed: clicked()
                }
                Button { id: confirmRemoval; text: "Remove"; onClicked: window.bridge.browse_action("remove-confirm", 0, suppressRescan.checked ? "suppress" : "") }
            }
        }
    }

    Timer { id: removalFocus; interval: 1; onTriggered: cancelRemoval.forceActiveFocus() }

    property var artworkKeys: []
    function requestArtwork(key) {
        if (!key || artworkKeys.indexOf(key) >= 0) return;
        artworkKeys.push(key); artworkBatchTimer.restart();
    }
    Timer {
        id: artworkBatchTimer
        interval: 10
        onTriggered: { const keys = window.artworkKeys; window.artworkKeys = []; window.bridge.artwork_batch(keys); }
    }

    component AlbumArt: Rectangle {
        required property string artworkKey
        onArtworkKeyChanged: window.requestArtwork(artworkKey)
        Component.onCompleted: window.requestArtwork(artworkKey)
        color: "#dedbd7"
        readonly property string artworkUrl: window.bridge.artwork_snapshot[artworkKey] || ""
        Image {
            id: coverImage
            objectName: "coverImage"
            anchors.centerIn: parent
            // Preserve aspect through the item's geometry. Qt's PreserveAspectFit
            // loader can override sourceSize and upscale small/non-square sources.
            width: implicitWidth > 0 && implicitHeight > 0 ? Math.min(parent.width, implicitWidth / Screen.devicePixelRatio, parent.height * implicitWidth / implicitHeight) : parent.width
            height: implicitWidth > 0 && implicitHeight > 0 ? width * implicitHeight / implicitWidth : parent.height
            source: parent.artworkUrl
            onStatusChanged: { if (status === Image.Error && parent.artworkKey) window.bridge.artwork_retry(parent.artworkKey); }
            asynchronous: true
            fillMode: Image.Stretch
            // Decode for the physical display size, rather than a 500px texture
            // for every 120px tile (or the compact 46px player cover).
            sourceSize: Qt.size(Math.ceil(parent.width * Screen.devicePixelRatio), Math.ceil(parent.height * Screen.devicePixelRatio))
            smooth: true
            mipmap: true
        }
        Label {
            anchors.centerIn: parent
            objectName: "artPlaceholder"
            visible: coverImage.status !== Image.Ready
            text: "♫"
            font.pixelSize: Math.max(20, parent.width * 0.28)
            color: "#aaa3aa"
        }
    }

    component SongColumnDivider: MouseArea {
        id: divider
        required property int index
        required property Item tablePane
        required property Item resizeViewport
        required property real scrollOffset
        property bool bodyDivider: false
        x: tablePane.columnEdge(index)-scrollOffset-width/2
        width: 12
        height: resizeViewport.height
        hoverEnabled: true
        cursorShape: Qt.SplitHCursor
        acceptedButtons: Qt.LeftButton
        preventStealing: true
        property real initialX: 0
        property var initialWidths: []
        onPressed: event => {
            initialX=mapToItem(resizeViewport,event.x,event.y).x;
            initialWidths=tablePane.tableColumns.slice();
        }
        onPositionChanged: event => {
            if(pressed) tablePane.resizeColumns(index,initialWidths,mapToItem(resizeViewport,event.x,event.y).x-initialX);
        }
        Rectangle {
            anchors.horizontalCenter: parent.horizontalCenter
            anchors.verticalCenter: parent.verticalCenter
            width: 1
            height: divider.bodyDivider ? parent.height : parent.height-8
            color: divider.pressed ? "#96506d" : divider.containsMouse ? "#827b80" : divider.bodyDivider ? "#e8e5e2" : "#d9d6d3"
        }
    }

    component LibraryPane: ColumnLayout {
        id: pane
        required property int paneIndex
        required property string heading
        readonly property bool playlistTable: paneIndex === 2 && window.library.view === "Playlists"
        readonly property bool reviewTable: paneIndex === 2 && window.library.view === "Spotify Connections"
        readonly property bool songsTable: paneIndex === 2 && (window.library.view === "Songs" || reviewTable)
        readonly property bool detailsTable: playlistTable || songsTable
        readonly property real tableWidth: Math.max(0, list.width - 16)
        readonly property var columnMinimums: reviewTable ? [180,120,120,200] : songsTable ? [180,120,120,100] : [32,160,120,120,64]
        readonly property var defaultColumns: songsTable ? [Math.max(220,tableWidth*0.36),Math.max(150,tableWidth*0.24),Math.max(150,tableWidth*0.24),Math.max(120,tableWidth*0.16)] : [40,Math.max(180,(tableWidth-104)*0.44),Math.max(120,(tableWidth-104)*0.28),Math.max(120,(tableWidth-104)*0.28),64]
        readonly property var adjustedColumns: reviewTable ? window.reviewColumnWidths : songsTable ? window.songsColumnWidths : window.playlistColumnWidths
        readonly property var columnMaximums: reviewTable ? [800,480,640,640] : songsTable ? [800,480,640,320] : [80,800,480,640,100]
        function boundedColumnWidth(value,index) {
            const number=Number(value);
            return Math.min(columnMaximums[index],Math.max(columnMinimums[index],Number.isFinite(number) ? number : defaultColumns[index]));
        }
        readonly property var tableColumns: columnMinimums.map((minimum,i) => boundedColumnWidth(Array.isArray(adjustedColumns) && adjustedColumns.length===columnMinimums.length ? adjustedColumns[i] : defaultColumns[i],i))
        onColumnsWidthChanged: Qt.callLater(function() {
            list.contentX=Math.max(0,Math.min(list.contentX,Math.max(0,list.contentWidth-list.width)));
        })
        readonly property real columnsWidth: tableColumns.reduce((sum,w) => sum+w,0)
        function columnEdge(index) { return tableColumns.slice(0,index+1).reduce((sum,w) => sum+w,0); }
        function resizeColumns(index, initialWidths, delta) {
            const widths=initialWidths.map((w,i) => boundedColumnWidth(w,i));
            widths[index]=boundedColumnWidth(widths[index]+delta,index);
            if(reviewTable) window.reviewColumnWidths=widths;
            else if(songsTable) window.songsColumnWidths=widths;
            else window.playlistColumnWidths=widths;
        }
        readonly property var pageData: window.library.panes[paneIndex]
        readonly property string selectedId: paneIndex === 0 ? (window.library.view === "Genres" ? window.library.genre : window.library.artist) : paneIndex === 1 ? window.library.album : songId
        property string songId: ""
        property string rowsSignature: ""
        property string datasetKey: ""
        property bool adjustingWindow: false
        property var renderedRows: []
        function viewportAnchor() {
            for (let i = 0; i < paneRows.count; ++i) {
                const item = list.itemAtIndex(i);
                if (!item) continue;
                if (paneIndex === 1) {
                    const anchor = item.viewportTileAnchor();
                    if (anchor) return anchor;
                } else {
                    const y = item.mapToItem(list, 0, 0).y;
                    if (y + item.height > 0 && y < list.height)
                        return {id: renderedRows[i].id, pixel: y};
                }
            }
            return null;
        }
        function rememberViewport() {
            if (adjustingWindow || !visible) return;
            const anchor = viewportAnchor();
            if (anchor) window.bridge.browse_action("scroll-position", paneIndex, anchor.id + "\n" + anchor.pixel);
        }
        function restoreAnchor(anchor) {
            if (!anchor) { list.positionViewAtBeginning(); return; }
            const index = pageData.rows.findIndex(r => r.id === anchor.id);
            if (index < 0) { list.positionViewAtBeginning(); return; }
            const visual = visualIndex(index);
            list.positionViewAtIndex(visual, ListView.Beginning);
            list.forceLayout();
            const section = list.itemAtIndex(visual);
            const item = paneIndex === 1 ? (section ? section.tileAt(index) : null) : section;
            if (item) {
                list.contentY += item.mapToItem(list, 0, 0).y - anchor.pixel;
            }
        }
        function updateViewport() {
            if (adjustingWindow || !visible || !pageData.rows.length) return;
            rememberViewport();
            const end = list.originY + list.contentHeight - list.height;
            if (pageData.more && end - list.contentY < Math.max(120, list.height))
                window.bridge.browse_action("scroll-forward", paneIndex, "");
            else if (pageData.before && list.contentY - list.originY < Math.max(80, list.height / 2))
                window.bridge.browse_action("scroll-backward", paneIndex, "");
        }
        Timer { id: viewportTimer; interval: 32; onTriggered: pane.updateViewport() }
        readonly property bool albumSongs: paneIndex === 2 && pageData.sort === "Album"
        property var albumNames: ({})
        property int logicalIndex: -1
        readonly property int tileWidth: 120
        readonly property int tileHeight: 174
        readonly property int tileGap: 12
        readonly property int tileInset: 6
        readonly property int scrollAllowance: 16
        readonly property int albumColumns: Math.max(1, Math.floor((list.width - 2 * tileInset - scrollAllowance + tileGap) / (tileWidth + tileGap)))
        function revealLogical(index) {
            const group = visualIndex(index);
            if (group < 0) return;
            list.currentIndex = group;
            if (paneIndex !== 1) {
                list.positionViewAtIndex(group, ListView.Contain);
                return;
            }
            // Current index identifies a whole Flow section, not the clicked tile.
            // Never position that oversized section when its tile already exists.
            list.forceLayout();
            let section = list.itemAtIndex(group);
            let tile = section ? section.tileAt(index) : null;
            if (!tile) {
                list.positionViewAtIndex(group, ListView.Contain);
                list.forceLayout();
                section = list.itemAtIndex(group);
                tile = section ? section.tileAt(index) : null;
            }
            if (!tile) return;
            const top = tile.mapToItem(list.contentItem, 0, 0).y;
            const bottom = top + tile.height;
            if (top < list.contentY) list.contentY = top;
            else if (bottom > list.contentY + list.height) list.contentY = bottom - list.height;
            list.contentY = Math.max(list.originY, Math.min(list.contentY, list.originY + Math.max(0, list.contentHeight - list.height)));
        }
        function preserveReflowPosition() {
            if (paneIndex === 1 && logicalIndex >= 0) Qt.callLater(function() { pane.revealLogical(pane.logicalIndex); });
        }
        onAlbumColumnsChanged: preserveReflowPosition()
        function visualIndex(index) {
            if (paneIndex !== 1) return index >= 0 && index < paneRows.count ? index : -1;
            for (let i = 0; i < paneRows.count; ++i) {
                const entry = paneRows.get(i).rowData;
                if (paneIndex !== 1 ? i === index : entry.tiles.some(t => t.logicalIndex === index)) return i;
            }
            return -1;
        }
        property int navigation: window.library.navigation
        onNavigationChanged: Qt.callLater(function() {
            const target = pane.paneIndex === 0 ? (window.library.view === "Genres" ? window.library.genre : window.library.artist) : pane.paneIndex === 1 ? window.library.album : window.library.song;
            if (pane.paneIndex === 2) pane.songId = target;
            if (pane.pageData.scrollId) return;
            for (let i = 0; i < pane.pageData.rows.length; ++i) {
                if (pane.pageData.rows[i].id === target) {
                    logicalIndex = i;
                    pane.revealLogical(i);
                    if ((pane.paneIndex === 2 && window.library.song.length > 0)
                        || (pane.paneIndex === 1 && window.library.song.length === 0 && window.library.album.length > 0)
                        || (pane.paneIndex === 0 && window.library.album.length === 0))
                        list.forceActiveFocus();
                    break;
                }
            }
        })
        RowLayout {
            visible: pane.reviewTable
            ComboBox { objectName: "spotifyReviewMode"; model: ["Unresolved", "Marked Not on Spotify"]; currentIndex: window.library.reviewMarked ? 1 : 0; onActivated: window.bridge.browse_action("review-mode",2,currentIndex === 1 ? "marked" : "unresolved") }
            Button { objectName: "spotifyRetryUnresolved"; text: "Retry unresolved"; ToolTip.visible: hovered; ToolTip.text: "Re-evaluates up to 20 Albums using fresh cached discovery where available. Unresolved Tracks can be searched individually. Click again for the next Album batch."; enabled: !window.library.reviewRetryPending && !window.library.reviewLocalActive; visible: !window.library.reviewMarked; onClicked: window.bridge.browse_action("review-retry",2,"") }
            Label { text: window.library.reviewMessage || ""; Layout.fillWidth: true; wrapMode: Text.Wrap }
        }
        RowLayout {
            visible: pane.reviewTable && !!window.library.reviewAlbum
            Label { text: "Reviewing unresolved Tracks in this Album"; Layout.fillWidth: true }
            Button { objectName: "clearReviewAlbum"; text: "All Albums"; onClicked: window.bridge.browse_action("review-album",2,"") }
        }
        ListModel { id: paneRows; dynamicRoles: true }
        function syncRows() {
            const key = window.library.view + ":" + pageData.epoch;
            const signature = JSON.stringify([key, pageData.rows, pageData.sort, paneIndex === 1 ? !!window.library.artist : false]);
            if (signature === rowsSignature) return;
            // Metadata and selection changes keep the actual ListModel and delegates.
            // PlaylistEntry IDs, rather than Track IDs, distinguish repeated occurrences.
            const sameRows = paneIndex !== 1 && pageData.rows.length === renderedRows.length
                && pageData.rows.every((row,i) => row.id === renderedRows[i].id);
            if (sameRows && datasetKey.split(":")[0] === window.library.view) {
                for (let i=0; i<pageData.rows.length; ++i) {
                    const row=pageData.rows[i];
                    if (JSON.stringify(row) !== JSON.stringify(renderedRows[i]))
                        paneRows.setProperty(i,"rowData",row);
                }
                const names={};
                for(let i=0;i<pageData.rows.length;i++) {
                    const row=pageData.rows[i];
                    const albumKey=paneIndex===2 && pageData.sort==="Album" ? row.albumId : "";
                    if(albumKey) names[albumKey]=row.track.release;
                    if(paneRows.get(i).albumKey!==albumKey) paneRows.setProperty(i,"albumKey",albumKey);
                }
                albumNames=names;
                renderedRows=pageData.rows;
                rowsSignature=signature;
                datasetKey=key;
                return;
            }
            if (pane.reviewTable && key === datasetKey && renderedRows.length > 0) {
                const anchor = viewportAnchor();
                const oldIndex = anchor ? renderedRows.findIndex(r => r.id === anchor.id) : -1;
                const ids = new Set(pageData.rows.map(r => r.id));
                let replacement = anchor;
                if (anchor && !ids.has(anchor.id)) {
                    const next = renderedRows.slice(oldIndex+1).find(r => ids.has(r.id)) || renderedRows.slice(0,oldIndex).reverse().find(r => ids.has(r.id));
                    replacement = next ? {id:next.id,pixel:anchor.pixel} : null;
                }
                adjustingWindow = true;
                // Shared window scrolling can add rows; preserve retained delegates.
                for(let i=paneRows.count-1;i>=0;i--) if(!ids.has(paneRows.get(i).rowData.id)) paneRows.remove(i);
                for(let i=0;i<pageData.rows.length;i++) {
                    const row=pageData.rows[i];
                    if(i>=paneRows.count || paneRows.get(i).rowData.id!==row.id) {
                        let existing=-1;
                        for(let j=i+1;j<paneRows.count;j++) if(paneRows.get(j).rowData.id===row.id) { existing=j; break; }
                        if(existing>=0) { paneRows.move(existing,i,1); paneRows.setProperty(i,"rowData",row); }
                        else paneRows.insert(i,{rowData:row,albumKey:""});
                    }
                    else paneRows.setProperty(i,"rowData",row);
                }
                renderedRows=pageData.rows;
                rowsSignature=signature;
                list.forceLayout();
                restoreAnchor(replacement);
                adjustingWindow=false;
                viewportTimer.restart();
                return;
            }
            const sameDataset = key === datasetKey;
            const anchor = sameDataset ? viewportAnchor() : (pageData.scrollId ? {id: pageData.scrollId, pixel: pageData.scrollPixel} : null);
            const focused = logicalIndex >= 0 && renderedRows[logicalIndex] ? renderedRows[logicalIndex].id : "";
            const flickVelocity = sameDataset && list.flicking ? list.verticalVelocity : 0;
            adjustingWindow = true;
            datasetKey = key;
            rowsSignature = signature;
            list.model = null;
            paneRows.clear();
            if (paneIndex === 1) {
                let tiles = [];
                let group = null;
                let groupTitle = "";
                for (let i = 0; i < pageData.rows.length; ++i) {
                    const row = pageData.rows[i];
                    const groupKey = pageData.sort === "Year" ? (row.year || "Unknown") : row.group;
                    const newGroup = (pageData.sort === "Artist" || pageData.sort === "Year") && groupKey !== group;
                    if (tiles.length && newGroup) {
                        paneRows.append({rowData: {tiles: tiles, groupTitle: groupTitle}});
                        tiles = []; groupTitle = "";
                    }
                    if (newGroup) { group = groupKey; groupTitle = pageData.sort === "Year" ? groupKey : (row.groupLabel || "Unknown Artist"); }
                    tiles.push({data: row, logicalIndex: i});
                }
                if (tiles.length) paneRows.append({rowData: {tiles: tiles, groupTitle: groupTitle}});
            } else {
                const names = {};
                for (const row of pageData.rows) {
                    const albumKey = pane.paneIndex === 2 && pageData.sort === "Album" ? row.albumId : "";
                    if (albumKey) names[albumKey] = row.track.release;
                    paneRows.append({rowData: row, albumKey: albumKey || ""});
                }
                pane.albumNames = names;
            }
            list.model = paneRows;
            renderedRows = pageData.rows;
            logicalIndex = sameDataset ? pageData.rows.findIndex(r => r.id === focused) : -1;
            songId = paneIndex === 2 ? window.library.song : "";
            list.currentIndex = logicalIndex >= 0 ? visualIndex(logicalIndex) : -1;
            list.forceLayout();
            if (paneIndex === 1) {
                for (let i = 0; i < paneRows.count; ++i) {
                    const section = list.itemAtIndex(i);
                    if (section) section.layoutTiles();
                }
                list.forceLayout();
            }
            restoreAnchor(anchor);
            // Model replacement stops the animation. Continue its remaining
            // motion with the current velocity after restoring the viewport.
            if (flickVelocity) list.flick(0, -flickVelocity);
            adjustingWindow = false;
            viewportTimer.restart();
        }
        onPageDataChanged: syncRows()
        Component.onCompleted: syncRows()
        Layout.fillHeight: true
        spacing: 0
        function isSelected(id) { return (pageData.selectedIds || []).indexOf(id) >= 0; }
        function selectRow(index, modifiers) {
            if (index < 0 || index >= pageData.rows.length) return;
            logicalIndex = index;
            pane.revealLogical(index);
            songId = pageData.rows[index].id;
            const action = (modifiers & Qt.ShiftModifier) ? "select-range" : (modifiers & Qt.ControlModifier) ? "select-toggle" : "select";
            window.bridge.browse_action(action, paneIndex, songId);
        }
        function playRow(index) {
            if (index >= 0 && index < pageData.rows.length)
                window.bridge.browse_action("play", paneIndex, pageData.rows[index].id);
        }
        RowLayout {
            Layout.fillWidth: true
            Layout.preferredHeight: 44
            Label { text: pane.heading; elide: Text.ElideRight; font.pixelSize: 13; font.bold: true; font.letterSpacing: 1.5; Layout.fillWidth: true; Layout.minimumWidth: 0 }
            ToolButton {
                text: "+"; objectName: "playlistAddButton"; Accessible.name: "Add playlist"
                visible: window.library.view === "Playlists" && pane.paneIndex === 0
                onClicked: playlistAddMenu.popup()
                Menu {
                    id: playlistAddMenu; objectName: "playlistAddMenu"
                    MenuItem { text: "New"; objectName: "playlistNewAction"; onTriggered: { playlistNameDialog.rename=false; playlistName.text=""; playlistNameDialog.open(); } }
                    MenuItem { text: "From Spotify"; objectName: "spotifyPlaylistImportButton"; onTriggered: spotifyPlaylistDialog.open() }
                }
            }
            ToolButton {
                id: sortControl
                objectName: "paneSortControl"
                text: pane.pageData.sort
                background: Rectangle { color: sortControl.hovered ? "#eeece9" : "transparent" }
                contentItem: Label { text: sortControl.text; color: "#827b80"; font.pixelSize: 12; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter }
                visible: text.length > 0 && !pane.songsTable
                font.pixelSize: 12
                Accessible.name: pane.heading + " sort: " + text
                onClicked: window.bridge.browse_action("sort", pane.paneIndex, "")
            }
            ToolButton {
                text: "Show all"
                visible: pane.paneIndex < 2 && pane.selectedId.length > 0
                Accessible.name: "Clear " + pane.heading.toLowerCase() + " selection"
                onClicked: { window.bridge.browse_action("select", pane.paneIndex, ""); list.currentIndex = -1; }
            }
        }
        Rectangle { Layout.fillWidth: true; implicitHeight: 1; color: "#d9d6d3" }
        ColumnLayout {
            id: playlistDetailsPane
            objectName: "playlistDetails"
            visible: window.library.view === "Playlists" && pane.paneIndex === 1
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.margins: 12
            spacing: 12
            readonly property var details: window.library.playlistDetails || {}
            ColumnLayout {
                visible: window.library.panes[0].selectionCount === 1
                Layout.fillWidth: true
                // Keep actions independent of changing metadata/loading height.
                RowLayout {
                    objectName: "playlistDetailsToolbar"
                    Layout.fillWidth: true
                    Button {
                        objectName: "playlistAddTracks"
                        text: "Add Tracks from Catalog"
                        Layout.fillWidth: true
                        enabled: !window.library.pending
                        onClicked: {
                            window.bridge.add_music_action("destination", window.library.panes[0].selectedIds[0]);
                            addMusicPanel.playlistMode = true;
                            addMusicPanel.open();
                        }
                    }
                    Item {
                        implicitWidth: 24
                        implicitHeight: 24
                        Layout.minimumWidth: implicitWidth
                        Layout.maximumWidth: implicitWidth
                        BusyIndicator {
                            objectName: "playlistDetailsBusy"
                            anchors.centerIn: parent
                            width: 24
                            height: 24
                            running: !!playlistDetailsPane.details.pending
                            visible: running
                        }
                    }
                }
                Label { objectName: "playlistDetailsName"; text: parent.parent.details.name || ""; textFormat: Text.PlainText; font.bold: true; wrapMode: Text.Wrap; Layout.fillWidth: true }
                Label { objectName: "playlistDetailsCount"; text: parent.parent.details.count !== undefined && parent.parent.details.count !== "" ? parent.parent.details.count + " tracks" : ""; textFormat: Text.PlainText }
                Label { objectName: "playlistDetailsDuration"; text: parent.parent.details.duration ? "Total duration: " + parent.parent.details.duration : ""; textFormat: Text.PlainText; wrapMode: Text.Wrap; Layout.fillWidth: true }
            }
            Item { Layout.fillHeight: true }
        }
        Item {
            id: tableHeaderViewport
            objectName: pane.songsTable ? "songsTableHeader" : "playlistTableHeader"
            visible: pane.detailsTable
            Layout.fillWidth: true
            Layout.minimumWidth: 0
            Layout.preferredWidth: 0
            Layout.preferredHeight: visible ? 28 : 0
            clip: true
            Row {
                x: -list.contentX
                height: 28
                Repeater {
                    model: pane.songsTable ? [{key:"song",label:"Song"},{key:"artist",label:"Artist"},{key:"album",label:"Album"},{key:pane.reviewTable ? "reason" : "genre",label:pane.reviewTable ? "Reason" : "Genre"}] : [{key:"position",label:"#"},{key:"title",label:"Title"},{key:"artist",label:"Artist"},{key:"album",label:"Album"},{key:"length",label:"Length"}]
                    delegate: Button {
                        id: playlistHeaderButton
                        required property var modelData
                        required property int index
                        objectName: (pane.songsTable ? "songsHeader" : "playlistHeader") + modelData.label
                        width: pane.tableColumns[index] || 0
                        height: 28
                        flat: true
                        leftPadding: 0
                        rightPadding: 0
                        text: modelData.label + ((pane.songsTable ? window.library.songsColumn : window.library.playlistSort) === modelData.key ? ((pane.songsTable ? window.library.songsDescending : window.library.playlistDescending) ? " ▾" : " ▴") : "")
                        font.pixelSize: 11
                        contentItem: Label {
                            text: playlistHeaderButton.text
                            font.pixelSize: 11
                            leftPadding: 6
                            rightPadding: 6
                            verticalAlignment: Text.AlignVCenter
                            elide: Text.ElideRight
                            color: (pane.songsTable ? window.library.songsColumn : window.library.playlistSort) === playlistHeaderButton.modelData.key ? "#242126" : "#68636a"
                        }
                        onClicked: window.bridge.browse_action(pane.songsTable ? "songs-sort" : "playlist-sort",2,modelData.key)
                    }
                }
            }
            Repeater {
                model: pane.tableColumns.length
                delegate: SongColumnDivider {
                    tablePane: pane
                    resizeViewport: tableHeaderViewport
                    scrollOffset: list.contentX
                    objectName: (pane.songsTable ? "songsColumnDivider" : "playlistColumnDivider")+index
                    z: 2
                }
            }
        }
        ListView {
            id: list
            visible: !(window.library.view === "Playlists" && pane.paneIndex === 1)
            objectName: "libraryPane" + pane.paneIndex
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: paneRows
            // Retain the bounded Album window so width reflow cannot recreate
            // section delegates or schedule new artwork requests.
            // Bound by the largest possible window height, including one header
            // per Album. Transient Flow height estimates must not evict sections.
            cacheBuffer: pane.paneIndex === 1 ? pane.pageData.rows.length * (pane.tileHeight + pane.tileGap + 30) : 0
            currentIndex: -1
            activeFocusOnTab: true
            keyNavigationEnabled: false
            // Focus must not scroll an entire Album Flow; revealLogical handles tiles.
            highlightFollowsCurrentItem: false
            section.property: pane.albumSongs ? "albumKey" : ""
            section.criteria: ViewSection.FullString
            section.labelPositioning: ViewSection.InlineLabels
            section.delegate: Label {
                objectName: "songAlbumHeader"
                required property string section
                width: list.width
                height: pane.albumSongs ? 28 : 0
                text: pane.albumNames[section] || "Untitled"
                textFormat: Text.PlainText
                elide: Text.ElideRight
                leftPadding: 9
                verticalAlignment: Text.AlignVCenter
                font.pixelSize: 12
                font.bold: true
                color: "#68636a"
            }
            onContentYChanged: if (!pane.adjustingWindow && !viewportTimer.running) viewportTimer.start()
            onHeightChanged: if (!pane.adjustingWindow) viewportTimer.restart()
            boundsBehavior: Flickable.StopAtBounds
            contentWidth: pane.detailsTable ? Math.max(width,pane.columnsWidth+16) : width
            flickableDirection: pane.detailsTable ? Flickable.HorizontalAndVerticalFlick : Flickable.VerticalFlick
            ScrollBar.horizontal: ScrollBar { policy: pane.detailsTable ? ScrollBar.AsNeeded : ScrollBar.AlwaysOff; active: pane.detailsTable && size<1; minimumSize: 0.04 }
            ScrollBar.vertical: ScrollBar {}
            // Viewport-level strips stay available over rows and empty space.
            // Keeping them outside contentItem also preserves vertical flicking.
            Item {
                id: tableBodyDividers
                parent: list
                visible: pane.detailsTable
                width: list.width-(list.ScrollBar.vertical.visible ? list.ScrollBar.vertical.width : 0)
                height: list.height-(list.ScrollBar.horizontal.visible ? list.ScrollBar.horizontal.height : 0)
                clip: true
                z: 3
                Repeater {
                    model: pane.tableColumns.length
                    delegate: SongColumnDivider {
                        tablePane: pane
                        resizeViewport: tableBodyDividers
                        scrollOffset: list.contentX
                        bodyDivider: true
                        objectName: (pane.songsTable ? "songsBodyColumnDivider" : "playlistBodyColumnDivider")+index
                    }
                }
            }
            Keys.onDownPressed: event => { pane.selectRow(Math.min(pane.pageData.rows.length - 1, pane.logicalIndex + (pane.paneIndex === 1 ? pane.albumColumns : 1)),event.modifiers); }
            Keys.onUpPressed: event => { pane.selectRow(Math.max(0, pane.logicalIndex - (pane.paneIndex === 1 ? pane.albumColumns : 1)),event.modifiers); }
            Keys.onLeftPressed: event => { if (pane.paneIndex === 1) pane.selectRow(Math.max(0, pane.logicalIndex - 1),event.modifiers); }
            Keys.onRightPressed: event => { if (pane.paneIndex === 1) pane.selectRow(Math.min(pane.pageData.rows.length - 1, pane.logicalIndex + 1),event.modifiers); }
            Keys.onReturnPressed: pane.playRow(pane.logicalIndex)
            Keys.onEnterPressed: pane.playRow(pane.logicalIndex)
            Keys.onEscapePressed: {
                if (pane.paneIndex < 2) window.bridge.browse_action("select", pane.paneIndex, "");
                else { window.bridge.browse_action("select",2,""); pane.songId = ""; }
                currentIndex = -1;
            }
            delegate: Rectangle {
                id: row
                opacity: modelData.ignored ? 0.55 : 1
                required property var rowData
                readonly property var modelData: rowData
                required property int index
                width: pane.detailsTable ? list.contentWidth : list.width
                height: pane.paneIndex === 0 ? 34 : pane.paneIndex === 1 ? albumLayout.implicitHeight + pane.tileGap : pane.detailsTable ? 30 : 48
                color: pane.paneIndex === 1 ? "transparent" : pane.isSelected(modelData.id) ? "#e8d9e0" : mouse.containsMouse ? "#eeece9" : "transparent"
                border.width: pane.paneIndex !== 1 && list.activeFocus && list.currentIndex === index ? 1 : 0
                border.color: "#96506d"
                Accessible.role: Accessible.ListItem
                Accessible.name: pane.paneIndex === 1 ? (modelData.groupTitle || "Albums") : modelData.title + " " + modelData.subtitle
                Accessible.selected: pane.isSelected(modelData.id)
                Accessible.description: pane.playlistTable && modelData.track && !modelData.track.available ? "Local unavailable" : ""
                ToolTip.visible: pane.playlistTable && mouse.containsMouse && modelData.track && !modelData.track.available
                ToolTip.text: "Local unavailable; playback will try supported providers"
                ToolTip.delay: 700
                Column {
                    visible: pane.paneIndex !== 1 && !pane.detailsTable
                    anchors.left: parent.left; anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter
                    anchors.leftMargin: 9; anchors.rightMargin: 16
                    spacing: 2
                    Row {
                        width: parent.width
                        spacing: 8
                        Label { id: trackNumber; objectName: pane.playlistTable ? "" : "trackNumber"; visible: pane.paneIndex === 2 && !!row.modelData.number; text: row.modelData.number || ""; width: visible ? Math.max(22, Math.ceil(numberMetrics.advanceWidth)) : 0; color: "#827b80"; font.pixelSize: 14 }
                        TextMetrics { id: numberMetrics; font: trackNumber.font; text: trackNumber.text }
                        Label { objectName: pane.detailsTable ? "" : "songTitle"; width: parent.width - (trackNumber.visible ? trackNumber.width + 8 : 0); text: row.modelData.title || "Untitled"; color: pane.paneIndex === 2 && (row.modelData.track ? row.modelData.track.trackId : row.modelData.id) === window.view.currentId ? "#c6283e" : "#242126"; textFormat: Text.PlainText; elide: Text.ElideRight; font.pixelSize: 14 }
                    }
                    Label {
                        width: parent.width
                        visible: pane.paneIndex > 0
                        objectName: pane.detailsTable ? "" : "songSubtitle"
                        text: pane.paneIndex === 2 ? [row.modelData.subtitle || "Unknown artist", pane.albumSongs ? "" : row.modelData.track.release, window.library.view === "Playlists" && !row.modelData.track.available ? "Local unavailable" : ""].filter(value => value.length > 0).join(" · ") : (row.modelData.subtitle || "")
                        textFormat: Text.PlainText; elide: Text.ElideRight; font.pixelSize: 11; color: "#68636a"
                    }
                }
                Row {
                    visible: pane.detailsTable
                    anchors.verticalCenter: parent.verticalCenter
                    Repeater {
                        model: pane.songsTable && row && row.modelData ? [row.modelData.title || "Untitled",row.modelData.subtitle || "Unknown artist",row.modelData.track ? row.modelData.track.release || "" : "",pane.reviewTable ? row.modelData.connectionReason || "No reconciliation attempted yet" : row.modelData.genres || ""] : pane.playlistTable && row && row.modelData ? [row.modelData.number || "", row.modelData.title || "Untitled", row.modelData.subtitle || "Unknown artist", row.modelData.track ? row.modelData.track.release || "" : "", row.modelData.length || "--:--"] : []
                        delegate: Label {
                            required property var modelData
                            required property int index
                            objectName: (pane.songsTable ? ["songTitle","songsArtist","songsAlbum",pane.reviewTable ? "connectionReason" : "songsGenre"] : ["trackNumber","songTitle","playlistArtist","playlistAlbum","playlistLength"])[index] || ""
                            width: pane.tableColumns[index] || 0
                            leftPadding: 6
                            rightPadding: 6
                            text: modelData
                            textFormat: Text.PlainText
                            elide: Text.ElideRight
                            font.pixelSize: 12
                            height: row.height
                            verticalAlignment: Text.AlignVCenter
                            color: index === (pane.songsTable ? 0 : 1) && row && row.modelData && row.modelData.track && row.modelData.track.trackId === window.view.currentId ? "#c6283e" : "#242126"
                            opacity: pane.playlistTable && row && row.modelData && row.modelData.track && !row.modelData.track.available ? 0.65 : 1
                        }
                    }
                }
                function viewportTileAnchor() {
                    for (let i = 0; i < tileRepeater.count; ++i) {
                        const tile = tileRepeater.itemAt(i);
                        if (!tile) continue;
                        const y = tile.mapToItem(list, 0, 0).y;
                        if (y + tile.height > 0 && y < list.height)
                            return {id: tile.album.id, pixel: y};
                    }
                    return null;
                }
                function layoutTiles() { albumFlow.forceLayout(); }
                function tileAt(logical) {
                    for (let i = 0; i < tileRepeater.count; ++i) {
                        const tile = tileRepeater.itemAt(i);
                        if (tile && tile.modelData.logicalIndex === logical) return tile;
                    }
                    return null;
                }
                Column {
                    id: albumLayout
                    visible: pane.paneIndex === 1
                    width: parent.width
                    Label {
                        visible: text.length > 0
                        height: visible ? 30 : 0
                        text: pane.paneIndex === 1 ? (row.modelData.groupTitle || "") : ""
                        font.pixelSize: 15
                        color: "#68636a"
                    }
                    Flow {
                        id: albumFlow
                        x: pane.tileInset
                        width: Math.max(pane.tileWidth, list.width - 2 * pane.tileInset - pane.scrollAllowance)
                        spacing: pane.tileGap
                        Repeater {
                            id: tileRepeater
                            model: pane.paneIndex === 1 ? row.modelData.tiles : []
                            delegate: Rectangle {
                                id: tile
                                objectName: "albumTile" + modelData.logicalIndex
                                required property var modelData
                                readonly property var album: modelData.data
                                width: pane.tileWidth
                                height: pane.tileHeight
                                color: pane.isSelected(album.id) ? "#e8d9e0" : tileMouse.containsMouse ? "#eeece9" : "transparent"
                                border.width: list.activeFocus && pane.logicalIndex === modelData.logicalIndex ? 1 : 0
                                border.color: "#96506d"
                                Accessible.role: Accessible.ListItem
                                opacity: album.ignored ? 0.55 : 1
                                Accessible.name: album.title + " " + album.subtitle
                                Accessible.selected: pane.isSelected(album.id)
                                Column {
                                    width: parent.width
                                    spacing: 4
                                    AlbumArt { objectName: "albumArtwork"; width: pane.tileWidth; height: pane.tileWidth; artworkKey: tile.album.id }
                                    Label { objectName: "albumTitle"; width: parent.width; text: tile.album.title || "Untitled"; textFormat: Text.PlainText; elide: Text.ElideRight; font.pixelSize: 13 }
                                    Label { objectName: "albumSecondary"; width: parent.width; text: window.library.artist ? (tile.album.year || "Unknown year") : tile.album.subtitle; textFormat: Text.PlainText; elide: Text.ElideRight; font.pixelSize: 11; color: "#68636a" }
                                }
                                MouseArea {
                                    id: tileMouse
                                    anchors.fill: parent
                                    acceptedButtons: Qt.LeftButton | Qt.RightButton
                                    hoverEnabled: true
                                    onClicked: event => {
                                        list.forceActiveFocus();
                                        if (event.button === Qt.RightButton) {
                                            window.bridge.browse_action("context",1,tile.album.id); window.contextPane = 1; window.contextId = tile.album.id; libraryMenu.popup();
                                        } else pane.selectRow(tile.modelData.logicalIndex,event.modifiers);
                                    }
                                    onDoubleClicked: event => { if (event.button === Qt.LeftButton) pane.playRow(tile.modelData.logicalIndex); }
                                }
                            }
                        }
                    }
                }
                MouseArea {
                    id: mouse
                    anchors.fill: parent
                    enabled: pane.paneIndex !== 1
                    acceptedButtons: Qt.LeftButton | Qt.RightButton
                    hoverEnabled: true
                    onClicked: event => {
                        list.forceActiveFocus();
                        if (event.button === Qt.RightButton) {
                            window.openSongContext(pane.paneIndex, row.modelData);
                        } else pane.selectRow(row.index,event.modifiers);
                    }
                    onDoubleClicked: event => { if (event.button === Qt.LeftButton) { if (pane.reviewTable) window.openSpotifyConnection(row.modelData.id); else pane.playRow(row.index); } }
                }
            }
            Label {
                anchors.centerIn: parent
                width: parent.width - 24
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.Wrap
                visible: list.count === 0
                objectName: "libraryEmptyState" + pane.paneIndex
                text: pane.reviewTable ? (window.library.reviewAlbum ? (window.library.reviewMarked ? "No marked Tracks in this Album." : "No unresolved Tracks in this Album.") : (window.library.reviewMarked ? "No Tracks have been marked as not on Spotify." : "All eligible Library Tracks are connected or reviewed.")) : window.library.view === "Playlists" ? (pane.paneIndex === 0 ? "No playlists yet" : "") : pane.paneIndex === 2 ? "No songs in this view" : "No " + pane.heading.toLowerCase() + " in this view"
                color: "#777078"
            }
        }

    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0
        RowLayout {
            Layout.fillWidth: true
            Layout.margins: 16
            RowLayout {
                spacing: 16
                Repeater {
                    model: ["Artists", "Genres", "Albums", "Songs", "Playlists"]
                    delegate: ToolButton {
                        required property string modelData
                        objectName: "libraryView" + modelData
                        text: modelData
                        Accessible.name: modelData + " library view"
                        background: Rectangle { color: parent.hovered ? "#eeece9" : "transparent" }
                        contentItem: Label { text: parent.text; font.pixelSize: 20; font.bold: window.library.view === text; color: window.library.view === text ? "#242126" : "#827b80" }
                        onClicked: {
                            artistsPane.rememberViewport();
                            albumsPane.rememberViewport();
                            songsPane.rememberViewport();
                            window.bridge.browse_action("view", 0, modelData);
                        }
                    }
                }
            }
            Item { Layout.fillWidth: true }
            Button {
                id: query
                Layout.preferredWidth: 260
                text: "Search library…"
                Accessible.name: "Search library"
                onClicked: searchPanel.open()
            }
            Button {
                id: addMusicButton
                objectName: "addMusicButton"
                text: "Add Music"
                onClicked: addMusicChooser.popup()
            }
            ToolButton { text: "⋯"; Accessible.name: "Settings and diagnostics"; onClicked: settingsMenu.popup() }
        }
        SplitView {
            id: librarySplit
            objectName: "librarySplit"
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.leftMargin: 16; Layout.rightMargin: 16
            orientation: Qt.Horizontal
            property bool playlistOrder: false
            function syncOrder() {
                const wanted = window.library.view === "Playlists";
                if (wanted === playlistOrder || count !== 3) return;
                moveItem(wanted ? 1 : 2, wanted ? 2 : 1);
                playlistOrder = wanted;
            }
            Component.onCompleted: syncOrder()
            Connections { target: window; function onLibraryChanged() { librarySplit.syncOrder(); } }
            handle: Rectangle {
                objectName: "librarySplitHandle"
                implicitWidth: 16
                color: "transparent"
                Rectangle {
                    anchors.centerIn: parent
                    width: SplitHandle.hovered || SplitHandle.pressed ? 3 : 1
                    height: parent.height
                    color: SplitHandle.pressed ? "#96506d" : "#dedbd8"
                }
            }
            // The center pane absorbs changes from either divider, keeping the
            // opposite outside pane (and thus the opposite divider) stationary.
            LibraryPane { id: artistsPane; objectName: "artistsPane"; paneIndex: 0; heading: window.library.view === "Genres" ? "GENRES" : window.library.view === "Playlists" ? "PLAYLISTS" : "ARTISTS"; visible: ["Artists", "Genres", "Playlists"].indexOf(window.library.view) >= 0; SplitView.minimumWidth: 160; SplitView.preferredWidth: (librarySplit.width - 32) * 230 / 1020 }
            LibraryPane { id: albumsPane; objectName: "albumsPane"; paneIndex: 1; heading: window.library.view === "Playlists" ? "DETAILS" : "ALBUMS"; visible: ["Artists", "Genres", "Albums", "Playlists"].indexOf(window.library.view) >= 0; SplitView.minimumWidth: window.library.view === "Playlists" ? 220 : tileWidth + 2 * tileInset + scrollAllowance; SplitView.preferredWidth: window.library.view === "Playlists" ? 260 : (librarySplit.width - 32) * 320 / 1020; SplitView.fillWidth: window.library.view !== "Playlists" }
            LibraryPane { id: songsPane; objectName: "songsPane"; paneIndex: 2; heading: window.library.view === "Spotify Connections" ? "SPOTIFY CONNECTIONS · " + window.library.unresolvedCount + " unresolved · " + window.library.markedCount + " marked" : "SONGS"; SplitView.fillWidth: !albumsPane.visible || window.library.view === "Playlists"; SplitView.minimumWidth: 260; SplitView.preferredWidth: (librarySplit.width - 32) * 470 / 1020 }
        }
        Label {
            visible: text.length > 0
            text: window.library.error || window.view.error || (window.library.pending ? "Preparing selection or tracks…" : "")
            textFormat: Text.PlainText
            color: window.library.pending ? "#68636a" : "#9d263d"
            wrapMode: Text.Wrap
            Layout.fillWidth: true; Layout.margins: visible ? 12 : 0
        }
        Rectangle {
            id: player
            Layout.fillWidth: true
            implicitHeight: 120
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
                    padding: 0
                    Layout.fillWidth: true
                    flat: true
                    Accessible.name: "Now Playing: " + window.view.currentTitle
                    onClicked: { if (queueDrawer.opened) queueDrawer.close(); else queueDrawer.open(); }
                    contentItem: RowLayout {
                        spacing: 10
                        AlbumArt {
                            Layout.preferredWidth: 46; Layout.preferredHeight: 46
                            artworkKey: window.view.currentId ? "track:" + window.view.currentId : ""
                        }
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 4
                            Label { Layout.fillWidth: true; text: window.view.position < 0 ? "Choose something to play" : window.view.currentTitle; font.pixelSize: 16; textFormat: Text.PlainText; elide: Text.ElideRight }
                            Label { Layout.fillWidth: true; text: window.view.position < 0 ? "Now Playing ⌃" : window.view.currentArtist + " — " + window.view.currentAlbum + "  ⌃"; color: "#68636a"; textFormat: Text.PlainText; elide: Text.ElideRight }
                        }
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
                Keys.onDownPressed: event => { searchResults.forceActiveFocus(); searchResults.currentIndex = 0; }
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
                            text: searchPanel.filter === 4 ? "Playlist search is not available yet" : searchText.text.trim().length === 0 ? "Type to search your library" : "No matching library items"
                        }
                    }
                    Label { visible: searchPanel.filter === 0; text: "PLAYLISTS · Search is not available yet"; font.pixelSize: 11; color: "#68636a" }
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
        MenuItem { text: "Hidden Artists"; objectName: "hiddenArtistsAction"; onTriggered: { preferenceManager.hiddenArtists=true; window.bridge.browse_action("preference-hidden",0,""); preferenceManager.open(); } }
        MenuItem { text: "Ignored Songs"; objectName: "ignoredSongsAction"; onTriggered: { preferenceManager.hiddenArtists=false; window.bridge.browse_action("preference-ignored",2,""); preferenceManager.open(); } }
        MenuItem { objectName: "spotifyConnectionsReview"; text: "Spotify Connections"; onTriggered: { artistsPane.rememberViewport(); albumsPane.rememberViewport(); songsPane.rememberViewport(); window.bridge.browse_action("view", 2, "Spotify Connections"); } }
        MenuItem { text: "Output calibration…"; onTriggered: outputCalibration.open() }
        MenuItem { text: "Playback connection…"; onTriggered: window.openSpotifyConnection(window.view.currentId) }
        MenuItem { text: "Local Album matches…"; onTriggered: matchingDialog.open() }
        MenuItem { text: "Retry matching"; visible: window.bridge.matching_provider.paused; enabled: !window.bridge.matching_provider.probe; onTriggered: window.bridge.retry_matching() }
    }
    // Existing diagnostic assertions remain available without occupying the player.
    Label { visible: false; text: "Current: " + window.view.currentTitle + " · " + window.view.status + window.view.pending + " · " + window.view.time }
    Label { visible: false; text: "State update " + window.view.revision + " · " + window.view.outcome }
}
