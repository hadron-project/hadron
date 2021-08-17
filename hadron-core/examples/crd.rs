//! A script used to generate the CRDs used by this project.
//!
//! Any time a CRD spec changes, this script can be run to ensure that the CRDs are up-to-date and
//! ready to be synced with the cluster.

use anyhow::{Context, Result};
use hadron_core::crd::{Pipeline, Stream, Token};
use kube::CustomResourceExt;

fn main() -> Result<()> {
    let canon = std::fs::canonicalize("..").context("error getting canonical path of current dir")?;
    let crds_path = canon.join("k8s").join("helm").join("crds");

    let stream = Stream::crd();
    let stream_yaml = serde_yaml::to_string(&stream).context("error serializing Stream CRD to yaml")?;
    std::fs::write(crds_path.join("stream.yaml"), &stream_yaml).with_context(|| format!("error writing Stream CRD to {:?}", &crds_path))?;
    println!("Stream CRD written to {:?}", &crds_path);

    let pipeline = Pipeline::crd();
    let pipeline_yaml = serde_yaml::to_string(&pipeline).context("error serializing Pipeline CRD to yaml")?;
    std::fs::write(crds_path.join("pipeline.yaml"), &pipeline_yaml).with_context(|| format!("error writing Pipeline CRD to {:?}", &crds_path))?;
    println!("Pipeline CRD written to {:?}", &crds_path);

    let token = Token::crd();
    let token_yaml = serde_yaml::to_string(&token).context("error serializing Token CRD to yaml")?;
    std::fs::write(crds_path.join("token.yaml"), &token_yaml).with_context(|| format!("error writing Token CRD to {:?}", &crds_path))?;
    println!("Token CRD written to {:?}", &crds_path);

    Ok(())
}
