use super::*;

#[test]
fn seal_open_round_trip_and_context_binding() {
    let k = WorkspaceKey::generate().unwrap();
    let sealed = k.seal(Purpose::Content, b"page-1", b"secret text").unwrap();
    assert!(!sealed.windows(6).any(|w| w == b"secret"));
    assert_eq!(
        k.open(Purpose::Content, b"page-1", &sealed).unwrap(),
        b"secret text"
    );
    // Wrong page, wrong purpose, wrong key, and tampering must all fail.
    assert_eq!(
        k.open(Purpose::Content, b"page-2", &sealed),
        Err(CryptoError::Decrypt)
    );
    assert_eq!(
        k.open(Purpose::Metadata, b"page-1", &sealed),
        Err(CryptoError::Decrypt)
    );
    let other = WorkspaceKey::generate().unwrap();
    assert_eq!(
        other.open(Purpose::Content, b"page-1", &sealed),
        Err(CryptoError::Decrypt)
    );
    let mut bad = sealed.clone();
    *bad.last_mut().unwrap() ^= 1;
    assert_eq!(
        k.open(Purpose::Content, b"page-1", &bad),
        Err(CryptoError::Decrypt)
    );
}

#[test]
fn nonces_are_unique_per_message_and_future_versions_are_rejected() {
    let k = WorkspaceKey::generate().unwrap();
    let a = k.seal(Purpose::Content, b"", b"same").unwrap();
    let b = k.seal(Purpose::Content, b"", b"same").unwrap();
    assert_ne!(a, b);
    let mut s = a.clone();
    s[0] = 2;
    assert_eq!(
        k.open(Purpose::Content, b"", &s),
        Err(CryptoError::UnsupportedVersion(2))
    );
    assert_eq!(
        k.open(Purpose::Content, b"", &[]),
        Err(CryptoError::Malformed)
    );
    assert_eq!(
        k.open(Purpose::Content, b"", &[1, 2, 3]),
        Err(CryptoError::Malformed)
    );
}

// ---- key ring --------------------------------------------------------------------------------

#[test]
fn ring_seals_with_the_current_epoch_and_opens_old_epochs() {
    let mut ring = KeyRing::generate().unwrap();
    let aad = |e| content_aad("ws", "doc", e);
    let (e0, old) = ring
        .seal(Purpose::Content, aad, b"before rotation")
        .unwrap();
    assert_eq!(e0, 0);
    assert_eq!(ring.rotate().unwrap(), 1);
    let (e1, new) = ring.seal(Purpose::Content, aad, b"after rotation").unwrap();
    assert_eq!(e1, 1);
    assert_eq!(
        ring.open(0, Purpose::Content, &content_aad("ws", "doc", 0), &old)
            .unwrap(),
        b"before rotation"
    );
    assert_eq!(
        ring.open(1, Purpose::Content, &content_aad("ws", "doc", 1), &new)
            .unwrap(),
        b"after rotation"
    );
    // Ciphertext is bound to its epoch, document and workspace.
    assert!(ring
        .open(1, Purpose::Content, &content_aad("ws", "doc", 1), &old)
        .is_err());
    assert!(ring
        .open(1, Purpose::Content, &content_aad("ws", "other", 1), &new)
        .is_err());
    assert!(ring
        .open(1, Purpose::Content, &content_aad("ws2", "doc", 1), &new)
        .is_err());
    assert_eq!(
        ring.open(9, Purpose::Content, b"", &new),
        Err(CryptoError::UnknownEpoch(9))
    );
}

