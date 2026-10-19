    TestCase {id:reviewShellTest;when:false}
    function reviewShellCheck(value,message){if(!value)throw new Error(message);}
    function reviewShellWait(){reviewShellTest.wait(40);}
    function exerciseSharedReview(album,track,single,zero) {
        try {
            window.showConnections("spotify");reviewShellWait();
            connectionList.forceLayout();const group=connectionList.itemAtIndex(0);
            reviewShellTest.findChild(group,"connectionAlbumExpand").clicked();reviewShellWait();
            connectionsPage.matchTrack(album,track);reviewShellWait();
            reviewShellCheck(connectionReviewDialog.visible&&!spotifyPlaybackDialog.visible,"direct comparison without diagnostic chooser");
            reviewShellCheck(connectionReviewDialog.review.count===2&&connectionReviewDialog.review.index===0,"best persisted candidate first");
            reviewShellCheck(reviewShellTest.findChild(connectionReviewDialog,"reviewLibraryHeading").text==="Known Library Track","source-neutral heading");
            const first=JSON.stringify(connectionReviewDialog.review.fields);
            reviewShellTest.findChild(connectionReviewDialog,"reviewNextCandidate").clicked();reviewShellWait();reviewShellCheck(connectionReviewDialog.review.index===1,"next candidate");
            reviewShellTest.findChild(connectionReviewDialog,"reviewPreviousCandidate").clicked();reviewShellWait();reviewShellCheck(JSON.stringify(connectionReviewDialog.review.fields)===first,"previous restores same comparison");
            reviewShellTest.findChild(connectionReviewDialog,"reviewConnect").clicked();reviewShellWait();
            reviewShellCheck(!connectionReviewDialog.visible&&!spotifyPlaybackDialog.visible,"successful Track action closes without next item");
            reviewShellCheck(applicationSection==="Connections"&&connectionsPage.provider==="spotify"&&!connectionsPage.marked,"same provider and tab");
            reviewShellCheck(connectionGroups.count===1&&connectionGroups.get(0).matched===1,"partial Album updates");
            connectionList.forceLayout();reviewShellCheck(connectionList.itemAtIndex(0).expanded,"reviewed Album stays expanded");
            reviewShellCheck(!connectionList.itemAtIndex(0).tracks.some(t=>t.id===track),"connected Track disappears");
            connectionsPage.matchTrack(album,single);reviewShellWait();
            reviewShellCheck(connectionReviewDialog.review.count===1,"single persisted candidate");
            reviewShellCheck(!reviewShellTest.findChild(connectionReviewDialog,"reviewNextCandidate").visible&&!reviewShellTest.findChild(connectionReviewDialog,"reviewPreviousCandidate").visible,"single candidate has no pointless arrows");
            connectionReviewDialog.close();reviewShellWait();
            connectionsPage.matchTrack(album,zero);reviewShellWait();
            reviewShellCheck(reviewShellTest.findChild(connectionReviewDialog,"reviewNoCandidates").visible,"zero candidate state");
            reviewShellCheck(reviewShellTest.findChild(connectionReviewDialog,"reviewManualMatch").enabled&&!reviewShellTest.findChild(connectionReviewDialog,"reviewConnect").enabled,"zero state exposes manual match");
            reviewShellTest.findChild(connectionReviewDialog,"reviewMarkUnavailable").clicked();reviewShellWait();reviewShellCheck(!connectionReviewDialog.visible,"Track marking closes");
            connectionsPage.matchAlbum(album);reviewShellWait();
            reviewShellCheck(connectionReviewDialog.visible&&!spotifyAlbumDialog.visible,"direct Album review");
            reviewShellCheck(!reviewShellTest.findChild(connectionReviewDialog,"reviewLoadMapping"),"candidate program loads automatically without another action");
            reviewShellTest.findChild(connectionReviewDialog,"reviewMarkUnavailable").clicked();reviewShellWait();
            reviewShellCheck(!connectionReviewDialog.visible&&connectionGroups.count===0,"Album marking removes remaining unresolved group, no automatic next");
            return "ok";
        }catch(e){return String(e)+" "+e.stack;}
    }
    function sharedReviewOpen(album,track,provider) {
        window.showConnections(provider||"spotify");reviewShellWait();
        connectionReviewDialog.begin(provider||"spotify",album,track||"");reviewShellWait();return connectionReviewDialog.visible;
    }
    function sharedReviewCheckFailure() {
        try {
            const before=JSON.stringify(connectionReviewDialog.review.fields);
            reviewShellTest.findChild(connectionReviewDialog,"reviewConnect").clicked();reviewShellWait();
            if(connectionReviewDifferences.visible){connectionReviewDifferences.accept();reviewShellWait();}
            reviewShellCheck(connectionReviewDialog.visible,"failure keeps review open");
            reviewShellCheck(connectionReviewDialog.error.length>0,"failure is explained");
            reviewShellCheck(JSON.stringify(connectionReviewDialog.review.fields)===before,"failure retains candidate");
            return "ok";
        }catch(e){return String(e);}
    }
    function sharedReviewRefresh(){connectionReviewDialog.update();reviewShellWait();return JSON.stringify(connectionReviewDialog.review);}
    function sharedReviewConnect() {
        reviewShellTest.findChild(connectionReviewDialog,"reviewConnect").clicked();reviewShellWait();
        if(connectionReviewDifferences.visible){connectionReviewDifferences.accept();reviewShellWait();}
        return !connectionReviewDialog.visible;
    }
    function sharedReviewNavigate() {
        const before=JSON.stringify(connectionReviewDialog.review.fields);
        if(connectionReviewDialog.review.count>1){connectionReviewDialog.action("navigate",1);reviewShellWait();connectionReviewDialog.action("navigate",0);reviewShellWait();}
        return before===JSON.stringify(connectionReviewDialog.review.fields);
    }
    property string reviewUnrelatedAlbum:""
    property real reviewBeforeY:0
    function sharedReviewOpenAtPlace(album,track) {
        window.showConnections("spotify");reviewShellWait();
        let index=-1;for(let i=0;i<connectionGroups.count;i++)if(connectionGroups.get(i).albumId===album)index=i;
        if(index<0)return false;
        const unrelated=index>0?index-1:index+1;
        connectionList.positionViewAtIndex(index,ListView.Center);connectionList.forceLayout();reviewShellWait();
        for(const i of [unrelated,index]) {
            if(i>=0&&i<connectionGroups.count){const id=connectionGroups.get(i).albumId;connectionsPage.expanded["spotifyfalse"+id]=true;const row=connectionList.itemAtIndex(i);if(row)row.expanded=true;}
        }
        if(unrelated>=0&&unrelated<connectionGroups.count)reviewUnrelatedAlbum=connectionGroups.get(unrelated).albumId;
        connectionList.forceLayout();reviewBeforeY=connectionList.contentY;
        connectionReviewDialog.begin("spotify",album,track||"");reviewShellWait();return connectionReviewDialog.visible;
    }
    function sharedReviewPlacePreserved() {
        reviewShellWait();
        return !connectionReviewDialog.visible&&connectionsPage.provider==="spotify"&&!connectionsPage.marked&&
            (!reviewUnrelatedAlbum||connectionsPage.expanded["spotifyfalse"+reviewUnrelatedAlbum])&&
            (reviewBeforeY<50||connectionList.contentY>0);
    }
    function sharedReviewChooseFile(track,url) {connectionReviewDialog.manualMatch();if(!connectionReviewDialog.manualMode||!sharedReviewNoLegacyWindows())return false;connectionReviewDialog.chooseLocalFile(track,url);for(let n=0;n<200&&connectionReviewDialog.review.pending;n++){reviewShellTest.wait(10);connectionReviewDialog.update();}return !connectionReviewDialog.manualMode&&sharedReviewNoLegacyWindows()&&!!connectionReviewDialog.review.can_connect;}
    function sharedReviewAwaitClosed() {for(let n=0;n<250&&connectionReviewDialog.visible;n++)reviewShellTest.wait(20);return !connectionReviewDialog.visible;}
    function sharedReviewCancel(){connectionReviewDialog.close();reviewShellWait();}
    function sharedReviewManualMusicBrainz(query) {
        connectionReviewDialog.manualMatch();manualQuery.text=query;connectionReviewDialog.manualAction("manual-search",0);
        for(let n=0;n<2000&&connectionReviewDialog.review.pending;n++){reviewShellTest.wait(20);connectionReviewDialog.update();}
        return JSON.stringify({error:connectionReviewDialog.error,candidates:connectionReviewDialog.review.manual.results});
    }
    function sharedReviewManualMusicBrainzInspect(index) {
        connectionReviewDialog.manualAction("manual-select",index);
        for(let n=0;n<2000&&connectionReviewDialog.review.pending;n++){reviewShellTest.wait(20);connectionReviewDialog.update();}
        return JSON.stringify({error:connectionReviewDialog.error,review:connectionReviewDialog.review});
    }
    function sharedReviewRetainedMusicBrainzConfirm(title) {
        connectionReviewDialog.manualMatch();connectionReviewDialog.update();reviewShellWait();
        const index=(connectionReviewDialog.review.manual.recordings||[]).findIndex(t=>t.title===title);
        if(index<0)return false;
        connectionReviewDialog.manualAction("manual-recording",index);reviewShellWait();
        if(!connectionReviewDialog.visible||connectionReviewDialog.manualMode||!sharedReviewNoLegacyWindows())return false;
        if(!connectionReviewDialog.review.fields.some(f=>f.label==="Title"&&f.candidate===title))return false;
        return sharedReviewConnect();
    }
    function sharedReviewCapture(path) {
        let done=false;let saved=false;
        connectionReviewDialog.contentItem.parent.grabToImage(function(result){saved=result.saveToFile(path);done=true;});
        for(let n=0;n<100&&!done;n++)reviewShellTest.wait(20);
        return saved;
    }
    function sharedReviewNoLegacyWindows() {
        return !spotifyPlaybackDialog.visible&&!spotifyAlbumDialog.visible&&!spotifyAdvancedMapping.visible&&!musicbrainzConnection.visible&&!connectionAlbumDialog.visible;
    }
    function sharedReviewInlineManual() {
        connectionReviewDialog.manualMatch();reviewShellWait();
        return connectionReviewDialog.visible&&connectionReviewDialog.manualMode&&sharedReviewNoLegacyWindows();
    }
    function sharedReviewSelectManualResult(index) {
        const before=connectionReviewDialog.visible;
        connectionReviewDialog.manualAction("manual-select",index);reviewShellWait();
        for(let n=0;n<500&&connectionReviewDialog.review.pending;n++){reviewShellTest.wait(20);connectionReviewDialog.update();}
        return before&&connectionReviewDialog.visible&&!connectionReviewDialog.manualMode&&sharedReviewNoLegacyWindows();
    }
    function sharedReviewManualExact(value) {
        connectionReviewDialog.manualMatch();manualExact.text=value;
        connectionReviewDialog.manualAction("manual-exact",0);reviewShellWait();
        for(let n=0;n<500&&connectionReviewDialog.review.pending;n++){reviewShellTest.wait(20);connectionReviewDialog.update();}
        return connectionReviewDialog.visible&&!connectionReviewDialog.manualMode&&sharedReviewNoLegacyWindows()&&!connectionReviewDialog.error;
    }
    function sharedReviewAssertMapping() {
        return !!reviewShellTest.findChild(connectionReviewDialog,"reviewMappingHeading")&&connectionReviewDialog.review.program.some(r=>r.connects===true)&&sharedReviewNoLegacyWindows();
    }
    function sharedReviewApplyInlineMapping(pairs) {
        reviewShellTest.findChild(connectionReviewDialog,"reviewAdjustMapping").clicked();reviewShellWait();
        if(!connectionReviewDialog.adjusting||!sharedReviewNoLegacyWindows())return false;
        for(const pair of JSON.parse(pairs))connectionReviewDialog.mappingChoices[pair[0]]=pair[1];
        connectionReviewDialog.applyMapping();reviewShellWait();
        return !connectionReviewDialog.manualMode&&sharedReviewAssertMapping();
    }
