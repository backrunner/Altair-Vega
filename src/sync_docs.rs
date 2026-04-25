use altair_vega::{
    SyncAction, SyncEntry, SyncEntryState, SyncManifest, SyncMergePlan, join_sync_path,
    merge_manifests, scan_directory, sync_apply_target_path, sync_temp_path, unix_time_now_ms,
    with_tombstones,
};
use anyhow::{Context, Result, bail, ensure};
use futures_util::StreamExt;
use iroh::{
    Endpoint,
    address_lookup::{MdnsAddressLookup, UserData},
    endpoint::{Connection, presets},
    protocol::{AcceptError, ProtocolHandler, Router},
};
use iroh_blobs::{
    ALPN as BLOBS_ALPN, BlobFormat, BlobsProtocol, HashAndFormat,
    api::Store as BlobsStore,
    api::blobs::{AddPathOptions, ImportMode},
    store::fs::FsStore,
};
use iroh_docs::{
    ALPN as DOCS_ALPN, DocTicket,
    api::{
        Doc,
        protocol::{AddrInfoOptions, ShareMode},
    },
    protocol::Docs,
    store::Query,
};
use iroh_gossip::{ALPN as GOSSIP_ALPN, net::Gossip};
use std::{fs, path::Path, str::FromStr, sync::Arc, time::Duration};
use tokio::io::AsyncWriteExt;

pub const LOCAL_SYNC_TICKET_ALPN: &[u8] = b"altair-vega/local-sync-ticket/1";

pub struct DocsSyncNode {
    router: Router,
    docs: Docs,
    blobs: BlobsStore,
    local_ticket: Arc<std::sync::RwLock<Option<LocalSyncTicket>>>,
}

#[derive(Clone, Debug)]
struct LocalSyncTicket {
    code: String,
    ticket: String,
}

#[derive(Clone, Debug)]
struct LocalSyncTicketProtocol {
    ticket: Arc<std::sync::RwLock<Option<LocalSyncTicket>>>,
}

#[derive(Clone, Debug)]
pub struct DocsExportResult {
    pub doc_id: String,
    pub ticket: String,
    pub manifest: SyncManifest,
    pub content_blobs: usize,
}

pub struct DocsImportState {
    pub doc: Doc,
    pub peer: iroh::EndpointAddr,
}

impl DocsSyncNode {
    pub async fn spawn_persistent(state_dir: &Path) -> Result<Self> {
        Self::spawn_persistent_with_local_code(state_dir, None).await
    }

    pub async fn spawn_persistent_with_local_code(
        state_dir: &Path,
        local_code: Option<&str>,
    ) -> Result<Self> {
        fs::create_dir_all(state_dir)
            .with_context(|| format!("create docs state dir {}", state_dir.display()))?;
        fs::create_dir_all(state_dir.join("docs-state")).with_context(|| {
            format!(
                "create nested docs state dir {}",
                state_dir.join("docs-state").display()
            )
        })?;
        let mut endpoint_builder = Endpoint::builder(presets::N0)
            .alpns(vec![
                BLOBS_ALPN.to_vec(),
                GOSSIP_ALPN.to_vec(),
                DOCS_ALPN.to_vec(),
                LOCAL_SYNC_TICKET_ALPN.to_vec(),
            ])
            .address_lookup(MdnsAddressLookup::builder());
        if let Some(code) = local_code {
            endpoint_builder =
                endpoint_builder.user_data_for_address_lookup(local_sync_user_data(code)?);
        }
        let endpoint = endpoint_builder
            .bind()
            .await
            .context("bind docs endpoint")?;
        let blobs = FsStore::load(state_dir.join("docs-blobs"))
            .await
            .context("load docs blobs store")?;
        let gossip = Gossip::builder().spawn(endpoint.clone());
        let docs = Docs::persistent(state_dir.join("docs-state"))
            .spawn(
                endpoint.clone(),
                BlobsStore::from(blobs.clone()),
                gossip.clone(),
            )
            .await
            .context("spawn docs protocol")?;
        let local_ticket = Arc::new(std::sync::RwLock::new(None));
        let router = Router::builder(endpoint)
            .accept(BLOBS_ALPN, BlobsProtocol::new(&blobs, None))
            .accept(GOSSIP_ALPN, gossip)
            .accept(DOCS_ALPN, docs.clone())
            .accept(
                LOCAL_SYNC_TICKET_ALPN,
                LocalSyncTicketProtocol {
                    ticket: local_ticket.clone(),
                },
            )
            .spawn();
        Ok(Self {
            router,
            docs,
            blobs: BlobsStore::from(blobs),
            local_ticket,
        })
    }

    pub fn set_local_sync_ticket(&self, code: &str, ticket: &str) -> Result<()> {
        let mut state = self
            .local_ticket
            .write()
            .map_err(|_| anyhow::anyhow!("local sync ticket state lock poisoned"))?;
        *state = Some(LocalSyncTicket {
            code: code.to_string(),
            ticket: ticket.to_string(),
        });
        Ok(())
    }

    pub async fn export_directory(
        &self,
        root: &Path,
        chunk_size_bytes: u32,
    ) -> Result<DocsExportResult> {
        let manifest = scan_directory(root, chunk_size_bytes)
            .with_context(|| format!("scan export root {}", root.display()))?;
        self.export_manifest(root, &SyncManifest::default(), manifest)
            .await
    }

    pub async fn export_manifest(
        &self,
        root: &Path,
        previous_manifest: &SyncManifest,
        manifest: SyncManifest,
    ) -> Result<DocsExportResult> {
        let doc = self.docs.create().await.context("create docs document")?;
        self.export_existing_manifest(&doc, root, previous_manifest, manifest)
            .await
    }

    pub async fn export_existing_manifest(
        &self,
        doc: &Doc,
        root: &Path,
        previous_manifest: &SyncManifest,
        manifest: SyncManifest,
    ) -> Result<DocsExportResult> {
        let (content_blobs, manifest) = self
            .publish_manifest(doc, root, previous_manifest, &manifest)
            .await?;
        let ticket = doc
            .share(ShareMode::Write, AddrInfoOptions::RelayAndAddresses)
            .await
            .context("share docs document")?;
        Ok(DocsExportResult {
            doc_id: doc.id().to_string(),
            ticket: ticket.to_string(),
            manifest,
            content_blobs,
        })
    }

