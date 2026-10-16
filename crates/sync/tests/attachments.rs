#![allow(clippy::field_reassign_with_default)]
//! Attachment sync: encrypted chunked blobs, resume after interruption, tamper detection.

mod common;

use common::*;
use notercrux_storage::{AttachmentState, ATTACHMENT_CHUNK};
use notercrux_sync::{JoinMode, Phase};

/// Two devices on one workspace, the first reached through a fault-injecting proxy.
async fn pair_via_proxy() -> (Server, Proxy, Dev, Dev) {
    let server = Server::start().await;
    let proxy = Proxy::to(&server.url).await;
    let (a, b) = (Dev::new("a"), Dev::new("b"));
    a.page("Album", &["photos"]);
    a.engine
        .create_workspace(&proxy.url, None, "A")
        .await
        .unwrap();
    enroll(&a, &b, "B", JoinMode::ReplaceLocal).await;
    settle(&[&a, &b]).await;
    (server, proxy, a, b)
}

fn point_at(d: &Dev, url: &str) -> Dev {
    d.with(|s| {
        let mut acct: serde_json::Value =
            serde_json::from_str(&s.sync_get("acct").unwrap().unwrap()).unwrap();
        acct["server"] = serde_json::Value::String(url.to_string());
        s.sync_set("acct", &acct.to_string()).unwrap();
    });
    d.relaunch()
}

#[tokio::test]
async fn an_image_added_on_one_device_arrives_intact_on_another() {
    let (_server, _proxy, a, b) = pair_via_proxy().await;
    let data = bytes(ATTACHMENT_CHUNK * 2 + 12_345, 1); // three chunks
    let id = a.add_image("Album", "holiday.png", &data);
    assert_eq!(a.attachment(&id).unwrap().state, AttachmentState::Local);

    let r = a.sync().await;
    assert_eq!(r.attachments.uploaded, 1);
    assert_eq!(a.attachment(&id).unwrap().state, AttachmentState::Synced);

    // b first sees the block (with a placeholder), then the bytes.
    let r = b.sync().await;
    assert_eq!(r.attachments.downloaded, 1, "{:?}", b.engine.status());
    let got = b.attachment(&id).unwrap();
    assert_eq!(
        (got.state, got.name.as_str(), got.mime.as_str()),
        (AttachmentState::Synced, "holiday.png", "image/png")
    );
    assert_eq!(b.attachment_data(&id).unwrap(), data);
    same_pages(&a, &b);
    assert_eq!(b.engine.status().attachments_pending, 0);
}

#[tokio::test]
async fn the_server_holds_only_encrypted_chunks_and_adding_the_same_file_twice_stores_it_once() {
    let (server, _proxy, a, b) = pair_via_proxy().await;
    let data = bytes(ATTACHMENT_CHUNK + 5000, 2);
    let id = a.add_image("Album", "a.png", &data);
    a.sync().await;
    settle(&[&a, &b]).await;

    let stored = server.blobs.all();
    assert_eq!(stored.len(), 2, "two chunks");
    for chunk in &stored {
        assert!(
            !chunk
                .windows(64)
                .any(|w| data.windows(64).next() == Some(w)),
            "plaintext leaked"
        );
        assert!(chunk.len() > 5000 && !chunk.windows(32).any(|w| w == &data[..32]));
        assert!(
            !chunk.windows(id.len()).any(|w| w == id.as_bytes()),
            "the content hash must not be visible either"
        );
    }

    // The same bytes added on the other device (under another name) reuse the stored blob.
    let before = server.blobs.all().len();
    b.add_image("Album", "copy-of-a.png", &data);
    settle(&[&a, &b]).await;
    assert_eq!(
        server.blobs.all().len(),
        before,
        "identical content is stored once"
    );
    assert_eq!(b.attachment(&id).unwrap().state, AttachmentState::Synced);
}

