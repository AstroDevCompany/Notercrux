//! A stand-in "desktop" device for trying other clients (the iOS app) against a server.
//!
//!   cargo run -p notercrux-sync --example peer -- http://192.168.1.20:8787 /tmp/peer-data
//!
//! Creates a workspace with a few pages, prints an invite code, approves the first device that asks
//! to join (printing the verification code to compare), then keeps syncing and appends a line to the
//! first page every 20 seconds so concurrent edits can be observed. Not part of the app.

use notercrux_domain::{Block, BlockKind};
use notercrux_storage::Store;
use notercrux_sync::{Engine, EngineConfig, MemorySecrets, SecretStore};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let server = args.next().expect("usage: peer <server url> <data dir>");
    let dir = args.next().expect("usage: peer <server url> <data dir>");
    std::fs::create_dir_all(&dir).unwrap();
    let store = Arc::new(Mutex::new(Store::open(format!("{dir}/peer.db")).unwrap()));
    {
        let mut s = store.lock().unwrap();
        if s.list_pages().unwrap().is_empty() {
            let p = s.create_page(None, "Shared plans").unwrap();
            let blocks = ["Groceries", "Call the dentist", "Ideas for the trip"]
                .iter()
                .map(|t| Block::new(BlockKind::Todo, *t))
                .collect();
            s.save_document(p.id, &notercrux_domain::Document { version: 1, blocks })
                .unwrap();
            s.create_page(None, "Reading list").unwrap();
        }
    }
    let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecrets::new());
    let engine = Engine::open(store.clone(), secrets, EngineConfig::default()).unwrap();
    let created = engine
        .create_workspace(&server, None, "Peer (desktop stand-in)")
        .await
        .unwrap();
    println!("workspace {}", created.workspace_id);
    println!("recovery key {}", created.recovery_phrase);
    engine.sync_once().await.unwrap();
    let (_stop, rx) = tokio::sync::watch::channel(false);
    tokio::spawn(engine.clone().run(rx));

    let offer = engine.create_invite().await.unwrap();
    println!("INVITE {}", offer.code);
    println!("(valid for 15 minutes; press Ctrl-C to stop)");
    let mut approved = false;
    let mut tick = 0u64;
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        tick += 1;
        if !approved {
            if let Ok(Some(req)) = engine.pending_join(&offer.invite_id).await {
                println!(
                    "join request from \"{}\", verification code {}, verified={}",
                    req.label, req.sas, req.verified
                );
                if req.verified {
                    engine.approve_join(&offer.invite_id).await.unwrap();
                    println!("approved");
                    approved = true;
                }
            }
        }
        if approved && tick.is_multiple_of(20) {
            let mut s = store.lock().unwrap();
            if let Some(p) = s
                .list_pages()
                .unwrap()
                .into_iter()
                .find(|p| p.title == "Shared plans")
            {
                let mut d = s.load_document(p.id).unwrap();
                d.blocks.push(Block::new(
                    BlockKind::Paragraph,
                    format!("desktop edit at {}", chrono_like(tick)),
                ));
                s.save_document(p.id, &d).unwrap();
                println!("appended a line to \"Shared plans\"");
            }
        }
        if tick.is_multiple_of(10) {
            let s = store.lock().unwrap();
            let titles: Vec<String> = s
                .list_pages()
                .unwrap()
                .into_iter()
                .filter(|p| p.archived_at.is_none())
                .map(|p| p.title)
                .collect();
            println!(
                "[{}s] pages: {:?}  status: {:?}",
                tick,
                titles,
                engine.status().phase
            );
        }
    }
}

fn chrono_like(tick: u64) -> String {
    format!("t+{tick}s")
}
