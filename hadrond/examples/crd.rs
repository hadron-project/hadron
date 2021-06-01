//! A script used to generate the CRDs used by this project.
//!
//! Any time a CRD spec changes, this script can be run to ensure that the CRDs are up-to-date and
//! ready to be synced with the cluster.

use anyhow::{Context, Result};
use hadrond::crd::stream::Stream;

fn main() -> Result<()> {
    let crd = Stream::crd();
    let contents = serde_yaml::to_string(&crd).context("error serializing CRD to yaml")?;
    let canon = std::fs::canonicalize("..").context("error getting canonical path of current dir")?;
    let crd_path = canon.join("k8s").join("helm").join("crds").join("stream.yaml");
    std::fs::write(&crd_path, &contents).with_context(|| format!("error writing CRD to {:?}", &crd_path))?;
    println!("CRD written to {:?}", &crd_path);
    Ok(())
}
