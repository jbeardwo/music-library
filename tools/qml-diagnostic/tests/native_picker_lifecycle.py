#!/usr/bin/env python3
"""Compile/run actual Qt Widgets lifecycle/selection tests using the shipped QML.

Requires Qt development tools, including QtTest. Offscreen runs still operate on
real QFileDialog widgets, rather than emitting logical QML rejection signals.
Run with QT_QPA_PLATFORM=offscreen when no desktop display is available.
"""
from pathlib import Path
import subprocess
import tempfile

here = Path(__file__).resolve().parent
main = (here.parent / "Main.qml").read_text()
start = main.index("    PlatformDialogs.FileDialog {")
end = main.index("    Timer {", start)
pickers = main[start:end]
with tempfile.TemporaryDirectory(prefix="music-native-picker-") as directory:
    work = Path(directory)
    (work / "Pickers.qml").write_text('''import QtQuick
import QtQuick.Controls.Basic
import Qt.labs.platform as PlatformDialogs
ApplicationWindow {
    id: window
    visible: true
    width: 700
    height: 500
    readonly property var bridge: diagnostic
''' + pickers + "}\n")
    (work / "native_picker_lifecycle.cpp").write_text((here / "native_picker_lifecycle.cpp").read_text())
    (work / "test.pro").write_text("QT += widgets quick qml testlib\nCONFIG += console c++17\nSOURCES += native_picker_lifecycle.cpp\nTARGET = native_picker_lifecycle\n")
    subprocess.run(["qmake6", "test.pro"], cwd=work, check=True, stdout=subprocess.DEVNULL)
    subprocess.run(["make", "-j2"], cwd=work, check=True, stdout=subprocess.DEVNULL)
    subprocess.run([str(work / "native_picker_lifecycle"), str(work / "Pickers.qml")], check=True)
