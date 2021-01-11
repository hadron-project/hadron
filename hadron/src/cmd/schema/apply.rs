//! Apply schema changesets.

#![allow(dead_code)] // TODO: remove

use anyhow::{bail, Context, Result};
use futures::stream::StreamExt;
use structopt::StructOpt;

use crate::cmd::schema::HadronState;
use crate::Hadron;

const ERR_FILE_NAME_INVALID: &str =
    "schema file in schema management dir appears to be malformed, file name must be formatted as `{{timestamp}}-{{name}}.yaml`";

/// Apply schema changes to the cluster.
#[derive(StructOpt)]
#[structopt(name = "apply")]
pub struct Apply {
    /// The schema management directory to apply to the cluster.
    #[structopt(short, long, default_value = "hadron-schema")]
    dir: String,
    /// Apply a one-off schema change to the cluster, using the given string value as the
    /// contents of the schema.
    ///
    /// Can not be used with `--one-off-file`. All other parameters are ignored.
    #[structopt(long, conflicts_with("one-off-file"))]
    one_off_str: Option<String>,
    /// Apply a one-off schema change to the cluster, using the contents of the file specified
    /// by this parameter as the contents of the schema.
    ///
    /// Can not be used with `--one-off-str`. All other parameters are ignored.
    #[structopt(long, conflicts_with("one-off-str"))]
    one_off_file: Option<String>,
}

impl Apply {
    pub async fn run(&self, base: &Hadron) -> Result<()> {
        tracing::info!("starting schema apply");
        // Handle a oneoff schema change based on a given string.
        if let Some(schema) = &self.one_off_str {
            return self.submit_oneoff_from_file(base, schema).await;
        }
        // Handle a oneoff schema change based on a given file path.
        if let Some(file) = &self.one_off_file {
            return self.submit_oneoff_from_file(base, file).await;
        }

        // Else we proceed with performing a managed schema change. Read the contents of the
        // schema management dir to extract needed info.
        let dir = tokio::fs::canonicalize(&self.dir)
            .await
            .with_context(|| format!("error taking canonical path of schema management dir at {}", &self.dir))?;

        // Read the .hadron-state.yaml file to its corresponding model.
        let state = HadronState::from_file(dir.join(HadronState::FILE_NAME)).await?;

        // List the contents of the schema dir, filtering out non-schema files, and sorting the
        // files that need to sent to the cluster as schema updates.
        tracing::debug!(dir = ?dir, "checking for entries in dir");
        let mut entries = tokio::fs::read_dir(&dir).await.context("error listing contents of schema dir")?;
        let mut schema_files = vec![];
        while let Some(Ok(entry)) = entries.next().await {
            // Bind some basic info variables on the entry.
            let (file_name, file_path) = (entry.file_name(), entry.path());
            let file_name = file_name.to_string_lossy();
            tracing::debug!(file = %file_name, "processing entry");

            // Skip the .hadron-state.yaml file.
            if file_name == HadronState::FILE_NAME {
                tracing::debug!(file = %file_name, "skipping .hadron-state.yaml file");
                continue;
            }
            // Skip dirs, symlinks & files which do not end in .yaml.
            let ext = file_path.extension().map(|val| val.to_string_lossy());
            let entry_type = entry
                .file_type()
                .await
                .with_context(|| format!("error getting file type info for entry {:?}", &file_path))?;
            if entry_type.is_dir() || entry_type.is_symlink() {
                tracing::debug!(file = %file_name, "skipping file as it is either a dir or symlink");
                continue;
            }
            if ext.as_deref() != Some("yaml") && ext.as_deref() != Some("yml") {
                tracing::debug!(file = %file_name, "skipping file with invalid extension");
                continue;
            }

            // Ensure the schema file has a leading timestamp.
            let prefix = match file_name.split('-').next() {
                Some(prefix) => prefix,
                None => {
                    tracing::error!(file = ?file_name, ERR_FILE_NAME_INVALID);
                    bail!("aborting schema apply command due to errors");
                }
            };
            let timestamp: i64 = prefix.parse().context("could not parse timestamp from schema file name")?;
            let contents = tokio::fs::read_to_string(&file_path)
                .await
                .context("could not read contents of schema file")?;
            schema_files.push(SchemaFile {
                timestamp,
                filename: file_name.to_string(),
                contents,
            });
        }

        // Ensure files are sorted by timestamp order.
        schema_files.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

        // Build a new client and submit the request to the cluster.
        let client = base.get_client().await?.schema();
        for file in schema_files {
            tracing::info!(file = %file.filename, timestamp = file.timestamp, "syncing file");
            let _res = client
                .update_schema(&file.contents, &state.branch, file.timestamp)
                .await
                .context(crate::error::ERR_REQUEST)?;
            // TODO: show response info so that users will know if the schema has already been applied &c.
        }
        Ok(())
    }

    async fn submit_oneoff_from_str(&self, base: &Hadron, schema: &str) -> Result<()> {
        // Build a new client and submit the request to the cluster.
        let client = base.get_client().await?;
        client.schema().update_schema_oneoff(schema).await.context(crate::error::ERR_REQUEST)?;
        Ok(())
    }

    async fn submit_oneoff_from_file(&self, base: &Hadron, file: &str) -> Result<()> {
        // Read the file as a string and submit it to the cluster as a oneoff change.
        let path = tokio::fs::canonicalize(file)
            .await
            .with_context(|| format!("error taking canonical path of given file {}", file))?;
        let schema = tokio::fs::read_to_string(&path)
            .await
            .with_context(|| format!("error reading schema file at {:?}", &path))?;

        // Build a new client and submit the request to the cluster.
        let client = base.get_client().await?;
        client.schema().update_schema_oneoff(&schema).await.context(crate::error::ERR_REQUEST)?;
        Ok(())
    }
}

struct SchemaFile {
    pub timestamp: i64,
    pub filename: String,
    pub contents: String,
}
