//! Account lifecycle: create a workspace, enroll devices, revoke them, recover, leave.

use crate::account::*;
use crate::engine::{lock, Engine, Session};
use crate::{Result, SyncError};
use notercrux_crypto::{
    constant_time_eq, hex, invite_proof, open_with_secret, recovery_message, seal_with_secret,
    short_auth_string, unwrap_ring, DeviceKeys, DevicePublic, InviteSecret, KeyRing, RecoveryKey,
};
use notercrux_protocol::*;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedWorkspace {
    pub workspace_id: String,
    /// Shown once. The only way back in if every device is lost.
    pub recovery_phrase: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceView {
    pub id: String,
    pub label: String,
    pub current: bool,
    pub revoked: bool,
    pub created_at: i64,
    pub last_seen: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InviteOffer {
    pub invite_id: String,
    /// What the new device enters (or scans).
    pub code: String,
    pub expires_at: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinRequest {
    pub device_id: String,
    pub label: String,
    /// Short code both screens show; the person confirms they match.
    pub sas: String,
    /// The request proves knowledge of the invite secret.
    pub verified: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JoinMode {
    /// Keep this device's pages and merge them into the workspace.
    KeepLocal,
    /// Delete this device's pages and start from the workspace's.
    ReplaceLocal,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JoinProgress {
    Waiting,
    Joined,
}

#[derive(Serialize, Deserialize)]
struct PendingJoin {
    code: String,
    device: String,
    label: String,
}

const PENDING_JOIN: &str = "pending-join";

fn invite_secret_name(id: &str) -> String {
    format!("invite-{id}")
}

impl Engine {
    /// Waits for any running sync cycle to finish (aborting a long poll) and blocks new ones.
    pub(crate) async fn exclusive(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.sync_soon_wake();
        self.gate.lock().await
    }

    fn ensure_unlinked(&self) -> Result<()> {
        if lock(&self.session).is_some() || self.with_store(|s| s.sync_get("acct"))?.is_some() {
            return Err(SyncError::AlreadyLinked);
        }
        Ok(())
    }

    /// Stores keys and the account, resets the sync bookkeeping, and turns sync on.
    fn link_local(
        &self,
        account: Account,
        keys: &DeviceKeys,
        ring: &KeyRing,
        replace_local: bool,
    ) -> Result<()> {
        self.secrets
            .set(crate::DEVICE_SECRET, &keys.to_bytes()[..])
            .map_err(SyncError::Secrets)?;
        self.secrets
            .set(crate::RING_SECRET, &ring.to_bytes())
            .map_err(SyncError::Secrets)?;
        let json =
            serde_json::to_string(&account).map_err(|e| SyncError::Protocol(e.to_string()))?;
        let linked = self.with_store(|s| {
            if replace_local {
                s.wipe_workspace()?;
            } else {
                s.disable_sync()?;
            }
            s.enable_sync()?;
            s.sync_set("acct", &json)
        });
        if let Err(e) = linked {
            let _ = self.secrets.delete(crate::DEVICE_SECRET);
            let _ = self.secrets.delete(crate::RING_SECRET);
            return Err(e);
        }
        self.reload()?;
        self.log("info", "Connected to the sync server.");
        Ok(())
    }

    /// Creates a workspace on `server` from this device's current pages.
    pub async fn create_workspace(
        &self,
        server: &str,
        token: Option<String>,
        label: &str,
    ) -> Result<CreatedWorkspace> {
        let _g = self.exclusive().await;
        self.ensure_unlinked()?;
        let server = normalize_server(server)?;
        let keys = DeviceKeys::generate()?;
        let ring = KeyRing::generate()?;
        let recovery = RecoveryKey::generate()?;
        let id = format!("w{}", uuid_simple());
        let req = CreateWorkspace {
            workspace_id: id.clone(),
            device_key: keys.public().to_bytes().to_vec(),
            label_sealed: seal_label(&ring, &id, label)?,
            recovery_key: recovery.public().to_bytes().to_vec(),
            recovery_wrap: notercrux_crypto::wrap_for_recovery(&ring, &recovery.public(), &id)?,
            registration_token: token,
        };
        let body = serde_json::to_vec(&req).map_err(|e| SyncError::Protocol(e.to_string()))?;
        self.api
            .call(
                &server,
                Some(&keys),
                Method::POST,
                "/v1/workspaces",
                body,
                None,
            )
            .await?;
        let account = Account {
            server,
            workspace_id: id.clone(),
            device_id: keys.id(),
            label: label.to_string(),
        };
        self.link_local(account, &keys, &ring, false)?;
        Ok(CreatedWorkspace {
            workspace_id: id,
            recovery_phrase: recovery.to_phrase(),
        })
    }

    // ---- inviting a device ------------------------------------------------------------------

    pub async fn create_invite(&self) -> Result<InviteOffer> {
        let sess = self.session()?;
        let target = format!("/v1/workspaces/{}/invites", sess.account.workspace_id);
        let reply = self
            .api
            .call(
                &sess.account.server,
                Some(&sess.keys),
                Method::POST,
                &target,
                b"{}".to_vec(),
                None,
            )
            .await?;
        let created: CreateInviteResponse = reply.json()?;
        let secret = InviteSecret::generate()?;
        self.secrets
            .set(&invite_secret_name(&created.invite_id), secret.as_bytes())
            .map_err(SyncError::Secrets)?;
        let code = InviteCode {
            server: sess.account.server.clone(),
            workspace: sess.account.workspace_id.clone(),
            invite: created.invite_id.clone(),
            secret: secret.to_phrase(),
        };
        Ok(InviteOffer {
            invite_id: created.invite_id,
            code: code.encode(),
            expires_at: created.expires_at,
        })
    }

    fn invite_secret(&self, invite_id: &str) -> Result<InviteSecret> {
        let bytes = self
            .secrets
            .get(&invite_secret_name(invite_id))
            .map_err(SyncError::Secrets)?
            .ok_or(SyncError::BadCode)?;
        let arr: [u8; 16] = bytes.try_into().map_err(|_| SyncError::BadCode)?;
        Ok(InviteSecret::from_bytes(arr))
    }

    /// The device waiting to join through `invite_id`, once it has asked.
    pub async fn pending_join(&self, invite_id: &str) -> Result<Option<JoinRequest>> {
        let sess = self.session()?;
        let target = format!(
            "/v1/workspaces/{}/invites/{invite_id}",
            sess.account.workspace_id
        );
        let reply = self
            .api
            .call(
                &sess.account.server,
                Some(&sess.keys),
                Method::GET,
                &target,
                Vec::new(),
                None,
            )
            .await?;
        let state: InviteState = reply.json()?;
        let Some(req) = state.request else {
            return Ok(None);
        };
        if state.status != InviteStatus::Requested {
            return Ok(None);
        }
        let secret = self.invite_secret(invite_id)?;
        let public = DevicePublic::from_bytes(&req.device_key).map_err(|_| SyncError::BadCode)?;
        let verified = constant_time_eq(&invite_proof(&secret, &public), &req.proof);
        let label = open_with_secret(
            secret.as_bytes(),
            &invite_label_aad(&sess.account.workspace_id, invite_id),
            &req.label_sealed,
        )
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .unwrap_or_else(|| "Unnamed device".into());
        Ok(Some(JoinRequest {
            device_id: public.id(),
            label,
            sas: short_auth_string(&public),
            verified,
        }))
    }

    /// Approves the waiting device: wraps the workspace key for it and registers it.
    pub async fn approve_join(&self, invite_id: &str) -> Result<()> {
        let sess = self.session()?;
        let ws = &sess.account.workspace_id;
        let target = format!("/v1/workspaces/{ws}/invites/{invite_id}");
        let reply = self
            .api
            .call(
                &sess.account.server,
                Some(&sess.keys),
                Method::GET,
                &target,
                Vec::new(),
                None,
            )
            .await?;
        let state: InviteState = reply.json()?;
        let req = state
            .request
            .ok_or_else(|| SyncError::Protocol("no device is waiting to join".into()))?;
        let secret = self.invite_secret(invite_id)?;
        let public = DevicePublic::from_bytes(&req.device_key).map_err(|_| SyncError::BadCode)?;
        if !constant_time_eq(&invite_proof(&secret, &public), &req.proof) {
            return Err(SyncError::Crypto(
                "the joining device did not prove it knows the invite code".into(),
            ));
        }
        let label = open_with_secret(
            secret.as_bytes(),
            &invite_label_aad(ws, invite_id),
            &req.label_sealed,
        )
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .unwrap_or_else(|| "Unnamed device".into());
        let device_id = public.id();
        let wrap = notercrux_crypto::wrap_ring(
            &sess.ring,
            &public.agree,
            &enroll_context(ws, &device_id, invite_id),
            Some(secret.as_bytes()),
        )?;
        let approve = ApproveInvite {
            device_id,
            key_wrap: wrap,
            label_sealed: seal_label(&sess.ring, ws, &label)?,
        };
        let body = serde_json::to_vec(&approve).map_err(|e| SyncError::Protocol(e.to_string()))?;
        let target = format!("/v1/workspaces/{ws}/invites/{invite_id}/approve");
        self.api
            .call(
                &sess.account.server,
                Some(&sess.keys),
                Method::POST,
                &target,
                body,
                None,
            )
            .await?;
        let _ = self.secrets.delete(&invite_secret_name(invite_id));
        self.log("info", format!("Added device \"{label}\"."));
        Ok(())
    }

    // ---- joining ----------------------------------------------------------------------------

    /// Starts joining with an invite code. Returns the short code to compare with the other screen.
    pub async fn begin_join(&self, code: &str, label: &str) -> Result<String> {
        self.ensure_unlinked()?;
        let invite = InviteCode::decode(code)?;
        let secret = InviteSecret::from_phrase(&invite.secret).map_err(|_| SyncError::BadCode)?;
        let keys = DeviceKeys::generate()?;
        let public = keys.public();
        let req = InviteRequest {
            workspace_id: invite.workspace.clone(),
            device_key: public.to_bytes().to_vec(),
            proof: invite_proof(&secret, &public).to_vec(),
            label_sealed: seal_with_secret(
                secret.as_bytes(),
                &invite_label_aad(&invite.workspace, &invite.invite),
                label.as_bytes(),
            )?,
        };
        let body = serde_json::to_vec(&req).map_err(|e| SyncError::Protocol(e.to_string()))?;
        self.api
            .call(
                &invite.server,
                None,
                Method::POST,
                &format!("/v1/invites/{}/request", invite.invite),
                body,
                None,
            )
            .await?;
        let pending = PendingJoin {
            code: code.trim().to_string(),
            device: hex(&keys.to_bytes()[..]),
            label: label.to_string(),
        };
        let json = serde_json::to_vec(&pending).map_err(|e| SyncError::Protocol(e.to_string()))?;
        self.secrets
            .set(PENDING_JOIN, &json)
            .map_err(SyncError::Secrets)?;
        Ok(short_auth_string(&public))
    }

    fn load_pending(&self) -> Result<(PendingJoin, InviteCode, DeviceKeys)> {
        let raw = self
            .secrets
            .get(PENDING_JOIN)
            .map_err(SyncError::Secrets)?
            .ok_or(SyncError::NotLinked)?;
        let pending: PendingJoin = serde_json::from_slice(&raw).map_err(|_| SyncError::BadCode)?;
        let invite = InviteCode::decode(&pending.code)?;
        let bytes = notercrux_crypto::from_hex(&pending.device).ok_or(SyncError::BadCode)?;
        let keys = DeviceKeys::from_bytes(&bytes)?;
        Ok((pending, invite, keys))
    }

    /// Checks whether the other device approved. On approval the workspace key arrives, local data is
    /// merged or replaced as chosen, and sync turns on.
    pub async fn poll_join(&self, mode: JoinMode) -> Result<JoinProgress> {
        let (pending, invite, keys) = self.load_pending()?;
        let target = format!("/v1/invites/{}/result?device={}", invite.invite, keys.id());
        let reply = self
            .api
            .call(&invite.server, None, Method::GET, &target, Vec::new(), None)
            .await?;
        let result: InviteResult = reply.json()?;
        let (true, Some(wrap)) = (result.approved, result.key_wrap) else {
            return Ok(JoinProgress::Waiting);
        };
        let secret = InviteSecret::from_phrase(&invite.secret).map_err(|_| SyncError::BadCode)?;
        let ring = unwrap_ring(
            &keys,
            &wrap,
            &enroll_context(&invite.workspace, &keys.id(), &invite.invite),
            Some(secret.as_bytes()),
        )
        .map_err(|e| SyncError::Keys(format!("could not open the workspace key: {e}")))?;
        let _g = self.exclusive().await;
        self.ensure_unlinked()?;
        let account = Account {
            server: invite.server.clone(),
            workspace_id: invite.workspace.clone(),
            device_id: keys.id(),
            label: pending.label,
        };
        self.link_local(account, &keys, &ring, mode == JoinMode::ReplaceLocal)?;
        let _ = self.secrets.delete(PENDING_JOIN);
        Ok(JoinProgress::Joined)
    }

    pub fn cancel_join(&self) -> Result<()> {
        self.secrets
            .delete(PENDING_JOIN)
            .map_err(SyncError::Secrets)
    }

    pub fn join_in_progress(&self) -> bool {
        matches!(self.secrets.get(PENDING_JOIN), Ok(Some(_)))
    }

    // ---- devices ----------------------------------------------------------------------------

    pub async fn server_info(&self) -> Result<WorkspaceInfo> {
        let sess = self.session()?;
        let target = format!("/v1/workspaces/{}", sess.account.workspace_id);
        let reply = self
            .api
            .call(
                &sess.account.server,
                Some(&sess.keys),
                Method::GET,
                &target,
                Vec::new(),
                None,
            )
            .await?;
        reply.json()
    }

    pub async fn devices(&self) -> Result<Vec<DeviceView>> {
        let sess = self.session()?;
        let info = self.server_info().await?;
        Ok(info
            .devices
            .iter()
            .map(|d| DeviceView {
                id: d.id.clone(),
                label: open_label(&sess.ring, &sess.account.workspace_id, &d.label_sealed)
                    .unwrap_or_else(|| "Recovered device".into()),
                current: d.id == sess.account.device_id,
                revoked: d.revoked_at.is_some(),
                created_at: d.created_at,
                last_seen: d.last_seen,
            })
            .collect())
    }

    /// Removes a device and rotates the workspace key so it cannot read anything written afterwards.
    pub async fn revoke_device(&self, target_id: &str) -> Result<()> {
        let _g = self.exclusive().await;
        let mut last = SyncError::Protocol("revocation failed".into());
        for _ in 0..3 {
            let sess = self.session()?;
            let ws = &sess.account.workspace_id;
            let info = self.server_info().await?;
            let recovery_public = DevicePublic::from_bytes(&info.recovery_key).map_err(|_| {
                SyncError::Protocol("the server sent an invalid recovery key".into())
            })?;
            let mut ring = KeyRing::from_bytes(&sess.ring.to_bytes())?;
            let new_epoch = ring.rotate()?;
            let mut wraps = Vec::new();
            for d in info
                .devices
                .iter()
                .filter(|d| d.revoked_at.is_none() && d.id != target_id)
            {
                let public = DevicePublic::from_bytes(&d.key)
                    .map_err(|_| SyncError::Protocol("invalid device key".into()))?;
                let key_wrap = notercrux_crypto::wrap_ring(
                    &ring,
                    &public.agree,
                    &rotate_context(ws, &d.id, new_epoch),
                    None,
                )?;
                wraps.push(KeyWrapEntry {
                    device_id: d.id.clone(),
                    key_wrap,
                });
            }
            let body = RevokeDevice {
                new_epoch,
                wraps,
                recovery_wrap: notercrux_crypto::wrap_for_recovery(&ring, &recovery_public, ws)?,
            };
            let body = serde_json::to_vec(&body).map_err(|e| SyncError::Protocol(e.to_string()))?;
            let target = format!("/v1/workspaces/{ws}/devices/{target_id}/revoke");
            match self
                .api
                .call(
                    &sess.account.server,
                    Some(&sess.keys),
                    Method::POST,
                    &target,
                    body,
                    None,
                )
                .await
            {
                Ok(_) => {
                    // Persist the rotated ring before anything else uses the new epoch.
                    self.secrets
                        .set(crate::RING_SECRET, &ring.to_bytes())
                        .map_err(SyncError::Secrets)?;
                    self.set_session(Session {
                        ring: Arc::new(ring),
                        ..sess
                    });
                    self.update_status(|s| s.epoch = new_epoch);
                    self.log("info", "Device removed and the workspace key rotated.");
                    return Ok(());
                }
                // Another device joined or left between reading the list and revoking: try again.
                Err(
                    e @ SyncError::Server {
                        status: 400 | 409, ..
                    },
                ) => last = e,
                Err(e) => return Err(e),
            }
        }
        Err(last)
    }

    // ---- recovery and leaving ---------------------------------------------------------------

    /// Restores access to a workspace with only the recovery key (every device lost).
    pub async fn recover(
        &self,
        server: &str,
        workspace_id: &str,
        phrase: &str,
        label: &str,
        mode: JoinMode,
    ) -> Result<()> {
        let _g = self.exclusive().await;
        self.ensure_unlinked()?;
        let server = normalize_server(server)?;
        let key = RecoveryKey::from_phrase(phrase).map_err(|_| SyncError::BadCode)?;
        let keys = DeviceKeys::generate()?;
        let req = RecoverRequest {
            workspace_id: workspace_id.to_string(),
            device_key: keys.public().to_bytes().to_vec(),
            recovery_signature: key.sign(&recovery_message(workspace_id, &keys.id())),
            label_sealed: seal_with_secret(
                &key.label_secret(),
                &recovery_label_aad(workspace_id),
                label.as_bytes(),
            )?,
        };
        let body = serde_json::to_vec(&req).map_err(|e| SyncError::Protocol(e.to_string()))?;
        let reply = self
            .api
            .call(&server, None, Method::POST, "/v1/recover", body, None)
            .await?;
        let res: RecoverResponse = reply.json()?;
        let ring = key
            .unwrap_ring(&res.recovery_wrap, workspace_id)
            .map_err(|_| SyncError::BadCode)?;
        let account = Account {
            server,
            workspace_id: workspace_id.to_string(),
            device_id: keys.id(),
            label: label.to_string(),
        };
        self.link_local(account, &keys, &ring, mode == JoinMode::ReplaceLocal)?;
        Ok(())
    }

    /// Disconnects this device. When other devices remain it first removes itself (rotating the key);
    /// local pages are kept either way.
    pub async fn leave(&self) -> Result<()> {
        if let Ok(devices) = self.devices().await {
            if devices.iter().filter(|d| !d.revoked).count() > 1 {
                if let Some(me) = devices.iter().find(|d| d.current) {
                    let id = me.id.clone();
                    let _ = self.revoke_device(&id).await;
                }
            }
        }
        self.leave_local().await
    }

    /// Forgets the link on this device only (also used after being revoked).
    pub async fn leave_local(&self) -> Result<()> {
        let _g = self.exclusive().await;
        self.with_store(|s| s.disable_sync())?;
        let _ = self.secrets.delete(crate::DEVICE_SECRET);
        let _ = self.secrets.delete(crate::RING_SECRET);
        let _ = self.secrets.delete(PENDING_JOIN);
        self.reload()?;
        self.log(
            "info",
            "Disconnected from the sync server. Pages are kept on this device.",
        );
        Ok(())
    }

    /// Deletes the workspace's data from the server (after the user confirmed), then disconnects.
    pub async fn delete_remote_workspace(&self) -> Result<()> {
        let sess = self.session()?;
        let target = format!("/v1/workspaces/{}", sess.account.workspace_id);
        self.api
            .call(
                &sess.account.server,
                Some(&sess.keys),
                Method::DELETE,
                &target,
                Vec::new(),
                None,
            )
            .await?;
        self.leave_local().await
    }
}

fn uuid_simple() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}