    pub async fn publish_manifest(
        &self,
        doc: &Doc,
        root: &Path,
        previous_manifest: &SyncManifest,
        current_manifest: &SyncManifest,
    ) -> Result<(usize, SyncManifest)> {
        let manifest = with_tombstones(previous_manifest, current_manifest, unix_time_now_ms());
        let content_blobs = preload_manifest_blobs(&self.blobs, root, &manifest).await?;
        let author = self.docs.author_default().await?;
        write_manifest(doc, author, &manifest).await?;
        self.advertise_peer_ticket(doc).await?;
        Ok((content_blobs, manifest))
    }

    pub async fn advertise_peer_ticket(&self, doc: &Doc) -> Result<String> {
        let ticket = doc
            .share(ShareMode::Write, AddrInfoOptions::RelayAndAddresses)
            .await
            .context("share docs document for peer advertisement")?
            .to_string();
        let author = self.docs.author_default().await?;
        let key = format!("{PEER_TICKET_PREFIX}{author}");
        doc.set_bytes(author, key, ticket.clone())
            .await
            .context("write docs peer ticket advertisement")?;
        Ok(ticket)
    }

    pub async fn read_peer_tickets(&self, doc: &Doc) -> Result<Vec<String>> {
        let query = Query::single_latest_per_key()
            .key_prefix(PEER_TICKET_PREFIX)
            .include_empty()
            .build();
        let stream = doc
            .get_many(query)
            .await
            .context("query docs peer ticket entries")?;
        tokio::pin!(stream);
        let mut tickets = Vec::new();
        while let Some(item) = stream.next().await {
            let entry = item.context("read docs peer ticket entry")?;
            if entry.content_len() == 0 {
                continue;
            }
            let bytes = self
                .blobs
                .get_bytes(entry.content_hash())
                .await
                .context("load docs peer ticket blob")?;
            tickets.push(String::from_utf8(bytes.to_vec()).context("decode docs peer ticket")?);
        }
        Ok(tickets)
    }

    pub async fn import_manifest(&self, ticket: &str, wait_ms: u64) -> Result<SyncManifest> {
        let DocsImportState { doc, .. } = self.import_doc(ticket).await?;
        tokio::time::sleep(Duration::from_millis(wait_ms)).await;
        read_manifest(&self.blobs, &doc).await
    }

