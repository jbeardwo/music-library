    TestCase { id: albumReviewTest; when:false }
    function albumReviewRapidTargets(trackA,trackB,albumA,albumB) {
        try {
            for(let i=0;i<5;++i) {
                window.openSpotifyConnection(trackA);
                spotifySongChoice.currentIndex=1;
                window.openSpotifyConnection(trackB);
                albumReviewCheck(spotifyPlayback.trackId===trackB,"new Track ID is immediate");
                albumReviewCheck(spotifyPlayback.title.indexOf("Fresh Target")>=0,"new title replaces old title");
                albumReviewCheck(JSON.parse(spotifyPlayback.comparisonJson).candidate_count===0,"previous Track candidates cleared");
                albumReviewCheck(spotifySongChoice.currentIndex===-1,"candidate index resets");
                window.openSpotifyAlbumConnection(albumA);
                spotifyAdvancedMapping.chosen={oldTarget:1};
                window.openSpotifyAlbumConnection(albumB);
                albumReviewCheck(spotifyAlbumDialog.inspection.known_album==="Fresh Target","new Album title belongs to B");
                albumReviewCheck(spotifyAlbumDialog.inspection.count===0 && !spotifyAlbumDialog.inspection.candidate,"previous Album comparison cleared");
                albumReviewCheck(Object.keys(spotifyAdvancedMapping.chosen).length===0,"mapping state reset");
            }
            return "ok";
        } catch(e) {return String(e);}
    }
    function albumReviewCheck(value,message) {if(!value) throw new Error(message);}
    function albumReviewOpen(track) {
        try {
            window.reviewColumnWidths=[260,160,220,300];
            bridge.browse_action("view",2,"Spotify Connections");albumReviewTest.wait(80);
            const groups=albumReviewTest.findChild(songsPane,"spotifyAlbumGroups");
            albumReviewCheck(groups.visible && groups.count===1,"one Album group");
            const row=groups.itemAtIndex(0);albumReviewCheck(row!==null,"Album delegate exists");
            groups.expanded=({[groups.model.get(0).group.id]:true});albumReviewTest.wait(30);
            albumReviewCheck(row.tracks.length===3,"expanded unresolved Tracks");
            window.openSpotifyConnection(track);albumReviewTest.wait(30);
            const trace=JSON.parse(spotifyPlayback.comparisonJson);
            albumReviewCheck(trace.candidate_count===2,"Track inspection retains both candidates");
            albumReviewCheck(trace.selected_candidate===0,"preferred candidate distinct from first inspected candidate");
            spotifySongChoice.currentIndex=0;albumReviewTest.wait(10);
            spotifySongChoice.currentIndex=1;albumReviewTest.wait(10);
            albumReviewCheck(JSON.parse(spotifyPlayback.comparisonJson).selected_candidate===0,"inspection cannot alter preference");
            albumReviewTest.findChild(spotifyPlaybackDialog,"spotifyTrackReviewAlbum").clicked();albumReviewTest.wait(30);
            albumReviewCheck(spotifyAlbumDialog.opened,"Track elevates to Album review");
            return "ok";
        } catch(e) {return String(e);}
    }
    function albumReviewNavigate() {
        try {
            spotifyAlbumDialog.inspect(0);albumReviewTest.wait(30);
            albumReviewCheck(spotifyAlbumDialog.inspection.count===2,"all Album candidates");
            const decision=albumReviewTest.findChild(spotifyAlbumDialog,"spotifyAutomaticDecision");
            albumReviewCheck(decision && decision.visible,"automatic decision is visible");
            albumReviewCheck(!!spotifyAlbumDialog.inspection.primary_reason,"blocked evaluator has primary reason");
            albumReviewCheck(decision.text.indexOf(spotifyAlbumDialog.inspection.primary_reason)>=0,"decision displays authoritative evaluator blocker");
            albumReviewCheck(spotifyAlbumDialog.inspection.programs[0].rows.length===3,"positioned comparison");
            spotifyAlbumDialog.inspect(1);albumReviewTest.wait(10);
            albumReviewCheck(spotifyAlbumDialog.inspection.index===1,"second candidate inspected");
            albumReviewCheck(spotifyAlbumDialog.inspection.plan.requires_confirmation,"significant differences require confirmation");
            albumReviewTest.findChild(spotifyAlbumDialog,"connectSpotifyAlbum").clicked();albumReviewTest.wait(20);
            albumReviewCheck(spotifyAlbumDifferences.opened,"significant-difference confirmation appears");
            spotifyAlbumDifferences.reject();albumReviewTest.wait(20);
            spotifyAlbumDialog.inspect(0);albumReviewTest.wait(10);
            albumReviewCheck(!spotifyAlbumDialog.inspection.plan.requires_confirmation,"harmless differences require no extra confirmation");
            return "ok";
        } catch(e) {return String(e);}
    }
    function albumReviewConnect() {
        try {
            spotifyAlbumDialog.connectCandidate(false);albumReviewTest.wait(150);
            const groups=albumReviewTest.findChild(songsPane,"spotifyAlbumGroups");
            albumReviewCheck(groups.count===0,"resolved Album disappears immediately");
            albumReviewCheck(window.reviewColumnWidths[0]===260,"column widths retained");
            albumReviewCheck(songsPane.reviewShowTracks===false,"group mode retained");
            return "ok";
        } catch(e) {return String(e);}
    }

    function albumReviewAdvanced() {
        try {
            const data=JSON.parse(bridge.spotify_review_action("mapping",spotifyAlbumDialog.albumId,0));
            albumReviewCheck(data.rows.length===3 && data.options.length===6,"one Album context and all cached program options");
            const duplicate=JSON.parse(bridge.spotify_review_action("prepare-mapping",JSON.stringify([[data.rows[0].id,data.options[0].identity.external_id],[data.rows[1].id,data.options[0].identity.external_id]]),0));
            albumReviewCheck(!!duplicate.error,"duplicate provider assignment rejected");
            const plan=JSON.parse(bridge.spotify_review_action("prepare-mapping",JSON.stringify([[data.rows[0].id,data.options[0].identity.external_id],[data.rows[1].id,data.options[4].identity.external_id]]),0));
            albumReviewCheck(plan.matches.length===2 && plan.requires_confirmation,"partial split-release mapping reviewed before confirmation");
            spotifyAlbumDialog.inspect(0);
            return "ok";
        } catch(e) {return String(e);}
    }
