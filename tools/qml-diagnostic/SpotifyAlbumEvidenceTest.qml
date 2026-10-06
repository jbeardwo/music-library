    TestCase { id: albumEvidenceTest; when: false }
    function exerciseSpotifyAlbumEvidence(track, playing) {
        function check(value, message) { if (!value) throw new Error(message); }
        function wait() { albumEvidenceTest.wait(30); }
        try {
            window.width = 1180; window.height = 800;
            wait();
            const before = JSON.stringify([view.currentId, view.queue, view.status]);
            check(view.currentId === playing, "different Now Playing Track");
            window.openSpotifyConnection(track); wait();
            const retry = albumEvidenceTest.findChild(spotifyPlaybackDialog, "spotifyAlbumReevaluate");
            const evidence = albumEvidenceTest.findChild(spotifyPlaybackDialog, "spotifyAlbumEvidence");
            check(retry && retry.enabled && evidence, "existing diagnostic exposes bounded Album evaluation");
            check(!spotifyPlayback.albumPending && !spotifyPlayback.available, "opening does not evaluate or associate");
            albumEvidenceTest.mouseClick(retry);
            for (let n = 0; n < 250 && spotifyPlayback.albumPending; ++n) wait();
            wait();
            check(!spotifyPlayback.albumPending && spotifyPlayback.available, "explicit retry persists association: " + spotifyPlayback.albumExplanation);
            check(spotifyPlayback.trackId === track, "retry retains clicked Track");
            check(evidence.text.indexOf("Accepted") >= 0 && evidence.text.indexOf("release-type suffix normalized") >= 0, "real decision reason rendered");
            check(evidence.text.indexOf("3 / 3 local positions corroborated") >= 0, "program evidence rendered");
            check(evidence.text.indexOf("Hugs EP") >= 0 && evidence.text.indexOf("Hugs") >= 0 && evidence.text.indexOf("spotify:album:") >= 0, "raw titles and candidate identity rendered");
            check(!retry.enabled, "connected association is retained");
            check(JSON.stringify([view.currentId, view.queue, view.status]) === before, "retry does not start or switch playback");
            albumEvidenceTest.grabImage(spotifyPlaybackDialog.contentItem).save("/tmp/spotify-album-evidence.png");
            spotifyPlaybackDialog.close(); wait();
            return "ok";
        } catch (e) { return String(e); }
    }
    function exerciseLiveAlbumEvidence(track, shouldConnect, expectedReason) {
        function check(value, message) { if (!value) throw new Error(message); }
        try {
            const before = JSON.stringify([view.currentId, view.queue, view.status]);
            window.openSpotifyConnection(track); albumEvidenceTest.wait(50);
            check(!spotifyPlayback.available, "fresh unresolved real Track");
            const retry = albumEvidenceTest.findChild(spotifyPlaybackDialog, "spotifyAlbumReevaluate");
            albumEvidenceTest.mouseClick(retry);
            for (let n = 0; n < 1000 && spotifyPlayback.albumPending; ++n) albumEvidenceTest.wait(30);
            check(!spotifyPlayback.albumPending, "bounded live evaluation completed");
            check(spotifyPlayback.trackId === track, "canonical selected identity retained");
            check(spotifyPlayback.available === shouldConnect, "expected conservative association decision: " + spotifyPlayback.albumExplanation);
            check(spotifyPlayback.albumExplanation.indexOf(expectedReason) >= 0, "specific real decision reason rendered: " + spotifyPlayback.albumExplanation);
            check(JSON.stringify([view.currentId, view.queue, view.status]) === before, "real diagnostic retry preserves playback");
            spotifyPlaybackDialog.close(); albumEvidenceTest.wait(30);
            return "ok";
        } catch (e) { return String(e); }
    }
