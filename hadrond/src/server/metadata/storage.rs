//! Metadata database logic.

use std::sync::Arc;

use anyhow::{Context, Result};
use prost::Message;
use sled::{Batch, Tree};

use crate::auth::{Claims, ClaimsV1, ClaimsVersion};
use crate::database::Database;
use crate::error::ShutdownResult;
use crate::error::{ERR_DB_FLUSH, ERR_ITER_FAILURE};
use crate::models::{auth, events, prelude::*, schema};
use crate::server::metadata::cache::{CacheWriteBatch, CacheWriteOp, MetadataCache};
use crate::utils;

const ROOT_USER_NAME: &str = "root";

// DB prefixes.
const PREFIX_BRANCHES: &str = "/branches/";
const PREFIX_NAMESPACE: &str = "/namespaces/";
const PREFIX_PIPELINES: &str = "/pipelines/";
const PREFIX_STREAMS: &str = "/streams/";
const PREFIX_USERS: &str = "/users/";

/// Apply a set of schema update statements to the system.
#[tracing::instrument(level = "trace", skip(db, cache, statements, branch, timestamp))]
pub async fn apply_schema_updates(
    db: Database, tree: Tree, cache: Arc<MetadataCache>, statements: Vec<schema::SchemaStatement>, branch: Option<&String>, timestamp: Option<&i64>,
) -> Result<Vec<Arc<events::Event>>> {
    tracing::debug!(?branch, ?timestamp, "applying update schema request to system");

    // First we prep all needed batches & buffers used for atomically updating the state
    // machine, indexes & event stream.
    let mut db_batch = Batch::default();
    let mut cache_batch = CacheWriteBatch::default();
    let mut events = Vec::with_capacity(statements.len());

    tracing::debug!("iterating over statements");
    // Now we iterate over our schema statements and perform their corresponding actions
    // operating only on our batches, which will be transactionally applied below.
    for statement in statements {
        match statement {
            schema::SchemaStatement::Namespace(ns) => create_namespace(&db, &cache, &mut cache_batch, &mut db_batch, ns)?,
            schema::SchemaStatement::Stream(stream) => create_stream(&db, &cache, &mut cache_batch, &mut db_batch, &mut events, stream)?,
            schema::SchemaStatement::Pipeline(pipeline) => create_pipeline(&db, &cache, &mut cache_batch, &mut db_batch, &mut events, pipeline)?,
            schema::SchemaStatement::Endpoint(_endpoint) => anyhow::bail!("TODO: finish up endpoint creation"),
        }
    }

    // Record the branch name & timestamp to disk & index if applicable.
    if let (Some(branch), Some(timestamp)) = (branch, timestamp) {
        update_schema_branch(&mut cache_batch, &mut db_batch, branch, timestamp)?;
    }

    // Apply the batch of changes.
    tracing::debug!("applying changes to db");
    Database::spawn_blocking(move || -> ShutdownResult<()> {
        tree.apply_batch(db_batch).context("error applying db batch")?;
        // TODO: don't flush when we have replicas.
        tree.flush().context("error flushing db changes")?;
        Ok(())
    })
    .await??;

    // Finally apply the index batch & publish events.
    tracing::debug!("applying index updates");
    cache.apply_batch(cache_batch);
    Ok(events)
}

/// Create a new namespace in the database.
#[tracing::instrument(level = "trace", skip(db, cache, cache_batch, db_batch, ns))]
fn create_namespace(
    db: &Database, cache: &MetadataCache, cache_batch: &mut CacheWriteBatch, db_batch: &mut Batch, mut ns: schema::Namespace,
) -> Result<()> {
    // Check if the namespace already exists. If so, done.
    if cache.get_namespace(&ns.name).is_some() {
        return Ok(());
    }

    // Namespace does not yet exist, so we create an entry for it.
    let id = db.generate_id()?;
    ns.id = id;
    let proto = utils::encode_model(&ns)?;
    let nskey = format!("{}{}", PREFIX_NAMESPACE, id);
    db_batch.insert(nskey.as_bytes(), proto.as_slice());
    cache_batch.push(CacheWriteOp::InsertNamespace(Arc::new(ns)));
    Ok(())
}

