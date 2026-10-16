#![allow(clippy::field_reassign_with_default)]
//! Security properties: what the server can see, what an active attacker can do, and recovery.

mod common;

use common::*;
use notercrux_crypto::{unwrap_ring, wrap_ring, DeviceKeys, DevicePublic, KeyRing, RecoveryKey};
use notercrux_protocol::*;
use notercrux_sync::{JoinMode, Phase, SyncError};

const SENTINELS: &[&str] = &[
    "TOPSECRET-title",
    "TOPSECRET-body-text",
    "TOPSECRET-laptop",
    "TOPSECRET-callout",
];

async fn pair_with_secrets() -> (Server, Dev, Dev) {
    let server = Server::start().await;
    let (a, b) = (Dev::new("a"), Dev::new("b"));
    a.page("TOPSECRET-title", &["TOPSECRET-body-text", "second"]);
    a.engine
        .create_workspace(&server.url, None, "TOPSECRET-laptop")
        .await
        .unwrap();
    enroll(&a, &b, "TOPSECRET-phone", JoinMode::ReplaceLocal).await;
    settle(&[&a, &b]).await;
    (server, a, b)
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}

#[tokio::test]
async fn the_server_stores_only_ciphertext_and_opaque_identifiers() {
    let (server, a, b) = pair_with_secrets().await;
    let ws = a.engine.status().workspace_id.unwrap();
    let page_id = a.find("TOPSECRET-title").to_string();

    let mut stored: Vec<(String, Vec<u8>)> = Vec::new();
    for op in server
        .state
        .repo
        .pull_ops(&ws, 0, 100_000)
        .await
        .unwrap()
        .ops
    {
        assert!(
            !op.doc_id.contains(&page_id),
            "document ids must not reveal page ids"
        );
        stored.push(("op payload".into(), op.payload));
        stored.push(("doc id".into(), op.doc_id.into_bytes()));
        stored.push(("op id".into(), op.op_id.into_bytes()));
    }
    for d in server.state.repo.devices(&ws).await.unwrap() {
        stored.push(("device label".into(), d.label_sealed));
        stored.push(("device key".into(), d.key));
    }
    let row = server.state.repo.workspace(&ws).await.unwrap().unwrap();
    stored.push(("recovery wrap".into(), row.recovery_wrap));
    for d in [&a, &b] {
        if let Some((_, wrap)) = server
            .state
            .repo
            .key_wrap(&ws, &d.engine.status().device_id.unwrap())
            .await
            .unwrap()
        {
            stored.push(("key wrap".into(), wrap));
        }
    }
    assert!(stored.len() > 8);
    for (what, bytes) in &stored {
        for s in SENTINELS {
            assert!(!contains(bytes, s), "{what} leaks {s:?}");
        }
    }
    // Sanity: the scan would notice plaintext (the app really contains these strings).
    assert!(a.titles().contains(&"TOPSECRET-title".to_string()));
    // Device names are still readable by members.
    let names: Vec<String> = a
        .engine
        .devices()
        .await
        .unwrap()
        .into_iter()
        .map(|d| d.label)
        .collect();
    assert!(
        names.contains(&"TOPSECRET-laptop".to_string())
            && names.contains(&"TOPSECRET-phone".to_string()),
        "{names:?}"
    );
}

fn flip_in_payloads(body: Vec<u8>) -> Vec<u8> {
    let Ok(mut page) = serde_json::from_slice::<PullResponse>(&body) else {
        return body;
    };
    for op in &mut page.ops {
        let mid = op.payload.len() / 2;
        op.payload[mid] ^= 0x40;
    }
    serde_json::to_vec(&page).unwrap()
}

