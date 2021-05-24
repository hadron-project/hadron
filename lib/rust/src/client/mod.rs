//! The core Hadron client.

mod auth;
mod pipeline;
mod publisher;
mod schema;
mod subscriber;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use bytes::{Bytes, BytesMut};
use h2::client::{handshake, SendRequest};
use http::request::Builder;
use http::{HeaderValue, StatusCode, Uri};
use prost::Message;
use proto::v1;
use tokio::net::TcpStream;
use tokio::sync::{watch, RwLock};

use crate::common::{H2Channel, H2DataChannel};
pub use pipeline::PipelineSubscription;
pub use subscriber::{SubscriberConfig, Subscription, SubscriptionStartingPoint};

pub(crate) type ConnectionMap = Arc<RwLock<HashMap<Arc<String>, Connection>>>;

/// A client for communicating with a Hadron cluster.
///
/// The client is cheap to clone, every clone uses the same underlying multiplexed connection,
/// and clients may be freely passed between threads.
///
/// ## Initialization
/// The client is initialized with a connection string for any replica set of the cluster, or even
/// a DNS name with IP entries for multiple/all replica sets of the cluster. The initial cluster
/// connection is handled in an asynchronous fashion, and if the original server connection is
/// ever lost, the client will attempt to reconnect using the originally provided URL.
///
/// Once a successful connection is established, the client will fetch metadata from the connected
/// server, and will establish client connections to other replica sets of the cluster as needed
/// using the information returned in the metadata payload.
///
/// Clients also setup a dedicated H2 channel on the original server connection used to stream in
/// metadata updates, which the server will push to its connected clients.
#[derive(Clone)]
pub struct Client(pub(crate) Arc<ClientInner>);

/// The inner state of the client.
pub(crate) struct ClientInner {
    /// The original URL provided for establishing a cluster connection.
    pub(crate) url: Arc<String>,
    /// The credentials associated with this client.
    pub(crate) creds: ClientCreds,
    /// A map of all Hadron connections along with their state.
    pub(crate) connections: ConnectionMap,
    /// A general purpose bytes buffer for use by the client.
    pub(crate) buf: BytesMut,
}

impl Client {
    /// Construct a new client instance establishing an initial Hadron cluster connection using
    /// the given URL.
    pub async fn new(url: &str, creds: ClientCreds) -> Result<Self> {
        let _uri: Uri = url.parse().context("failed to parse URL")?;
        let connections = Default::default();
        let inner = ClientInner {
            url: Arc::new(url.to_string()),
            creds,
            connections,
            buf: BytesMut::with_capacity(5000),
        };
        let this = Self(Arc::new(inner));
        this.spawn_connection_builder(this.0.url.clone(), false).await;
        Ok(this)
    }

    /// Set the credentials for the request based on the client's current credentials.
    pub(crate) fn set_request_credentials(&self, builder: Builder) -> Builder {
        match &self.0.creds {
            ClientCreds::Token(_, header) | ClientCreds::User(_, _, header) => builder.header("authorization", header),
        }
    }