/// Create a new stream in the database.
#[tracing::instrument(level = "trace", skip(db, cache, cache_batch, db_batch, events, stream))]
fn create_stream(
    db: &Database, cache: &MetadataCache, cache_batch: &mut CacheWriteBatch, db_batch: &mut Batch, events: &mut Vec<Arc<events::Event>>,
    mut stream: schema::Stream,
) -> Result<()> {
    // Check if the stream already exists. If so, done.
    if cache.get_stream(stream.namespace(), stream.name()).is_some() {
        return Ok(());
    }

    // Stream does not yet exist, so we create an entry for it, setting its initial index to 0.
    let id = db.generate_id()?;
    stream.id = id;
    let model = utils::encode_model(&stream)?;
    let stream_key = format!("{}{}", PREFIX_STREAMS, id);
    db_batch.insert(stream_key.as_bytes(), model);
    let stream = Arc::new(stream);
    cache_batch.push(CacheWriteOp::InsertStream(stream.clone()));

    // Send out an event describing these changes.
    events.push(Arc::new(events::Event::StreamCreated(events::StreamCreated { stream })));

    Ok(())
}

/// Create a new pipeline in the database.
#[tracing::instrument(level = "trace", skip(db, cache, cache_batch, db_batch, events, pipeline))]
fn create_pipeline(
    db: &Database, cache: &MetadataCache, cache_batch: &mut CacheWriteBatch, db_batch: &mut Batch, events: &mut Vec<Arc<events::Event>>,
    mut pipeline: schema::Pipeline,
) -> Result<()> {
    // Check if the pipeline already exists. If so, done.
    if cache.get_pipeline(pipeline.namespace(), pipeline.name()).is_some() {
        return Ok(());
    }

    // Stream does not yet exist, so we create an entry for it, setting its initial index to 0.
    let id = db.generate_id()?;
    pipeline.id = id;
    let model = utils::encode_model(&pipeline)?;
    let pipeline_key = format!("{}{}", PREFIX_PIPELINES, id);
    db_batch.insert(pipeline_key.as_bytes(), model);
    let pipeline = Arc::new(pipeline);
    cache_batch.push(CacheWriteOp::InsertPipeline(pipeline.clone()));

    // Send out an event describing these changes.
    events.push(Arc::new(events::Event::PipelineCreated(events::PipelineCreated { pipeline })));

    Ok(())
}

