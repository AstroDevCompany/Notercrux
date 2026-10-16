//! The multi-device scenarios from the phase 4 blueprint, through the real server and encryption.

mod common;

use common::*;
use notercrux_domain::{Block, BlockKind, Document};
use notercrux_sync::{JoinMode, Phase};
use std::time::Duration;

/// A synced pair with one shared page.
async fn pair() -> (Server, Dev, Dev) {
    let server = Server::start().await;
    let (a, b) = (Dev::new("a"), Dev::new("b"));
    a.page("Shared", &["one", "two", "three"]);
    a.engine
        .create_workspace(&server.url, None, "A")
        .await
        .unwrap();
    enroll(&a, &b, "B", JoinMode::ReplaceLocal).await;
    settle(&[&a, &b]).await;
    assert_eq!(b.texts(b.find("Shared")), vec!["one", "two", "three"]);
    (server, a, b)
}

fn edit(d: &Dev, title: &str, f: impl FnOnce(&mut Document)) {
    let id = d.find(title);
    d.with(|s| {
        let mut doc = s.load_document(id).unwrap();
        f(&mut doc);
        s.save_document(id, &doc).unwrap();
    });
}

#[tokio::test]
async fn different_parts_edited_offline_on_two_devices_merge() {
    let (_server, a, b) = pair().await;
    edit(&a, "Shared", |d| {
        d.blocks[0].text = "one (edited on a)".into()
    });
    edit(&b, "Shared", |d| {
        d.blocks[2].text = "three (edited on b)".into()
    });
    a.page("Only on a", &["a"]);
    b.page("Only on b", &["b"]);
    settle(&[&a, &b]).await;
    let id = a.find("Shared");
    assert_eq!(
        a.texts(id),
        vec!["one (edited on a)", "two", "three (edited on b)"]
    );
    assert_eq!(a.titles(), vec!["Only on a", "Only on b", "Shared"]);
    same_pages(&a, &b);
}

#[tokio::test]
async fn editing_the_same_text_on_both_devices_converges_without_loss() {
    let (_server, a, b) = pair().await;
    edit(&a, "Shared", |d| d.blocks[1].text = "two plus A".into());
    edit(&b, "Shared", |d| d.blocks[1].text = "B plus two".into());
    settle(&[&a, &b]).await;
    same_pages(&a, &b);
    let merged = a.texts(a.find("Shared"))[1].clone();
    assert!(
        merged.contains('A') && merged.contains('B'),
        "both contributions survive: {merged}"
    );
}

#[tokio::test]
async fn deleting_a_block_on_one_device_while_another_edits_it_keeps_the_edit() {
    let (_server, a, b) = pair().await;
    edit(&a, "Shared", |d| d.blocks.retain(|b| b.text != "two"));
    edit(&b, "Shared", |d| {
        d.blocks[1].text = "two, carefully rewritten".into()
    });
    settle(&[&a, &b]).await;
    same_pages(&a, &b);
    let texts = a.all_texts(a.find("Shared"));
    assert!(
        texts.iter().any(|t| t.contains("carefully rewritten")),
        "the edit was recovered: {texts:?}"
    );
    assert!(a.engine.status().recovered_blocks + b.engine.status().recovered_blocks >= 1);
}

#[tokio::test]
async fn moving_a_page_while_another_device_edits_it_applies_both() {
    let (_server, a, b) = pair().await;
    let parent = a.page("Folder", &[]);
    settle(&[&a, &b]).await;
    let shared = a.find("Shared");
    a.with(|s| s.move_page(shared, Some(parent), 0).unwrap());
    edit(&b, "Shared", |d| {
        d.blocks[0].text = "edited while moved".into()
    });
    settle(&[&a, &b]).await;
    same_pages(&a, &b);
    assert_eq!(
        b.with(|s| s.get_page(shared).unwrap().unwrap().parent_id),
        Some(parent)
    );
    assert_eq!(b.texts(shared)[0], "edited while moved");
}

#[tokio::test]
async fn a_long_offline_stretch_syncs_in_one_go() {
    let (_server, a, b) = pair().await;
    for i in 0..60 {
        a.page(&format!("Offline {i}"), &["text"]);
        edit(&a, "Shared", |d| {
            d.blocks
                .push(Block::new(BlockKind::Paragraph, format!("line {i}")))
        });
    }
    b.page("Meanwhile", &["b"]);
    assert!(a.pending() > 100);
    settle(&[&a, &b]).await;
    same_pages(&a, &b);
    assert_eq!(a.titles().len(), 1 + 60 + 1);
    assert_eq!(a.texts(a.find("Shared")).len(), 3 + 60);
    assert_eq!(a.pending(), 0);
}

