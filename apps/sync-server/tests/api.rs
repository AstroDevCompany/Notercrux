#![allow(clippy::field_reassign_with_default)]
mod common;

use axum::http::{Method, StatusCode};
use common::*;
use notercrux_crypto::*;
use notercrux_protocol::*;
use notercrux_sync_server::config::{Config, Registration};

fn app() -> TestApp {
    TestApp::new(Config::default())
}

// ---- workspaces and authentication ------------------------------------------------------------

#[tokio::test]
async fn creating_a_workspace_returns_its_info_and_lists_the_creator() {
    let app = app();
    let w = make_workspace(&app, "info").await;
    let info: WorkspaceInfo = app
        .get(&w.owner, &format!("/v1/workspaces/{}", w.id))
        .await
        .json();
    assert_eq!(info.workspace_id, w.id);
    assert_eq!(info.epoch, 0);
    assert_eq!(info.devices.len(), 1);
    assert_eq!(info.devices[0].id, w.owner.id());
    assert_eq!(info.usage.devices, 1);
    assert_eq!(info.protocol, PROTOCOL_VERSION);
}

#[tokio::test]
async fn a_workspace_id_cannot_be_registered_twice() {
    let app = app();
    let w = make_workspace(&app, "dup").await;
    let other = DeviceKeys::generate().unwrap();
    let req = create_request(&w.id, &other, &w.ring, &w.recovery, None);
    assert_eq!(
        app.post(&other, "/v1/workspaces", &req).await.status,
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn creation_requires_proof_of_the_device_key() {
    let app = app();
    let (owner, thief) = (
        DeviceKeys::generate().unwrap(),
        DeviceKeys::generate().unwrap(),
    );
    let ring = KeyRing::generate().unwrap();
    let rk = RecoveryKey::generate().unwrap();
    let req = create_request(&ws_name("proof"), &owner, &ring, &rk, None);
    // Signed by a different key than the one being registered.
    assert_eq!(
        app.post(&thief, "/v1/workspaces", &req).await.status,
        StatusCode::UNAUTHORIZED
    );
    // Unsigned.
    let body = serde_json::to_vec(&req).unwrap();
    assert_eq!(
        app.send(None, Method::POST, "/v1/workspaces", body)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn registration_policy_is_enforced() {
    let mut cfg = Config::default();
    cfg.registration = Registration::Tokens(vec!["let-me-in".into()]);
    let app = TestApp::new(cfg);
    let (owner, ring, rk) = (
        DeviceKeys::generate().unwrap(),
        KeyRing::generate().unwrap(),
        RecoveryKey::generate().unwrap(),
    );
    let none = create_request(&ws_name("tok"), &owner, &ring, &rk, None);
    assert_eq!(
        app.post(&owner, "/v1/workspaces", &none).await.status,
        StatusCode::FORBIDDEN
    );
    let wrong = create_request(&ws_name("tok"), &owner, &ring, &rk, Some("nope"));
    assert_eq!(
        app.post(&owner, "/v1/workspaces", &wrong).await.status,
        StatusCode::FORBIDDEN
    );
    let good = create_request(&ws_name("tok"), &owner, &ring, &rk, Some("let-me-in"));
    assert_eq!(
        app.post(&owner, "/v1/workspaces", &good).await.status,
        StatusCode::CREATED
    );

    let mut cfg = Config::default();
    cfg.registration = Registration::Closed;
    let closed = TestApp::new(cfg);
    let req = create_request(&ws_name("closed"), &owner, &ring, &rk, None);
    assert_eq!(
        closed.post(&owner, "/v1/workspaces", &req).await.status,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn rejects_unsigned_badly_signed_and_foreign_requests() {
    let app = app();
    let w = make_workspace(&app, "auth").await;
    let path = format!("/v1/workspaces/{}", w.id);

    assert_eq!(
        app.send(None, Method::GET, &path, vec![]).await.status,
        StatusCode::UNAUTHORIZED
    );

    // Unknown device.
    let stranger = DeviceKeys::generate().unwrap();
    assert_eq!(
        app.get(&stranger, &path).await.status,
        StatusCode::UNAUTHORIZED
    );

    // A signature over a different path (cross-endpoint reuse).
    let nonce = app.fresh_nonce();
    let sig = sign_request(
        &w.owner,
        "GET",
        "/v1/workspaces/other",
        app.now(),
        &nonce,
        &[],
    );
    let mut req = axum::http::Request::builder().method("GET").uri(&path);
    req = req
        .header(headers::DEVICE, w.owner.id())
        .header(headers::TIME, app.now().to_string())
        .header(headers::NONCE, &nonce)
        .header(headers::SIGNATURE, encode_b64(&sig));
    let res = tower::ServiceExt::oneshot(
        app.router.clone(),
        req.body(axum::body::Body::empty()).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    // A device of one workspace cannot act in another.
    let other = make_workspace(&app, "other").await;
    assert_eq!(
        app.get(&w.owner, &format!("/v1/workspaces/{}", other.id))
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn tampered_bodies_fail_signature_verification() {
    let app = app();
    let w = make_workspace(&app, "tamper").await;
    let path = format!("/v1/workspaces/{}/ops", w.id);
    let body = serde_json::to_vec(&PushRequest {
        ops: vec![op("o1", &doc(1), b"hello")],
    })
    .unwrap();
    let nonce = app.fresh_nonce();
    let sig = sign_request(&w.owner, "POST", &path, app.now(), &nonce, &body);
    let mut forged = body.clone();
    let at = forged.iter().position(|b| *b == b'o').unwrap();
    forged[at] = b'x';
    let req = axum::http::Request::builder()
        .method("POST")
        .uri(&path)
        .header(headers::DEVICE, w.owner.id())
        .header(headers::TIME, app.now().to_string())
        .header(headers::NONCE, &nonce)
        .header(headers::SIGNATURE, encode_b64(&sig))
        .body(axum::body::Body::from(forged))
        .unwrap();
    let res = tower::ServiceExt::oneshot(app.router.clone(), req)
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn replayed_requests_are_rejected_but_fresh_nonces_pass() {
    let app = app();
    let w = make_workspace(&app, "replay").await;
    let path = format!("/v1/workspaces/{}", w.id);
    let first = app
        .send_with(
            Some(&w.owner),
            Method::GET,
            &path,
            vec![],
            app.now(),
            "fixed-nonce-1",
            None,
        )
        .await;
    assert_eq!(first.status, StatusCode::OK);
    let replay = app
        .send_with(
            Some(&w.owner),
            Method::GET,
            &path,
            vec![],
            app.now(),
            "fixed-nonce-1",
            None,
        )
        .await;
    assert_eq!(replay.status, StatusCode::UNAUTHORIZED);
    assert_eq!(replay.error_code(), code::REPLAY);
    assert_eq!(app.get(&w.owner, &path).await.status, StatusCode::OK);
}

#[tokio::test]
async fn requests_outside_the_clock_window_are_rejected() {
    let app = app();
    let w = make_workspace(&app, "skew").await;
    let path = format!("/v1/workspaces/{}", w.id);
    for skew in [-400, 400] {
        let r = app
            .send_with(
                Some(&w.owner),
                Method::GET,
                &path,
                vec![],
                app.now() + skew,
                &app.fresh_nonce(),
                None,
            )
            .await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED);
        assert_eq!(r.error_code(), code::CLOCK_SKEW);
    }
    let ok = app
        .send_with(
            Some(&w.owner),
            Method::GET,
            &path,
            vec![],
            app.now() - 200,
            &app.fresh_nonce(),
            None,
        )
        .await;
    assert_eq!(ok.status, StatusCode::OK);
}

#[tokio::test]
async fn protocol_versions_are_negotiated() {
    let app = app();
    let w = make_workspace(&app, "proto").await;
    let path = format!("/v1/workspaces/{}", w.id);
    let future = app
        .send_with(
            Some(&w.owner),
            Method::GET,
            &path,
            vec![],
            app.now(),
            &app.fresh_nonce(),
            Some(PROTOCOL_VERSION + 1),
        )
        .await;
    assert_eq!(future.status, StatusCode::UPGRADE_REQUIRED);
    assert_eq!(future.error_code(), code::UNSUPPORTED_PROTOCOL);
    let current = app
        .send_with(
            Some(&w.owner),
            Method::GET,
            &path,
            vec![],
            app.now(),
            &app.fresh_nonce(),
            Some(PROTOCOL_VERSION),
        )
        .await;
    assert_eq!(current.status, StatusCode::OK);
}

#[tokio::test]
async fn rate_limits_apply_per_device() {
    let mut cfg = Config::default();
    cfg.device_rate = 5;
    let app = TestApp::new(cfg);
    let w = make_workspace(&app, "rate").await;
    let path = format!("/v1/workspaces/{}", w.id);
    let mut limited = false;
    for _ in 0..10 {
        let r = app.get(&w.owner, &path).await;
        if r.status == StatusCode::TOO_MANY_REQUESTS {
            assert_eq!(r.error_code(), code::RATE_LIMITED);
            limited = true;
        }
    }
    assert!(limited);
    app.advance(61);
    assert_eq!(app.get(&w.owner, &path).await.status, StatusCode::OK);
}

// ---- operations -------------------------------------------------------------------------------

#[tokio::test]
async fn pushed_operations_are_ordered_and_pulled_after_a_cursor() {
    let app = app();
    let w = make_workspace(&app, "ops").await;
    let base = format!("/v1/workspaces/{}/ops", w.id);
    let push = PushRequest {
        ops: vec![op("a", &doc(1), b"one"), op("b", &doc(2), b"two")],
    };
    let res: PushResponse = app.post(&w.owner, &base, &push).await.json();
    assert_eq!(res.accepted, vec!["a", "b"]);
    assert_eq!(res.cursor, 2);

    let all: PullResponse = app.get(&w.owner, &format!("{base}?after=0")).await.json();
    assert_eq!(
        all.ops.iter().map(|o| o.seq).collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(all.ops[0].payload, b"one");
    assert_eq!(all.ops[0].device_id, w.owner.id());
    assert!(!all.more);

    let rest: PullResponse = app.get(&w.owner, &format!("{base}?after=1")).await.json();
    assert_eq!(rest.ops.len(), 1);
    assert_eq!(rest.cursor, 2);
    let none: PullResponse = app.get(&w.owner, &format!("{base}?after=2")).await.json();
    assert!(none.ops.is_empty());
    assert_eq!(none.cursor, 2);

    let paged: PullResponse = app
        .get(&w.owner, &format!("{base}?after=0&limit=1"))
        .await
        .json();
    assert_eq!(paged.ops.len(), 1);
    assert!(paged.more);
}

#[tokio::test]
async fn pushing_the_same_operation_twice_is_idempotent() {
    let app = app();
    let w = make_workspace(&app, "idem").await;
    let base = format!("/v1/workspaces/{}/ops", w.id);
    let push = PushRequest {
        ops: vec![op("a", &doc(1), b"one")],
    };
    let first: PushResponse = app.post(&w.owner, &base, &push).await.json();
    let again: PushResponse = app.post(&w.owner, &base, &push).await.json();
    assert_eq!((first.accepted.len(), again.accepted.len()), (1, 0));
    assert_eq!(again.duplicate, vec!["a"]);
    assert_eq!(again.cursor, first.cursor);
    let all: PullResponse = app.get(&w.owner, &format!("{base}?after=0")).await.json();
    assert_eq!(all.ops.len(), 1);
    // The same id twice inside one batch is also stored once.
    let batch = PushRequest {
        ops: vec![op("c", &doc(1), b"x"), op("c", &doc(1), b"x")],
    };
    let res: PushResponse = app.post(&w.owner, &base, &batch).await.json();
    assert_eq!((res.accepted.len(), res.duplicate.len()), (1, 1));
}

#[tokio::test]
async fn invalid_and_oversized_operations_are_rejected() {
    let mut cfg = Config::default();
    cfg.limits.max_op_size = 100;
    cfg.limits.max_op_bytes_total = 250;
    let app = TestApp::new(cfg);
    let w = make_workspace(&app, "bad").await;
    let base = format!("/v1/workspaces/{}/ops", w.id);

    let huge = app
        .post(
            &w.owner,
            &base,
            &PushRequest {
                ops: vec![op("a", &doc(1), &[1u8; 101])],
            },
        )
        .await;
    assert_eq!(huge.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(huge.error_code(), code::TOO_LARGE);

    for bad in [
        op("../x", &doc(1), b"p"),
        op("ok", "short", b"p"),
        op("ok", &doc(1), b""),
    ] {
        assert_eq!(
            app.post(&w.owner, &base, &PushRequest { ops: vec![bad] })
                .await
                .status,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        app.post(&w.owner, &base, &PushRequest { ops: vec![] })
            .await
            .status,
        StatusCode::BAD_REQUEST
    );

    let mut future_epoch = op("e", &doc(1), b"p");
    future_epoch.epoch = 5;
    assert_eq!(
        app.post(
            &w.owner,
            &base,
            &PushRequest {
                ops: vec![future_epoch]
            }
        )
        .await
        .status,
        StatusCode::CONFLICT
    );

    // Quota: 3 x 90 bytes exceeds 250, and the rejected batch stores nothing.
    let over = PushRequest {
        ops: (0..3)
            .map(|i| op(&format!("q{i}"), &doc(1), &[2u8; 90]))
            .collect(),
    };
    let res = app.post(&w.owner, &base, &over).await;
    assert_eq!(res.status, StatusCode::FORBIDDEN);
    assert_eq!(res.error_code(), code::QUOTA);
    let none: PullResponse = app.get(&w.owner, &format!("{base}?after=0")).await.json();
    assert!(none.ops.is_empty());
}

#[tokio::test]
async fn long_polling_wakes_up_when_operations_arrive() {
    let app = std::sync::Arc::new(app());
    let w = make_workspace(&app, "poll").await;
    let base = format!("/v1/workspaces/{}/ops", w.id);

    // Nothing pending: returns empty after the wait.
    let started = std::time::Instant::now();
    let empty: PullResponse = app
        .get(&w.owner, &format!("{base}?after=0&wait=1"))
        .await
        .json();
    assert!(empty.ops.is_empty());
    assert!(started.elapsed() >= std::time::Duration::from_millis(900));

    // A concurrent push wakes the waiting request well before its deadline.
    let waiter = {
        let app = app.clone();
        let owner = DeviceKeys::from_bytes(&*w.owner.to_bytes()).unwrap();
        let target = format!("{base}?after=0&wait=20");
        tokio::spawn(async move { app.get(&owner, &target).await.json::<PullResponse>() })
    };
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    let t = std::time::Instant::now();
    app.post(
        &w.owner,
        &base,
        &PushRequest {
            ops: vec![op("late", &doc(1), b"x")],
        },
    )
    .await;
    let woke = waiter.await.unwrap();
    assert_eq!(woke.ops.len(), 1);
    assert!(t.elapsed() < std::time::Duration::from_secs(5));
}

// ---- enrollment, revocation, recovery ---------------------------------------------------------

struct Enrolled {
    keys: DeviceKeys,
    ring: KeyRing,
}

/// Runs the whole enrollment protocol (what the clients do) against the server.
async fn enroll(app: &TestApp, w: &Workspace, owner_ring: &KeyRing, label: &str) -> Enrolled {
    let ws = &w.id;
    let created: CreateInviteResponse = app
        .post(&w.owner, &format!("/v1/workspaces/{ws}/invites"), &())
        .await
        .json();
    let invite_id = created.invite_id;
    let secret = InviteSecret::generate().unwrap();

    let keys = DeviceKeys::generate().unwrap();
    let public = keys.public();
    let label_sealed = seal_with_secret(
        secret.as_bytes(),
        format!("{ws}|{invite_id}").as_bytes(),
        label.as_bytes(),
    )
    .unwrap();
    let req = InviteRequest {
        workspace_id: ws.clone(),
        device_key: public.to_bytes().to_vec(),
        proof: invite_proof(&secret, &public).to_vec(),
        label_sealed,
    };
    let r = app
        .send(
            None,
            Method::POST,
            &format!("/v1/invites/{invite_id}/request"),
            serde_json::to_vec(&req).unwrap(),
        )
        .await;
    assert_eq!(r.status, StatusCode::ACCEPTED);

    // The new device polls and sees "not yet".
    let pending: InviteResult = app
        .send(
            None,
            Method::GET,
            &format!("/v1/invites/{invite_id}/result?device={}", keys.id()),
            vec![],
        )
        .await
        .json();
    assert!(!pending.approved);

    // The existing device reads the request, checks the proof, wraps the ring and approves.
    let state: InviteState = app
        .get(
            &w.owner,
            &format!("/v1/workspaces/{ws}/invites/{invite_id}"),
        )
        .await
        .json();
    assert_eq!(state.status, InviteStatus::Requested);
    let seen = state.request.unwrap();
    let seen_public = DevicePublic::from_bytes(&seen.device_key).unwrap();
    assert!(constant_time_eq(
        &invite_proof(&secret, &seen_public),
        &seen.proof
    ));
    let ctx = format!("{ws}|{}|{invite_id}", seen_public.id());
    let wrap = wrap_ring(
        owner_ring,
        &seen_public.agree,
        ctx.as_bytes(),
        Some(secret.as_bytes()),
    )
    .unwrap();
    let label_plain = open_with_secret(
        secret.as_bytes(),
        format!("{ws}|{invite_id}").as_bytes(),
        &seen.label_sealed,
    )
    .unwrap();
    let relabel = owner_ring
        .seal(
            Purpose::Metadata,
            |e| format!("{ws}|label|{e}").into_bytes(),
            &label_plain,
        )
        .unwrap()
        .1;
    let approve = ApproveInvite {
        device_id: seen_public.id(),
        key_wrap: wrap,
        label_sealed: relabel,
    };
    let ok = app
        .post(
            &w.owner,
            &format!("/v1/workspaces/{ws}/invites/{invite_id}/approve"),
            &approve,
        )
        .await;
    assert_eq!(
        ok.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&ok.body)
    );

    // The new device fetches its wrap and opens it with its private key and the invite secret.
    let result: InviteResult = app
        .send(
            None,
            Method::GET,
            &format!("/v1/invites/{invite_id}/result?device={}", keys.id()),
            vec![],
        )
        .await
        .json();
    assert!(result.approved);
    let ring = unwrap_ring(
        &keys,
        &result.key_wrap.unwrap(),
        ctx.as_bytes(),
        Some(secret.as_bytes()),
    )
    .unwrap();
    assert_eq!(ring.current_epoch(), owner_ring.current_epoch());
    Enrolled { keys, ring }
}

#[tokio::test]
async fn a_second_device_enrolls_and_shares_the_workspace_key() {
    let app = app();
    let w = make_workspace(&app, "enroll").await;
    let second = enroll(&app, &w, &w.ring, "Phone").await;
    let sealed = w
        .ring
        .seal(
            Purpose::Content,
            |e| content_aad(&w.id, &doc(1), e),
            b"secret text",
        )
        .unwrap();
    assert_eq!(
        second
            .ring
            .open(
                sealed.0,
                Purpose::Content,
                &content_aad(&w.id, &doc(1), sealed.0),
                &sealed.1
            )
            .unwrap(),
        b"secret text"
    );

    // The enrolled device can now authenticate and sees both devices (with sealed labels only).
    let info: WorkspaceInfo = app
        .get(&second.keys, &format!("/v1/workspaces/{}", w.id))
        .await
        .json();
    assert_eq!(info.devices.len(), 2);
    assert!(info
        .devices
        .iter()
        .all(|d| !d.label_sealed.windows(5).any(|x| x == b"Phone")));
}

#[tokio::test]
async fn invites_are_single_use_expire_and_reject_a_swapped_requester() {
    let app = app();
    let w = make_workspace(&app, "invite").await;
    let ws = &w.id;

    // Expiry.
    let inv: CreateInviteResponse = app
        .post(&w.owner, &format!("/v1/workspaces/{ws}/invites"), &())
        .await
        .json();
    app.advance(1000);
    let dev = DeviceKeys::generate().unwrap();
    let req = InviteRequest {
        workspace_id: ws.clone(),
        device_key: dev.public().to_bytes().to_vec(),
        proof: vec![0; 32],
        label_sealed: vec![1; 20],
    };
    let late = app
        .send(
            None,
            Method::POST,
            &format!("/v1/invites/{}/request", inv.invite_id),
            serde_json::to_vec(&req).unwrap(),
        )
        .await;
    assert_eq!(late.status, StatusCode::CONFLICT);
    let st: InviteState = app
        .get(
            &w.owner,
            &format!("/v1/workspaces/{ws}/invites/{}", inv.invite_id),
        )
        .await
        .json();
    assert_eq!(st.status, InviteStatus::Expired);

    // A request replaced after the approver looked is not approved with the old device's wrap.
    let inv: CreateInviteResponse = app
        .post(&w.owner, &format!("/v1/workspaces/{ws}/invites"), &())
        .await
        .json();
    let (a, b) = (
        DeviceKeys::generate().unwrap(),
        DeviceKeys::generate().unwrap(),
    );
    for k in [&a, &b] {
        let req = InviteRequest {
            workspace_id: ws.clone(),
            device_key: k.public().to_bytes().to_vec(),
            proof: vec![0; 32],
            label_sealed: vec![1; 20],
        };
        let r = app
            .send(
                None,
                Method::POST,
                &format!("/v1/invites/{}/request", inv.invite_id),
                serde_json::to_vec(&req).unwrap(),
            )
            .await;
        assert_eq!(r.status, StatusCode::ACCEPTED);
    }
    let approve = ApproveInvite {
        device_id: a.id(),
        key_wrap: vec![9; 100],
        label_sealed: vec![1; 20],
    };
    let res = app
        .post(
            &w.owner,
            &format!("/v1/workspaces/{ws}/invites/{}/approve", inv.invite_id),
            &approve,
        )
        .await;
    assert_eq!(res.status, StatusCode::CONFLICT);

    // Approve the current requester once; a second approval and a late request are refused.
    let approve = ApproveInvite {
        device_id: b.id(),
        key_wrap: vec![9; 100],
        label_sealed: vec![1; 20],
    };
    assert_eq!(
        app.post(
            &w.owner,
            &format!("/v1/workspaces/{ws}/invites/{}/approve", inv.invite_id),
            &approve
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        app.post(
            &w.owner,
            &format!("/v1/workspaces/{ws}/invites/{}/approve", inv.invite_id),
            &approve
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    let req = InviteRequest {
        workspace_id: ws.clone(),
        device_key: a.public().to_bytes().to_vec(),
        proof: vec![0; 32],
        label_sealed: vec![1; 20],
    };
    assert_eq!(
        app.send(
            None,
            Method::POST,
            &format!("/v1/invites/{}/request", inv.invite_id),
            serde_json::to_vec(&req).unwrap()
        )
        .await
        .status,
        StatusCode::CONFLICT
    );

    // Only the requesting device can read the result.
    let stranger = app
        .send(
            None,
            Method::GET,
            &format!("/v1/invites/{}/result?device={}", inv.invite_id, a.id()),
            vec![],
        )
        .await;
    assert_eq!(stranger.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_device_limit_caps_enrollment() {
    let mut cfg = Config::default();
    cfg.limits.max_devices = 2;
    let app = TestApp::new(cfg);
    let w = make_workspace(&app, "limit").await;
    enroll(&app, &w, &w.ring, "second").await;
    let ws = &w.id;
    let inv: CreateInviteResponse = app
        .post(&w.owner, &format!("/v1/workspaces/{ws}/invites"), &())
        .await
        .json();
    let k = DeviceKeys::generate().unwrap();
    let req = InviteRequest {
        workspace_id: ws.clone(),
        device_key: k.public().to_bytes().to_vec(),
        proof: vec![0; 32],
        label_sealed: vec![1; 20],
    };
    app.send(
        None,
        Method::POST,
        &format!("/v1/invites/{}/request", inv.invite_id),
        serde_json::to_vec(&req).unwrap(),
    )
    .await;
    let approve = ApproveInvite {
        device_id: k.id(),
        key_wrap: vec![9; 100],
        label_sealed: vec![1; 20],
    };
    let res = app
        .post(
            &w.owner,
            &format!("/v1/workspaces/{ws}/invites/{}/approve", inv.invite_id),
            &approve,
        )
        .await;
    assert_eq!(res.status, StatusCode::FORBIDDEN);
    assert_eq!(res.error_code(), code::QUOTA);
}

fn revoke_body(w: &Workspace, ring: &mut KeyRing, remaining: &[&DeviceKeys]) -> RevokeDevice {
    let epoch = ring.rotate().unwrap();
    let wraps = remaining
        .iter()
        .map(|k| KeyWrapEntry {
            device_id: k.id(),
            key_wrap: wrap_ring(
                ring,
                &k.public().agree,
                format!("{}|{}|rotate|{epoch}", w.id, k.id()).as_bytes(),
                None,
            )
            .unwrap(),
        })
        .collect();
    RevokeDevice {
        new_epoch: epoch,
        wraps,
        recovery_wrap: wrap_for_recovery(ring, &w.recovery.public(), &w.id).unwrap(),
    }
}

#[tokio::test]
async fn revoking_a_device_rotates_the_key_and_locks_it_out() {
    let app = app();
    let w = make_workspace(&app, "revoke").await;
    let second = enroll(&app, &w, &w.ring, "Second").await;
    let third = enroll(&app, &w, &w.ring, "Third").await;
    let ws = &w.id;

    let mut ring = KeyRing::from_bytes(&w.ring.to_bytes()).unwrap();
    // Wraps must cover every remaining device: leaving one out is refused.
    let mut partial = revoke_body(&w, &mut ring, &[&w.owner]);
    let res = app
        .post(
            &w.owner,
            &format!("/v1/workspaces/{ws}/devices/{}/revoke", second.keys.id()),
            &partial,
        )
        .await;
    assert_eq!(
        res.status,
        StatusCode::BAD_REQUEST,
        "{}",
        String::from_utf8_lossy(&res.body)
    );
    // Unknown extra recipients (e.g. the revoked device itself) are refused too.
    partial.wraps = revoke_body(
        &w,
        &mut KeyRing::from_bytes(&w.ring.to_bytes()).unwrap(),
        &[&w.owner, &second.keys, &third.keys],
    )
    .wraps;
    let res = app
        .post(
            &w.owner,
            &format!("/v1/workspaces/{ws}/devices/{}/revoke", second.keys.id()),
            &partial,
        )
        .await;
    assert_eq!(res.status, StatusCode::BAD_REQUEST);

    // A correct revocation succeeds; stale epochs do not.
    let mut ring = KeyRing::from_bytes(&w.ring.to_bytes()).unwrap();
    let body = revoke_body(&w, &mut ring, &[&w.owner, &third.keys]);
    let ok = app
        .post(
            &w.owner,
            &format!("/v1/workspaces/{ws}/devices/{}/revoke", second.keys.id()),
            &body,
        )
        .await;
    assert_eq!(
        ok.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&ok.body)
    );
    let again = app
        .post(
            &w.owner,
            &format!("/v1/workspaces/{ws}/devices/{}/revoke", third.keys.id()),
            &body,
        )
        .await;
    assert_eq!(again.status, StatusCode::CONFLICT);

    // The revoked device is locked out and is told why; others continue.
    let denied = app.get(&second.keys, &format!("/v1/workspaces/{ws}")).await;
    assert_eq!(denied.status, StatusCode::FORBIDDEN);
    assert_eq!(denied.error_code(), code::REVOKED);
    let push = app
        .post(
            &second.keys,
            &format!("/v1/workspaces/{ws}/ops"),
            &PushRequest {
                ops: vec![op("z", &doc(1), b"x")],
            },
        )
        .await;
    assert_eq!(push.status, StatusCode::FORBIDDEN);

    // Remaining devices learn about the rotation and get a wrap for the new epoch.
    let keys: KeysResponse = app
        .get(&third.keys, &format!("/v1/workspaces/{ws}/keys?have=0"))
        .await
        .json();
    assert_eq!(keys.epoch, 1);
    let rotated = unwrap_ring(
        &third.keys,
        &keys.key_wrap.unwrap(),
        format!("{ws}|{}|rotate|1", third.keys.id()).as_bytes(),
        None,
    )
    .unwrap();
    assert_eq!(rotated.current_epoch(), 1);
    // Nothing new when already current.
    let none: KeysResponse = app
        .get(&third.keys, &format!("/v1/workspaces/{ws}/keys?have=1"))
        .await
        .json();
    assert!(none.key_wrap.is_none());

    // Pull responses carry the epoch so idle devices notice rotation without polling /keys.
    let pull: PullResponse = app
        .get(&third.keys, &format!("/v1/workspaces/{ws}/ops?after=0"))
        .await
        .json();
    assert_eq!(pull.epoch, 1);

    // The last remaining device cannot be revoked.
    let solo = make_workspace(&app, "solo").await;
    let body = RevokeDevice {
        new_epoch: 1,
        wraps: vec![],
        recovery_wrap: vec![1],
    };
    assert_eq!(
        app.post(
            &solo.owner,
            &format!(
                "/v1/workspaces/{}/devices/{}/revoke",
                solo.id,
                solo.owner.id()
            ),
            &body
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn recovery_re_enrolls_a_device_with_only_the_recovery_key() {
    let app = app();
    let w = make_workspace(&app, "recover").await;
    let ws = &w.id;
    let fresh = DeviceKeys::generate().unwrap();
    let label = seal_with_secret(
        &w.recovery.label_secret(),
        format!("{ws}|recovery").as_bytes(),
        b"New laptop",
    )
    .unwrap();
    let sig = w.recovery.sign(&recovery_message(ws, &fresh.id()));
    let req = RecoverRequest {
        workspace_id: ws.clone(),
        device_key: fresh.public().to_bytes().to_vec(),
        recovery_signature: sig,
        label_sealed: label.clone(),
    };
    let res = app
        .send(
            None,
            Method::POST,
            "/v1/recover",
            serde_json::to_vec(&req).unwrap(),
        )
        .await;
    assert_eq!(
        res.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&res.body)
    );
    let out: RecoverResponse = res.json();
    // Only the recovery key opens the wrap; the server sent nothing a thief could use alone.
    let ring = w.recovery.unwrap_ring(&out.recovery_wrap, ws).unwrap();
    assert_eq!(
        ring.doc_id("page:1").unwrap(),
        w.ring.doc_id("page:1").unwrap()
    );
    assert_eq!(
        app.get(&fresh, &format!("/v1/workspaces/{ws}"))
            .await
            .status,
        StatusCode::OK
    );

    // A wrong recovery key, or a signature for another device, is refused.
    let imposter = RecoveryKey::generate().unwrap();
    let bad = RecoverRequest {
        recovery_signature: imposter.sign(&recovery_message(ws, &fresh.id())),
        ..req.clone()
    };
    assert_eq!(
        app.send(
            None,
            Method::POST,
            "/v1/recover",
            serde_json::to_vec(&bad).unwrap()
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    let other = DeviceKeys::generate().unwrap();
    let swapped = RecoverRequest {
        device_key: other.public().to_bytes().to_vec(),
        ..req
    };
    assert_eq!(
        app.send(
            None,
            Method::POST,
            "/v1/recover",
            serde_json::to_vec(&swapped).unwrap()
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn recovery_then_revocation_removes_lost_devices() {
    let app = app();
    let w = make_workspace(&app, "lost").await;
    let ws = &w.id;
    let fresh = DeviceKeys::generate().unwrap();
    let sig = w.recovery.sign(&recovery_message(ws, &fresh.id()));
    let req = RecoverRequest {
        workspace_id: ws.clone(),
        device_key: fresh.public().to_bytes().to_vec(),
        recovery_signature: sig,
        label_sealed: vec![1; 20],
    };
    app.send(
        None,
        Method::POST,
        "/v1/recover",
        serde_json::to_vec(&req).unwrap(),
    )
    .await;
    // The recovered device revokes the lost original and rotates the key.
    let mut ring = KeyRing::from_bytes(&w.ring.to_bytes()).unwrap();
    let body = revoke_body(&w, &mut ring, &[&fresh]);
    let res = app
        .post(
            &fresh,
            &format!("/v1/workspaces/{ws}/devices/{}/revoke", w.owner.id()),
            &body,
        )
        .await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(
        app.get(&w.owner, &format!("/v1/workspaces/{ws}"))
            .await
            .status,
        StatusCode::FORBIDDEN
    );
}

// ---- attachments ------------------------------------------------------------------------------

#[tokio::test]
async fn blobs_upload_in_chunks_resume_and_commit() {
    let app = app();
    let w = make_workspace(&app, "blob").await;
    let base = format!("/v1/workspaces/{}/blobs/{}", w.id, "b".repeat(32));
    let chunks: Vec<Vec<u8>> = (0..3).map(|i| vec![i as u8 + 1; 1000]).collect();

    assert_eq!(app.get(&w.owner, &base).await.status, StatusCode::NOT_FOUND);
    // The upload is interrupted after chunks 0 and 2.
    for i in [0usize, 2] {
        let r = app
            .send(
                Some(&w.owner),
                Method::PUT,
                &format!("{base}/chunks/{i}"),
                chunks[i].clone(),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK);
    }
    let status: BlobStatus = app.get(&w.owner, &base).await.json();
    assert!(!status.complete);
    assert_eq!(status.present, vec![0, 2]);

    // Committing early fails; resuming sends only what is missing.
    let commit = CommitBlob {
        total: 3,
        epoch: 0,
        size: 3000,
    };
    assert_eq!(
        app.post(&w.owner, &format!("{base}/commit"), &commit)
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        app.send(
            Some(&w.owner),
            Method::PUT,
            &format!("{base}/chunks/1"),
            chunks[1].clone()
        )
        .await
        .status,
        StatusCode::OK
    );
    // A wrong total size is refused, the right one accepted (and committing again is harmless).
    assert_eq!(
        app.post(
            &w.owner,
            &format!("{base}/commit"),
            &CommitBlob {
                total: 3,
                epoch: 0,
                size: 2999
            }
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        app.post(&w.owner, &format!("{base}/commit"), &commit)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        app.post(&w.owner, &format!("{base}/commit"), &commit)
            .await
            .status,
        StatusCode::NO_CONTENT
    );

    let status: BlobStatus = app.get(&w.owner, &base).await.json();
    assert!(status.complete);
    assert_eq!((status.total, status.size), (3, 3000));
    for (i, c) in chunks.iter().enumerate() {
        let r = app.get(&w.owner, &format!("{base}/chunks/{i}")).await;
        assert_eq!(&r.body, c);
    }
    // A complete blob accepts retried uploads without changing anything.
    let again = app
        .send(
            Some(&w.owner),
            Method::PUT,
            &format!("{base}/chunks/0"),
            chunks[0].clone(),
        )
        .await;
    assert_eq!(again.status, StatusCode::OK);
    let info: WorkspaceInfo = app
        .get(&w.owner, &format!("/v1/workspaces/{}", w.id))
        .await
        .json();
    assert_eq!(info.usage.blob_bytes, 3000);
}

#[tokio::test]
async fn blob_quotas_and_ids_are_enforced() {
    let mut cfg = Config::default();
    cfg.limits.max_blob_size = 1500;
    cfg.limits.max_blob_bytes_total = 2000;
    let app = TestApp::new(cfg);
    let w = make_workspace(&app, "bq").await;
    let blob = |c: char| format!("/v1/workspaces/{}/blobs/{}", w.id, c.to_string().repeat(32));

    assert_eq!(
        app.send(
            Some(&w.owner),
            Method::PUT,
            &format!("{}/chunks/0", blob('a')),
            vec![1; 1000]
        )
        .await
        .status,
        StatusCode::OK
    );
    // Over the per-blob limit.
    let r = app
        .send(
            Some(&w.owner),
            Method::PUT,
            &format!("{}/chunks/1", blob('a')),
            vec![1; 600],
        )
        .await;
    assert_eq!(
        (r.status, r.error_code().as_str()),
        (StatusCode::FORBIDDEN, code::QUOTA)
    );
    // Over the workspace total: 1000 stored + 1200 > 2000.
    let r = app
        .send(
            Some(&w.owner),
            Method::PUT,
            &format!("{}/chunks/0", blob('b')),
            vec![1; 1200],
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    // Re-sending a chunk replaces it rather than double counting.
    assert_eq!(
        app.send(
            Some(&w.owner),
            Method::PUT,
            &format!("{}/chunks/0", blob('a')),
            vec![1; 1000]
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        app.send(
            Some(&w.owner),
            Method::PUT,
            &format!("{}/chunks/0", blob('c')),
            vec![1; 900]
        )
        .await
        .status,
        StatusCode::OK
    );

    // Path traversal and malformed ids never reach the store.
    let r = app
        .send(
            Some(&w.owner),
            Method::PUT,
            &format!("/v1/workspaces/{}/blobs/..%2F..%2Fx/chunks/0", w.id),
            vec![1],
        )
        .await;
    assert!(r.status == StatusCode::BAD_REQUEST || r.status == StatusCode::NOT_FOUND);
    let r = app
        .send(
            Some(&w.owner),
            Method::PUT,
            &format!("/v1/workspaces/{}/blobs/short/chunks/0", w.id),
            vec![1],
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = app
        .send(
            Some(&w.owner),
            Method::PUT,
            &format!("{}/chunks/0", blob('d')),
            vec![],
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

// ---- operations endpoints and lifecycle -------------------------------------------------------

#[tokio::test]
async fn metrics_count_activity_without_exposing_content() {
    let app = app();
    let w = make_workspace(&app, "metrics").await;
    app.post(
        &w.owner,
        &format!("/v1/workspaces/{}/ops", w.id),
        &PushRequest {
            ops: vec![op("m1", &doc(1), b"top secret")],
        },
    )
    .await;
    let text =
        String::from_utf8(app.send(None, Method::GET, "/metrics", vec![]).await.body).unwrap();
    assert!(text.contains("notercrux_ops_pushed_total 1"), "{text}");
    assert!(text.contains("notercrux_workspaces_created_total 1"));
    assert!(!text.contains(&w.id) && !text.contains("secret"));
    assert_eq!(
        app.send(None, Method::GET, "/healthz", vec![]).await.status,
        StatusCode::OK
    );

    let mut cfg = Config::default();
    cfg.metrics_token = Some("tok".into());
    let guarded = TestApp::new(cfg);
    assert_eq!(
        guarded
            .send(None, Method::GET, "/metrics", vec![])
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn deleting_a_workspace_removes_everything() {
    let app = app();
    let w = make_workspace(&app, "delete").await;
    let path = format!("/v1/workspaces/{}", w.id);
    app.post(
        &w.owner,
        &format!("{path}/ops"),
        &PushRequest {
            ops: vec![op("d1", &doc(1), b"x")],
        },
    )
    .await;
    let r = app
        .send(Some(&w.owner), Method::DELETE, &path, vec![])
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(
        app.get(&w.owner, &path).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert!(app.state.repo.workspace(&w.id).await.unwrap().is_none());
    // The id can be registered again by a different owner.
    let again = DeviceKeys::generate().unwrap();
    let req = create_request(&w.id, &again, &w.ring, &w.recovery, None);
    assert_eq!(
        app.post(&again, "/v1/workspaces", &req).await.status,
        StatusCode::CREATED
    );
}

#[tokio::test]
async fn oversized_bodies_and_unknown_routes_fail_cleanly() {
    let mut cfg = Config::default();
    cfg.max_body = 2048;
    let app = TestApp::new(cfg);
    let w = make_workspace(&app, "body").await;
    let r = app
        .send(
            Some(&w.owner),
            Method::PUT,
            &format!("/v1/workspaces/{}/blobs/{}/chunks/0", w.id, "a".repeat(32)),
            vec![0; 5000],
        )
        .await;
    assert_eq!(r.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(
        app.send(None, Method::GET, "/v1/nothing", vec![])
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let r = app
        .send(
            Some(&w.owner),
            Method::POST,
            &format!("/v1/workspaces/{}/ops", w.id),
            b"{not json".to_vec(),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_pushes_get_unique_contiguous_sequence_numbers() {
    let app = std::sync::Arc::new(app());
    let w = make_workspace(&app, "conc").await;
    let base = format!("/v1/workspaces/{}/ops", w.id);
    let owner_bytes = w.owner.to_bytes();
    let mut tasks = Vec::new();
    for t in 0..8 {
        let app = app.clone();
        let base = base.clone();
        let owner = DeviceKeys::from_bytes(&*owner_bytes).unwrap();
        tasks.push(tokio::spawn(async move {
            for batch in 0..5 {
                let ops = (0..4)
                    .map(|i| op(&format!("t{t}-b{batch}-{i}"), &doc(t), b"payload"))
                    .collect();
                let r = app.post(&owner, &base, &PushRequest { ops }).await;
                assert_eq!(
                    r.status,
                    StatusCode::OK,
                    "{}",
                    String::from_utf8_lossy(&r.body)
                );
            }
        }));
    }
    for t in tasks {
        t.await.unwrap();
    }
    let pulled: PullResponse = app
        .get(&w.owner, &format!("{base}?after=0&limit=1000"))
        .await
        .json();
    let seqs: Vec<i64> = pulled.ops.iter().map(|o| o.seq).collect();
    assert_eq!(seqs, (1..=160).collect::<Vec<_>>());
    let ids: std::collections::HashSet<_> = pulled.ops.iter().map(|o| o.op_id.clone()).collect();
    assert_eq!(ids.len(), 160);
}