    pub async fn import_doc(&self, ticket: &str) -> Result<DocsImportState> {
        let ticket = DocTicket::from_str(ticket).context("parse doc ticket")?;
        let peer = ticket
            .nodes
            .first()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("doc ticket did not include any peers"))?;
        let doc = self
            .docs
            .import(ticket)
            .await
            .context("import doc ticket")?;
        Ok(DocsImportState { doc, peer })
    }

    pub async fn import_ticket_namespace(&self, ticket: &str) -> Result<Doc> {
        let ticket = DocTicket::from_str(ticket).context("parse doc ticket")?;
        self.docs
            .import_namespace(ticket.capability)
            .await
            .context("import doc ticket namespace")
    }

    pub async fn read_doc_manifest(&self, doc: &Doc) -> Result<SyncManifest> {
        read_manifest(&self.blobs, doc).await
    }

    pub async fn open_doc(&self, doc_id: &str) -> Result<Doc> {
        let doc_id = doc_id.parse().context("parse docs namespace id")?;
        self.docs
            .open(doc_id)
            .await?
            .context("open docs document by id")
    }

    pub async fn fetch_path_from_ticket(
        &self,
        ticket: &str,
        relative_path: &str,
        output_root: &Path,
        wait_ms: u64,
    ) -> Result<SyncManifest> {
        let DocsImportState { doc, peer } = self.import_doc(ticket).await?;
        tokio::time::sleep(Duration::from_millis(wait_ms)).await;
        let manifest = read_manifest(&self.blobs, &doc).await?;
        let entry = manifest.get(relative_path).cloned().ok_or_else(|| {
            anyhow::anyhow!("path {relative_path} not found in imported manifest")
        })?;
        let target = sync_apply_target_path(output_root, relative_path, &entry)?;
        let descriptor = match &entry.state {
            SyncEntryState::File(descriptor) => descriptor,
            SyncEntryState::Tombstone => bail!("path {relative_path} is a tombstone"),
        };
        self.fetch_descriptor_to_path(peer, descriptor, &target)
            .await?;
        Ok(manifest)
    }

    pub async fn apply_ticket_merge(
        &self,
        ticket: &str,
        base_root: &Path,
        local_root: &Path,
        wait_ms: u64,
    ) -> Result<SyncMergePlan> {
        let DocsImportState { doc, peer } = self.import_doc(ticket).await?;
        tokio::time::sleep(Duration::from_millis(wait_ms)).await;

        let base_manifest =
            scan_directory(base_root, altair_vega::DEFAULT_SYNC_CHUNK_SIZE_BYTES)
                .with_context(|| format!("scan base sync root {}", base_root.display()))?;
        let remote_manifest = read_manifest(&self.blobs, &doc).await?;
        self.apply_remote_manifest(peer, &base_manifest, local_root, &remote_manifest)
            .await
    }

    pub async fn shutdown(self) -> Result<()> {
        self.router
            .shutdown()
            .await
            .context("shutdown docs router")?;
        Ok(())
    }

    async fn fetch_descriptor_to_path(
        &self,
        peer: iroh::EndpointAddr,
        descriptor: &altair_vega::FileDescriptor,
        target: &Path,
    ) -> Result<()> {
        let local = self
            .blobs
            .remote()
            .local(HashAndFormat {
                hash: descriptor.hash.into(),
                format: BlobFormat::Raw,
            })
            .await
            .context("inspect local blob availability")?;
        if !local.is_complete() {
            let connection = self
                .router
                .endpoint()
                .connect(peer, BLOBS_ALPN)
                .await
                .context("connect to blob peer from doc ticket")?;
            let request = local.missing();
            self.blobs
                .remote()
                .execute_get(connection, request)
                .await
                .context("fetch blob content for manifest path")?;
        }

        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create fetched file parent {}", parent.display()))?;
        }
        ensure!(
            !target.is_dir(),
            "sync target is a directory: {}",
            target.display()
        );
        let bytes = self
            .blobs
            .get_bytes(descriptor.hash)
            .await
            .context("read fetched blob bytes")?;
        let tmp = sync_temp_path(target);
        let mut file = tokio::fs::File::create(&tmp)
            .await
            .with_context(|| format!("create fetched temp file {}", tmp.display()))?;
        file.write_all(&bytes)
            .await
            .with_context(|| format!("write fetched temp file {}", tmp.display()))?;
        file.flush().await?;
        drop(file);
        tokio::fs::rename(&tmp, target)
            .await
            .with_context(|| format!("finalize fetched file {}", target.display()))?;
        Ok(())
    }

    pub async fn apply_remote_manifest(
        &self,
        peer: iroh::EndpointAddr,
        base_manifest: &SyncManifest,
        local_root: &Path,
        remote_manifest: &SyncManifest,
    ) -> Result<SyncMergePlan> {
        let local_manifest = scan_directory(local_root, altair_vega::DEFAULT_SYNC_CHUNK_SIZE_BYTES)
            .with_context(|| format!("scan local sync root {}", local_root.display()))?;
        let plan = merge_manifests(base_manifest, &local_manifest, remote_manifest);

        for action in &plan.actions {
            match action {
                SyncAction::UpsertFile { path, entry } => {
                    let SyncEntryState::File(descriptor) = &entry.state else {
                        continue;
                    };
                    let target = sync_apply_target_path(local_root, path, entry)?;
                    self.fetch_descriptor_to_path(peer.clone(), descriptor, &target)
                        .await?;
                }
                SyncAction::RenamePath {
                    from_path,
                    to_path,
                    entry,
                } => {
                    let source = join_sync_path(local_root, from_path)?;
                    let target = sync_apply_target_path(local_root, to_path, entry)?;
                    if source.is_file() {
                        if let Some(parent) = target.parent() {
                            fs::create_dir_all(parent).with_context(|| {
                                format!("create sync rename target parent {}", parent.display())
                            })?;
                        }
                        fs::rename(&source, &target).with_context(|| {
                            format!(
                                "rename synced local file from {} to {}",
                                source.display(),
                                target.display()
                            )
                        })?;
                        prune_empty_parent_dirs(local_root, &source)?;
                    } else {
                        let SyncEntryState::File(descriptor) = &entry.state else {
                            continue;
                        };
                        self.fetch_descriptor_to_path(peer.clone(), descriptor, &target)
                            .await?;
                    }
                }
                SyncAction::DeletePath { path } => {
                    let target = join_sync_path(local_root, path)?;
                    if target.exists() {
                        if target.is_dir() {
                            continue;
                        }
                        fs::remove_file(&target).with_context(|| {
                            format!("remove synced local file {}", target.display())
                        })?;
                        prune_empty_parent_dirs(local_root, &target)?;
                    }
                }
                SyncAction::CreateConflictCopy {
                    conflict_path,
                    entry,
                    ..
                } => {
                    let SyncEntryState::File(descriptor) = &entry.state else {
                        continue;
                    };
                    self.fetch_descriptor_to_path(
                        peer.clone(),
                        descriptor,
                        &sync_apply_target_path(local_root, conflict_path, entry)?,
                    )
                    .await?;
                }
            }
        }

        Ok(plan)
    }

    pub async fn seed_local_from_manifest(
        &self,
        peer: iroh::EndpointAddr,
        local_root: &Path,
        remote_manifest: &SyncManifest,
    ) -> Result<usize> {
        let mut applied = 0usize;
        for entry in remote_manifest.entries.values() {
            match &entry.state {
                SyncEntryState::File(descriptor) => {
                    self.fetch_descriptor_to_path(
                        peer.clone(),
                        descriptor,
                        &sync_apply_target_path(local_root, &entry.path, entry)?,
                    )
                    .await?;
                    applied += 1;
                }
                SyncEntryState::Tombstone => {}
            }
        }
        Ok(applied)
    }
}

pub async fn write_manifest(
    doc: &Doc,
    author: iroh_docs::AuthorId,
    manifest: &SyncManifest,
) -> Result<()> {
    for entry in manifest.entries.values() {
        let key = manifest_key(&entry.path);
        let value = serde_json::to_vec(entry).context("serialize sync manifest entry")?;
        doc.set_bytes(author, key, value)
            .await
            .with_context(|| format!("set docs entry for {}", entry.path))?;
    }
    Ok(())
}

pub async fn read_manifest(blobs: &BlobsStore, doc: &Doc) -> Result<SyncManifest> {
    let query = Query::single_latest_per_key()
        .key_prefix(MANIFEST_PREFIX)
        .include_empty()
        .build();
    let stream = doc
        .get_many(query)
        .await
        .context("query docs manifest entries")?;
    tokio::pin!(stream);
    let mut entries = Vec::new();
    while let Some(item) = stream.next().await {
        let entry = item.context("read docs manifest entry")?;
        if entry.content_len() == 0 {
            continue;
        }
        let key = std::str::from_utf8(entry.key()).context("decode docs key as utf8")?;
        let _path = key
            .strip_prefix(MANIFEST_PREFIX)
            .ok_or_else(|| anyhow::anyhow!("docs entry outside manifest namespace"))?;
        let bytes = blobs
            .get_bytes(entry.content_hash())
            .await
            .context("load docs metadata blob")?;
        let sync_entry: SyncEntry =
            serde_json::from_slice(&bytes).context("deserialize docs sync manifest entry")?;
        entries.push(sync_entry);
    }
    Ok(SyncManifest::new(entries))
}