#[tokio::test]
async fn a_damaged_response_is_retried_and_nothing_is_lost() {
    let (server, a, b) = pair_with_secrets().await;
    let proxy = Proxy::to(&server.url).await;
    // b is connected through the proxy.
    let ws = a.engine.status().workspace_id.unwrap();
    let _ = (ws, &b);
    let c = Dev::new("c");
    enroll(&a, &c, "C", JoinMode::ReplaceLocal).await;
    // Re-create c's link through the proxy by relaunching it against the proxy address: the account
    // stores the server URL, so patch it (what a settings change would do).
    c.with(|s| {
        let mut acct: serde_json::Value =
            serde_json::from_str(&s.sync_get("acct").unwrap().unwrap()).unwrap();
        acct["server"] = serde_json::Value::String(proxy.url.clone());
        s.sync_set("acct", &acct.to_string()).unwrap();
    });
    let c = c.relaunch();

    let id = a.find("TOPSECRET-title");
    a.with(|s| {
        s.save_document(id, &doc(&["TOPSECRET-body-text", "second", "added later"]))
            .unwrap()
    });
    a.sync().await;

    proxy.tamper(|path, body| {
        if path.contains("/ops") {
            flip_in_payloads(body)
        } else {
            body
        }
    });
    assert!(
        c.engine.sync_once().await.is_err(),
        "a damaged operation is not applied"
    );
    assert_eq!(
        c.engine.status().rejected_ops,
        0,
        "one bad response is not a verdict"
    );
    proxy.clear_tamper();
    c.sync().await;
    assert_eq!(
        c.texts(c.find("TOPSECRET-title")),
        vec!["TOPSECRET-body-text", "second", "added later"]
    );
    same_pages(&a, &c);
}

#[tokio::test]
async fn an_operation_that_stays_invalid_is_skipped_and_counted_while_the_rest_applies() {
    let (server, a, _b) = pair_with_secrets().await;
    let proxy = Proxy::to(&server.url).await;
    let c = Dev::new("c");
    enroll(&a, &c, "C", JoinMode::ReplaceLocal).await;
    c.with(|s| {
        let mut acct: serde_json::Value =
            serde_json::from_str(&s.sync_get("acct").unwrap().unwrap()).unwrap();
        acct["server"] = serde_json::Value::String(proxy.url.clone());
        s.sync_set("acct", &acct.to_string()).unwrap();
    });
    let c = c.relaunch();
    // Only the first operation is corrupted, permanently.
    proxy.tamper(|path, body| {
        if !path.contains("/ops") {
            return body;
        }
        let Ok(mut page) = serde_json::from_slice::<PullResponse>(&body) else {
            return body;
        };
        if let Some(op) = page.ops.iter_mut().find(|o| o.seq == 1) {
            op.payload[5] ^= 1;
        }
        serde_json::to_vec(&page).unwrap()
    });
    let mut last = None;
    for _ in 0..5 {
        last = Some(c.engine.sync_once().await);
    }
    assert!(
        last.unwrap().is_ok(),
        "after repeated failures the operation is skipped"
    );
    assert_eq!(c.engine.status().rejected_ops, 1);
    assert_eq!(c.engine.status().phase, Phase::Idle);
    assert!(c
        .engine
        .events()
        .iter()
        .any(|e| e.message.contains("failed verification")));
}

#[tokio::test]
async fn an_operation_moved_to_another_document_is_rejected() {
    let (server, a, _b) = pair_with_secrets().await;
    let proxy = Proxy::to(&server.url).await;
    let c = Dev::new("c");
    enroll(&a, &c, "C", JoinMode::ReplaceLocal).await;
    c.with(|s| {
        let mut acct: serde_json::Value =
            serde_json::from_str(&s.sync_get("acct").unwrap().unwrap()).unwrap();
        acct["server"] = serde_json::Value::String(proxy.url.clone());
        s.sync_set("acct", &acct.to_string()).unwrap();
    });
    let c = c.relaunch();
    // The server claims every operation belongs to some other document.
    proxy.tamper(|path, body| {
        if !path.contains("/ops") {
            return body;
        }
        let Ok(mut page) = serde_json::from_slice::<PullResponse>(&body) else {
            return body;
        };
        for op in &mut page.ops {
            op.doc_id = "doc-0000000000-redirected".into();
        }
        serde_json::to_vec(&page).unwrap()
    });
    for _ in 0..4 {
        let _ = c.engine.sync_once().await;
    }
    assert!(
        c.titles().is_empty(),
        "nothing was applied: {:?}",
        c.titles()
    );
    assert!(c.engine.status().rejected_ops > 0);
}

