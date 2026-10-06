    TestCase { id: identityTest; when: false }
    function identityCheck(ok,message) { if (!ok) throw new Error(message); }
    function identityInspect() {
        try {
            const trace=spotifyComparison.trace;
            identityCheck(trace.candidate_count === 1,"one candidate count");
            identityCheck(trace.candidates[0].primary_code === "artist_mismatch","actual Artist blocker");
            const fields=trace.candidates[0].fields;
            identityCheck(fields.map(f=>f.label).join("|") === "Song|Artist credit|Album|Release type|Disc|Track|Track count|Duration|Release date|Trusted external IDs|Artist IDs","shared field order");
            identityCheck(fields[1].status === "conflict","Artist difference highlighted");
            identityCheck(fields[0].status === "equivalent","agreement shown");
            spotifySongChoice.currentIndex=0;identityTest.wait(50);
            const artistRow=identityTest.findChild(spotifyComparison,"spotifyComparisonFieldArtist credit");
            identityCheck(artistRow !== null,"Artist comparison rendered");
            identityCheck(identityTest.findChild(artistRow,"spotifyLocalValue").text===fields[1].local,"local value visible");
            identityCheck(identityTest.findChild(artistRow,"spotifyCandidateValue").text===fields[1].candidate,"candidate value visible");
            identityCheck(identityTest.findChild(spotifyPlaybackDialog,"spotifyPrimaryBlocker").text.indexOf("not established as equivalent")>=0,"human readable blocker");
            identityTest.findChild(spotifyPlaybackDialog,"spotifyArtistsSame").clicked();identityTest.wait(50);
            identityCheck(artistEquivalenceDialog.visible,"confirmation required");
            identityCheck(artistEquivalenceDialog.contentItem.text.indexOf("Other matching evidence is still required")>=0,"scope explained");
            identityCheck(artistEquivalenceDialog.contentItem.text.indexOf(fields[1].local)>=0 && artistEquivalenceDialog.contentItem.text.indexOf(fields[1].candidate)>=0,"both Artists shown");
            artistEquivalenceDialog.reject();identityTest.wait(50);
            return "ok";
        }catch(e){return String(e);}
    }
    function identityConfirm() {
        try {
            spotifySongChoice.currentIndex=0;identityTest.wait(50);
            identityTest.findChild(spotifyPlaybackDialog,"spotifyArtistsSame").clicked();identityTest.wait(50);
            identityCheck(artistEquivalenceDialog.visible,"explicit dialog before confirmation");
            artistEquivalenceDialog.accept();identityTest.wait(50);
            identityCheck(!artistEquivalenceDialog.visible,"confirmed");return "ok";
        }catch(e){return String(e);}
    }
    property string identityPlaybackBefore: ""
    function identityLiveSearch(artist) {
        identityPlaybackBefore=JSON.stringify([view.currentId,view.queue,view.status]);
        spotifySearchArtist.text=artist;
        identityTest.findChild(spotifyPlaybackDialog,"spotifyConnectionSearch").clicked();
        for(let n=0;n<2000 && spotifyPlayback.resolutionPending;n++) identityTest.wait(20);
        return !spotifyPlayback.resolutionPending;
    }
    function identityWaitAlbum() {
        for(let n=0;n<2000 && spotifyPlayback.albumPending;n++) identityTest.wait(20);
        identityTest.wait(100);
        return !spotifyPlayback.albumPending;
    }

    function reviewLiveState(track,connected) {
        try {
            identityTest.wait(100);
            identityCheck(identityPlaybackBefore===JSON.stringify([view.currentId,view.queue,view.status]),"playback and queue unchanged");
            identityCheck(spotifyPlayback.trackId===track && spotifyPlayback.available===connected,"source resolution follows trusted association");
            identityCheck(library.panes[2].rows.some(r=>r.id===track)!==connected,"targeted review removal");
            spotifyPlaybackDialog.close();return "ok";
        }catch(e){return String(e);}
    }

    function identityProgramDetails() {
        try {
            identityCheck(spotifyComparison.trace.programs.length>0,"actual Album program retained");
            spotifyProgramExpanded.checked=true;identityTest.wait(100);
            const first=spotifyComparison.trace.programs[0].rows[0];
            const row=identityTest.findChild(spotifyComparison,"spotifyProgramRow");
            identityCheck(row!==null,"expanded program row rendered");
            identityCheck(identityTest.findChild(row,"spotifyProgramLocal").text===first.local,"local program shown");
            identityCheck(identityTest.findChild(row,"spotifyProgramCandidate").text===first.candidate,"provider program shown");
            spotifyProgramExpanded.checked=false;return "ok";
        }catch(e){return String(e);}
    }
