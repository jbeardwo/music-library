    TestCase { id: importTest; name: "LocalImport"; when: false }
    function exerciseLocalImport(fileUrl, folderUrl, badUrl, unsupportedUrl, folderFileUrl) {
        function check(ok, message) { if (!ok) throw new Error(message); }
        function waitImport() {
            for (let n=0; n<500 && bridge.local_import_snapshot.busy; ++n) importTest.wait(20);
            check(!bridge.local_import_snapshot.busy,"background ingestion and scheduling finish");
            importTest.wait(250);
        }
        function chooseFiles(urls) {
            if (!localImportPanel.visible) localImportPanel.open();
            importTest.wait(250);
            localFilesPicker.folder=urls[0].substring(0,urls[0].lastIndexOf("/"));
            const button=importTest.findChild(localImportPanel.contentItem,"chooseLocalFiles");
            importTest.mouseClick(button,20,15); importTest.wait(100);
            check(localFilesPicker.visible,"native/platform file picker opens");
            check(localFilesPicker.fileMode===PlatformDialogs.FileDialog.OpenFiles,"multiple file mode");
            // Native dialogs are outside the Qt Quick input tree. Supply their
            // accepted result and exercise the shipped accepted handler.
            localFilesPicker.files=urls;
            localFilesPicker.accepted();
            check(!localFilesPicker.visible,"accepted file picker dismisses");
            waitImport();
        }
        try {
            importTest.wait(100);
            const queueBefore=JSON.stringify(view.queue);
            const playbackBefore=view.currentId;
            importTest.mouseClick(addMusicButton,20,15); importTest.wait(30);
            check(addMusicChooser.visible,"Add Music opens chooser");
            check(addMusicChooser.itemAt(0).text==="From catalog" && addMusicChooser.itemAt(1).text==="From file","chooser has only discovery origins");
            addMusicChooser.itemAt(0).triggered(); addMusicChooser.close(); importTest.wait(250);
            check(addMusicPanel.visible,"existing catalog panel opens");
            addMusicPanel.close(); importTest.wait(250);
            importTest.mouseClick(addMusicButton,20,15); importTest.wait(30);
            addMusicChooser.itemAt(1).triggered();addMusicChooser.close();importTest.wait(250);
            check(localImportPanel.visible,"From file workflow opens");
            const filesButton=importTest.findChild(localImportPanel.contentItem,"chooseLocalFiles");
            localFilesPicker.folder=fileUrl.substring(0,fileUrl.lastIndexOf("/"));
            importTest.mouseClick(filesButton,20,15); importTest.wait(100);
            localFilesPicker.currentFiles=[fileUrl];
            localFilesPicker.reject(); importTest.wait(100);
            check(!localFilesPicker.visible,"Cancel hides actual file picker");
            check(!bridge.local_import_snapshot.busy && library.panes[2].rows.length===0,"Cancel does not import");
            importTest.mouseClick(filesButton,20,15); importTest.wait(100);
            check(localFilesPicker.visible && localFilesPicker.currentFiles.length===0 && localFilesPicker.files.length===0,"reopen has no stale selection");
            localFilesPicker.reject(); importTest.wait(100);
            chooseFiles([fileUrl]);
            check(bridge.local_import_snapshot.imported===1,"single native selection imports: " + bridge.local_import_snapshot.status + " selected=" + localFilesPicker.files);
            check(library.panes[2].rows.length===1 && library.panes[0].rows.length>0 && library.panes[1].rows.length===1,"all library panes refresh");
            importTest.mouseClick(filesButton,20,15); importTest.wait(100);
            check(localFilesPicker.files.length===0 && localFilesPicker.currentFiles.length===0,"accepted selection is cleared on fresh open");
            localFilesPicker.reject(); importTest.wait(100);
            check(!localFilesPicker.visible,"repeated Cancel dismisses picker");
            const originalTrack=library.panes[2].rows[0].id;
            localImportPanel.close();importTest.wait(250);
            bridge.browse_action("remove-preview",2,originalTrack);
            for(let n=0;n<100 && library.pending;++n) importTest.wait(20);
            check(library.removal.local && suppressRescan.checked,"local suppression confirmation");
            bridge.browse_action("remove-confirm",0,"suppress");
            for(let n=0;n<100 && library.pending;++n) importTest.wait(20);
            importTest.wait(250);
            bridge.local_import("rescan",[]);waitImport();
            check(library.panes[2].rows.length===0,"automatic import respects suppression");
            chooseFiles([fileUrl]);
            check(bridge.local_import_snapshot.imported===1 && library.panes[2].rows[0].id===originalTrack,"explicit same file clears exclusion and restores original Track");
            const button=importTest.findChild(localImportPanel.contentItem,"chooseLocalFolder");
            importTest.mouseClick(button,20,15);importTest.wait(100);
            check(localFolderPicker.visible,"native/platform folder picker opens");
            localFolderPicker.reject(); importTest.wait(100);
            check(!localFolderPicker.visible,"Cancel hides folder picker too");
            importTest.mouseClick(button,20,15);importTest.wait(100);
            localFolderPicker.folder=folderUrl;localFolderPicker.accepted();waitImport();
            check(bridge.local_import_snapshot.imported===2,"folder files use scanner import");
            check(library.panes[2].rows.length===3,"new folder music visible immediately");
            check(bridge.local_import_snapshot.status.indexOf("unsupported")>=0 && bridge.local_import_snapshot.status.indexOf("could not be read")>=0,"mixed folder results are compact");
            localFilesPicker.selectedNameFilter.index=1;
            chooseFiles([folderFileUrl,badUrl,unsupportedUrl]);
            check(localFilesPicker.files.length===3,"accepted selection retains every selected file: " + localFilesPicker.files);
            check(bridge.local_import_snapshot.imported===0,"existing multi-file selection deduplicates");
            check(bridge.local_import_snapshot.status.indexOf("unsupported")>=0 && bridge.local_import_snapshot.status.indexOf("could not be read")>=0,"mixed file selection does not abort valid files: " + bridge.local_import_snapshot.status);
            check(JSON.stringify(view.queue)===queueBefore && view.currentId===playbackBefore,"import never starts playback or rebuilds queue");
            return "ok";
        } catch(e) { return String(e); }
    }
    function exerciseSecondLocation(folderUrl) {
        try {
            if (!localImportPanel.visible) localImportPanel.open();
            importTest.wait(250);
            const button=importTest.findChild(localImportPanel.contentItem,"chooseLocalFolder");
            importTest.mouseClick(button,20,15);importTest.wait(100);
            localFolderPicker.folder=folderUrl;localFolderPicker.accepted();
            for(let n=0;n<500 && bridge.local_import_snapshot.busy;++n) importTest.wait(20);
            if (bridge.local_import_snapshot.busy) throw new Error("second folder import completes");
            if (bridge.local_import_snapshot.imported!==0 || library.panes[2].rows.length!==3) throw new Error("adding a location adopts the existing standalone source");
            return "ok";
        } catch(e) { return String(e); }
    }
