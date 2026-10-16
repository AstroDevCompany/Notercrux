#![allow(dead_code)]

use axum::body::Body;
use axum::extract::{Request, State};
use axum::response::Response;
use axum::Router;
use notercrux_domain::{Block, BlockKind, Document, PageId};
use notercrux_storage::Store;
use notercrux_sync::{Engine, EngineConfig, MemorySecrets, SecretStore};
use notercrux_sync_server::config::Config;
use notercrux_sync_server::{router, AppState};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::task::JoinHandle;

// ---- a real server on a loopback port ---------------------------------------------------------

pub struct Server {
    pub addr: SocketAddr,
    pub url: String,
    pub state: AppState,
    /// The attachment chunk store, to inspect what the server holds.
    pub blobs: Arc<notercrux_sync_server::blobstore::MemBlobStore>,
    task: Option<JoinHandle<()>>,
}

impl Server {
    pub async fn start() -> Self {
        Self::start_with(Config::default()).await
    }

    /// In memory by default; set NOTERCRUX_TEST_DATABASE_URL to run on PostgreSQL instead.
    pub async fn start_with(config: Config) -> Self {
        let blobs = Arc::new(notercrux_sync_server::blobstore::MemBlobStore::new());
        let state = match std::env::var("NOTERCRUX_TEST_DATABASE_URL") {
            Ok(url) if !url.is_empty() => {
                let repo = notercrux_sync_server::pg::PgRepo::connect(&url).expect("database URL");
                AppState::new(config, Arc::new(repo), blobs.clone())
            }
            _ => AppState::new(
                config,
                Arc::new(notercrux_sync_server::mem::MemRepo::new()),
                blobs.clone(),
            ),
        };
        Self::serve_with(state, blobs, "127.0.0.1:0".parse().unwrap()).await
    }

    pub async fn serve(state: AppState, addr: SocketAddr) -> Self {
        Self::serve_with(
            state,
            Arc::new(notercrux_sync_server::blobstore::MemBlobStore::new()),
            addr,
        )
        .await
    }

    pub async fn serve_with(
        state: AppState,
        blobs: Arc<notercrux_sync_server::blobstore::MemBlobStore>,
        addr: SocketAddr,
    ) -> Self {
        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = router(state.clone());
        let task = tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .ok();
        });
        Self {
            addr,
            url: format!("http://{addr}"),
            state,
            blobs,
            task: Some(task),
        }
    }

    /// Stops listening (the data stays in `state`, so `restart` brings the same server back).
    pub fn stop(&mut self) {
        if let Some(t) = self.task.take() {
            t.abort();
        }
    }

    pub async fn restart(&mut self) {
        self.stop();
        tokio::time::sleep(Duration::from_millis(50)).await;
        let mut again = Self::serve_with(self.state.clone(), self.blobs.clone(), self.addr).await;
        self.task = again.task.take();
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

// ---- a misbehaving network between a client and the server ------------------------------------

type Tamper = Box<dyn Fn(&str, Vec<u8>) -> Vec<u8> + Send + Sync>;

#[derive(Default)]
pub struct Faults {
    /// Every request fails as if the server were unreachable.
    pub down: AtomicBool,
    /// Requests whose path contains one of these fail *before* reaching the server.
    pub lose_request: Mutex<Vec<String>>,
    /// Requests whose path contains one of these reach the server but their response is lost.
    pub lose_response: Mutex<Vec<String>>,
    /// Rewrites response bodies (an actively malicious server).
    pub tamper: Mutex<Option<Tamper>>,
    pub seen: Mutex<Vec<String>>,
    pub forwarded: AtomicUsize,
}

pub struct Proxy {
    pub url: String,
    pub faults: Arc<Faults>,
    task: JoinHandle<()>,
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Clone)]
struct ProxyState {
    upstream: String,
    faults: Arc<Faults>,
    http: reqwest::Client,
}

impl Proxy {
    pub async fn to(upstream: &str) -> Self {
        notercrux_sync::install_tls_provider();
        let faults = Arc::new(Faults::default());
        let st = ProxyState {
            upstream: upstream.to_string(),
            faults: faults.clone(),
            http: reqwest::Client::new(),
        };
        let app = Router::new().fallback(forward).with_state(st);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.ok();
        });
        Self { url, faults, task }
    }

    pub fn set_down(&self, down: bool) {
        self.faults.down.store(down, Ordering::SeqCst);
    }

    pub fn lose_next_response(&self, path_part: &str) {
        self.faults
            .lose_response
            .lock()
            .unwrap()
            .push(path_part.to_string());
    }

    pub fn lose_next_request(&self, path_part: &str) {
        self.faults
            .lose_request
            .lock()
            .unwrap()
            .push(path_part.to_string());
    }

    pub fn tamper(&self, f: impl Fn(&str, Vec<u8>) -> Vec<u8> + Send + Sync + 'static) {
        *self.faults.tamper.lock().unwrap() = Some(Box::new(f));
    }

    pub fn clear_tamper(&self) {
        *self.faults.tamper.lock().unwrap() = None;
    }

    /// Requests whose "METHOD /path" contains both strings.
    pub fn count_both(&self, a: &str, b: &str) -> usize {
        self.faults
            .seen
            .lock()
            .unwrap()
            .iter()
            .filter(|p| p.contains(a) && p.contains(b))
            .count()
    }

    pub fn count(&self, needle: &str) -> usize {
        self.faults
            .seen
            .lock()
            .unwrap()
            .iter()
            .filter(|p| p.contains(needle))
            .count()
    }
}

