#!/usr/bin/env python3
"""Render the production AlbumArt component beside the previous 500px load path.

Usage: QT_QPA_PLATFORM=offscreen python3 album_rendering.py OUT.png [CACHED.png ...]
Uses only supplied cached files and disposable generated test images; no providers.
"""
from pathlib import Path
import subprocess
import sys
import tempfile

here = Path(__file__).resolve().parent
main = (here.parent / "Main.qml").read_text()
component = main[main.index("    component AlbumArt:"):main.index("    component LibraryPane:")]
with tempfile.TemporaryDirectory(prefix="music-album-render-") as directory:
    work = Path(directory)
    (work / "Render.qml").write_text('''import QtQuick
import QtQuick.Window
import QtQuick.Controls.Basic
ApplicationWindow {
    id: window
    visible: true
    width: 660
    height: 40 + diagnostic.samples.length * 150
    readonly property var bridge: diagnostic
    function requestArtwork(key) { if (key) bridge.artwork_batch([key]); }
''' + component + '''
    Column {
        x: 12; y: 8; spacing: 8
        Row {
            spacing: 12
            Label { width: 120; text: "Previous: 500px decode" }
            Label { width: 120; text: "New: display-sized" }
            Label { width: 120; text: "Player: stays 46px" }
        }
        Repeater {
            model: diagnostic.samples
            delegate: Column {
                required property var modelData
                spacing: 4
                Row {
                    spacing: 12
                    Image { width: 120; height: 120; source: modelData.url; sourceSize: Qt.size(500,500); fillMode: Image.PreserveAspectFit }
                    AlbumArt { width: 120; height: 120; artworkKey: modelData.key }
                    AlbumArt { width: 46; height: 46; artworkKey: modelData.key }
                }
                Label { text: modelData.name }
            }
        }
    }
}
''')
    (work / "album_rendering.cpp").write_text((here / "album_rendering.cpp").read_text())
    (work / "test.pro").write_text("QT += widgets quick qml testlib\nCONFIG += console c++17\nSOURCES += album_rendering.cpp\nTARGET = album_rendering\n")
    subprocess.run(["qmake6", "test.pro"], cwd=work, check=True, stdout=subprocess.DEVNULL)
    subprocess.run(["make", "-j2"], cwd=work, check=True, stdout=subprocess.DEVNULL)
    subprocess.run([str(work / "album_rendering"), str(work / "Render.qml"), *sys.argv[1:]], check=True)