    /// Get a cloned handle of the H2 channel to the target node, defaulting to the client's
    /// originally connected node.
    ///
    /// This method will ensure that the H2 channel is ready for use before returning it.
    ///
    /// NOTE WELL: calls to this method do not come directly from client users, they come about
    /// through internal algorithms. This method is NOT responsible for establishing new
    /// connections. It is only responsible for getting a handle to an active connection, or
    /// triggering a reconnect if needed.
    #[tracing::instrument(level = "debug", skip(self, node))]
    pub(crate) async fn get_channel(&self, node: Option<Arc<String>>) -> Result<H2Channel> {
        // First we perform an optimistic read. If everything is connected, then this will be all.
        let node = node.unwrap_or_else(|| self.0.url.clone());
        {
            // Get a handle to the target connection.
            let conns = self.0.connections.read().await;
            let conn = match conns.get(&self.0.url) {
                Some(conn) => conn,
                None => anyhow::bail!("no connection to target node {}", node),
            };
            // If we have a live connection, test it and return it. Else, we continue to the write
            // path below where we may need to drive a reconnect.
            if let ConnectionState::Connected(h2) = &conn.state {
                let h2_res = h2.clone().ready().await.context("error testing H2 connection to hadron server");
                match h2_res {
                    Ok(h2) => return Ok(h2),
                    Err(err) => {
                        tracing::error!(error = ?err);
                    }
                }
            }
        }

        // We were not able to get a handle to the connection, as it is pending or already
        // connecting. Get a write lock so that we can update the connection state.
        let mut rx = {
            let mut conns = self.0.connections.write().await;
            let mut conn = match conns.get_mut(&self.0.url) {
                Some(conn) => conn,
                None => anyhow::bail!("no connection to target node {}", node),
            };

            // Start a reconnect if needed.
            match conn.state {
                // Start building a new connection. State connected should never be hit as it is covered
                // by the read path above.
                ConnectionState::Pending | ConnectionState::Connected(_) => {
                    conn.state = ConnectionState::Connecting;
                    tokio::spawn(establish_connection(self.0.connections.clone(), node.clone()));
                }
                ConnectionState::Connecting => (),
            }
            conn.conn_updates_rx.clone()
        };

        // Wait on the connection's notification channel, then we attempt to read the value one last time.
        let _ = tokio::time::timeout(Duration::from_secs(10), rx.changed()).await;
        let conns = self.0.connections.read().await;
        let conn = match conns.get(&self.0.url) {
            Some(conn) => conn,
            None => anyhow::bail!("no connection to target node {}", node),
        };
        let h2 = match &conn.state {
            ConnectionState::Connected(h2) => h2.clone(),
            _ => anyhow::bail!("no connection to target node {}", node),
        };
        h2.ready().await.context("error testing H2 connection to hadron server")
    }

    /// Deserialize the given response body as a concrete type, or an error depending
    /// on the status code.
    #[tracing::instrument(level = "debug", skip(self, buf))]
    pub(crate) fn deserialize_response_or_error<M: Message + Default>(&self, status: StatusCode, buf: Bytes) -> Result<M> {
        // If not a successful response, then interpret the message as an error.
        if !status.is_success() {
            let body_err = v1::Error::decode(buf.as_ref()).context("failed to deserialize error message from body")?;
            return Err(anyhow!(body_err.message));
        }
        M::decode(buf.as_ref()).context("error decoding response body")
    }

    /// Spawn a new task to build a connection to the target node.
    ///
    /// If `set_disconnected` is `true` then the corresponding connection state will be marked as
    /// disconnected before beginning to build a new connection.
    #[allow(clippy::needless_return)]
    async fn spawn_connection_builder(&self, target: Arc<String>, set_disconnected: bool) {
        {
            // Get a handle to or initialize a new connection state object.
            let mut conns = self.0.connections.write().await;
            let mut conn = conns.entry(target.clone()).or_insert_with(|| Connection::new(target.clone()));
            match &conn.state {
                // If a new connection is already being established elsewhere, then do nothing.
                ConnectionState::Connecting => return,
                // If we already have a live connection and we do not need to drop it, then do nothing.
                ConnectionState::Connected(_) if !set_disconnected => return,
                ConnectionState::Connected(_) | ConnectionState::Pending => {
                    conn.state = ConnectionState::Connecting;
                }
            }
        }

        // Spawn a routine to establish the connection.
        tokio::spawn(establish_connection(self.0.connections.clone(), target));
    }
}

/// Spawn a new task to build a connection to the given target.
async fn establish_connection(connections: ConnectionMap, target: Arc<String>) {
    tracing::debug!(node = %&target, "establishing new connection to hadron node");
    // Establish H2 connection.
    let h2_res = establish_h2_connection_with_backoff(target.clone()).await;
    let h2 = match h2_res {
        Ok(h2) => h2,
        Err(_) => {
            let mut conns = connections.write().await;
            if let Some(state) = conns.remove(&*target) {
                let _ = state.conn_updates_tx.send(());
            }
            return;
        }
    };

    // Update the connections map with the newly established connection.
    let mut conns = connections.write().await;
    if let Some(mut conn) = conns.get_mut(&target) {
        conn.state = ConnectionState::Connected(h2);
        let _ = conn.conn_updates_tx.send(());
    }
}

