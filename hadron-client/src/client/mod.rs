//! Hadron client.

pub mod pipeline;
pub mod publisher;
pub mod subscriber;

use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::{Context, Result};
use arc_swap::ArcSwap;
use futures::prelude::*;
use tokio::sync::{broadcast, watch};
use tokio_stream::wrappers::BroadcastStream;
use tonic::transport::{Channel, Endpoint};
use tonic::Request;
use tower::discover::Change;

use crate::common::ClientCreds;
use crate::grpc::stream::stream_controller_client::StreamControllerClient;
use crate::grpc::stream::{MetadataRequest, MetadataResponse};

type ConnsMap = BTreeMap<u32, StreamControllerClient<Channel>>;
pub type ArcSwapConns = Arc<ArcSwap<ConnsMap>>;
type ChangeBuf = Vec<Change<u32, StreamControllerClient<Channel>>>;
type EndpointChangeTx = watch::Sender<(Option<Change<u32, StreamControllerClient<Channel>>>, Arc<ConnsMap>)>;
type EndpointChangeRx = watch::Receiver<(Option<Change<u32, StreamControllerClient<Channel>>>, Arc<ConnsMap>)>;

/// Hadron client.
#[derive(Clone)]
pub struct Client {
    pub(crate) inner: Arc<ClientInner>,
}

/// Client internal state.
pub(crate) struct ClientInner {
    pub(crate) creds: ClientCreds,

    /// All active Stream connections.
    pub(crate) conns: ArcSwapConns,
    /// A watch channel for communicating Stream topology changes.
    pub(crate) changes: EndpointChangeRx,
    /// The channel used to shutdown all related sub-tasks.
    shutdown: broadcast::Sender<()>,
}

impl Drop for ClientInner {
    fn drop(&mut self) {
        let _res = self.shutdown.send(());
    }
}

impl Client {
    /// Construct a new client instance.
    ///
    /// ## parameters
    /// ### url
    /// The URL of the target Stream's Kubernetes Service.
    ///
    /// This may be either the internal or external URL. Use the internal URL when the client will
    /// be running inside of the same Kubernetes cluster as the Hadron workloads. Use the external
    /// URL when the client is not running within the same Kubernetes cluster.
    ///
    /// ### mode
    /// The internal or external mode of this client.
    ///
    /// If the client will be running within the same Kubernetes cluster as the Hadron cluster,
    /// then use `Mode::Internal`, else `Mode::External`.
    ///
    /// ### creds
    /// The client credentials to use for connecting to the Hadron cluster.
    pub fn new(url: &str, mode: Mode, creds: ClientCreds) -> Result<Self> {
        let (shutdown, shutdown_rx) = broadcast::channel(1);
        let (changes_tx, changes) = watch::channel((None, Default::default()));
        let conns: ArcSwapConns = Default::default();

        let metadata_chan = Endpoint::from_str(url).context("invalid URL for Hadron Stream connection")?;
        tokio::spawn(metadata_task(metadata_chan, changes_tx, conns.clone(), creds.clone(), mode, shutdown_rx));
        Ok(Self {
            inner: Arc::new(ClientInner { creds, conns, changes, shutdown }),
        })
    }
}

/// The client connection mode used for connecting with Hadron.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Mode {
    /// The client is running inside of the same Kubernetes cluster as the Hadron cluster.
    Internal,
    /// The client is running outside of the Hadron cluster's Kubernetes cluster.
    External,
}

async fn metadata_task(metadata_endpoint: Endpoint, mut tx: EndpointChangeTx, conns: ArcSwapConns, creds: ClientCreds, mode: Mode, shutdown: broadcast::Receiver<()>) {
    // Establish initial metadata stream.
    let mut shutdown = BroadcastStream::new(shutdown);
    let mut changes: ChangeBuf = Vec::with_capacity(50); // A buffer used to collect change batches.
    let (key, val) = creds.header();
    loop {
        let mut req = Request::new(MetadataRequest {});
        req.metadata_mut().insert(key.clone(), val.clone());
        let mut stream = match try_metadata_task(metadata_endpoint.clone(), req).await {
            Ok(stream) => stream,
            Err(err) => {
                tracing::error!(error = ?err, "error opening metadata stream with Hadron cluster");
                let delay = tokio::time::sleep(std::time::Duration::from_secs(5));
                tokio::select! {
                    _ = shutdown.next() => return,
                    _ = delay => continue,
                }
            }
        };

        // Listen on channels and perform needed actions.
        loop {
            tokio::select! {
                msg_opt = stream.next() => match msg_opt {
                    Some(Ok(update)) => diff_and_update_endpoints(update, conns.clone(), &mut changes, mode, &mut tx).await,
                    Some(Err(err)) => {
                        tracing::error!(error = ?err, "error received from metadata stream");
                        break;
                    },
                    None => break,
                },
                _ = shutdown.next() => return,
            }
        }
    }
}

async fn try_metadata_task(metadata_endpoint: Endpoint, req: Request<MetadataRequest>) -> Result<tonic::Streaming<MetadataResponse>> {
    let mut conn = StreamControllerClient::connect(metadata_endpoint.clone())
        .await
        .context("error establishing connection to Hadron cluster for metadata connection")?;
    let metadata = conn.metadata(req).await.context("error opening metadata stream with Hadron cluster")?.into_inner();
    Ok(metadata)
}

/// Diff and update connection endpoints.
#[tracing::instrument(level = "debug", skip(updated, arc_conns, changes, mode, tx))]
async fn diff_and_update_endpoints(updated: MetadataResponse, arc_conns: ArcSwapConns, changes: &mut ChangeBuf, mode: Mode, tx: &mut EndpointChangeTx) {
    tracing::debug!("metadata update received");

    // Look for new endpoints.
    let conns = arc_conns.load_full();
    for partition in updated.partitions.iter() {
        if conns.contains_key(&partition.partition) {
            continue;
        }
        let new_endpoint_str = match mode {
            Mode::Internal => partition.internal.as_str(),
            Mode::External => partition.external.as_str(),
        };
        let endpoint = match Endpoint::from_shared(format!("http://{}", new_endpoint_str)) {
            Ok(endpoint) => endpoint,
            Err(err) => {
                tracing::error!(error = ?err, "invalid endpoint received on metadata stream");
                continue;
            }
        };
        let chan = endpoint.connect_lazy();
        let conn = StreamControllerClient::new(chan);
        changes.push(Change::Insert(partition.partition, conn.clone()));
        tracing::debug!(endpoint = new_endpoint_str, "new endpoint connection established");
    }

    // Look for old endpoints to remove.
    for offset in conns.keys() {
        let exists = updated.partitions.iter().any(|partition| &partition.partition == offset);
        if !exists {
            changes.push(Change::Remove(*offset));
            tracing::debug!(partition = offset, "pruning old connection");
        }
    }

    // Emit changes. Each change ships with the final snapshot of connections for this full changeset.
    if !changes.is_empty() {
        let mut updated = conns.as_ref().clone();
        for change in changes.iter() {
            match change {
                Change::Insert(idx, conn) => {
                    updated.insert(*idx, conn.clone());
                }
                Change::Remove(idx) => {
                    updated.remove(idx);
                }
            }
        }
        let updated = Arc::new(updated);
        arc_conns.store(updated.clone());
        for change in changes.drain(..) {
            match change {
                Change::Insert(idx, conn) => {
                    let _res = tx.send((Some(Change::Insert(idx, conn)), updated.clone()));
                }
                Change::Remove(idx) => {
                    let _res = tx.send((Some(Change::Remove(idx)), updated.clone()));
                }
            }
        }
    }
}