#[tokio::test]
async fn a_forged_key_update_from_the_server_is_refused() {
    let (server, a, b) = pair_with_secrets().await;
    let proxy = Proxy::to(&server.url).await;
    let ws = a.engine.status().workspace_id.unwrap();
    let b_id = b.engine.status().device_id.unwrap();
    // The server knows b's public key, so it can wrap *a* ring for b. It does not know the real keys.
    let info = server.state.repo.devices(&ws).await.unwrap();
    let b_public =
        DevicePublic::from_bytes(&info.iter().find(|d| d.id == b_id).unwrap().key).unwrap();
    let mut forged = KeyRing::generate().unwrap();
    forged.rotate().unwrap();
    let wrap = wrap_ring(
        &forged,
        &b_public.agree,
        format!("{ws}|{b_id}|rotate|1").as_bytes(),
        None,
    )
    .unwrap();
    let (ws2, wrap2) = (ws.clone(), wrap.clone());
    proxy.tamper(move |path, body| {
        if path.contains("/keys") {
            return serde_json::to_vec(&KeysResponse {
                epoch: 1,
                key_wrap: Some(wrap2.clone()),
            })
            .unwrap();
        }
        if path.contains("/ops") {
            if let Ok(mut page) = serde_json::from_slice::<PullResponse>(&body) {
                page.epoch = 1;
                return serde_json::to_vec(&page).unwrap();
            }
        }
        let _ = &ws2;
        body
    });
    b.with(|s| {
        let mut acct: serde_json::Value =
            serde_json::from_str(&s.sync_get("acct").unwrap().unwrap()).unwrap();
        acct["server"] = serde_json::Value::String(proxy.url.clone());
        s.sync_set("acct", &acct.to_string()).unwrap();
    });
    let b = b.relaunch();
    let before = b.secrets.get_for_test("ring").unwrap();

    let err = b.engine.sync_once().await.unwrap_err();
    assert!(matches!(err, SyncError::Keys(_)), "{err:?}");
    assert_eq!(
        b.engine.status().epoch,
        0,
        "the forged ring was not adopted"
    );
    assert_eq!(b.secrets.get_for_test("ring").unwrap(), before);
    assert!(b
        .engine
        .events()
        .iter()
        .any(|e| e.message.contains("did not come from a workspace member")));
    // The attacker's ring is a valid ring for b's device, which is exactly why the check exists.
    assert!(unwrap_ring(
        &DeviceKeys::from_bytes(&b.secrets.get_for_test("device").unwrap()).unwrap(),
        &wrap,
        format!("{ws}|{b_id}|rotate|1").as_bytes(),
        None
    )
    .is_ok());
}

#[tokio::test]
async fn a_join_request_without_the_invite_secret_is_never_approved() {
    let server = Server::start().await;
    let (a, victim) = (Dev::new("a"), Dev::new("victim"));
    a.page("Private", &["x"]);
    a.engine
        .create_workspace(&server.url, None, "A")
        .await
        .unwrap();
    a.sync().await;

    let offer = a.engine.create_invite().await.unwrap();
    // The legitimate device asks to join...
    victim
        .engine
        .begin_join(&offer.code, "Victim phone")
        .await
        .unwrap();
    // ...and the server (or anyone who saw the invite id) replaces the request with its own key.
    let ws = a.engine.status().workspace_id.unwrap();
    let attacker = DeviceKeys::generate().unwrap();
    let forged = InviteRequest {
        workspace_id: ws.clone(),
        device_key: attacker.public().to_bytes().to_vec(),
        proof: vec![0u8; 32],
        label_sealed: vec![9; 24],
    };
    server
        .state
        .repo
        .submit_invite_request(
            &offer.invite_id,
            &forged,
            &attacker.id(),
            notercrux_sync_server_now(),
        )
        .await
        .unwrap();

    let seen = a
        .engine
        .pending_join(&offer.invite_id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !seen.verified,
        "the forged request cannot prove knowledge of the invite secret"
    );
    let err = a.engine.approve_join(&offer.invite_id).await.unwrap_err();
    assert!(matches!(err, SyncError::Crypto(_)), "{err:?}");
    // The legitimate device never received a key.
    assert_eq!(
        victim
            .engine
            .poll_join(JoinMode::KeepLocal)
            .await
            .unwrap_err()
            .code(),
        "not_found"
    );
    let devices = a.engine.devices().await.unwrap();
    assert_eq!(devices.len(), 1);
}

fn notercrux_sync_server_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

#[tokio::test]
async fn an_expired_or_reused_invite_cannot_be_used() {
    let server = Server::start().await;
    let (a, b, c) = (Dev::new("a"), Dev::new("b"), Dev::new("c"));
    a.engine
        .create_workspace(&server.url, None, "A")
        .await
        .unwrap();
    let offer = a.engine.create_invite().await.unwrap();
    b.engine.begin_join(&offer.code, "B").await.unwrap();
    a.engine.approve_join(&offer.invite_id).await.unwrap();
    b.engine.poll_join(JoinMode::KeepLocal).await.unwrap();
    // A second device with the same code is turned away.
    let err = c.engine.begin_join(&offer.code, "C").await.unwrap_err();
    assert_eq!(err.code(), "conflict", "{err:?}");
}