/// Establish a HTTP2 connection using an exponential backoff and retry.
///
/// This never stop attempting to reconnect, unless the target node disappears from the client's
/// metadata system. In the case of this being the initial cluster connection of a client, the
/// reconnect attempts will continue indefinitely. This tends to align with the practical use case
/// where the initial connection would only retry indefinitely if the client was misconfigured, in
/// which case the parent application would need to be restarted.
///
/// This routine will only return an error when it has determined that retries should no longer
/// take place, and the corresponding connection state info should be removed from the parent client.
async fn establish_h2_connection_with_backoff(target: Arc<String>) -> Result<H2Channel> {
    let backoff = backoff::ExponentialBackoff {
        max_elapsed_time: None,
        ..Default::default()
    };
    backoff::future::retry(backoff, || async {
        tracing::debug!(node = %&target, "establishing TCP connection");
        // At the beginning of each iteration, we check the metadata stream to see if the target
        // still exists. If not, we refuse to reconnect, unless it is the original node of the client.
        // FUTURE[metadata]: impl this once we have the metadata system in place.

        // Establish TCP connection.
        let tcp_res = TcpStream::connect(target.as_str()).await;
        let tcp = match tcp_res {
            Ok(tcp) => tcp,
            Err(err) => {
                tracing::error!(error = ?err, "error establishing TCP connection to hadron node {}", &*target);
                return Err(backoff::Error::Transient(()));
            }
        };

        // Perform H2 handshake.
        let h2_res = handshake(tcp).await;
        let (h2, connection) = match h2_res {
            Ok(h2_conn) => h2_conn,
            Err(err) => {
                tracing::error!(error = ?err, "error during HTTP2 handshake with hadron node {}", &*target);
                return Err(backoff::Error::Transient(()));
            }
        };

        // Spawn off the new connection for continual processing.
        let node = target.clone();
        tokio::spawn(async move {
            if let Err(err) = connection.await {
                tracing::error!(error = ?err, %node, "connection with hadron node severed");
            }
        });
        Ok(h2)
    })
    .await
    .map_err(|_| anyhow!("refusing to continue reconnect attempts to hadron node {}", target))
}

/// Connection state of a connection to a node of a Hadron cluster.
pub(crate) struct Connection {
    /// The URI of the Hadron of this connection.
    pub uri: Arc<String>,
    /// The state of the connection.
    pub state: ConnectionState,
    /// A channel used to notify of updates to the connection state.
    pub conn_updates_tx: watch::Sender<()>,
    /// A channel used to notify of updates to the connection state.
    pub conn_updates_rx: watch::Receiver<()>,
}

/// The internal state of a connection to a Hadron node.
pub(crate) enum ConnectionState {
    /// No action related to this connection has taken place.
    Pending,
    /// The connection is being established.
    Connecting,
    /// The connection is live and ready for use.
    Connected(H2Channel),
}

impl Connection {
    /// Create a new instance.
    pub fn new(uri: Arc<String>) -> Self {
        let (conn_updates_tx, conn_updates_rx) = watch::channel(());
        Self {
            uri,
            state: ConnectionState::Pending,
            conn_updates_tx,
            conn_updates_rx,
        }
    }
}

/// Client credentials.
#[derive(Clone)]
pub enum ClientCreds {
    Token(String, http::HeaderValue),
    User(String, String, http::HeaderValue),
}

impl ClientCreds {
    /// Create a new credentials set using the given token.
    pub fn new_with_token(token: &str) -> Result<Self> {
        let orig = token.to_string();
        let header: HeaderValue = format!("bearer {}", token).parse().context("error setting auth token")?;
        Ok(Self::Token(orig, header))
    }

    /// Create a new credentials set using the given username & password.
    pub fn new_with_password(username: &str, password: &str) -> Result<Self> {
        let user = username.to_string();
        let pass = password.to_string();
        let basic_auth = base64::encode(format!("{}:{}", username, password));
        let header: HeaderValue = format!("basic {}", basic_auth).parse().context("error setting username/password")?;
        Ok(Self::User(user, pass, header))
    }
}