fn fail(status: u16, msg: &str) -> Response {
    Response::builder()
        .status(status)
        .body(Body::from(msg.to_string()))
        .unwrap()
}

async fn forward(State(st): State<ProxyState>, req: Request) -> Response {
    let (parts, body) = req.into_parts();
    let target = parts
        .uri
        .path_and_query()
        .map_or("/", |p| p.as_str())
        .to_string();
    st.faults
        .seen
        .lock()
        .unwrap()
        .push(format!("{} {}", parts.method, target));
    if st.faults.down.load(Ordering::SeqCst) {
        return fail(503, "down");
    }
    let take = |list: &Mutex<Vec<String>>| {
        let mut l = list.lock().unwrap();
        l.iter()
            .position(|p| target.contains(p.as_str()))
            .map(|i| l.remove(i))
            .is_some()
    };
    if take(&st.faults.lose_request) {
        return fail(502, "request lost");
    }
    let bytes = axum::body::to_bytes(body, usize::MAX)
        .await
        .unwrap()
        .to_vec();
    let mut up = st
        .http
        .request(parts.method.clone(), format!("{}{}", st.upstream, target));
    for (k, v) in &parts.headers {
        if k != "host" && k != "content-length" {
            up = up.header(k, v);
        }
    }
    let res = match up.body(bytes).send().await {
        Ok(r) => r,
        Err(_) => return fail(502, "upstream"),
    };
    st.faults.forwarded.fetch_add(1, Ordering::SeqCst);
    let status = res.status().as_u16();
    let ctype = res.headers().get("content-type").cloned();
    let mut body = res.bytes().await.unwrap().to_vec();
    if take(&st.faults.lose_response) {
        return fail(502, "response lost");
    }
    if let Some(f) = st.faults.tamper.lock().unwrap().as_ref() {
        body = f(&target, body);
    }
    let mut out = Response::builder().status(status);
    if let Some(c) = ctype {
        out = out.header("content-type", c);
    }
    out.body(Body::from(body)).unwrap()
}

// ---- devices ----------------------------------------------------------------------------------

pub struct Dev {
    pub name: &'static str,
    pub store: Arc<Mutex<Store>>,
    pub secrets: Arc<MemorySecrets>,
    pub engine: Arc<Engine>,
}

pub fn fast() -> EngineConfig {
    EngineConfig {
        long_poll: Duration::from_millis(300),
        backoff_base: Duration::from_millis(40),
        backoff_max: Duration::from_millis(400),
        batch_bytes: 4 * 1024 * 1024,
    }
}

impl Dev {
    pub fn new(name: &'static str) -> Self {
        Self::with_config(name, fast())
    }

    pub fn with_config(name: &'static str, cfg: EngineConfig) -> Self {
        let store = Arc::new(Mutex::new(Store::open_in_memory().unwrap()));
        let secrets = Arc::new(MemorySecrets::new());
        let engine =
            Engine::open(store.clone(), secrets.clone() as Arc<dyn SecretStore>, cfg).unwrap();
        Self {
            name,
            store,
            secrets,
            engine,
        }
    }

    /// Same data and secrets, as after quitting and relaunching the app.
    pub fn relaunch(&self) -> Dev {
        let engine = Engine::open(
            self.store.clone(),
            self.secrets.clone() as Arc<dyn SecretStore>,
            fast(),
        )
        .unwrap();
        Dev {
            name: self.name,
            store: self.store.clone(),
            secrets: self.secrets.clone(),
            engine,
        }
    }

    pub fn with<R>(&self, f: impl FnOnce(&mut Store) -> R) -> R {
        f(&mut self.store.lock().unwrap())
    }

    pub async fn sync(&self) -> notercrux_sync::CycleReport {
        self.engine
            .sync_once()
            .await
            .unwrap_or_else(|e| panic!("{} failed to sync: {e}", self.name))
    }

    pub fn page(&self, title: &str, texts: &[&str]) -> PageId {
        self.with(|s| {
            let p = s.create_page(None, title).unwrap();
            if !texts.is_empty() {
                s.save_document(p.id, &doc(texts)).unwrap();
            }
            p.id
        })
    }