#[tokio::test]
async fn an_interrupted_upload_resumes_with_only_the_missing_chunks() {
    let (server, proxy, a, b) = pair_via_proxy().await;
    let a = point_at(&a, &proxy.url);
    let data = bytes(ATTACHMENT_CHUNK * 3 + 1, 3); // four chunks
    let id = a.add_image("Album", "big.png", &data);

    // The connection dies while sending chunk 2.
    proxy.lose_next_request("/chunks/2");
    assert!(a.engine.sync_once().await.is_err());
    assert_eq!(a.attachment(&id).unwrap().state, AttachmentState::Local);
    assert_eq!(
        proxy.count("PUT"),
        3,
        "chunks 0, 1 and the failed attempt at 2"
    );

    // Meanwhile the text (the block) already reached the other device, which shows a placeholder.
    b.sync().await;
    assert_eq!(b.attachment(&id).unwrap().state, AttachmentState::Missing);
    assert!(b
        .all_texts(b.find("Album"))
        .contains(&"caption".to_string()));

    // Retrying sends chunks 2 and 3 only.
    a.sync().await;
    assert_eq!(proxy.count("PUT"), 5, "no chunk was sent twice");
    assert_eq!(proxy.count("/chunks/0"), 1);
    assert_eq!(a.attachment(&id).unwrap().state, AttachmentState::Synced);
    b.sync().await;
    assert_eq!(b.attachment_data(&id).unwrap(), data);
    assert_eq!(server.blobs.all().len(), 4);
}

#[tokio::test]
async fn an_interrupted_download_keeps_what_it_has_and_finishes_later() {
    let (_server, proxy, a, b) = pair_via_proxy().await;
    let b = point_at(&b, &proxy.url);
    let data = bytes(ATTACHMENT_CHUNK * 2 + 10, 4); // three chunks
    let id = a.add_image("Album", "dl.png", &data);
    a.sync().await;

    proxy.lose_next_response("/chunks/1");
    assert!(b.engine.sync_once().await.is_err());
    let part = b.attachment(&id).unwrap();
    assert_eq!(part.state, AttachmentState::Missing);
    assert_eq!(part.chunks_present, 1, "chunk 0 was kept");

    // The app restarts; the next attempt fetches only chunks 1 and 2.
    let b = b.relaunch();
    b.sync().await;
    assert_eq!(
        proxy.count_both("GET", "/chunks/0"),
        1,
        "chunk 0 was not downloaded again"
    );
    assert_eq!(
        proxy.count_both("GET", "/chunks/1"),
        2,
        "chunk 1 was retried"
    );
    assert_eq!(b.attachment_data(&id).unwrap(), data);
    assert_eq!(b.attachment(&id).unwrap().state, AttachmentState::Synced);
}

#[tokio::test]
async fn text_keeps_syncing_while_an_attachment_cannot_be_transferred() {
    let (_server, proxy, a, b) = pair_via_proxy().await;
    let a = point_at(&a, &proxy.url);
    let id = a.add_image("Album", "x.png", &bytes(5000, 5));
    let page = a.find("Album");
    a.with(|s| {
        let mut d = s.load_document(page).unwrap();
        d.blocks.push(notercrux_domain::Block::new(
            notercrux_domain::BlockKind::Paragraph,
            "typed alongside",
        ));
        s.save_document(page, &d).unwrap();
    });
    proxy.lose_next_request("/chunks/0");
    assert!(a.engine.sync_once().await.is_err());
    assert_eq!(a.engine.status().phase, Phase::Offline);
    assert_eq!(
        a.pending(),
        0,
        "operations were not held up by the attachment"
    );
    b.sync().await;
    assert!(b
        .texts(b.find("Album"))
        .contains(&"typed alongside".to_string()));
    assert_eq!(b.attachment(&id).unwrap().state, AttachmentState::Missing);
    // Recovers on its own once the network behaves.
    a.sync().await;
    b.sync().await;
    assert_eq!(b.attachment(&id).unwrap().state, AttachmentState::Synced);
}