#[test]
fn ring_serialization_round_trips_and_rejects_garbage() {
    let mut ring = KeyRing::generate().unwrap();
    ring.rotate().unwrap();
    let back = KeyRing::from_bytes(&ring.to_bytes()).unwrap();
    assert_eq!(back.epochs(), vec![0, 1]);
    let (e, sealed) = ring
        .seal(Purpose::Metadata, |_| b"x".to_vec(), b"hi")
        .unwrap();
    assert_eq!(
        back.open(e, Purpose::Metadata, b"x", &sealed).unwrap(),
        b"hi"
    );
    let bytes = ring.to_bytes();
    assert!(KeyRing::from_bytes(&bytes[..bytes.len() - 1]).is_err());
    assert!(KeyRing::from_bytes(&[]).is_err());
    assert!(KeyRing::from_bytes(&[1, 0, 0]).is_err()); // zero keys
    let mut no_epoch_zero = bytes.to_vec();
    no_epoch_zero[3..7].copy_from_slice(&5u32.to_be_bytes()); // first key renumbered: epoch 0 missing
    assert!(KeyRing::from_bytes(&no_epoch_zero).is_err());
}

#[test]
fn document_ids_are_stable_opaque_and_survive_rotation() {
    let mut ring = KeyRing::generate().unwrap();
    let id = ring.doc_id("0190a000-0000-7000-8000-000000000001").unwrap();
    assert_eq!(id.len(), 32);
    assert!(!id.contains("0190a000"));
    assert_eq!(
        id,
        ring.doc_id("0190a000-0000-7000-8000-000000000001").unwrap()
    );
    assert_ne!(
        id,
        ring.doc_id("0190a000-0000-7000-8000-000000000002").unwrap()
    );
    ring.rotate().unwrap();
    assert_eq!(
        id,
        ring.doc_id("0190a000-0000-7000-8000-000000000001").unwrap(),
        "rotation must not rename documents"
    );
    assert_ne!(
        id,
        KeyRing::generate()
            .unwrap()
            .doc_id("0190a000-0000-7000-8000-000000000001")
            .unwrap()
    );
    let h = sha256(b"file contents");
    assert_eq!(ring.blob_id(&h).unwrap(), ring.blob_id(&h).unwrap());
    assert_ne!(
        ring.blob_id(&h).unwrap(),
        KeyRing::generate().unwrap().blob_id(&h).unwrap()
    );
}

// ---- device keys and requests -------------------------------------------------------------------

#[test]
fn device_ids_are_bound_to_keys_and_round_trip() {
    let d = DeviceKeys::generate().unwrap();
    let again = DeviceKeys::from_bytes(&*d.to_bytes()).unwrap();
    assert_eq!(d.public(), again.public());
    assert_eq!(d.id(), again.id());
    assert_eq!(d.id().len(), 32);
    assert_ne!(d.id(), DeviceKeys::generate().unwrap().id());
    assert_eq!(
        DevicePublic::from_bytes(&d.public().to_bytes()).unwrap(),
        d.public()
    );
    assert!(DevicePublic::from_bytes(&[0u8; 10]).is_err());
}

#[test]
fn signed_requests_verify_and_detect_any_change() {
    let d = DeviceKeys::generate().unwrap();
    let sig = sign_request(
        &d,
        "POST",
        "/v1/ops?after=3",
        1_700_000_000,
        "n-1",
        b"{\"a\":1}",
    );
    let ok = |m: &str, t: &str, ts: i64, n: &str, b: &[u8], s: &[u8]| {
        verify_request(&d.public().sign, m, t, ts, n, b, s)
    };
    assert!(ok(
        "POST",
        "/v1/ops?after=3",
        1_700_000_000,
        "n-1",
        b"{\"a\":1}",
        &sig
    ));
    assert!(
        !ok(
            "GET",
            "/v1/ops?after=3",
            1_700_000_000,
            "n-1",
            b"{\"a\":1}",
            &sig
        ),
        "method is signed"
    );
    assert!(
        !ok(
            "POST",
            "/v1/ops?after=4",
            1_700_000_000,
            "n-1",
            b"{\"a\":1}",
            &sig
        ),
        "target is signed"
    );
    assert!(
        !ok(
            "POST",
            "/v1/ops?after=3",
            1_700_000_001,
            "n-1",
            b"{\"a\":1}",
            &sig
        ),
        "timestamp is signed"
    );
    assert!(
        !ok(
            "POST",
            "/v1/ops?after=3",
            1_700_000_000,
            "n-2",
            b"{\"a\":1}",
            &sig
        ),
        "nonce is signed"
    );
    assert!(
        !ok(
            "POST",
            "/v1/ops?after=3",
            1_700_000_000,
            "n-1",
            b"{\"a\":2}",
            &sig
        ),
        "body is signed"
    );
    let other = DeviceKeys::generate().unwrap();
    assert!(!verify_request(
        &other.public().sign,
        "POST",
        "/v1/ops?after=3",
        1_700_000_000,
        "n-1",
        b"{\"a\":1}",
        &sig
    ));
    assert!(!ok(
        "POST",
        "/v1/ops?after=3",
        1_700_000_000,
        "n-1",
        b"{\"a\":1}",
        &sig[..63]
    ));
}