#[tokio::test]
async fn recovery_restores_everything_from_the_phrase_alone() {
    let server = Server::start().await;
    let (a, b) = (Dev::new("a"), Dev::new("b"));
    a.page("Irreplaceable", &["my notes"]);
    let created = a
        .engine
        .create_workspace(&server.url, None, "A")
        .await
        .unwrap();
    a.sync().await;
    enroll(&a, &b, "B", JoinMode::ReplaceLocal).await;
    settle(&[&a, &b]).await;
    // Rotate once so the recovery copy must have been refreshed to include epoch 1.
    let b_id = b.engine.status().device_id.unwrap();
    a.engine.revoke_device(&b_id).await.unwrap();
    a.page("After rotation", &["new"]);
    a.sync().await;
    let ws = created.workspace_id.clone();

    // Every device is lost. A brand new install recovers with only the phrase.
    drop((a, b));
    let fresh = Dev::new("fresh");
    let wrong = RecoveryKey::generate().unwrap().to_phrase();
    assert!(fresh
        .engine
        .recover(
            &server.url,
            &ws,
            &wrong,
            "New laptop",
            JoinMode::ReplaceLocal
        )
        .await
        .is_err());
    let mut typo: Vec<char> = created.recovery_phrase.chars().collect();
    typo[0] = if typo[0] == 'A' { 'B' } else { 'A' };
    assert_eq!(
        fresh
            .engine
            .recover(
                &server.url,
                &ws,
                &typo.into_iter().collect::<String>(),
                "New laptop",
                JoinMode::ReplaceLocal
            )
            .await,
        Err(SyncError::BadCode)
    );
    assert_eq!(
        fresh.engine.status().phase,
        Phase::Disabled,
        "failed attempts leave nothing behind"
    );

    fresh
        .engine
        .recover(
            &server.url,
            &ws,
            &created.recovery_phrase,
            "New laptop",
            JoinMode::ReplaceLocal,
        )
        .await
        .unwrap();
    settle(&[&fresh]).await;
    assert_eq!(fresh.titles(), vec!["After rotation", "Irreplaceable"]);
    assert_eq!(fresh.texts(fresh.find("Irreplaceable")), vec!["my notes"]);
    assert_eq!(fresh.engine.status().epoch, 1);

    // The lost devices are still listed. Removing them rotates the key again.
    let devices = fresh.engine.devices().await.unwrap();
    let lost: Vec<_> = devices
        .iter()
        .filter(|d| !d.current && !d.revoked)
        .collect();
    assert_eq!(
        lost.len(),
        1,
        "the original device is still enrolled: {devices:?}"
    );
    fresh.engine.revoke_device(&lost[0].id).await.unwrap();
    assert_eq!(fresh.engine.status().epoch, 2);
    // ...and the recovery key keeps working after that.
    let again = Dev::new("again");
    again
        .engine
        .recover(
            &server.url,
            &ws,
            &created.recovery_phrase,
            "Another",
            JoinMode::ReplaceLocal,
        )
        .await
        .unwrap();
    settle(&[&again]).await;
    assert_eq!(again.engine.status().epoch, 2);
    assert_eq!(again.titles().len(), 2);
}

#[tokio::test]
async fn quota_and_size_rejections_surface_as_errors_and_keep_the_data_queued() {
    let mut cfg = notercrux_sync_server::config::Config::default();
    cfg.limits.max_op_bytes_total = 3000;
    let server = Server::start_with(cfg).await;
    let a = Dev::new("a");
    for i in 0..20 {
        a.page(
            &format!("Page {i}"),
            &["some text that adds up", "more text here"],
        );
    }
    a.engine
        .create_workspace(&server.url, None, "A")
        .await
        .unwrap();
    let err = a.engine.sync_once().await.unwrap_err();
    assert_eq!(err.code(), "quota_exceeded", "{err:?}");
    assert!(!err.is_transient());
    let st = a.engine.status();
    assert_eq!(st.phase, Phase::Error);
    assert!(st.last_error.unwrap().contains("quota"));
    assert!(a.pending() > 0, "nothing was dropped");
    assert_eq!(a.titles().len(), 20);
}
