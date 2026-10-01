    TestCase { id: browsingScrollTest; name: "MainBrowsingScrolling"; when: false }
    function exerciseMainBrowsingScrolling() {
        function check(v,m) { if(!v) throw new Error(m); }
        function wait() { browsingScrollTest.wait(100); }
        try {
            wait();
            for(const spec of [["Artists",0,artistsPane],["Genres",0,artistsPane],["Albums",1,albumsPane],["Songs",2,songsPane]]) {
                const name=spec[0],p=spec[1],pane=spec[2];
                bridge.browse_action("view",0,name);wait();
                const l=browsingScrollTest.findChild(window.contentItem,"libraryPane"+p);
                const first=library.panes[p].rows[0].id;
                const epoch=library.panes[p].epoch;
                let fetches=0;
                for(let n=0;n<8 && library.panes[p].more;n++) {
                    const last=library.panes[p].rows.slice(-1)[0].id;
                    l.forceLayout();l.positionViewAtEnd();
                    for(let poll=0;poll<50 && library.panes[p].rows.slice(-1)[0].id===last;poll++) browsingScrollTest.wait(20);
                    check(library.panes[p].rows.slice(-1)[0].id!==last,"automatic "+name+" fetch n="+n+" rows="+library.panes[p].rows.length+" y="+l.contentY+" height="+l.contentHeight+" origin="+l.originY+" paneh="+l.height);
                    check(library.panes[p].rows.length<=600 && l.count<=600,"bounded "+name+" model");
                    check(library.panes[p].epoch===epoch,"fetch keeps logical dataset");fetches++;
                }
                check(fetches>0 || !library.panes[p].more,"query exhausted or fetched");
                if(p===1) {
                    let tiles=0;
                    for(let i=0;i<l.count;i++) {const section=l.itemAtIndex(i);if(section) tiles+=section.rowData.tiles.length;}
                    check(tiles<=600,"bounded instantiated Album tiles");
                }
                const saved=pane.viewportAnchor();
                bridge.browse_action("scroll-position",p,saved.id+"\n"+saved.pixel);
                bridge.browse_action("view",0,name==="Songs"?"Artists":"Songs");wait();
                bridge.browse_action("view",0,name);wait();
                const restored=pane.viewportAnchor();
                check(restored && restored.id===saved.id && Math.abs(restored.pixel-saved.pixel)<2,"per-view "+name+" scroll restoration");
                browsingScrollTest.grabImage(window.contentItem).save("/tmp/library-continuous-"+name.toLowerCase()+"-deep.png");
                for(let n=0;n<12 && library.panes[p].before;n++) { l.positionViewAtBeginning();wait(); }
                check(!library.panes[p].before && library.panes[p].rows[0].id===first,"bidirectional "+name+" scrolling");
            }
            return "ok";
        } catch(e) {return String(e);}
    }