// ---- wrapping, enrollment and revocation --------------------------------------------------------

#[test]
fn a_wrapped_ring_opens_only_for_its_device_context_and_secret() {
    let ring = KeyRing::generate().unwrap();
    let dev = DeviceKeys::generate().unwrap();
    let secret = InviteSecret::generate().unwrap();
    let blob = wrap_ring(
        &ring,
        &dev.public().agree,
        b"ws|invite-1",
        Some(secret.as_bytes()),
    )
    .unwrap();
    let got = unwrap_ring(&dev, &blob, b"ws|invite-1", Some(secret.as_bytes())).unwrap();
    assert_eq!(got.epochs(), ring.epochs());
    assert_eq!(got.doc_id("p").unwrap(), ring.doc_id("p").unwrap());

    // Wrong device, wrong context, missing or wrong secret, tampering: all fail.
    let eve = DeviceKeys::generate().unwrap();
    assert!(unwrap_ring(&eve, &blob, b"ws|invite-1", Some(secret.as_bytes())).is_err());
    assert!(unwrap_ring(&dev, &blob, b"ws|invite-2", Some(secret.as_bytes())).is_err());
    assert!(unwrap_ring(&dev, &blob, b"ws|invite-1", None).is_err());
    let other_secret = InviteSecret::generate().unwrap();
    assert!(unwrap_ring(&dev, &blob, b"ws|invite-1", Some(other_secret.as_bytes())).is_err());
    let mut bad = blob.clone();
    *bad.last_mut().unwrap() ^= 1;
    assert!(unwrap_ring(&dev, &bad, b"ws|invite-1", Some(secret.as_bytes())).is_err());
    assert!(unwrap_ring(&dev, &blob[..20], b"ws|invite-1", Some(secret.as_bytes())).is_err());
}

#[test]
fn a_server_that_swaps_the_enrolling_devices_key_is_caught_by_the_invite_proof() {
    let secret = InviteSecret::generate().unwrap();
    let real = DeviceKeys::generate().unwrap().public();
    let mitm = DeviceKeys::generate().unwrap().public();
    let proof = invite_proof(&secret, &real);
    assert!(constant_time_eq(&proof, &invite_proof(&secret, &real)));
    // The server can replace the public key but cannot forge a proof for it without the secret.
    assert!(!constant_time_eq(&proof, &invite_proof(&secret, &mitm)));
    assert!(!constant_time_eq(
        &proof,
        &invite_proof(&InviteSecret::generate().unwrap(), &real)
    ));
    assert_ne!(short_auth_string(&real), short_auth_string(&mitm));
    assert_eq!(short_auth_string(&real).len(), 6);
}

#[test]
fn invite_secrets_round_trip_as_text() {
    let s = InviteSecret::generate().unwrap();
    let phrase = s.to_phrase();
    assert_eq!(
        InviteSecret::from_phrase(&phrase.to_lowercase().replace('-', " "))
            .unwrap()
            .as_bytes(),
        s.as_bytes()
    );
    assert!(InviteSecret::from_phrase("AAAA").is_err());
    assert!(InviteSecret::from_phrase("1111-1111-1111-1111-1111-1111-11").is_err());
}