#[tokio::test]
async fn closing_the_app_between_upload_and_acknowledgement_loses_and_duplicates_nothing() {
    let (server, a, b) = pair().await;
    let proxy = Proxy::to(&server.url).await;
    // Re-point a's engine at the proxy by creating a workspace link through it is not possible for an
    // existing account, so build a fresh pair through the proxy instead.
    drop((a, b));
    let (a, b) = (Dev::new("a"), Dev::new("b"));
    a.page("Doc", &["start"]);
    a.engine
        .create_workspace(&proxy.url, None, "A")
        .await
        .unwrap();
    enroll(&a, &b, "B", JoinMode::ReplaceLocal).await;
    settle(&[&a, &b]).await;

    edit(&a, "Doc", |d| {
        d.blocks
            .push(Block::new(BlockKind::Paragraph, "typed before the crash"))
    });
    // The server stores the upload, but the response never reaches the app (it was closed).
    proxy.lose_next_response("/ops");
    assert!(a.engine.sync_once().await.is_err());
    assert!(a.pending() > 0, "no acknowledgement was received");
    let on_server = server
        .state
        .repo
        .pull_ops(&a.engine.status().workspace_id.unwrap(), 0, 10_000)
        .await
        .unwrap()
        .ops
        .len();

    // Relaunch and sync: the same operation ids are re-sent, the server recognizes them.
    let again = a.relaunch();
    again.sync().await;
    assert_eq!(again.pending(), 0);
    let after = server
        .state
        .repo
        .pull_ops(&again.engine.status().workspace_id.unwrap(), 0, 10_000)
        .await
        .unwrap()
        .ops
        .len();
    assert_eq!(after, on_server, "nothing was stored twice");
    b.sync().await;
    assert_eq!(
        b.texts(b.find("Doc")),
        vec!["start", "typed before the crash"]
    );
}

#[tokio::test]
async fn a_lost_request_is_retried_and_a_dead_server_is_survived() {
    let server = Server::start().await;
    let proxy = Proxy::to(&server.url).await;
    let a = Dev::new("a");
    a.page("Doc", &["x"]);
    a.engine
        .create_workspace(&proxy.url, None, "A")
        .await
        .unwrap();

    proxy.lose_next_request("/ops");
    assert!(a.engine.sync_once().await.is_err());
    assert_eq!(a.engine.status().phase, Phase::Offline);
    assert!(a.engine.status().last_error.is_some());
    a.sync().await;
    assert_eq!(a.engine.status().phase, Phase::Idle);

    proxy.set_down(true);
    edit(&a, "Doc", |d| d.blocks[0].text = "while down".into());
    assert!(a.engine.sync_once().await.is_err());
    assert!(a.pending() > 0, "edits wait in the outbox");
    proxy.set_down(false);
    a.sync().await;
    assert_eq!(a.pending(), 0);
}

#[tokio::test]
async fn the_background_loop_reconnects_with_backoff_and_pushes_edits_promptly() {
    let server = Server::start().await;
    let proxy = Proxy::to(&server.url).await;
    let (a, b) = (Dev::new("a"), Dev::new("b"));
    a.page("Doc", &["start"]);
    a.engine
        .create_workspace(&proxy.url, None, "A")
        .await
        .unwrap();
    enroll(&a, &b, "B", JoinMode::ReplaceLocal).await;
    settle(&[&a, &b]).await;

    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let runner_a = tokio::spawn(a.engine.clone().run(stop_rx.clone()));
    let runner_b = tokio::spawn(b.engine.clone().run(stop_rx));

    // A local edit on a reaches b with nobody calling sync.
    edit(&a, "Doc", |d| {
        d.blocks.push(Block::new(BlockKind::Paragraph, "live edit"))
    });
    // (the engine notices the queued operation by itself; nobody tells it)
    wait_for(|| b.texts(b.find("Doc")).contains(&"live edit".to_string())).await;

    // The network drops; edits pile up; the loop backs off and then recovers by itself.
    proxy.set_down(true);
    edit(&a, "Doc", |d| {
        d.blocks
            .push(Block::new(BlockKind::Paragraph, "typed offline"))
    });
    a.engine.notify_local_change();
    wait_for(|| a.engine.status().phase == Phase::Offline).await;
    assert!(a.engine.status().retry_in_secs.is_some());
    let before = proxy.count("/ops");
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let attempts_while_down = proxy.count("/ops") - before;
    proxy.set_down(false);
    wait_for(|| {
        b.texts(b.find("Doc"))
            .contains(&"typed offline".to_string())
    })
    .await;
    wait_for(|| a.engine.status().phase == Phase::Idle).await;
    assert!(
        attempts_while_down < 40,
        "backoff keeps retries sparse ({attempts_while_down} in 1.5 s)"
    );

    stop_tx.send(true).unwrap();
    runner_a.await.unwrap();
    runner_b.await.unwrap();
}

