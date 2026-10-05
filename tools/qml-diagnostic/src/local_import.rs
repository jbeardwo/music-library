//! UI entry points only; all local discovery and import live in the backend pipeline.
use crate::{Bridge, string};
use music_library::{
    domain::ImportedRelease,
    local_ingestion::{Progress, Report, Request},
};
use qmetaobject::{QPointer, QVariantList, QVariantMap};
use std::{
    collections::VecDeque,
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct State {
    pub busy: bool,
    status: String,
    summary: String,
    scanned: u64,
    imported: u64,
    releases: VecDeque<ImportedRelease>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Drop for State {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
impl Bridge {
    pub fn local_import_urls(&mut self, kind: &str, urls: QVariantList) {
        let mut invalid = 0;
        let paths: Vec<PathBuf> = urls
            .into_iter()
            .filter_map(|value| {
                let path = url::Url::parse(&value.to_qstring().to_string())
                    .ok()
                    .and_then(|u| u.to_file_path().ok());
                if path.is_none() {
                    invalid += 1;
                }
                path
            })
            .collect();
        let request = match kind {
            "files" if !paths.is_empty() => Request::Files(paths),
            "folder" if paths.len() == 1 => Request::Folder(paths[0].clone()),
            "rescan" => Request::ConfiguredLocations,
            _ => {
                self.local_import_state.status = "No readable local files were selected.".into();
                self.local_import_changed();
                return;
            }
        };
        self.start_local_import(request, invalid);
    }
    pub fn start_local_import(&mut self, request: Request, invalid_urls: u64) {
        if self.local_import_state.busy {
            return;
        }
        if self.browser.pending {
            self.local_import_state.status =
                "Please wait for the current library operation.".into();
            self.local_import_changed();
            return;
        }
        let worker = match self.session.library.local_ingestion_worker() {
            Ok(w) => w,
            Err(_) => {
                self.local_import_state.status = "Local import could not start.".into();
                self.local_import_changed();
                return;
            }
        };
        let pointer = QPointer::from(&*self);
        let progress_pointer = pointer.clone();
        let update = qmetaobject::queued_callback(move |progress: Progress| {
            if let Some(object) = progress_pointer.as_pinned() {
                let mut b = object.borrow_mut();
                b.local_import_state.scanned = progress.scanned;
                b.local_import_state.imported = progress.imported;
                b.local_import_state.status =
                    format!("Scanning… {} files processed", progress.scanned);
                b.local_import_changed();
            }
        });
        let finish = qmetaobject::queued_callback(move |result: Result<Report, String>| {
            if let Some(object) = pointer.as_pinned() {
                let mut b = object.borrow_mut();
                if let Some(worker) = b.local_import_state.worker.take() {
                    let _ = worker.join();
                }
                match result {
                    Ok(mut report) => {
                        report.unreadable += invalid_urls;
                        let mut summary = vec![format!("Imported {} Tracks", report.imported)];
                        if report.attached > 0 {
                            summary.push(format!(
                                "Attached {} local files to existing music",
                                report.attached
                            ));
                        }
                        if report.unresolved > 0 {
                            summary.push(format!("{} files had no confident existing match and were imported separately", report.unresolved));
                        }
                        if report.unsupported > 0 {
                            summary
                                .push(format!("Skipped {} unsupported files", report.unsupported));
                        }
                        if report.unreadable > 0 {
                            summary.push(format!("{} files could not be read", report.unreadable));
                        }
                        if report.locations_failed > 0 {
                            summary.push(format!(
                                "{} locations could not be scanned completely",
                                report.locations_failed
                            ));
                        }
                        b.local_import_state.scanned = report.scanned;
                        b.local_import_state.imported = report.imported;
                        b.local_import_state.summary = summary.join("\n");
                        b.local_import_state.status = b.local_import_state.summary.clone();
                        b.local_import_state.releases = report.releases.into();
                    }
                    Err(_) => {
                        b.local_import_state.status =
                            "Import could not finish. Successfully imported music has been kept."
                                .into();
                        b.local_import_state.busy = false;
                    }
                }
                b.browse_action_impl("refresh", 0, String::new());
                b.refresh_local_search();
                if let Err(e) = b.music_context() {
                    b.session.error = e;
                }
                b.music_changed();
                b.local_import_schedule_next();
                b.local_import_changed();
            }
        });
        self.local_import_state.busy = true;
        self.local_import_state.status = "Scanning…".into();
        self.local_import_state.scanned = 0;
        self.local_import_state.imported = 0;
        self.local_import_changed();
        self.local_import_state.worker = Some(std::thread::spawn(move || {
            let mut last = Instant::now();
            let result = worker.run(request, &mut |progress| {
                // Batch reports are further coalesced to keep large imports cheap for QML.
                if last.elapsed() >= Duration::from_millis(100) {
                    update(progress);
                    last = Instant::now();
                }
            });
            finish(result.map_err(|_| "Local import failed".to_owned()));
        }));
    }
    pub fn local_import_schedule_next(&mut self) {
        let chunk: Vec<_> = self
            .local_import_state
            .releases
            .drain(..self.local_import_state.releases.len().min(16))
            .collect();
        if !chunk.is_empty() {
            let enabled = self.auto_match;
            if let Err(error) = self.post_import(&chunk, enabled) {
                self.session.error = error;
                self.changed();
            }
        }
        if self.local_import_state.releases.is_empty() && self.local_import_state.worker.is_none() {
            self.local_import_state.busy = false;
        }
        self.local_import_changed();
    }
    pub fn local_import_value(&self) -> QVariantMap {
        [
            ("busy", self.local_import_state.busy.into()),
            (
                "scheduling",
                (!self.local_import_state.releases.is_empty()).into(),
            ),
            ("status", string(&self.local_import_state.status)),
            ("scanned", (self.local_import_state.scanned as f64).into()),
            ("imported", (self.local_import_state.imported as f64).into()),
        ]
        .into_iter()
        .collect()
    }
}
