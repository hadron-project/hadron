//! Schema management initialization.

use std::path::PathBuf;
use std::time::SystemTime;

use anyhow::{ensure, Context, Result};
use regex::Regex;
use structopt::StructOpt;
use tokio::fs;

/// The contents of an initial schema file.
const INIT_SCHEMA: &str = r#"# Add your schema changes here. Each schema statement should be delimited by
# a line with the text `---` which indicates the start of a new YAML object.
#
# All of the schema statements in this file will be transactionally applied
# to the system as a whole.
#
# Here is an example `Namespace` declaration:
---
kind: Namespace
name: example
description: This is just an example namespace.

# See the docs at https://docs.hadron.rs/guide/reference/schema.html
"#;

lazy_static::lazy_static! {
    /// Regular expression used to validate branch names.
    static ref RE_BRANCH: Regex = Regex::new(r"^[-/_.a-zA-Z0-9]{1,255}$").expect("failed to compile RE_BRANCH regex");
}

/// Initialize a new schema management directory.
#[derive(StructOpt)]
#[structopt(name = "init")]
pub struct Init {
    /// The directory in which to create the new Hadron schema management files.
    #[structopt(short, long, default_value = "hadron-schema")]
    dir: String,
    /// The name of the branch to use for the new changeset.
    branch_name: String,
}

impl Init {
    pub async fn run(&self) -> Result<()> {
        // Validate branch name.
        ensure!(
            RE_BRANCH.is_match(&self.branch_name),
            "invalid branch name provided, must match `{}`",
            RE_BRANCH.as_str(),
        );

        // Canonicalize the given dir name, and then create the new dir if needed.
        let canon_dir = PathBuf::from(&self.dir);
        fs::create_dir_all(&canon_dir).await.context("could not create dir path")?;

        // Take a new timestamp for use in the changeset files & render our templates.
        let ts = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .context("error generating timestamp")?;
        let fileinit = format!("{}-initial.yaml", ts.as_millis());
        let tmplstate = gen_tmpl_state(&self.branch_name);

        // If the state file already exists in the directory, then no-op.
        let hadron_state_file = canon_dir.join(".hadron-state.yaml");
        if hadron_state_file.exists() {
            tracing::info!("the target schema management directory already exists");
            return Ok(());
        }

        // Write the template files to the new directory.
        fs::write(canon_dir.join(&fileinit), INIT_SCHEMA)
            .await
            .context("error writing initial schema file")?;
        fs::write(hadron_state_file, &tmplstate)
            .await
            .context("error writing hadron state file")?;

        tracing::info!(dir = ?canon_dir, "schema management directory initialized");
        Ok(())
    }
}

/// Generate a template for the new `.hadron-state.yaml` file.
fn gen_tmpl_state(branch: &str) -> String {
    format!(
        r#"# NOTE: this file was generated by the Hadron CLI. Do not edit.
branch: {branch}
"#,
        branch = branch,
    )
}
