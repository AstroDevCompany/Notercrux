-- Notercrux sync server schema. Everything content-bearing is an opaque bytea produced by clients.
create table workspaces (
    id text primary key,
    epoch integer not null default 0,
    head bigint not null default 0,
    recovery_key bytea not null,
    recovery_wrap bytea not null,
    op_bytes bigint not null default 0,
    op_count bigint not null default 0,
    blob_bytes bigint not null default 0,
    created_at bigint not null
);

create table devices (
    workspace_id text not null references workspaces (id) on delete cascade,
    id text not null,
    key bytea not null,
    label_sealed bytea not null,
    created_at bigint not null,
    revoked_at bigint,
    last_seen bigint,
    primary key (workspace_id, id)
);

create table key_wraps (
    workspace_id text not null,
    device_id text not null,
    epoch integer not null,
    wrap bytea not null,
    primary key (workspace_id, device_id, epoch),
    foreign key (workspace_id, device_id) references devices (workspace_id, id) on delete cascade
);

create table invites (
    id text primary key,
    workspace_id text not null references workspaces (id) on delete cascade,
    created_by text not null,
    expires_at bigint not null,
    request text,
    request_device text,
    approved_device text,
    created_at bigint not null
);
create index invites_expiry on invites (expires_at);

create table ops (
    workspace_id text not null references workspaces (id) on delete cascade,
    seq bigint not null,
    op_id text not null,
    device_id text not null,
    doc_id text not null,
    epoch integer not null,
    format integer not null,
    payload bytea not null,
    created_at bigint not null,
    primary key (workspace_id, seq),
    unique (workspace_id, op_id)
);

create table nonces (
    workspace_id text not null references workspaces (id) on delete cascade,
    device_id text not null,
    nonce text not null,
    expires_at bigint not null,
    primary key (workspace_id, device_id, nonce)
);
create index nonces_expiry on nonces (expires_at);

create table blobs (
    workspace_id text not null references workspaces (id) on delete cascade,
    blob_id text not null,
    epoch integer not null default 0,
    total integer not null default 0,
    size bigint not null default 0,
    complete boolean not null default false,
    created_at bigint not null,
    primary key (workspace_id, blob_id)
);

create table blob_chunks (
    workspace_id text not null,
    blob_id text not null,
    idx integer not null,
    size bigint not null,
    primary key (workspace_id, blob_id, idx),
    foreign key (workspace_id, blob_id) references blobs (workspace_id, blob_id) on delete cascade
);
