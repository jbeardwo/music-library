    TestCase { id: multiTest; name: "MultiSelection"; when: false }
    function exerciseMultiSelection() {
        function check(v,m) { if(!v) throw new Error(m); }
        function wait() { multiTest.wait(25); }
        function done() { for(let n=0;n<250 && library.pending;n++) wait(); check(!library.pending,"background operation completes"); check(!library.error,library.error); }
        function page(v) { bridge.browse_action("view",0,v);wait(); }
        function rows(p) { return library.panes[p].rows; }
        function index(p,title) { return rows(p).findIndex(r=>r.title===title); }
        function item(p,i) {
            const list=multiTest.findChild(window.contentItem,"libraryPane"+p);list.forceLayout();
            if(p===1) return multiTest.findChild(list.itemAtIndex(0),"albumTile"+i);
            return list.itemAtIndex(i);
        }
        function click(p,i,modifiers,button) { const r=item(p,i);check(r,"actual delegate");multiTest.mouseClick(r,20,16,button||Qt.LeftButton,modifiers||Qt.NoModifier);wait();done(); }
        function context(p,i) {click(p,i,Qt.NoModifier,Qt.RightButton);check(libraryMenu.visible,"context opened");}
        function menu(text) { for(let i=0;i<libraryMenu.count;i++) {const m=libraryMenu.itemAt(i);if(m && m.text===text) return m;}throw new Error("missing action "+text); }
        function queueFrom(p,i,expected) {
            const before=view.queueTotal;context(p,i);menu("Add to queue").triggered();libraryMenu.close();done();
            check(view.queueTotal===before+expected,"pane-local queue batch "+p+" expected "+expected+" got "+(view.queueTotal-before));
        }
        function addFrom(p,i,destination,answer,duplicateCount) {
            context(p,i);menu("Add to Playlist…").triggered();libraryMenu.close();wait();
            check(addPlaylistDialog.visible,"chooser opens for pane "+p);
            const choice=library.playlistChoices.find(p=>p.id===destination);check(choice,"paged chooser destination");
            bridge.browse_action("picker-add",0,destination);addPlaylistDialog.close();done();
            if(duplicateCount) {
                check(duplicatePlaylistDialog.visible,"one duplicate confirmation");
                if(duplicateCount>1)check(library.duplicateMessage.indexOf(duplicateCount+" of ")>=0,"aggregate duplicate count");
                const button=duplicatePlaylistDialog.standardButton(answer?Dialog.Yes:Dialog.No);
                multiTest.mouseClick(button);done();
                check(!duplicatePlaylistDialog.visible,"single confirmation resolved");
            } else check(!duplicatePlaylistDialog.visible,"no confirmation for new canonical identities");
        }
        try {
            wait();
            const a=index(0,"Artist A"), b=index(0,"Artist B"), c=index(0,"Artist C");
            click(0,a);const retainedArtist=item(0,a);click(0,b,Qt.ControlModifier);
            check(item(0,a)===retainedArtist,"selection preserves unchanged delegates");
            check(String(item(0,a).color)==="#e8d9e0" && String(item(0,b).color)==="#e8d9e0" && String(item(0,c).color)!=="#e8d9e0","multiple selections visibly highlighted");
            check(library.panes[0].selectionCount===2 && rows(1).length===2 && rows(2).length===8,"Artist unions");
            const artists=library.panes[0].selectedIds.slice();
            click(1,0);click(1,1,Qt.ControlModifier);
            check(library.panes[1].selectionCount===2 && library.panes[0].selectionCount===2,"Albums preserve Artists");
            click(2,0);click(2,3,Qt.ShiftModifier);check(library.panes[2].selectionCount===4,"Shift contiguous range");
            click(2,1,Qt.ControlModifier);check(library.panes[2].selectionCount===3,"Ctrl toggles out");
            check(library.panes[0].selectionCount===2 && library.panes[1].selectionCount===2,"Songs preserve upstream selections");
            queueFrom(2,0,3);
            check(library.panes[2].selectionCount===3,"selected context preserves all Songs");
            queueFrom(2,1,1);check(library.panes[2].selectionCount===1,"unselected context replaces only Songs");
            queueFrom(0,a,8);queueFrom(1,0,8);
            check(JSON.stringify(library.panes[0].selectedIds)===JSON.stringify(artists),"context actions do not collapse Artists");
            bridge.browse_action("playlist-create",0,"Destination");wait();
            bridge.browse_action("picker-open",0,rows(0)[a].id);wait();
            const destination=library.playlistChoices.find(p=>p.name==="Destination").id;
            addFrom(0,a,destination,false,0);
            addFrom(1,0,destination,false,8); // No skips all eight canonical duplicates.
            click(0,a,Qt.ControlModifier);check(library.panes[0].selectionCount===1,"Ctrl Artist removal");
            check(library.panes[1].selectionCount===1 && rows(2).length===4,"upstream change prunes invisible Albums");
            check(library.panes[2].selectionCount===0,"upstream change prunes invisible Songs");
            click(0,c);check(library.panes[1].selectionCount===0,"new upstream single selection prunes old Album");
            page("Genres");click(0,index(0,"Pop"));click(0,index(0,"Rock"),Qt.ControlModifier);
            check(library.panes[0].selectionCount===2 && rows(1).length===2 && rows(2).length===8,"Genre unions");
            click(1,0);click(1,1,Qt.ControlModifier);check(library.panes[0].selectionCount===2,"Albums preserve Genres");
            queueFrom(0,index(0,"Pop"),8);addFrom(0,index(0,"Pop"),destination,true,8);
            page("Songs");
            click(2,index(2,"A 0"));click(2,index(2,"C 0"),Qt.ControlModifier);
            queueFrom(2,index(2,"A 0"),2);
            addFrom(2,index(2,"A 0"),destination,false,1); // equal/new identity mixed batch.
            click(2,index(2,"A 0"));addFrom(2,index(2,"A 0"),destination,true,1);
            page("Playlists");
            const p=index(0,"Source One"),q=index(0,"Source Two");
            click(0,p);click(0,q,Qt.ControlModifier);
            check(rows(2).length===5 && library.panes[0].selectionCount===2,"multiple playlists retain all ordered occurrences");
            const persisted=rows(2).map(r=>r.track.trackId);
            const before=view.queueTotal;queueFrom(0,p,5);
            check(JSON.stringify(view.queue.slice(before).map(r=>r.trackId))===JSON.stringify(persisted),"source playlist order and duplicate occurrences");
            addFrom(0,p,destination,true,5);
            click(2,0);click(2,2,Qt.ShiftModifier);queueFrom(2,0,3);
            const playlistSelection=library.panes[0].selectedIds.slice();page("Albums");page("Playlists");
            check(JSON.stringify(library.panes[0].selectedIds)===JSON.stringify(playlistSelection) && library.panes[2].selectionCount===3,"per-view selection retained");
            click(0,index(0,"Destination"));check(rows(2).length===23,"batch Yes/No results persisted");
            check(library.panes.every(p=>p.rows.length<=200 && p.selectedIds.length<=200),"bounded row and selection projection");
            return "ok";
        } catch(e) {return e.message;}
    }