/// Update a schema branch.
#[tracing::instrument(level = "trace", skip(cache_batch, db_batch, branch, timestamp))]
fn update_schema_branch(cache_batch: &mut CacheWriteBatch, db_batch: &mut Batch, branch: &str, timestamp: &i64) -> Result<()> {
    let model = utils::encode_model(&schema::SchemaBranch {
        name: branch.into(),
        timestamp: *timestamp,
    })
    .context("error encoding schema branch model")?;
    let key = format!("{}{}", PREFIX_BRANCHES, branch);
    db_batch.insert(key.as_bytes(), model);
    cache_batch.push(CacheWriteOp::UpdateSchemaBranch {
        branch: branch.to_string(),
        timestamp: *timestamp,
    });
    Ok(())
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// System State Recovery /////////////////////////////////////////////////////////////////////////

/// Recover the system's state, building indices and the like.
#[tracing::instrument(level = "trace", skip(db))]
pub async fn recover_system_state(db: &Tree) -> Result<(MetadataCache, events::InitialState)> {
    // Perform parallel recovery of system state.
    let (users, tokens, namespaces, streams, pipelines, branches) = tokio::try_join!(
        recover_user_permissions(&db),
        recover_token_permissions(&db),
        recover_namespaces(&db),
        recover_streams(&db),
        recover_pipelines(&db),
        recover_schema_branches(&db),
    )
    .context("error during parallel metadata state recovery")?;

    // Update initial CRC event with recovered data.
    let initial_event = events::InitialState::new(streams.clone(), pipelines.clone());

    // Update index with recovered data.
    let index = MetadataCache::new(users, tokens, namespaces, streams, pipelines, branches);

    Ok((index, initial_event))
}

/// Recover namespaces.
#[tracing::instrument(level = "trace", skip(db))]
pub async fn recover_namespaces(db: &Tree) -> Result<Vec<Arc<schema::Namespace>>> {
    let db = db.clone();
    let data = Database::spawn_blocking(move || -> Result<Vec<Arc<schema::Namespace>>> {
        let mut data = vec![];
        for entry_res in db.scan_prefix(PREFIX_NAMESPACE) {
            let (_key, entry) = entry_res.context(ERR_ITER_FAILURE)?;
            let model: schema::Namespace = utils::decode_model(&entry).context("error decoding namespace from storage")?;
            data.push(Arc::new(model));
        }
        Ok(data)
    })
    .await??;
    tracing::debug!(count = data.len(), "recovered namespaces");
    Ok(data)
}

/// Recover streams.
#[tracing::instrument(level = "trace", skip(db))]
pub async fn recover_streams(db: &Tree) -> Result<Vec<Arc<schema::Stream>>> {
    let db = db.clone();
    let data = Database::spawn_blocking(move || -> Result<Vec<Arc<schema::Stream>>> {
        let mut data = vec![];
        for entry_res in db.scan_prefix(PREFIX_STREAMS) {
            let (_key, entry) = entry_res.context(ERR_ITER_FAILURE)?;
            let model: schema::Stream = utils::decode_model(&entry).context("error decoding stream from storage")?;
            data.push(Arc::new(model));
        }
        Ok(data)
    })
    .await??;
    tracing::debug!(count = data.len(), "recovered streams");
    Ok(data)
}

/// Recover pipelines.
#[tracing::instrument(level = "trace", skip(db))]
pub async fn recover_pipelines(db: &Tree) -> Result<Vec<Arc<schema::Pipeline>>> {
    let db = db.clone();
    let data = Database::spawn_blocking(move || -> Result<Vec<Arc<schema::Pipeline>>> {
        let mut data = vec![];
        for entry_res in db.scan_prefix(PREFIX_PIPELINES) {
            let (_key, entry) = entry_res.context(ERR_ITER_FAILURE)?;
            let model: schema::Pipeline = utils::decode_model(&entry).context("error decoding pipeline from storage")?;
            data.push(Arc::new(model));
        }
        Ok(data)
    })
    .await??;
    tracing::debug!(count = data.len(), "recovered pipelines");
    Ok(data)
}

/// Recover pipeline replicas.
#[tracing::instrument(level = "trace", skip(db))]
pub async fn recover_schema_branches(db: &Tree) -> Result<Vec<schema::SchemaBranch>> {
    let db = db.clone();
    let data = Database::spawn_blocking(move || -> Result<Vec<schema::SchemaBranch>> {
        let mut data = vec![];
        for entry_res in db.scan_prefix(PREFIX_BRANCHES) {
            let (_key, entry) = entry_res.context(ERR_ITER_FAILURE)?;
            let model: schema::SchemaBranch = utils::decode_model(&entry).context("error decoding schema branch from storage")?;
            data.push(model);
        }
        Ok(data)
    })
    .await??;
    tracing::debug!(count = data.len(), "recovered schema branches");
    Ok(data)
}

/// Recover the system's user permissions state.
#[tracing::instrument(level = "trace", skip(db))]
pub async fn recover_user_permissions(db: &Tree) -> Result<Vec<auth::User>> {
    let db_inner = db.clone();
    let mut data = Database::spawn_blocking(move || -> Result<Vec<auth::User>> {
        let mut data = vec![];
        for entry_res in db_inner.scan_prefix(PREFIX_USERS) {
            let (_key, entry) = entry_res.context(ERR_ITER_FAILURE)?;
            let model: auth::User = utils::decode_model(&entry).context("error decoding user model from storage")?;
            data.push(model);
        }
        Ok(data)
    })
    .await??;

    // If there are no users on disk, this means that the node is pristine and we need to
    // initialize the root user. At any point in the future, the system will not allow for 0 users.
    if data.is_empty() {
        let user = initialize_root_user(&db).await.context("error initializing root user")?;
        data.push(user);
    }

    tracing::debug!(count = data.len(), "recovered users");
    Ok(data)
}

/// Recover the system's token permissions state.
#[tracing::instrument(level = "trace", skip(_db))]
pub async fn recover_token_permissions(_db: &Tree) -> Result<Vec<(u128, Claims)>> {
    // TODO: recover token state.
    Ok(vec![(
        0,
        Claims {
            id: uuid::Uuid::from_u128(0),
            claims: ClaimsVersion::V1(ClaimsV1::All),
        },
    )])
}

/// Initialize the root user.
#[tracing::instrument(level = "trace", skip(db))]
async fn initialize_root_user(db: &Tree) -> Result<auth::User> {
    let db = db.clone();
    let user = Database::spawn_blocking(move || -> Result<auth::User> {
        // Generate the initial root user state.
        let pwhash = bcrypt::hash(ROOT_USER_NAME, bcrypt::DEFAULT_COST).context("error hasing root user password")?;
        let user = auth::User {
            name: ROOT_USER_NAME.into(),
            role: auth::UserRole::Root as i32,
            pwhash,
        };

        // Serialize the user model.
        let mut buf = Vec::with_capacity(user.encoded_len());
        user.encode(&mut buf).context("error encoding root user model for storage")?;

        // Store the model.
        let key = format!("{}{}", PREFIX_USERS, user.name);
        db.insert(key.as_bytes(), buf.as_slice()).context("error inserting user model")?;
        db.flush().context(ERR_DB_FLUSH)?;
        Ok(user)
    })
    .await??;
    tracing::debug!("initialized root user");
    Ok(user)
}