const MANIFEST_PREFIX: &str = "manifest/";
const PEER_TICKET_PREFIX: &str = "peer-ticket/";
const LOCAL_SYNC_USER_DATA_PREFIX: &str = "altair-vega:sync:";
const MAX_LOCAL_SYNC_CODE_BYTES: usize = 128;

pub fn local_sync_user_data_value(code: &str) -> String {
    format!("{LOCAL_SYNC_USER_DATA_PREFIX}{code}")
}

fn local_sync_user_data(code: &str) -> Result<UserData> {
    UserData::try_from(local_sync_user_data_value(code))
        .context("build local sync discovery metadata")
}

impl ProtocolHandler for LocalSyncTicketProtocol {
    async fn accept(&self, connection: Connection) -> std::result::Result<(), AcceptError> {
        let (mut send, mut recv) = connection.accept_bi().await?;
        let request = recv
            .read_to_end(MAX_LOCAL_SYNC_CODE_BYTES)
            .await
            .map_err(map_accept_error)?;
        let requested_code = String::from_utf8(request).map_err(map_accept_error)?;
        let ticket = {
            let state = self
                .ticket
                .read()
                .map_err(|_| map_accept_error("lock poisoned"))?;
            match state.as_ref() {
                Some(ticket) if ticket.code == requested_code => ticket.ticket.clone(),
                _ => return Err(map_accept_error("local sync code is not available")),
            }
        };
        send.write_all(ticket.as_bytes())
            .await
            .map_err(map_accept_error)?;
        send.finish()?;
        connection.closed().await;
        Ok(())
    }
}

fn map_accept_error(err: impl std::fmt::Display) -> AcceptError {
    std::io::Error::other(err.to_string()).into()
}

fn manifest_key(path: &str) -> String {
    format!("{MANIFEST_PREFIX}{path}")
}

pub fn summarize_manifest(manifest: &SyncManifest) -> Vec<String> {
    manifest
        .entries
        .values()
        .map(|entry| match &entry.state {
            SyncEntryState::File(descriptor) => format!(
                "file {} {} {:02x?}",
                entry.path,
                descriptor.size_bytes,
                &descriptor.hash[..4]
            ),
            SyncEntryState::Tombstone => format!("tombstone {}", entry.path),
        })
        .collect()
}

async fn preload_manifest_blobs(
    blobs: &BlobsStore,
    root: &Path,
    manifest: &SyncManifest,
) -> Result<usize> {
    let mut count = 0usize;
    for entry in manifest.entries.values() {
        let SyncEntryState::File(descriptor) = &entry.state else {
            continue;
        };
        let path = root
            .join(&entry.path)
            .canonicalize()
            .with_context(|| format!("canonicalize sync content file {}", entry.path))?;
        let display_path = path.display().to_string();
        let tag = blobs
            .add_path_with_opts(AddPathOptions {
                path,
                format: BlobFormat::Raw,
                mode: ImportMode::Copy,
            })
            .await
            .with_context(|| format!("add sync content blob {display_path}"))?;
        if tag.hash != descriptor.hash.into() {
            bail!(
                "blob hash for {} does not match sync descriptor",
                entry.path
            );
        }
        count += 1;
    }
    Ok(count)
}