#[test]
fn revocation_rotates_so_the_revoked_device_cannot_read_new_data() {
    let mut ring = KeyRing::generate().unwrap();
    let keep = DeviceKeys::generate().unwrap();
    let revoked = DeviceKeys::generate().unwrap();
    let (_, before) = ring
        .seal(Purpose::Content, |e| content_aad("ws", "d", e), b"old data")
        .unwrap();

    // Both devices hold epoch 0. The revoked device is dropped, a new epoch is created and wrapped
    // for the remaining device only.
    let revoked_ring = KeyRing::from_bytes(&ring.to_bytes()).unwrap();
    ring.rotate().unwrap();
    let pkg = wrap_ring(&ring, &keep.public().agree, b"ws|1|keep", None).unwrap();
    assert!(unwrap_ring(&revoked, &pkg, b"ws|1|keep", None).is_err());

    let (epoch, after) = ring
        .seal(Purpose::Content, |e| content_aad("ws", "d", e), b"new data")
        .unwrap();
    assert_eq!(epoch, 1);
    assert_eq!(
        revoked_ring.open(1, Purpose::Content, &content_aad("ws", "d", 1), &after),
        Err(CryptoError::UnknownEpoch(1))
    );
    // Data the revoked device could already read stays readable (documented limitation).
    assert_eq!(
        revoked_ring
            .open(0, Purpose::Content, &content_aad("ws", "d", 0), &before)
            .unwrap(),
        b"old data"
    );
    let kept = unwrap_ring(&keep, &pkg, b"ws|1|keep", None).unwrap();
    assert_eq!(
        kept.open(1, Purpose::Content, &content_aad("ws", "d", 1), &after)
            .unwrap(),
        b"new data"
    );
}

// ---- recovery -----------------------------------------------------------------------------------

#[test]
fn recovery_key_restores_every_epoch_and_detects_typos() {
    let mut ring = KeyRing::generate().unwrap();
    ring.rotate().unwrap();
    let rk = RecoveryKey::generate().unwrap();
    let wrapped = wrap_for_recovery(&ring, &rk.public(), "ws-1").unwrap();
    let phrase = rk.to_phrase();

    let typed = phrase.to_lowercase().replace('-', " ");
    let restored = RecoveryKey::from_phrase(&typed)
        .unwrap()
        .unwrap_ring(&wrapped, "ws-1")
        .unwrap();
    assert_eq!(restored.epochs(), vec![0, 1]);

    assert!(
        rk.unwrap_ring(&wrapped, "ws-2").is_err(),
        "bound to the workspace id"
    );
    let mut chars: Vec<char> = phrase.chars().collect();
    chars[0] = if chars[0] == 'A' { 'B' } else { 'A' };
    assert!(matches!(
        RecoveryKey::from_phrase(&chars.into_iter().collect::<String>()),
        Err(CryptoError::InvalidRecoveryKey)
    ));
    assert!(RecoveryKey::generate()
        .unwrap()
        .unwrap_ring(&wrapped, "ws-1")
        .is_err());
}

#[test]
fn recovery_keys_are_deterministic_and_devices_can_rewrap_without_the_secret() {
    let rk = RecoveryKey::generate().unwrap();
    let again = RecoveryKey::from_phrase(&rk.to_phrase()).unwrap();
    assert_eq!(rk.public(), again.public());
    assert_ne!(rk.public(), RecoveryKey::generate().unwrap().public());
    // Signatures verify against the public half the server stores.
    let sig = rk.sign(b"recover");
    assert!(verify_signature(&rk.public().sign, b"recover", &sig));
    // A device that only knows the public half (all of them) can produce a copy the phrase opens,
    // including after the key rotates.
    let mut ring = KeyRing::generate().unwrap();
    let public = DevicePublic::from_bytes(&rk.public().to_bytes()).unwrap();
    let first = wrap_for_recovery(&ring, &public, "ws").unwrap();
    assert_eq!(again.unwrap_ring(&first, "ws").unwrap().epochs(), vec![0]);
    ring.rotate().unwrap();
    let second = wrap_for_recovery(&ring, &public, "ws").unwrap();
    assert_eq!(
        again.unwrap_ring(&second, "ws").unwrap().epochs(),
        vec![0, 1]
    );
    // The public half alone cannot open it.
    let device = DeviceKeys::generate().unwrap();
    assert!(unwrap_ring(&device, &second, b"NCX-recovery|ws", None).is_err());
}