#[tokio::test]
async fn a_damaged_chunk_is_rejected_and_can_be_retried() {
    let (_server, proxy, a, b) = pair_via_proxy().await;
    let b = point_at(&b, &proxy.url);
    let data = bytes(ATTACHMENT_CHUNK + 50, 6);
    let id = a.add_image("Album", "t.png", &data);
    a.sync().await;

    proxy.tamper(|path, mut body| {
        if path.contains("/chunks/") && !body.is_empty() {
            let mid = body.len() / 2;
            body[mid] ^= 0x01;
        }
        body
    });
    b.sync().await;
    let m = b.attachment(&id).unwrap();
    assert_eq!(m.state, AttachmentState::Failed, "{m:?}");
    assert!(m.error.unwrap().contains("verification"));
    assert!(
        b.attachment_data(&id).is_none(),
        "no unverified bytes are exposed"
    );
    assert_eq!(b.engine.status().attachments_failed, 1);

    proxy.clear_tamper();
    assert_eq!(b.engine.retry_attachments().unwrap(), 1);
    b.sync().await;
    assert_eq!(b.attachment_data(&id).unwrap(), data);
    assert_eq!(b.engine.status().attachments_failed, 0);
}

#[tokio::test]
async fn chunks_cannot_be_reordered_by_the_server() {
    let (_server, proxy, a, b) = pair_via_proxy().await;
    let b = point_at(&b, &proxy.url);
    let data = bytes(ATTACHMENT_CHUNK * 2, 7);
    let id = a.add_image("Album", "o.png", &data);
    a.sync().await;
    // Serve chunk 0's ciphertext for every request.
    let first: std::sync::Arc<std::sync::Mutex<Option<Vec<u8>>>> = Default::default();
    let f = first.clone();
    proxy.tamper(move |path, body| {
        if path.ends_with("/chunks/0") {
            *f.lock().unwrap() = Some(body.clone());
            return body;
        }
        if path.ends_with("/chunks/1") {
            if let Some(c0) = f.lock().unwrap().clone() {
                return c0;
            }
        }
        body
    });
    b.sync().await;
    assert_eq!(b.attachment(&id).unwrap().state, AttachmentState::Failed);
    assert!(b.attachment_data(&id).is_none());
}

#[tokio::test]
async fn attachments_over_the_server_quota_are_parked_without_blocking_text() {
    let mut cfg = notercrux_sync_server::config::Config::default();
    cfg.limits.max_blob_bytes_total = 10_000;
    let server = Server::start_with(cfg).await;
    let (a, b) = (Dev::new("a"), Dev::new("b"));
    a.page("Album", &["photos"]);
    a.engine
        .create_workspace(&server.url, None, "A")
        .await
        .unwrap();
    enroll(&a, &b, "B", JoinMode::ReplaceLocal).await;
    settle(&[&a, &b]).await;

    let id = a.add_image("Album", "huge.png", &bytes(50_000, 8));
    let r = a.sync().await;
    assert_eq!(r.attachments.failed, 1);
    let m = a.attachment(&id).unwrap();
    assert_eq!(m.state, AttachmentState::Failed);
    assert!(m.error.unwrap().contains("quota"));
    assert_eq!(a.engine.status().attachments_failed, 1);
    assert_eq!(
        a.engine.status().phase,
        Phase::Idle,
        "a parked attachment is not a sync outage"
    );
    b.sync().await;
    assert_eq!(b.attachment(&id).unwrap().state, AttachmentState::Missing);
}

#[tokio::test]
async fn attachments_survive_key_rotation() {
    let (_server, _proxy, a, b) = pair_via_proxy().await;
    let c = Dev::new("c");
    enroll(&a, &c, "C", JoinMode::ReplaceLocal).await;
    settle(&[&a, &b, &c]).await;

    let before = bytes(ATTACHMENT_CHUNK + 1, 9);
    let id_before = a.add_image("Album", "before.png", &before);
    a.sync().await;

    let c_id = c.engine.status().device_id.unwrap();
    a.engine.revoke_device(&c_id).await.unwrap();
    let after = bytes(777, 10);
    let id_after = a.add_image("Album", "after.png", &after);
    a.sync().await;

    // b learns about the rotation while fetching, and opens chunks sealed under both epochs.
    b.sync().await;
    assert_eq!(b.engine.status().epoch, 1);
    assert_eq!(b.attachment_data(&id_before).unwrap(), before);
    assert_eq!(b.attachment_data(&id_after).unwrap(), after);
}
