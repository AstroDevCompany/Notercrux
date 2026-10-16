//! Runs only when NOTERCRUX_TEST_DATABASE_URL points at a PostgreSQL database.

mod common;

use axum::http::StatusCode;
use common::*;
use notercrux_protocol::*;
use notercrux_sync_server::blobstore::MemBlobStore;
use notercrux_sync_server::config::Config;
use notercrux_sync_server::pg::PgRepo;
use notercrux_sync_server::repo::Repo;
use notercrux_sync_server::AppState;
use std::sync::Arc;

fn url() -> Option<String> {
    let u = std::env::var("NOTERCRUX_TEST_DATABASE_URL")
        .ok()
        .filter(|u| !u.is_empty());
    if u.is_none() {
        eprintln!("skipped: set NOTERCRUX_TEST_DATABASE_URL to run the PostgreSQL tests");
    }
    u
}

fn app_on(url: &str) -> TestApp {
    let repo = PgRepo::connect(url).unwrap();
    TestApp::with_state(AppState::new(
        Config::default(),
        Arc::new(repo),
        Arc::new(MemBlobStore::new()),
    ))
}

#[tokio::test]
async fn data_survives_a_server_restart_and_migrations_are_idempotent() {
    let Some(url) = url() else { return };
    let first = app_on(&url);
    let w = make_workspace(&first, "restart").await;
    let base = format!("/v1/workspaces/{}/ops", w.id);
    first
        .post(
            &w.owner,
            &base,
            &PushRequest {
                ops: vec![op("persist", &doc(1), b"kept")],
            },
        )
        .await;
    drop(first);

    // A new process: new pool, migrations run again, state is read back from the database.
    let second = app_on(&url);
    second.state.repo.workspace(&w.id).await.unwrap();
    let pulled: PullResponse = second
        .get(&w.owner, &format!("{base}?after=0"))
        .await
        .json();
    assert_eq!(pulled.ops.len(), 1);
    assert_eq!(pulled.ops[0].payload, b"kept");
}

#[tokio::test]
async fn nonces_are_shared_between_server_instances() {
    let Some(url) = url() else { return };
    let (a, b) = (app_on(&url), app_on(&url));
    let w = make_workspace(&a, "nonce").await;
    let path = format!("/v1/workspaces/{}", w.id);
    let ok = a
        .send_with(
            Some(&w.owner),
            axum::http::Method::GET,
            &path,
            vec![],
            a.now(),
            "shared-nonce-1",
            None,
        )
        .await;
    assert_eq!(ok.status, StatusCode::OK);
    let replay = b
        .send_with(
            Some(&w.owner),
            axum::http::Method::GET,
            &path,
            vec![],
            b.now(),
            "shared-nonce-1",
            None,
        )
        .await;
    assert_eq!(
        replay.status,
        StatusCode::UNAUTHORIZED,
        "a replay must be caught across instances"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_instances_pushing_concurrently_keep_one_total_order() {
    let Some(url) = url() else { return };
    let apps = Arc::new([app_on(&url), app_on(&url)]);
    let w = make_workspace(&apps[0], "multi").await;
    let base = format!("/v1/workspaces/{}/ops", w.id);
    let bytes = w.owner.to_bytes();
    let mut tasks = Vec::new();
    for t in 0..6 {
        let apps = apps.clone();
        let base = base.clone();
        let owner = notercrux_crypto::DeviceKeys::from_bytes(&*bytes).unwrap();
        tasks.push(tokio::spawn(async move {
            for i in 0..10 {
                let r = apps[t % 2]
                    .post(
                        &owner,
                        &base,
                        &PushRequest {
                            ops: vec![op(&format!("m{t}-{i}"), &doc(1), b"x")],
                        },
                    )
                    .await;
                assert_eq!(r.status, StatusCode::OK);
            }
        }));
    }
    for t in tasks {
        t.await.unwrap();
    }
    let pulled: PullResponse = apps[1]
        .get(&w.owner, &format!("{base}?after=0&limit=1000"))
        .await
        .json();
    assert_eq!(
        pulled.ops.iter().map(|o| o.seq).collect::<Vec<_>>(),
        (1..=60).collect::<Vec<_>>()
    );
}

/// Needs NOTERCRUX_TEST_DATABASE_TLS_URL (sslmode=require, host matching the certificate) and
/// NOTERCRUX_TEST_DATABASE_CA (PEM file of the CA that signed the server certificate).
#[tokio::test]
async fn tls_connections_verify_the_server_certificate() {
    let (Ok(url), Ok(ca)) = (
        std::env::var("NOTERCRUX_TEST_DATABASE_TLS_URL"),
        std::env::var("NOTERCRUX_TEST_DATABASE_CA"),
    ) else {
        eprintln!("skipped: set NOTERCRUX_TEST_DATABASE_TLS_URL and NOTERCRUX_TEST_DATABASE_CA");
        return;
    };
    let pem = std::fs::read(ca).unwrap();

    // Trusting the right CA works end to end.
    let trusted = PgRepo::connect_with(&url, Some(&pem)).unwrap();
    let app = TestApp::with_state(AppState::new(
        Config::default(),
        Arc::new(trusted),
        Arc::new(MemBlobStore::new()),
    ));
    let w = make_workspace(&app, "tls").await;
    assert!(app.state.repo.workspace(&w.id).await.unwrap().is_some());

    // Without it the handshake must fail rather than silently downgrade.
    let untrusted = PgRepo::connect_with(&url, None).unwrap();
    assert!(
        untrusted.workspace(&w.id).await.is_err(),
        "an unknown CA must be rejected"
    );

    // A garbage CA bundle is a configuration error, not a silent no-op.
    assert!(PgRepo::connect_with(
        &url,
        Some(b"-----BEGIN CERTIFICATE-----\nnot base64!\n-----END CERTIFICATE-----\n")
    )
    .is_err());
}
