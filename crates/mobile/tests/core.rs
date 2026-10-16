//! The Swift-facing API against a real server, with a "desktop" peer using the engine directly.

use notercrux_domain::{Block, BlockKind, Document};
use notercrux_mobile::{Core, CoreListener, SecretVault};
use notercrux_storage::Store;
use notercrux_sync::{Engine, EngineConfig, JoinMode, MemorySecrets, SecretStore};
use notercrux_sync_server::{config::Config, router, AppState};
use serde_json::Value;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Vault(Mutex<HashMap<String, Vec<u8>>>);

impl SecretVault for Vault {
    fn get(&self, name: String) -> Option<Vec<u8>> {
        self.0.lock().unwrap().get(&name).cloned()
    }
    fn set(&self, name: String, value: Vec<u8>) -> bool {
        self.0.lock().unwrap().insert(name, value);
        true
    }
    fn delete(&self, name: String) -> bool {
        self.0.lock().unwrap().remove(&name);
        true
    }
}

#[derive(Default)]
struct Listener {
    statuses: Mutex<Vec<String>>,
    changes: Mutex<Vec<String>>,
}

impl CoreListener for Listener {
    fn status_changed(&self, s: String) {
        self.statuses.lock().unwrap().push(s);
    }
    fn data_changed(&self, r: String) {
        self.changes.lock().unwrap().push(r);
    }
}

async fn server() -> String {
    let state = AppState::in_memory(Config::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = router(state);
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .ok();
    });
    format!("http://{addr}")
}

fn desktop() -> (Arc<Mutex<Store>>, Arc<Engine>) {
    let store = Arc::new(Mutex::new(Store::open_in_memory().unwrap()));
    let engine = Engine::open(
        store.clone(),
        Arc::new(MemorySecrets::new()) as Arc<dyn SecretStore>,
        EngineConfig::default(),
    )
    .unwrap();
    (store, engine)
}

fn mobile(tag: &str) -> Arc<Core> {
    let dir = std::env::temp_dir().join(format!("ncx-mobile-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    Core::new(dir.to_string_lossy().into(), Arc::new(Vault::default())).unwrap()
}

fn texts(doc_json: &str) -> Vec<String> {
    let d: Document = serde_json::from_str(doc_json).unwrap();
    d.blocks.iter().map(|b| b.text.clone()).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_phone_joins_a_desktop_workspace_and_both_edit_independently_without_loss() {
    let url = server().await;
    let (dstore, dengine) = desktop();
    let page = {
        let mut s = dstore.lock().unwrap();
        let p = s.create_page(None, "Shared").unwrap();
        s.save_document(
            p.id,
            &Document {
                version: 1,
                blocks: vec![
                    Block::new(BlockKind::Paragraph, "one"),
                    Block::new(BlockKind::Paragraph, "two"),
                ],
            },
        )
        .unwrap();
        p
    };
    dengine.create_workspace(&url, None, "Mac").await.unwrap();
    dengine.sync_once().await.unwrap();

    // Enrollment, as the two people would do it.
    let phone = mobile("join");
    let offer = dengine.create_invite().await.unwrap();
    let sas = phone
        .begin_join(offer.code.clone(), "iPhone".into())
        .await
        .unwrap();
    let seen = dengine
        .pending_join(&offer.invite_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(seen.sas, sas);
    assert!(seen.verified);
    assert_eq!(
        phone.poll_join("keepLocal".into()).await.unwrap(),
        "waiting"
    );
    dengine.approve_join(&offer.invite_id).await.unwrap();
    assert_eq!(phone.poll_join("keepLocal".into()).await.unwrap(), "joined");

    let listener = Arc::new(Listener::default());
    phone.start_sync(listener.clone());
    // The phone receives the page through its own background loop.
    let mut got = None;
    for _ in 0..100 {
        let pages: Value = serde_json::from_str(&phone.list_pages_json().unwrap()).unwrap();
        if let Some(p) = pages
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["title"] == "Shared")
        {
            got = Some(p["id"].as_str().unwrap().to_string());
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let id = got.expect("the page reached the phone");
    assert_eq!(id, page.id.to_string());
    assert!(!listener.statuses.lock().unwrap().is_empty());
    assert!(listener
        .changes
        .lock()
        .unwrap()
        .iter()
        .any(|c| c.contains("\"pagesChanged\":true")));
    let status: Value = serde_json::from_str(&phone.sync_status_json().unwrap()).unwrap();
    assert_ne!(status["phase"], "disabled");

    // Both sides edit different blocks and add pages, each while "offline" from the other.
    let base = phone.load_document_json(id.clone()).unwrap();
    let mut edited: Document = serde_json::from_str(&base).unwrap();
    edited.blocks[0].text = "one (phone)".into();
    edited
        .blocks
        .push(Block::new(BlockKind::Todo, "phone todo"));
    let saved = phone
        .save_document_rebased_json(id.clone(), base, serde_json::to_string(&edited).unwrap())
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&saved).unwrap()["conflicts"],
        0
    );
    let phone_page: Value = serde_json::from_str(
        &phone
            .create_page_json(None, "From the phone".into())
            .unwrap(),
    )
    .unwrap();
    {
        let mut s = dstore.lock().unwrap();
        let mut d = s.load_document(page.id).unwrap();
        d.blocks[1].text = "two (desktop)".into();
        s.save_document(page.id, &d).unwrap();
        s.create_page(None, "From the desktop").unwrap();
    }
    phone.sync_soon();

    // Converge.
    let mut converged = false;
    for _ in 0..150 {
        dengine.sync_once().await.unwrap();
        let on_phone = texts(&phone.load_document_json(id.clone()).unwrap());
        let on_desktop = dstore.lock().unwrap().load_document(page.id).unwrap();
        let d: Vec<String> = on_desktop.blocks.iter().map(|b| b.text.clone()).collect();
        let titles: Value = serde_json::from_str(&phone.list_pages_json().unwrap()).unwrap();
        let has = |t: &str| titles.as_array().unwrap().iter().any(|p| p["title"] == t);
        if on_phone == d
            && d == ["one (phone)", "two (desktop)", "phone todo"]
            && has("From the desktop")
        {
            converged = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(converged, "both devices hold both sets of edits");
    assert!(dstore
        .lock()
        .unwrap()
        .list_pages()
        .unwrap()
        .iter()
        .any(|p| p.id.to_string() == phone_page["id"].as_str().unwrap()));
    phone.stop_sync();
}

#[tokio::test(flavor = "multi_thread")]
async fn bad_input_becomes_errors_not_crashes_and_leaving_keeps_pages() {
    let phone = mobile("errors");
    assert!(phone.load_document_json("not-an-id".into()).is_err());
    assert!(phone
        .begin_join("garbage".into(), "x".into())
        .await
        .is_err());
    assert!(phone.poll_join("sideways".into()).await.is_err());
    let p: Value =
        serde_json::from_str(&phone.create_page_json(None, "Local".into()).unwrap()).unwrap();
    let id = p["id"].as_str().unwrap().to_string();
    phone.rename_page(id.clone(), "Renamed".into()).unwrap();
    phone.archive_page(id.clone()).unwrap();
    phone.restore_page(id.clone()).unwrap();
    let hits: Value = serde_json::from_str(&phone.search_json("Renamed".into()).unwrap()).unwrap();
    assert_eq!(hits.as_array().unwrap().len(), 1, "titles are searchable");
    phone.leave_local().await.unwrap();
    assert!(phone.list_pages_json().unwrap().contains("Renamed"));
    let _ = JoinMode::KeepLocal;
}