#[test]
fn chunk_aad_binds_position_and_epoch() {
    let ring = KeyRing::generate().unwrap();
    let (e, sealed) = ring
        .seal(
            Purpose::Attachment,
            |e| chunk_aad("ws", "blob", 3, 10, e),
            b"chunk",
        )
        .unwrap();
    assert!(ring
        .open(
            e,
            Purpose::Attachment,
            &chunk_aad("ws", "blob", 3, 10, e),
            &sealed
        )
        .is_ok());
    assert!(
        ring.open(
            e,
            Purpose::Attachment,
            &chunk_aad("ws", "blob", 4, 10, e),
            &sealed
        )
        .is_err(),
        "chunks cannot be reordered"
    );
    assert!(
        ring.open(
            e,
            Purpose::Attachment,
            &chunk_aad("ws", "blob", 3, 11, e),
            &sealed
        )
        .is_err(),
        "or truncated/extended"
    );
    assert!(ring
        .open(
            e,
            Purpose::Attachment,
            &chunk_aad("ws", "other", 3, 10, e),
            &sealed
        )
        .is_err());
}

#[test]
fn hex_helpers_round_trip() {
    assert_eq!(hex(&[0, 255, 16]), "00ff10");
    assert_eq!(from_hex("00ff10").unwrap(), vec![0, 255, 16]);
    assert!(from_hex("0").is_none() && from_hex("zz").is_none());
}

#[test]
fn labels_are_sealed_under_a_shared_secret_and_bound_to_their_context() {
    let secret = InviteSecret::generate().unwrap();
    let sealed = seal_with_secret(secret.as_bytes(), b"ws|invite", b"Luca's laptop").unwrap();
    assert!(!sealed.windows(4).any(|w| w == b"Luca"));
    assert_eq!(
        open_with_secret(secret.as_bytes(), b"ws|invite", &sealed).unwrap(),
        b"Luca's laptop"
    );
    assert!(open_with_secret(secret.as_bytes(), b"ws|other", &sealed).is_err());
    assert!(open_with_secret(
        InviteSecret::generate().unwrap().as_bytes(),
        b"ws|invite",
        &sealed
    )
    .is_err());
}

#[test]
fn recovery_messages_bind_workspace_and_device() {
    assert_ne!(
        recovery_message("ws1", "dev"),
        recovery_message("ws2", "dev")
    );
    assert_ne!(
        recovery_message("ws1", "dev"),
        recovery_message("ws1", "dev2")
    );
    let rk = RecoveryKey::generate().unwrap();
    assert_eq!(
        rk.label_secret(),
        RecoveryKey::from_phrase(&rk.to_phrase())
            .unwrap()
            .label_secret()
    );
}

#[test]
fn a_ring_is_only_accepted_as_an_update_if_it_extends_the_current_one() {
    let mut ring = KeyRing::generate().unwrap();
    let mut newer = KeyRing::from_bytes(&ring.to_bytes()).unwrap();
    newer.rotate().unwrap();
    assert!(ring.is_extended_by(&newer));
    assert!(ring.is_extended_by(&ring));
    assert!(
        !newer.is_extended_by(&ring),
        "dropping an epoch is not an extension"
    );
    let forged = KeyRing::generate().unwrap();
    assert!(
        !ring.is_extended_by(&forged),
        "a ring made up by someone else must be refused"
    );
    ring.rotate().unwrap();
    assert!(
        !ring.is_extended_by(&newer),
        "same epoch number, different key"
    );
}
