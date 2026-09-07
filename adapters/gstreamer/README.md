# First local-audio adapter

This optional package implements `PlaybackEngine` with GstPlay. It is an empirical
backend choice, not a permanent architecture commitment. Queue policy stays in
`music_library::playback`; no GStreamer types reach domain data or QML.

GstPlay provides application playback over playbin3, state/error/EOS messages,
and media timing without a custom decoder pipeline
([GstPlay API](https://gstreamer.freedesktop.org/documentation/play/gstplay.html)).
The lockfile currently selects stable Rust `gstreamer` 0.25.3 and
`gstreamer-play` 0.25.0. No seeking, preloading, network URI input, or device UI is added.

## Setup and launch

Install Qt as described in the [diagnostic guide](../../tools/qml-diagnostic/README.md).
GstPlay requires GStreamer 1.20+ and its development libraries. On this Ubuntu 26.04
machine, GstPlay development files are in the separate
[plugins-extra package](https://packages.ubuntu.com/resolute/libgstreamer-plugins-extra1.0-dev):

```sh
sudo apt-get install libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
  libgstreamer-plugins-extra1.0-dev gstreamer1.0-tools \
  gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-libav \
  gstreamer1.0-pulseaudio
pkg-config --modversion gstreamer-play-1.0
cargo run --manifest-path tools/qml-diagnostic/Cargo.toml --features gstreamer \
  -- --gstreamer "$HOME/Music/diagnostic"
```

Other distributions may package GstPlay development files under plugins-bad;
the decisive check is that `pkg-config` resolves `gstreamer-play-1.0`. The real
adapter has no dependency on Qt, and normal backend/fake builds require neither
GStreamer headers nor plugins.

Real mode scans the supplied folder before creating the window, using the existing
scanner and importer. It creates a temporary database, conservatively imports one
Release per source, and explicitly adds those Tracks to that disposable library.
It neither edits the music files nor infers shared Release identity from tags.
Use a small folder; startup scanning is synchronous and metadata errors are reported
on the terminal. The database disappears on exit. Launch without arguments/features
for the unchanged synthetic library and fake engine:

```sh
cargo run --manifest-path tools/qml-diagnostic/Cargo.toml
```

## Manual audible smoke test

Use two short, known-good WAV/FLAC/MP3 files. Keep Windows output volume at a
comfortable level; this diagnostic has no volume or device-selection UI.

1. Put copies under `$HOME/Music/diagnostic` in WSL. Run the real-mode command above.
   The window must identify **Real local audio**, show available results, and start
   with an empty queue. Click **Add to queue** on each Track, then **Play / resume**.
   Expect a pending Playing target followed by confirmed Playing and audible sound.
2. Wait several seconds. Confirm `mm:ss / mm:ss` advances and duration becomes known.
   Click **Pause**, wait for confirmed Paused, and wait three seconds. Sound and
   position should remain paused. **Play / resume** should continue from that position.
3. Click **Stop** and wait for confirmation. Sound must end, displayed position
   become `00:00`, and queue/current entry remain. **Play / resume** must begin that
   Track again from the start, not from its previously paused position.
4. Let the first Track finish naturally. Confirm exactly one automatic advance to
   the second, which starts at the beginning. Both queue entries remain visible.
   At the second Track's EOS, expect Stopped at `00:00`, retaining the final index.
5. Add the same Track twice. Exercise Next/Previous, then rapid Next/Stop/Play while
   an input is starting. Old EOS/error/state messages must not change the newly
   selected Track. Previous/Next do not wrap or automatically skip failures.
6. Close the window while playing. Audio should stop and the process should exit.
7. Repeat steps 1–6 using an existing small Windows folder, changing the launch path:

   ```sh
   cargo run --manifest-path tools/qml-diagnostic/Cargo.toml --features gstreamer \
     -- --gstreamer "/mnt/c/Users/YOUR_WINDOWS_USER/Music/diagnostic"
   ```

   Include filenames with spaces, `#`, `%`, and Unicode. Pass WSL-native `/mnt/c/...`
   paths, not `C:\...` strings. The adapter uses
   [GLib filename_to_uri](https://docs.gtk.org/glib/func.filename_to_uri.html) to escape
   native paths; there is no string concatenation or Windows-path rewriting.
8. For a safe error check, rename a **disposable copy** after the window has scanned
   it but before playing it. The stored availability observation may still say
   available; playback should report the file-open error and Failed. Navigation to
   another playable entry should recover without a separate Stop. Restore the copy.

A missing/unplayable next entry must surface an error and remain selected at EOS;
it must not silently advance twice. The fake-mode fixture also exercises the
separate no-source and unavailable-source cases without modifying real files.

## Event flow and lifetime

An owned worker receives engine commands and drains the GstPlay message bus
(nonblocking bus reads, up to 20 ms idle wait). GstPlay's internal machinery performs
decoding/output. The worker sends plain generation-tagged events to a callback;
Qt's queued callback applies them to `Playback` on the GUI thread, then notifies QML.
`pending` records an accepted command; `status` is confirmed. Search/source lookup
still use the application's existing synchronous APIs.

A fresh GstPlay instance belongs to each Start generation. Its messages retain that
generation, never a mutable “latest generation” read at delivery time. Stop and
replacement invalidate previous inputs immediately. Consumed EOS/error also retire
the input, so duplicate or delayed terminal events cannot advance twice. Pause/resume
keep that instance; state messages only confirm the latest requested target. EOS
and decoder errors still belong to the same media lifetime across pause/resume.
This does not attempt to assign arbitrary GstPlay messages to operation IDs that
GstPlay does not supply.

EOS calls application Next. Stop releases the input and resets the application
clock; a later Play creates a fresh player/source. Duration is optional and position
updates are requested every 200 ms. There is no QML polling, seek slider, or generic
notification framework. Routine position/state updates do not clear a visible error.

On replacement and Stop, the worker calls stop, flushes the bus, and releases the
player before proceeding. GstPlay documents the bus/player reference cycle and
[requires flushing before final unref](https://gstreamer.freedesktop.org/documentation/play/gstplay.html).
No signal adapter, bus watch, or application GLib loop is installed. Shutdown joins
the worker before Qt/callback-target destruction. A stuck plugin can still delay
shutdown; the worker is never detached to hide that condition.

## Validation and limits

Hardware-free tests use generated PCM WAV files and a synchronized `fakesink`.
With GStreamer 1.28.2, a measured run held 404 ms throughout a 300 ms pause, resumed
past it, and restarted at 6 ms. After Stopped, GstPlay's getter still returned its
cached 606 ms value. This is observed behavior, not a universal timing guarantee;
the application resets its own stopped clock. A weak-reference test confirmed
player release after flushing. Actual bus EOS also drove a two-entry application
queue to completion without consuming entries.

The read-only `/mnt/c/Windows/Media/Windows Notify.wav` probe decoded through EOS
with `fakesink`. Reserved/Unicode and Unix non-UTF8 filename URI round trips pass.
No mounted-path decoding issue was observed in these cases. Audible WSLg output
was independently confirmed with `gst-launch-1.0 audiotestsrc wave=sine !
audioconvert ! audioresample ! autoaudiosink`. The diagnostic's normal GstPlay
configuration also reached Playing with advancing position on an MP3 under
`/mnt/f/music/...`, including pause/resume and stop/restart, without a test sink.
The observed `No GstStream on pad ??` warning did not prevent these transitions.
Listening to music from the diagnostic still requires the manual check above.

The initial frozen Stopped display was a Qt callback initialization bug, not an
observed terminal GstPlay startup event: URI load → Buffering → Playing arrived
on the bus, but the prematurely captured null QPointer discarded notifications.
Creating its C++ target first restored delivery; generation/state filtering was
unchanged. A sandboxed attempt could not connect to PulseAudio; the successful
real-output trace ran outside that sandbox.

Validation used installed GStreamer 1.28.2 runtime/base development libraries plus
an unpacked Ubuntu plugins-extra development package in `/tmp`, because sudo
required interactive authentication. `PKG_CONFIG_PATH` selected that temporary SDK;
no system libraries, production configuration, or repository build paths were
modified. Install the package above for ordinary reproducible builds.

Before seeking, define seek completion, clock discontinuities, and stale seek
acknowledgements. Before gapless playback, reconsider the one-player-per-input
lifetime and ownership of preloaded inputs/EOS. Also investigate device failure,
long-running shutdown, and list snapshot churn from timing updates. These results
increase confidence in GstPlay for local playback, not yet in audible WSLg routing,
gapless output, cross-platform packaging, or a permanent backend choice.

## Checks

From the repository root, in addition to normal backend and QML checks:

```sh
cargo fmt --manifest-path adapters/gstreamer/Cargo.toml --all -- --check
cargo test --manifest-path adapters/gstreamer/Cargo.toml
cargo clippy --manifest-path adapters/gstreamer/Cargo.toml --all-targets --all-features -- -D warnings
cargo build --manifest-path adapters/gstreamer/Cargo.toml
QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software \
  cargo test --manifest-path tools/qml-diagnostic/Cargo.toml --all-features
cargo build --manifest-path tools/qml-diagnostic/Cargo.toml --features gstreamer
QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software \
  tools/qml-diagnostic/target/debug/qml-diagnostic --smoke-test
```

Real-mode startup/teardown can also be checked without playing sound. This loads
the actual QML window and imports the folder, then joins the idle audio worker:

```sh
QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software \
  tools/qml-diagnostic/target/debug/qml-diagnostic \
  --gstreamer "$HOME/Music/diagnostic" --smoke-test
```

The optional read-only file probe accepts a short file (under ten seconds) and
never uses an audio device:

```sh
MUSIC_LIBRARY_TEST_AUDIO='/mnt/c/Windows/Media/Windows Notify.wav' \
  cargo test --manifest-path adapters/gstreamer/Cargo.toml supplied_file_decodes \
  -- --ignored
```


### Output failure versus silent playback

A later real-mode trace found WSLg PulseAudio refusing connections; a direct
`gst-launch … ! pulsesink` check failed too. The tested MP3 reached Playing in
about 50 ms, so this run did not reproduce the reported long startup delay.
[GstAutoDetect](https://github.com/GStreamer/gstreamer/blob/1.28.2/subprojects/gst-plugins-good/gst/autodetect/gstautodetect.c)
can replace all failed output candidates with a silent fakesink and post only a
warning. Playing and advancing position therefore do not prove real output.
The adapter now checks for that automatic fallback at Playing confirmation,
stops it, and reports an output error with the last GstPlay warning. Explicit
test sinks remain supported. This does not select an audio device or repair the
host audio service; retest the WSLg connection independently if it fails.


### Diagnostic volume

The 0–100% slider calls the application boundary with a finite 0–1 value, default
1. The worker applies [GstPlay.set_volume](https://gstreamer.freedesktop.org/documentation/play/gstplay.html#gst_play_set_volume)
to the current player and retains it for subsequent inputs, applying it before
Play. No input replacement or media generation change is involved. Displayed
volume is the accepted session setting, not a readback of a system mixer.

The value maps directly to [playbin3's linear gain](https://gstreamer.freedesktop.org/documentation/playback/playbin3.html#playbin3:volume).
A perceptual slider curve is deferred UX work. For manual testing, lower volume
while playing, change it while paused, then stop/restart and advance to another
Track; the percentage should persist and playback position should not reset
merely from a volume change. Volume resets to 100% on a new session.
