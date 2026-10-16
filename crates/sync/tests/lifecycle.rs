mod common;

use common::*;
use notercrux_sync::{JoinMode, Phase, SecretStore, SyncError};

#[tokio::test]
async fn creating_a_workspace_uploads_existing_pages_and_a_second_device_receives_them() {
    let server = Server::start().await;
    let (a, b) = (Dev::new("a"), Dev::new("b"));
    a.page("Plans", &["first", "second"]);
    a.page("Notes", &["hello"]);

    let created = a
        .engine
        .create_workspace(&server.url, None, "Laptop")
        .await
        .unwrap();
    assert!(created.workspace_id.starts_with('w'));
    assert_eq!(
        created.recovery_phrase.matches('-').count(),
        13,
        "a grouped, typo-checked phrase"
    );
    assert!(a.pending() > 0, "existing pages are queued for upload");
    let r = a.sync().await;
    assert!(r.sent > 0);
    assert_eq!(a.pending(), 0);

    enroll(&a, &b, "Phone", JoinMode::KeepLocal).await;
    let r = b.sync().await;
    assert!(r.received > 0);
    assert_eq!(b.titles(), vec!["Notes", "Plans"]);
    assert_eq!(b.texts(b.find("Plans")), vec!["first", "second"]);
    same_pages(&a, &b);

    // Edits flow both ways.
    let notes = b.find("Notes");
    b.with(|s| {
        s.save_document(notes, &doc(&["hello", "from phone"]))
            .unwrap()
    });
    let id = a.find("Plans");
    a.with(|s| {
        s.save_document(id, &doc(&["first", "second", "third"]))
            .unwrap()
    });
    settle(&[&a, &b]).await;
    assert_eq!(a.texts(a.find("Notes")), vec!["hello", "from phone"]);
    assert_eq!(b.texts(b.find("Plans")), vec!["first", "second", "third"]);
    same_pages(&a, &b);

    let st = a.engine.status();
    assert_eq!(st.phase, Phase::Idle);
    assert!(st.last_sync_at.is_some() && st.ops_sent > 0 && st.ops_received > 0);
}

#[tokio::test]
async fn joining_can_keep_or_replace_the_local_pages() {
    let server = Server::start().await;
    let (a, keep, replace) = (Dev::new("a"), Dev::new("keep"), Dev::new("replace"));
    a.page("Shared", &["from a"]);
    a.engine
        .create_workspace(&server.url, None, "A")
        .await
        .unwrap();
    a.sync().await;

    keep.page("Local only", &["kept"]);
    replace.page("Throwaway", &["gone"]);
    enroll(&a, &keep, "Keep", JoinMode::KeepLocal).await;
    enroll(&a, &replace, "Replace", JoinMode::ReplaceLocal).await;
    settle(&[&a, &keep, &replace]).await;

    assert_eq!(keep.titles(), vec!["Local only", "Shared"]);
    assert_eq!(
        a.titles(),
        vec!["Local only", "Shared"],
        "the merged page reaches the other device"
    );
    assert_eq!(
        replace.titles(),
        vec!["Local only", "Shared"],
        "...and the replacing device, but its own page is gone"
    );
    same_pages(&a, &keep);
    same_pages(&a, &replace);
}

#[tokio::test]
async fn state_survives_relaunch_and_status_is_restored() {
    let server = Server::start().await;
    let a = Dev::new("a");
    a.page("Persist", &["x"]);
    a.engine
        .create_workspace(&server.url, None, "A")
        .await
        .unwrap();
    a.sync().await;
    let before = a.engine.status();

    let again = a.relaunch();
    let st = again.engine.status();
    assert_eq!(st.phase, Phase::Idle);
    assert_eq!(st.workspace_id, before.workspace_id);
    assert_eq!(st.device_id, before.device_id);
    again.sync().await;

    // Losing the keys (secure storage wiped) is reported, not silently ignored.
    let wiped = a.relaunch();
    a.secrets.delete("ring").unwrap();
    let broken = wiped.relaunch();
    assert_eq!(broken.engine.status().phase, Phase::KeysMissing);
    assert!(matches!(
        broken.engine.sync_once().await,
        Err(SyncError::NotLinked)
    ));
}

#[tokio::test]
async fn an_unlinked_workspace_reports_disabled_and_refuses_to_sync() {
    let a = Dev::new("a");
    assert_eq!(a.engine.status().phase, Phase::Disabled);
    assert!(matches!(
        a.engine.sync_once().await,
        Err(SyncError::NotLinked)
    ));
    assert!(matches!(
        a.engine.create_invite().await,
        Err(SyncError::NotLinked)
    ));
    assert!(matches!(
        a.engine.begin_join("garbage", "x").await,
        Err(SyncError::BadCode)
    ));
}

#[tokio::test]
async fn a_linked_workspace_cannot_be_linked_twice() {
    let server = Server::start().await;
    let a = Dev::new("a");
    a.engine
        .create_workspace(&server.url, None, "A")
        .await
        .unwrap();
    assert!(matches!(
        a.engine.create_workspace(&server.url, None, "A").await,
        Err(SyncError::AlreadyLinked)
    ));
    assert!(matches!(
        a.engine.begin_join("NCX1-abc", "A").await,
        Err(SyncError::AlreadyLinked)
    ));
}

#[tokio::test]
async fn leaving_keeps_pages_and_removes_the_device() {
    let server = Server::start().await;
    let (a, b) = (Dev::new("a"), Dev::new("b"));
    a.page("Keep me", &["body"]);
    a.engine
        .create_workspace(&server.url, None, "A")
        .await
        .unwrap();
    a.sync().await;
    enroll(&a, &b, "B", JoinMode::ReplaceLocal).await;
    b.sync().await;

    b.engine.leave().await.unwrap();
    assert_eq!(b.engine.status().phase, Phase::Disabled);
    assert_eq!(
        b.titles(),
        vec!["Keep me"],
        "pages stay on the device that left"
    );
    assert!(b.secrets.get_for_test("ring").is_none() && b.secrets.get_for_test("device").is_none());

    // The remaining device sees it gone and the key rotated.
    let devices = a.engine.devices().await.unwrap();
    assert_eq!(devices.iter().filter(|d| !d.revoked).count(), 1);
    assert_eq!(a.engine.server_info().await.unwrap().epoch, 1);
    a.sync().await;
}