fn prune_empty_parent_dirs(root: &Path, path: &Path) -> Result<()> {
    let mut current = path.parent();
    while let Some(dir) = current {
        if dir == root {
            break;
        }
        if fs::read_dir(dir)
            .with_context(|| format!("read parent dir {}", dir.display()))?
            .next()
            .is_some()
        {
            break;
        }
        fs::remove_dir(dir).with_context(|| format!("remove empty dir {}", dir.display()))?;
        current = dir.parent();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::DocsSyncNode;
    use altair_vega::{
        DEFAULT_SYNC_CHUNK_SIZE_BYTES, SyncAction, SyncEntryState, SyncManifest,
        conflict_copy_path, scan_directory,
    };
    use anyhow::Result;
    use tempfile::TempDir;
    use tokio::time::{Duration, sleep};

    #[tokio::test]
    async fn docs_export_import_and_fetch_round_trip() -> Result<()> {
        let temp = TempDir::new()?;
        let remote_root = temp.path().join("remote");
        let output_root = temp.path().join("output");
        std::fs::create_dir_all(&remote_root)?;
        std::fs::create_dir_all(&output_root)?;
        std::fs::write(remote_root.join("readme.txt"), b"hello docs bridge\n")?;

        let server = DocsSyncNode::spawn_persistent(&temp.path().join("server-state")).await?;
        let export = server
            .export_directory(&remote_root, DEFAULT_SYNC_CHUNK_SIZE_BYTES)
            .await?;

        let client = DocsSyncNode::spawn_persistent(&temp.path().join("client-state")).await?;
        let manifest = client.import_manifest(&export.ticket, 1500).await?;
        assert_eq!(manifest.len(), 1);
        assert!(manifest.get("readme.txt").is_some());

        client
            .fetch_path_from_ticket(&export.ticket, "readme.txt", &output_root, 1500)
            .await?;
        assert_eq!(
            std::fs::read(output_root.join("readme.txt"))?,
            b"hello docs bridge\n"
        );

        client.shutdown().await?;
        server.shutdown().await?;
        Ok(())
    }

    #[tokio::test]
    async fn docs_apply_propagates_add_and_delete() -> Result<()> {
        let temp = TempDir::new()?;
        let base_root = temp.path().join("base");
        let local_root = temp.path().join("local");
        let remote_root = temp.path().join("remote");
        std::fs::create_dir_all(&base_root)?;
        std::fs::create_dir_all(&local_root)?;
        std::fs::create_dir_all(&remote_root)?;
        std::fs::write(base_root.join("keep.txt"), b"keep\n")?;
        std::fs::write(base_root.join("drop.txt"), b"drop\n")?;
        std::fs::copy(base_root.join("keep.txt"), local_root.join("keep.txt"))?;
        std::fs::copy(base_root.join("drop.txt"), local_root.join("drop.txt"))?;
        std::fs::copy(base_root.join("keep.txt"), remote_root.join("keep.txt"))?;
        std::fs::write(remote_root.join("add.txt"), b"added\n")?;

        let server = DocsSyncNode::spawn_persistent(&temp.path().join("server-state")).await?;
        let export = server
            .export_directory(&remote_root, DEFAULT_SYNC_CHUNK_SIZE_BYTES)
            .await?;
        let client = DocsSyncNode::spawn_persistent(&temp.path().join("client-state")).await?;
        let plan = client
            .apply_ticket_merge(&export.ticket, &base_root, &local_root, 1500)
            .await?;

        assert_eq!(plan.actions.len(), 2);
        assert_eq!(std::fs::read(local_root.join("add.txt"))?, b"added\n");
        assert!(!local_root.join("drop.txt").exists());

        client.shutdown().await?;
        server.shutdown().await?;
        Ok(())
    }

    #[tokio::test]
    async fn docs_remote_conflict_creates_conflict_copy() -> Result<()> {
        let temp = TempDir::new()?;
        let remote_root = temp.path().join("remote");
        let local_root = temp.path().join("local");
        std::fs::create_dir_all(&remote_root)?;
        std::fs::create_dir_all(&local_root)?;
        std::fs::write(remote_root.join("readme.txt"), b"base\n")?;

        let server = DocsSyncNode::spawn_persistent(&temp.path().join("server-state")).await?;
        let export = server
            .export_directory(&remote_root, DEFAULT_SYNC_CHUNK_SIZE_BYTES)
            .await?;
        let doc = server.open_doc(&export.doc_id).await?;

        let client = DocsSyncNode::spawn_persistent(&temp.path().join("client-state")).await?;
        let imported = client.import_doc(&export.ticket).await?;
        let initial_remote = wait_for_manifest(&client, &imported.doc, 1).await?;
        let seeded = client
            .seed_local_from_manifest(imported.peer.clone(), &local_root, &initial_remote)
            .await?;
        assert_eq!(seeded, 1);
        std::fs::write(local_root.join("readme.txt"), b"local change\n")?;
        std::fs::write(remote_root.join("readme.txt"), b"remote change\n")?;

        let remote_manifest = scan_directory(&remote_root, DEFAULT_SYNC_CHUNK_SIZE_BYTES)?;
        let (_, published_manifest) = server
            .publish_manifest(&doc, &remote_root, &export.manifest, &remote_manifest)
            .await?;

        let synced_remote =
            wait_for_specific_manifest(&client, &imported.doc, &published_manifest).await?;
        let plan = client
            .apply_remote_manifest(
                imported.peer.clone(),
                &initial_remote,
                &local_root,
                &synced_remote,
            )
            .await?;

        assert_eq!(plan.actions.len(), 1);
        assert_eq!(plan.conflicts.len(), 1);
        let conflict_action = &plan.actions[0];
        let conflict_path = match conflict_action {
            SyncAction::CreateConflictCopy { conflict_path, .. } => conflict_path,
            other => panic!("expected conflict copy action, got {other:?}"),
        };
        assert_eq!(
            std::fs::read(local_root.join("readme.txt"))?,
            b"local change\n"
        );
        assert_eq!(
            std::fs::read(local_root.join(conflict_path))?,
            b"remote change\n"
        );

        client.shutdown().await?;
        server.shutdown().await?;
        Ok(())
    }

    #[tokio::test]
    async fn docs_join_applies_missed_delete_after_later_publish() -> Result<()> {
        let temp = TempDir::new()?;
        let remote_root = temp.path().join("remote");
        let local_root = temp.path().join("local");
        std::fs::create_dir_all(&remote_root)?;
        std::fs::create_dir_all(&local_root)?;
        std::fs::write(remote_root.join("deleted.txt"), b"remove me\n")?;

        let server = DocsSyncNode::spawn_persistent(&temp.path().join("server-state")).await?;
        let export = server
            .export_directory(&remote_root, DEFAULT_SYNC_CHUNK_SIZE_BYTES)
            .await?;
        let doc = server.open_doc(&export.doc_id).await?;

        let client = DocsSyncNode::spawn_persistent(&temp.path().join("client-state")).await?;
        let imported = client.import_doc(&export.ticket).await?;
        let initial_remote =
            wait_for_specific_manifest(&client, &imported.doc, &export.manifest).await?;
        client
            .seed_local_from_manifest(imported.peer.clone(), &local_root, &initial_remote)
            .await?;
        assert_eq!(
            std::fs::read(local_root.join("deleted.txt"))?,
            b"remove me\n"
        );

        std::fs::remove_file(remote_root.join("deleted.txt"))?;
        let after_delete = scan_directory(&remote_root, DEFAULT_SYNC_CHUNK_SIZE_BYTES)?;
        let (_, deleted_manifest) = server
            .publish_manifest(&doc, &remote_root, &export.manifest, &after_delete)
            .await?;
        assert!(
            deleted_manifest
                .get("deleted.txt")
                .is_some_and(|entry| entry.is_tombstone())
        );

        std::fs::write(remote_root.join("added-after-delete.txt"), b"new file\n")?;
        let after_add = scan_directory(&remote_root, DEFAULT_SYNC_CHUNK_SIZE_BYTES)?;
        let (_, republished_manifest) = server
            .publish_manifest(&doc, &remote_root, &deleted_manifest, &after_add)
            .await?;
        assert!(
            republished_manifest
                .get("deleted.txt")
                .is_some_and(|entry| entry.is_tombstone())
        );

        let synced_remote =
            wait_for_specific_manifest(&client, &imported.doc, &republished_manifest).await?;
        let plan = client
            .apply_remote_manifest(
                imported.peer.clone(),
                &initial_remote,
                &local_root,
                &synced_remote,
            )
            .await?;

        assert!(plan.actions.iter().any(|action| matches!(
            action,
            SyncAction::DeletePath { path } if path == "deleted.txt"
        )));
        assert!(!local_root.join("deleted.txt").exists());
        assert_eq!(
            std::fs::read(local_root.join("added-after-delete.txt"))?,
            b"new file\n"
        );

        client.shutdown().await?;
        server.shutdown().await?;
        Ok(())
    }

    #[tokio::test]
    async fn docs_join_applies_remote_rename_without_refetching_as_add_delete() -> Result<()> {
        let temp = TempDir::new()?;
        let remote_root = temp.path().join("remote");
        let local_root = temp.path().join("local");
        std::fs::create_dir_all(&remote_root)?;
        std::fs::create_dir_all(&local_root)?;
        std::fs::write(remote_root.join("old.txt"), b"rename me\n")?;

        let server = DocsSyncNode::spawn_persistent(&temp.path().join("server-state")).await?;
        let export = server
            .export_directory(&remote_root, DEFAULT_SYNC_CHUNK_SIZE_BYTES)
            .await?;
        let doc = server.open_doc(&export.doc_id).await?;

        let client = DocsSyncNode::spawn_persistent(&temp.path().join("client-state")).await?;
        let imported = client.import_doc(&export.ticket).await?;
        let initial_remote =
            wait_for_specific_manifest(&client, &imported.doc, &export.manifest).await?;
        client
            .seed_local_from_manifest(imported.peer.clone(), &local_root, &initial_remote)
            .await?;

        std::fs::rename(remote_root.join("old.txt"), remote_root.join("new.txt"))?;
        let remote_renamed = scan_directory(&remote_root, DEFAULT_SYNC_CHUNK_SIZE_BYTES)?;
        let (_, remote_manifest) = server
            .publish_manifest(&doc, &remote_root, &export.manifest, &remote_renamed)
            .await?;
        let synced_remote =
            wait_for_specific_manifest(&client, &imported.doc, &remote_manifest).await?;

        let plan = client
            .apply_remote_manifest(
                imported.peer.clone(),
                &initial_remote,
                &local_root,
                &synced_remote,
            )
            .await?;

        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(
            &plan.actions[0],
            SyncAction::RenamePath { from_path, to_path, .. }
                if from_path == "old.txt" && to_path == "new.txt"
        ));
        assert!(!local_root.join("old.txt").exists());
        assert_eq!(std::fs::read(local_root.join("new.txt"))?, b"rename me\n");

        client.shutdown().await?;
        server.shutdown().await?;
        Ok(())
    }

    #[tokio::test]
    async fn docs_join_reconciles_remote_conflict_before_publishing_local() -> Result<()> {
        let temp = TempDir::new()?;
        let remote_root = temp.path().join("remote");
        let local_root = temp.path().join("local");
        std::fs::create_dir_all(&remote_root)?;
        std::fs::create_dir_all(&local_root)?;
        std::fs::write(remote_root.join("readme.txt"), b"base\n")?;

        let server = DocsSyncNode::spawn_persistent(&temp.path().join("server-state")).await?;
        let export = server
            .export_directory(&remote_root, DEFAULT_SYNC_CHUNK_SIZE_BYTES)
            .await?;
        let doc = server.open_doc(&export.doc_id).await?;

        let client = DocsSyncNode::spawn_persistent(&temp.path().join("client-state")).await?;
        let imported = client.import_doc(&export.ticket).await?;
        let initial_remote =
            wait_for_specific_manifest(&client, &imported.doc, &export.manifest).await?;
        client
            .seed_local_from_manifest(imported.peer.clone(), &local_root, &initial_remote)
            .await?;

        std::fs::write(local_root.join("readme.txt"), b"local change\n")?;
        std::fs::write(remote_root.join("readme.txt"), b"remote change\n")?;
        let remote_changed = scan_directory(&remote_root, DEFAULT_SYNC_CHUNK_SIZE_BYTES)?;
        let (_, remote_manifest) = server
            .publish_manifest(&doc, &remote_root, &export.manifest, &remote_changed)
            .await?;
        let synced_remote =
            wait_for_specific_manifest(&client, &imported.doc, &remote_manifest).await?;

        let plan = client
            .apply_remote_manifest(
                imported.peer.clone(),
                &initial_remote,
                &local_root,
                &synced_remote,
            )
            .await?;
        assert_eq!(plan.conflicts.len(), 1);
        let conflict_path = match &plan.actions[0] {
            SyncAction::CreateConflictCopy { conflict_path, .. } => conflict_path,
            other => panic!("expected conflict copy action, got {other:?}"),
        };
        assert_eq!(
            std::fs::read(local_root.join(conflict_path))?,
            b"remote change\n"
        );
        assert_eq!(
            std::fs::read(local_root.join("readme.txt"))?,
            b"local change\n"
        );

        let local_after_conflict = scan_directory(&local_root, DEFAULT_SYNC_CHUNK_SIZE_BYTES)?;
        let (_, published_local) = client
            .publish_manifest(
                &imported.doc,
                &local_root,
                &synced_remote,
                &local_after_conflict,
            )
            .await?;
        assert!(published_local.get(conflict_path).is_none());
        assert_eq!(
            published_local.get("readme.txt"),
            local_after_conflict.get("readme.txt")
        );

        client.shutdown().await?;
        server.shutdown().await?;
        Ok(())
    }

    #[tokio::test]
    async fn docs_join_keeps_local_edit_when_remote_deletes() -> Result<()> {
        let temp = TempDir::new()?;
        let remote_root = temp.path().join("remote");
        let local_root = temp.path().join("local");
        std::fs::create_dir_all(&remote_root)?;
        std::fs::create_dir_all(&local_root)?;
        std::fs::write(remote_root.join("readme.txt"), b"base\n")?;

        let server = DocsSyncNode::spawn_persistent(&temp.path().join("server-state")).await?;
        let export = server
            .export_directory(&remote_root, DEFAULT_SYNC_CHUNK_SIZE_BYTES)
            .await?;
        let doc = server.open_doc(&export.doc_id).await?;

        let client = DocsSyncNode::spawn_persistent(&temp.path().join("client-state")).await?;
        let imported = client.import_doc(&export.ticket).await?;
        let initial_remote =
            wait_for_specific_manifest(&client, &imported.doc, &export.manifest).await?;
        client
            .seed_local_from_manifest(imported.peer.clone(), &local_root, &initial_remote)
            .await?;

        std::fs::write(local_root.join("readme.txt"), b"local edit\n")?;
        std::fs::remove_file(remote_root.join("readme.txt"))?;
        let remote_deleted = scan_directory(&remote_root, DEFAULT_SYNC_CHUNK_SIZE_BYTES)?;
        let (_, remote_manifest) = server
            .publish_manifest(&doc, &remote_root, &export.manifest, &remote_deleted)
            .await?;
        let synced_remote =
            wait_for_specific_manifest(&client, &imported.doc, &remote_manifest).await?;

        let plan = client
            .apply_remote_manifest(
                imported.peer.clone(),
                &initial_remote,
                &local_root,
                &synced_remote,
            )
            .await?;

        assert!(plan.actions.is_empty());
        assert_eq!(plan.conflicts.len(), 1);
        assert_eq!(
            std::fs::read(local_root.join("readme.txt"))?,
            b"local edit\n"
        );

        client.shutdown().await?;
        server.shutdown().await?;
        Ok(())
    }

    #[tokio::test]
    async fn docs_join_preserves_local_delete_as_remote_conflict_copy() -> Result<()> {
        let temp = TempDir::new()?;
        let remote_root = temp.path().join("remote");
        let local_root = temp.path().join("local");
        std::fs::create_dir_all(&remote_root)?;
        std::fs::create_dir_all(&local_root)?;
        std::fs::write(remote_root.join("readme.txt"), b"base\n")?;

        let server = DocsSyncNode::spawn_persistent(&temp.path().join("server-state")).await?;
        let export = server
            .export_directory(&remote_root, DEFAULT_SYNC_CHUNK_SIZE_BYTES)
            .await?;
        let doc = server.open_doc(&export.doc_id).await?;

        let client = DocsSyncNode::spawn_persistent(&temp.path().join("client-state")).await?;
        let imported = client.import_doc(&export.ticket).await?;
        let initial_remote =
            wait_for_specific_manifest(&client, &imported.doc, &export.manifest).await?;
        client
            .seed_local_from_manifest(imported.peer.clone(), &local_root, &initial_remote)
            .await?;

        std::fs::remove_file(local_root.join("readme.txt"))?;
        std::fs::write(remote_root.join("readme.txt"), b"remote edit\n")?;
        let remote_changed = scan_directory(&remote_root, DEFAULT_SYNC_CHUNK_SIZE_BYTES)?;
        let (_, remote_manifest) = server
            .publish_manifest(&doc, &remote_root, &export.manifest, &remote_changed)
            .await?;
        let synced_remote =
            wait_for_specific_manifest(&client, &imported.doc, &remote_manifest).await?;

        let plan = client
            .apply_remote_manifest(
                imported.peer.clone(),
                &initial_remote,
                &local_root,
                &synced_remote,
            )
            .await?;
        assert_eq!(plan.conflicts.len(), 1);
        let conflict_path = match &plan.actions[0] {
            SyncAction::CreateConflictCopy { conflict_path, .. } => conflict_path,
            other => panic!("expected conflict copy action, got {other:?}"),
        };

        assert!(!local_root.join("readme.txt").exists());
        assert_eq!(
            std::fs::read(local_root.join(conflict_path))?,
            b"remote edit\n"
        );

        client.shutdown().await?;
        server.shutdown().await?;
        Ok(())
    }

    #[tokio::test]
    async fn docs_join_writes_remote_file_directory_collision_as_conflict_copy() -> Result<()> {
        let temp = TempDir::new()?;
        let remote_root = temp.path().join("remote");
        let local_root = temp.path().join("local");
        std::fs::create_dir_all(&remote_root)?;
        std::fs::create_dir_all(local_root.join("docs.txt"))?;
        std::fs::write(local_root.join("docs.txt/local.txt"), b"local dir file\n")?;
        std::fs::write(remote_root.join("docs.txt"), b"remote file\n")?;

        let server = DocsSyncNode::spawn_persistent(&temp.path().join("server-state")).await?;
        let export = server
            .export_directory(&remote_root, DEFAULT_SYNC_CHUNK_SIZE_BYTES)
            .await?;

        let client = DocsSyncNode::spawn_persistent(&temp.path().join("client-state")).await?;
        let imported = client.import_doc(&export.ticket).await?;
        let remote_manifest =
            wait_for_specific_manifest(&client, &imported.doc, &export.manifest).await?;
        let plan = client
            .apply_remote_manifest(
                imported.peer.clone(),
                &SyncManifest::default(),
                &local_root,
                &remote_manifest,
            )
            .await?;

        assert_eq!(plan.actions.len(), 1);
        let remote_entry = remote_manifest.get("docs.txt").unwrap();
        let conflict_path = conflict_copy_path("docs.txt", remote_entry);
        assert_eq!(
            std::fs::read(local_root.join("docs.txt/local.txt"))?,
            b"local dir file\n"
        );
        assert_eq!(
            std::fs::read(local_root.join(conflict_path))?,
            b"remote file\n"
        );

        client.shutdown().await?;
        server.shutdown().await?;
        Ok(())
    }

    #[tokio::test]
    async fn docs_join_writes_remote_nested_file_parent_collision_as_conflict_copy() -> Result<()> {
        let temp = TempDir::new()?;
        let remote_root = temp.path().join("remote");
        let local_root = temp.path().join("local");
        std::fs::create_dir_all(remote_root.join("docs"))?;
        std::fs::create_dir_all(&local_root)?;
        std::fs::write(local_root.join("docs"), b"local parent file\n")?;
        std::fs::write(remote_root.join("docs/readme.txt"), b"remote nested file\n")?;

        let server = DocsSyncNode::spawn_persistent(&temp.path().join("server-state")).await?;
        let export = server
            .export_directory(&remote_root, DEFAULT_SYNC_CHUNK_SIZE_BYTES)
            .await?;

        let client = DocsSyncNode::spawn_persistent(&temp.path().join("client-state")).await?;
        let imported = client.import_doc(&export.ticket).await?;
        let remote_manifest =
            wait_for_specific_manifest(&client, &imported.doc, &export.manifest).await?;
        let plan = client
            .apply_remote_manifest(
                imported.peer.clone(),
                &SyncManifest::default(),
                &local_root,
                &remote_manifest,
            )
            .await?;

        assert_eq!(plan.actions.len(), 1);
        let remote_entry = remote_manifest.get("docs/readme.txt").unwrap();
        let conflict_path = conflict_copy_path("docs__readme.txt", remote_entry);
        assert_eq!(
            std::fs::read(local_root.join("docs"))?,
            b"local parent file\n"
        );
        assert_eq!(
            std::fs::read(local_root.join(conflict_path))?,
            b"remote nested file\n"
        );

        client.shutdown().await?;
        server.shutdown().await?;
        Ok(())
    }

    #[tokio::test]
    async fn docs_join_restart_uses_saved_base_without_reconflicting() -> Result<()> {
        let temp = TempDir::new()?;
        let remote_root = temp.path().join("remote");
        let local_root = temp.path().join("local");
        let client_state = temp.path().join("client-state");
        std::fs::create_dir_all(&remote_root)?;
        std::fs::create_dir_all(&local_root)?;
        std::fs::write(remote_root.join("readme.txt"), b"base\n")?;

        let server = DocsSyncNode::spawn_persistent(&temp.path().join("server-state")).await?;
        let export = server
            .export_directory(&remote_root, DEFAULT_SYNC_CHUNK_SIZE_BYTES)
            .await?;
        let doc = server.open_doc(&export.doc_id).await?;

        let client = DocsSyncNode::spawn_persistent(&client_state).await?;
        let imported = client.import_doc(&export.ticket).await?;
        let initial_remote =
            wait_for_specific_manifest(&client, &imported.doc, &export.manifest).await?;
        client
            .seed_local_from_manifest(imported.peer.clone(), &local_root, &initial_remote)
            .await?;
        client.shutdown().await?;

        std::fs::write(remote_root.join("readme.txt"), b"remote after restart\n")?;
        let remote_changed = scan_directory(&remote_root, DEFAULT_SYNC_CHUNK_SIZE_BYTES)?;
        let (_, remote_manifest) = server
            .publish_manifest(&doc, &remote_root, &export.manifest, &remote_changed)
            .await?;

        let restarted = DocsSyncNode::spawn_persistent(&client_state).await?;
        let restarted_import = restarted.import_doc(&export.ticket).await?;
        let synced_remote =
            wait_for_specific_manifest(&restarted, &restarted_import.doc, &remote_manifest).await?;
        let plan = restarted
            .apply_remote_manifest(
                restarted_import.peer.clone(),
                &initial_remote,
                &local_root,
                &synced_remote,
            )
            .await?;

        assert_eq!(plan.actions.len(), 1);
        assert!(plan.conflicts.is_empty());
        assert_eq!(
            std::fs::read(local_root.join("readme.txt"))?,
            b"remote after restart\n"
        );

        restarted.shutdown().await?;
        server.shutdown().await?;
        Ok(())
    }

    #[tokio::test]
    async fn docs_host_restart_reuses_existing_document_namespace() -> Result<()> {
        let temp = TempDir::new()?;
        let remote_root = temp.path().join("remote");
        std::fs::create_dir_all(&remote_root)?;
        std::fs::write(remote_root.join("readme.txt"), b"base\n")?;

        let server_state = temp.path().join("server-state");
        let first_server = DocsSyncNode::spawn_persistent(&server_state).await?;
        let first_manifest = scan_directory(&remote_root, DEFAULT_SYNC_CHUNK_SIZE_BYTES)?;
        let first_export = first_server
            .export_directory(&remote_root, DEFAULT_SYNC_CHUNK_SIZE_BYTES)
            .await?;
        first_server.shutdown().await?;

        std::fs::write(remote_root.join("readme.txt"), b"after restart\n")?;
        let restarted_server = DocsSyncNode::spawn_persistent(&server_state).await?;
        let doc = restarted_server.open_doc(&first_export.doc_id).await?;
        let second_manifest = scan_directory(&remote_root, DEFAULT_SYNC_CHUNK_SIZE_BYTES)?;
        let second_export = restarted_server
            .export_existing_manifest(&doc, &remote_root, &first_manifest, second_manifest)
            .await?;

        assert_eq!(second_export.doc_id, first_export.doc_id);
        assert_eq!(
            second_export
                .manifest
                .get("readme.txt")
                .and_then(|entry| match &entry.state {
                    SyncEntryState::File(descriptor) => Some(descriptor.hash),
                    SyncEntryState::Tombstone => None,
                }),
            scan_directory(&remote_root, DEFAULT_SYNC_CHUNK_SIZE_BYTES)?
                .get("readme.txt")
                .and_then(|entry| match &entry.state {
                    SyncEntryState::File(descriptor) => Some(descriptor.hash),
                    SyncEntryState::Tombstone => None,
                })
        );

        restarted_server.shutdown().await?;
        Ok(())
    }

    async fn wait_for_manifest(
        client: &DocsSyncNode,
        doc: &iroh_docs::api::Doc,
        expected_entries: usize,
    ) -> Result<altair_vega::SyncManifest> {
        for _ in 0..20 {
            match client.read_doc_manifest(doc).await {
                Ok(manifest) if manifest.len() >= expected_entries => return Ok(manifest),
                Ok(_) | Err(_) => {
                    sleep(Duration::from_millis(250)).await;
                }
            }
        }
        anyhow::bail!("timed out waiting for manifest entries")
    }

    async fn wait_for_specific_manifest(
        client: &DocsSyncNode,
        doc: &iroh_docs::api::Doc,
        expected: &altair_vega::SyncManifest,
    ) -> Result<altair_vega::SyncManifest> {
        for _ in 0..20 {
            match client.read_doc_manifest(doc).await {
                Ok(manifest) if altair_vega::manifests_state_eq(&manifest, expected) => {
                    return Ok(manifest);
                }
                Ok(_) | Err(_) => {
                    sleep(Duration::from_millis(250)).await;
                }
            }
        }
        anyhow::bail!("timed out waiting for specific manifest state")
    }
}
