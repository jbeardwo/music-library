//! Disposable catalog worker and presentation state; no Qt or provider data in the backend.
use music_library::{
    catalog::{AlbumCandidate, CatalogError, CatalogProvider, Page, Release, ReleaseCandidate},
    domain::ExternalIdentity,
};
use std::{
    sync::mpsc,
    thread::{self, JoinHandle},
};

pub enum Request {
    Groups(String, u32),
    Editions(AlbumCandidate, u32),
    Add(AlbumCandidate, ExternalIdentity),
    AddAlbum(AlbumCandidate),
}
pub enum Reply {
    Groups(Page<AlbumCandidate>),
    Editions(Page<ReleaseCandidate>),
    Add(Box<Release>),
}
pub struct Worker {
    send: Option<mpsc::Sender<Request>>,
    thread: Option<JoinHandle<()>>,
}
impl Worker {
    pub fn new(
        provider: impl CatalogProvider + 'static,
        emit: impl Fn(Result<Reply, CatalogError>) + Send + 'static,
    ) -> Result<Self, String> {
        let (send, recv) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("catalog".into())
            .spawn(move || {
                let mut provider = music_library::catalog::CatalogSession::new(provider);
                while let Ok(request) = recv.recv() {
                    let result = match request {
                        Request::Groups(q, o) => provider.search_albums(&q, o).map(Reply::Groups),
                        Request::Editions(id, o) => provider.editions(&id, o).map(Reply::Editions),
                        Request::Add(album, id) => {
                            provider.edition(&album, &id).map(Box::new).map(Reply::Add)
                        }
                        Request::AddAlbum(album) => {
                            provider.add_album(&album).map(Box::new).map(Reply::Add)
                        }
                    };
                    emit(result);
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            send: Some(send),
            thread: Some(thread),
        })
    }
    pub fn send(&self, request: Request) -> Result<(), String> {
        self.send
            .as_ref()
            .ok_or("catalog worker stopped")?
            .send(request)
            .map_err(|e| e.to_string())
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.send.take();
        if let Some(worker) = self.thread.take() {
            let _ = worker.join();
        }
    }
}
#[derive(Default)]
pub struct State {
    pub worker: Option<Worker>,
    pub pending: bool,
    pub timing: Option<music_library::catalog::Timing>,
    pub status: String,
    pub groups: Vec<AlbumCandidate>,
    pub editions: Vec<ReleaseCandidate>,
    pub group_next: Option<u32>,
    pub edition_next: Option<u32>,
    pub query: String,
    pub group: Option<AlbumCandidate>,
}
impl State {
    pub fn request(&mut self, action: &str, value: &str) -> Result<Request, String> {
        match action {
            "search" => {
                if value.trim().is_empty() {
                    return Err("Enter a catalog query".into());
                }
                self.query = value.into();
                self.groups.clear();
                self.editions.clear();
                self.group = None;
                self.group_next = None;
                self.edition_next = None;
                Ok(Request::Groups(value.into(), 0))
            }
            "more_groups" => Ok(Request::Groups(
                self.query.clone(),
                self.group_next.ok_or("No more groups")?,
            )),
            "editions" | "add_album" => {
                let i = value.parse::<usize>().map_err(|e| e.to_string())?;
                let id = self.groups.get(i).ok_or("Select an Album")?.clone();
                self.group = Some(id.clone());
                self.editions.clear();
                self.edition_next = None;
                if action == "add_album" {
                    Ok(Request::AddAlbum(id))
                } else {
                    Ok(Request::Editions(id, 0))
                }
            }
            "more_editions" => Ok(Request::Editions(
                self.group.clone().ok_or("Select an Album")?,
                self.edition_next.ok_or("No more editions")?,
            )),
            "add" => {
                let i = value.parse::<usize>().map_err(|e| e.to_string())?;
                Ok(Request::Add(
                    self.group.clone().ok_or("Select an Album")?,
                    self.editions
                        .get(i)
                        .ok_or("Select a concrete Release")?
                        .identity
                        .clone(),
                ))
            }
            _ => Err("Unknown catalog action".into()),
        }
    }
}