    pub fn titles(&self) -> Vec<String> {
        self.with(|s| {
            let mut v: Vec<String> = s
                .list_pages()
                .unwrap()
                .into_iter()
                .filter(|p| p.archived_at.is_none())
                .map(|p| p.title)
                .collect();
            v.sort();
            v
        })
    }

    pub fn texts(&self, id: PageId) -> Vec<String> {
        self.with(|s| {
            s.load_document(id)
                .unwrap()
                .blocks
                .iter()
                .map(|b| b.text.clone())
                .collect()
        })
    }

    /// Every block text on the page, including nested blocks.
    pub fn all_texts(&self, id: PageId) -> Vec<String> {
        fn walk(blocks: &[Block], out: &mut Vec<String>) {
            for b in blocks {
                out.push(b.text.clone());
                walk(&b.children, out);
            }
        }
        self.with(|s| {
            let mut out = Vec::new();
            walk(&s.load_document(id).unwrap().blocks, &mut out);
            out
        })
    }

    pub fn find(&self, title: &str) -> PageId {
        self.with(|s| {
            s.list_pages()
                .unwrap()
                .into_iter()
                .find(|p| p.title == title)
                .unwrap_or_else(|| panic!("no page {title}"))
                .id
        })
    }

    pub fn pending(&self) -> i64 {
        self.with(|s| s.sync_stats().unwrap().pending_ops)
    }
}

/// A pseudo-random file that does not compress into repeats.
pub fn bytes(len: usize, seed: u8) -> Vec<u8> {
    let mut x = 0x2545F4914F6CDD1Du64 ^ seed as u64;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x >> 24) as u8
        })
        .collect()
}

impl Dev {
    /// Adds a file and an image block for it at the end of `title`'s page. Returns its content id.
    pub fn add_image(&self, title: &str, name: &str, data: &[u8]) -> String {
        let page = self.find(title);
        self.with(|s| {
            let meta = s.attachment_add(name, "image/png", data).unwrap();
            let mut doc = s.load_document(page).unwrap();
            let mut b = Block::new(BlockKind::Image, "caption");
            b.attrs.insert("attachment".into(), meta.id.clone().into());
            b.attrs.insert("name".into(), name.into());
            b.attrs.insert("mime".into(), "image/png".into());
            b.attrs.insert("size".into(), meta.size.into());
            doc.blocks.push(b);
            s.save_document(page, &doc).unwrap();
            meta.id
        })
    }

    pub fn attachment(&self, id: &str) -> Option<notercrux_storage::AttachmentMeta> {
        self.with(|s| s.attachment(id).unwrap())
    }

    pub fn attachment_data(&self, id: &str) -> Option<Vec<u8>> {
        self.with(|s| s.attachment_data(id).unwrap())
    }
}

pub fn doc(texts: &[&str]) -> Document {
    Document {
        version: 1,
        blocks: texts
            .iter()
            .map(|t| Block::new(BlockKind::Paragraph, *t))
            .collect(),
    }
}

/// Runs the whole enrollment between two devices, as the two people would.
pub async fn enroll(from: &Dev, to: &Dev, label: &str, mode: notercrux_sync::JoinMode) {
    let offer = from.engine.create_invite().await.unwrap();
    let sas_new = to.engine.begin_join(&offer.code, label).await.unwrap();
    let req = from
        .engine
        .pending_join(&offer.invite_id)
        .await
        .unwrap()
        .expect("a waiting device");
    assert!(
        req.verified,
        "the request must prove knowledge of the invite secret"
    );
    assert_eq!(
        req.sas, sas_new,
        "both screens must show the same short code"
    );
    assert_eq!(req.label, label);
    assert_eq!(
        to.engine.poll_join(mode).await.unwrap(),
        notercrux_sync::JoinProgress::Waiting
    );
    from.engine.approve_join(&offer.invite_id).await.unwrap();
    assert_eq!(
        to.engine.poll_join(mode).await.unwrap(),
        notercrux_sync::JoinProgress::Joined
    );
}

/// Everyone syncs until nothing changes.
pub async fn settle(devs: &[&Dev]) {
    for _ in 0..4 {
        for d in devs {
            d.sync().await;
        }
    }
}

pub fn same_pages(a: &Dev, b: &Dev) {
    let norm = |d: &Dev| {
        d.with(|s| {
            let mut pages = s.list_pages().unwrap();
            pages.sort_by_key(|p| p.id);
            pages
                .into_iter()
                .map(|p| {
                    let texts: Vec<String> = s
                        .load_document(p.id)
                        .unwrap()
                        .blocks
                        .iter()
                        .map(|b| b.text.clone())
                        .collect();
                    (p.id, p.parent_id, p.title, p.archived_at.is_some(), texts)
                })
                .collect::<Vec<_>>()
        })
    };
    assert_eq!(norm(a), norm(b), "{} and {} diverged", a.name, b.name);
}