async fn wait_for(mut cond: impl FnMut() -> bool) {
    for _ in 0..200 {
        if cond() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for a condition");
}

#[tokio::test]
async fn a_revoked_device_keeps_its_unsynced_pages_but_stops_syncing() {
    let (server, a, b) = pair().await;
    // a works offline and has changes nobody has seen.
    edit(&a, "Shared", |d| {
        d.blocks
            .push(Block::new(BlockKind::Paragraph, "unsynced work on a"))
    });
    a.page("Written offline", &["precious"]);

    let a_id = a.engine.status().device_id.unwrap();
    b.engine.revoke_device(&a_id).await.unwrap();

    let err = a.engine.sync_once().await.unwrap_err();
    assert_eq!(err, notercrux_sync::SyncError::Revoked);
    let st = a.engine.status();
    assert_eq!(st.phase, Phase::Revoked);
    assert!(st.pending_ops > 0, "the unsynced changes are still queued");
    assert!(
        a.titles().contains(&"Written offline".to_string()),
        "local pages are untouched"
    );
    assert!(a
        .texts(a.find("Shared"))
        .contains(&"unsynced work on a".to_string()));

    // Stays revoked after a restart, and cannot be talked back in.
    let again = a.relaunch();
    assert_eq!(again.engine.status().phase, Phase::Revoked);
    assert!(again.engine.sync_once().await.is_err());
    assert!(!server
        .state
        .repo
        .pull_ops(&b.engine.status().workspace_id.unwrap(), 0, 1000)
        .await
        .unwrap()
        .ops
        .iter()
        .any(|o| o.device_id == a_id && o.seq > 1000));

    // The person can keep the pages as a standalone copy.
    again.engine.leave_local().await.unwrap();
    assert_eq!(again.engine.status().phase, Phase::Disabled);
    assert!(again.titles().contains(&"Written offline".to_string()));
}

#[tokio::test]
async fn remaining_devices_pick_up_the_rotated_key_and_keep_syncing() {
    let (server, a, b) = pair().await;
    let c = Dev::new("c");
    enroll(&a, &c, "C", JoinMode::ReplaceLocal).await;
    settle(&[&a, &b, &c]).await;

    let c_id = c.engine.status().device_id.unwrap();
    a.engine.revoke_device(&c_id).await.unwrap();
    assert_eq!(a.engine.status().epoch, 1);

    // b has not heard about the rotation. It learns from the next pull and re-keys itself.
    edit(&a, "Shared", |d| {
        d.blocks
            .push(Block::new(BlockKind::Paragraph, "after rotation"))
    });
    a.sync().await;
    b.sync().await;
    assert_eq!(b.engine.status().epoch, 1);
    assert!(b
        .texts(b.find("Shared"))
        .contains(&"after rotation".to_string()));
    edit(&b, "Shared", |d| {
        d.blocks
            .push(Block::new(BlockKind::Paragraph, "b after rotation"))
    });
    settle(&[&a, &b]).await;
    same_pages(&a, &b);

    // New operations are sealed under epoch 1, which the revoked device never received.
    let ws = a.engine.status().workspace_id.unwrap();
    let ops = server
        .state
        .repo
        .pull_ops(&ws, 0, 10_000)
        .await
        .unwrap()
        .ops;
    assert!(ops.iter().any(|o| o.epoch == 1));
    let stale_ring =
        notercrux_crypto::KeyRing::from_bytes(&c.secrets.get_for_test("ring").unwrap()).unwrap();
    assert_eq!(stale_ring.current_epoch(), 0);
    for op in ops.iter().filter(|o| o.epoch == 1) {
        assert!(!stale_ring.has_epoch(op.epoch));
        assert!(stale_ring
            .open(
                0,
                notercrux_crypto::Purpose::Content,
                &notercrux_crypto::content_aad(&ws, &op.doc_id, 0),
                &op.payload
            )
            .is_err());
    }
}

#[tokio::test]
async fn operations_from_a_newer_app_are_held_and_applied_after_an_update() {
    let (server, a, _b) = pair().await;
    let ws = a.engine.status().workspace_id.unwrap();
    let ring =
        notercrux_crypto::KeyRing::from_bytes(&a.secrets.get_for_test("ring").unwrap()).unwrap();

    // A newer app on another device writes an operation in a format this build does not know.
    let doc_key = a.with(|s| format!("page:{}", s.list_pages().unwrap()[0].id));
    let env = notercrux_protocol::OpEnvelope {
        format: 999,
        doc_key: doc_key.clone(),
        update: vec![1, 2, 3],
    }
    .encode();
    let doc_id = ring.doc_id(&doc_key).unwrap();
    let (epoch, payload) = ring
        .seal(
            notercrux_crypto::Purpose::Content,
            |e| notercrux_crypto::content_aad(&ws, &doc_id, e),
            &env,
        )
        .unwrap();
    let op = notercrux_protocol::PushOp {
        op_id: "from-the-future".into(),
        doc_id,
        epoch,
        format: 999,
        payload,
    };
    let limits = server.state.config.limits.clone();
    server
        .state
        .repo
        .push_ops(&ws, "some-newer-device", &[op], 1, &limits)
        .await
        .unwrap();

    let r = a.sync().await;
    assert_eq!(r.applied.held, 1);
    let st = a.engine.status();
    assert_eq!(st.held_ops, 1, "kept for later, not dropped");
    assert_eq!(st.rejected_ops, 0);
    // Normal syncing continues around it.
    edit(&a, "Shared", |d| {
        d.blocks
            .push(Block::new(BlockKind::Paragraph, "still works"))
    });
    a.sync().await;
    assert_eq!(a.pending(), 0);
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Three devices make random edits (some offline, some synced at random moments); after a final
/// round of syncing all of them hold exactly the same pages and text.
#[tokio::test]
async fn three_devices_with_random_edits_converge_through_the_real_server() {
    for seed in [1u64, 7, 42] {
        let server = Server::start().await;
        let devs = [Dev::new("a"), Dev::new("b"), Dev::new("c")];
        devs[0].page("Home", &["start"]);
        devs[0]
            .engine
            .create_workspace(&server.url, None, "A")
            .await
            .unwrap();
        enroll(&devs[0], &devs[1], "B", JoinMode::ReplaceLocal).await;
        enroll(&devs[0], &devs[2], "C", JoinMode::ReplaceLocal).await;
        settle(&[&devs[0], &devs[1], &devs[2]]).await;

        let mut rng = Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1);
        for step in 0..60 {
            let d = &devs[rng.below(3)];
            match rng.below(6) {
                0 => {
                    d.page(&format!("page-{step}"), &["hello"]);
                }
                1 | 2 => {
                    let ids: Vec<_> = d.with(|s| {
                        s.list_pages()
                            .unwrap()
                            .into_iter()
                            .filter(|p| p.archived_at.is_none())
                            .map(|p| p.id)
                            .collect()
                    });
                    if let Some(id) = ids.get(rng.below(ids.len().max(1))).copied() {
                        let n = rng.below(3);
                        d.with(|s| {
                            let mut doc = s.load_document(id).unwrap();
                            match n {
                                0 => doc.blocks.push(Block::new(
                                    BlockKind::Paragraph,
                                    format!("{} added {step}", d.name),
                                )),
                                1 if !doc.blocks.is_empty() => {
                                    let i = rng.below(doc.blocks.len());
                                    doc.blocks[i].text.push_str(&format!(" +{}{step}", d.name));
                                }
                                _ if doc.blocks.len() > 1 => {
                                    let i = rng.below(doc.blocks.len());
                                    doc.blocks.remove(i);
                                }
                                _ => {}
                            }
                            s.save_document(id, &doc).unwrap();
                        });
                    }
                }
                3 => {
                    let ids: Vec<_> =
                        d.with(|s| s.list_pages().unwrap().into_iter().map(|p| p.id).collect());
                    if let Some(id) = ids.get(rng.below(ids.len().max(1))).copied() {
                        d.with(|s| {
                            s.rename_page(id, &format!("renamed {} {step}", d.name))
                                .unwrap()
                        });
                    }
                }
                _ => {
                    d.sync().await;
                }
            }
        }
        settle(&[&devs[0], &devs[1], &devs[2]]).await;
        same_pages(&devs[0], &devs[1]);
        same_pages(&devs[0], &devs[2]);
        assert!(devs.iter().all(|d| d.pending() == 0), "seed {seed}");
    }
}
