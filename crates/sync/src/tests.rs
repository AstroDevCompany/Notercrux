//! The scenarios from the blueprint's "Required sync test scenarios" that apply at this layer.

use super::*;

fn setup() -> (Replica, Replica, Relay) {
    let key = WorkspaceKey::generate().unwrap();
    let a = Replica::new("laptop", 1, key.clone(), "page-1");
    let b = Replica::new("phone", 2, key, "page-1");
    (a, b, Relay::new())
}

fn converge(a: &mut Replica, b: &mut Replica, r: &mut Relay) {
    for _ in 0..2 {
        a.sync(r).unwrap();
        b.sync(r).unwrap();
    }
}

#[test]
fn different_parts_of_the_same_page_offline_both_survive() {
    let (mut a, mut b, mut r) = setup();
    let h = a.page.add_block(None, 0, "heading", "Plan").unwrap();
    let p = a.page.add_block(None, 1, "paragraph", "intro").unwrap();
    converge(&mut a, &mut b, &mut r);
    assert_eq!(b.page.texts(), ["Plan", "intro"]);

    // Offline on both sides.
    a.page.insert_text(h, 4, " 2026").unwrap();
    let b_p = b
        .page
        .doc
        .get_tree("blocks")
        .children(TreeParentId::Root)
        .unwrap()[1];
    b.page.insert_text(b_p, 5, " text").unwrap();
    b.page.add_block(None, 2, "todo", "new from phone").unwrap();
    let _ = p;
    converge(&mut a, &mut b, &mut r);

    assert_eq!(
        a.page.texts(),
        ["Plan 2026", "intro text", "new from phone"]
    );
    assert_eq!(a.page.texts(), b.page.texts());
}

#[test]
fn simultaneous_edits_to_the_same_text_merge_without_loss() {
    let (mut a, mut b, mut r) = setup();
    let blk = a
        .page
        .add_block(None, 0, "paragraph", "hello world")
        .unwrap();
    converge(&mut a, &mut b, &mut r);

    a.page.insert_text(blk, 0, "Oh, ").unwrap();
    a.page.insert_text(blk, 15, "!").unwrap();
    b.page.insert_text(blk, 5, ",").unwrap();
    b.page.insert_text(blk, 12, " (waves)").unwrap();
    converge(&mut a, &mut b, &mut r);

    let merged = a.page.texts()[0].clone();
    assert_eq!(merged, b.page.texts()[0]);
    for fragment in ["Oh, ", "!", ",", " (waves)", "hello", "world"] {
        assert!(merged.contains(fragment), "lost {fragment:?} in {merged:?}");
    }
}

#[test]
fn delete_versus_edit_keeps_the_edited_text_recoverable() {
    let (mut a, mut b, mut r) = setup();
    let blk = a.page.add_block(None, 0, "paragraph", "draft").unwrap();
    a.page.add_block(None, 1, "paragraph", "keep").unwrap();
    converge(&mut a, &mut b, &mut r);

    a.page.delete_block(blk).unwrap();
    b.page.insert_text(blk, 5, " + important edit").unwrap();
    converge(&mut a, &mut b, &mut r);

    // Both replicas agree the block is gone from the visible page...
    assert_eq!(a.page.texts(), ["keep"]);
    assert_eq!(b.page.texts(), ["keep"]);
    assert!(a.page.is_deleted(blk));
    // ...but nothing was silently destroyed: the concurrent edit is retained in the tombstone,
    // so the app can surface it ("restore block") instead of dropping it.
    assert_eq!(a.page.block_text(blk).unwrap(), "draft + important edit");
}

#[test]
fn moving_a_block_while_another_device_edits_it() {
    let (mut a, mut b, mut r) = setup();
    let first = a.page.add_block(None, 0, "paragraph", "first").unwrap();
    let parent = a.page.add_block(None, 1, "paragraph", "parent").unwrap();
    converge(&mut a, &mut b, &mut r);

    a.page.move_block(first, Some(parent), 0).unwrap();
    b.page.insert_text(first, 5, " edited").unwrap();
    converge(&mut a, &mut b, &mut r);

    let expect = vec![(0, "parent".to_string()), (1, "first edited".to_string())];
    assert_eq!(a.page.outline(), expect);
    assert_eq!(b.page.outline(), expect);
}

