These three tiny silent audio containers are generated test inputs, not music.
Tests copy them to temporary files and add tags with pinned Lofty. No codec tools
or audio hardware are needed to run tests. Native tag conversion tests separately
verify the raw Picard field names, avoiding reliance on round-trip tests alone.

Generation (GStreamer, one 1024-sample silent mono buffer at 44100 Hz):

```sh
gst-launch-1.0 -q audiotestsrc wave=silence num-buffers=1 samplesperbuffer=1024 ! audio/x-raw,rate=44100,channels=1 ! flacenc ! filesink location=silence.flac
gst-launch-1.0 -q audiotestsrc wave=silence num-buffers=1 samplesperbuffer=1024 ! audioconvert ! audio/x-raw,rate=44100,channels=1 ! lamemp3enc ! filesink location=silence.mp3
gst-launch-1.0 -q audiotestsrc wave=silence num-buffers=1 samplesperbuffer=1024 ! audioconvert ! audio/x-raw,rate=44100,channels=1 ! avenc_aac ! mp4mux ! filesink location=silence.m4a
```