#[test]
fn concurrent_moves_that_would_form_a_cycle_resolve_identically() {
    let (mut a, mut b, mut r) = setup();
    let x = a.page.add_block(None, 0, "paragraph", "x").unwrap();
    let y = a.page.add_block(None, 1, "paragraph", "y").unwrap();
    converge(&mut a, &mut b, &mut r);

    a.page.move_block(x, Some(y), 0).unwrap(); // x under y
    b.page.move_block(y, Some(x), 0).unwrap(); // y under x
    converge(&mut a, &mut b, &mut r);

    assert_eq!(a.page.outline(), b.page.outline());
    assert_eq!(a.page.outline().len(), 2, "both blocks must remain visible");
}

#[test]
fn long_offline_period_then_catch_up() {
    let (mut a, mut b, mut r) = setup();
    let blk = a.page.add_block(None, 0, "paragraph", "").unwrap();
    converge(&mut a, &mut b, &mut r);
    for i in 0..200 {
        a.page.insert_text(blk, i, "a").unwrap();
        a.sync(&mut r).unwrap();
    }
    b.page
        .add_block(None, 1, "paragraph", "offline note")
        .unwrap();
    converge(&mut a, &mut b, &mut r);
    assert_eq!(b.page.texts()[0].len(), 200);
    assert_eq!(a.page.texts(), b.page.texts());
}

#[test]
fn relay_outage_mid_sync_is_retried_without_loss_or_duplication() {
    let (mut a, mut b, mut r) = setup();
    a.page.add_block(None, 0, "paragraph", "important").unwrap();

    r.fail_next = 2;
    assert!(matches!(a.sync(&mut r), Err(SyncError::Unavailable)));
    assert!(matches!(a.sync(&mut r), Err(SyncError::Unavailable)));
    a.sync(&mut r).unwrap();
    // Retrying a successful sync with nothing new is a no-op.
    a.sync(&mut r).unwrap();
    b.sync(&mut r).unwrap();
    assert_eq!(b.page.texts(), ["important"]);
}

#[test]
fn app_closing_during_sync_replays_safely() {
    let (mut a, mut b, mut r) = setup();
    let blk = a.page.add_block(None, 0, "paragraph", "one").unwrap();
    a.sync(&mut r).unwrap();
    a.page.insert_text(blk, 3, " two").unwrap();

    // The push reaches the relay, but the app dies before the cursor is saved.
    let before_crash = a.restart(10).unwrap();
    a.sync(&mut r).unwrap();
    let mut a2 = before_crash; // restarts from the pre-sync bookkeeping
    a2.sync(&mut r).unwrap(); // re-pushes the same blob: relay de-duplicates, CRDT import is idempotent
    b.sync(&mut r).unwrap();
    a2.sync(&mut r).unwrap();

    assert_eq!(b.page.texts(), ["one two"]);
    assert_eq!(a2.page.texts(), ["one two"]);
}

#[test]
fn revoked_device_cannot_push_or_pull_but_keeps_its_local_data() {
    let (mut a, mut b, mut r) = setup();
    a.page.add_block(None, 0, "paragraph", "shared").unwrap();
    converge(&mut a, &mut b, &mut r);

    r.revoke("phone");
    b.page
        .add_block(None, 1, "paragraph", "unsynced work")
        .unwrap();
    assert!(matches!(b.sync(&mut r), Err(SyncError::Revoked)));
    // The revoked device's unsynced work is still intact locally (exportable), and never reached
    // the other devices.
    assert_eq!(b.page.texts(), ["shared", "unsynced work"]);
    a.sync(&mut r).unwrap();
    assert_eq!(a.page.texts(), ["shared"]);
}

#[test]
fn relay_only_ever_stores_ciphertext_and_wrong_key_cannot_read_it() {
    let (mut a, _, mut r) = setup();
    a.page
        .add_block(None, 0, "paragraph", "TOP-SECRET-PLAN")
        .unwrap();
    a.sync(&mut r).unwrap();
    let stored = r.stored_bytes();
    assert!(!stored.windows(15).any(|w| w == b"TOP-SECRET-PLAN"));

    let mut intruder = Replica::new("intruder", 9, WorkspaceKey::generate().unwrap(), "page-1");
    assert!(matches!(intruder.sync(&mut r), Err(SyncError::Crypto(_))));
}

#[test]
fn ciphertext_for_one_page_cannot_be_replayed_into_another() {
    let key = WorkspaceKey::generate().unwrap();
    let mut a = Replica::new("laptop", 1, key.clone(), "page-1");
    let mut other_page = Replica::new("laptop", 3, key, "page-2");
    let mut r = Relay::new();
    a.page.add_block(None, 0, "paragraph", "x").unwrap();
    a.sync(&mut r).unwrap();
    assert!(matches!(other_page.sync(&mut r), Err(SyncError::Crypto(_))));
}
