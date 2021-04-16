use anyhow::{Context, Result};

fn main() -> Result<()> {
    prost_build::Config::new()
        .out_dir("src/models/proto")
        .type_attribute(".", "#[derive(::serde::Serialize, ::serde::Deserialize)]")
        .type_attribute(".", r#"#[serde(rename_all = "camelCase")]"#)
        .field_attribute("storage.Metadata.description", "#[serde(default)]")
        .field_attribute("storage.Namespace.id", "#[serde(default, skip_deserializing)]")
        .field_attribute("storage.Namespace.description", "#[serde(default)]")
        .field_attribute("storage.Stream.id", "#[serde(default, skip_deserializing)]")
        .field_attribute("storage.Stream.metadata", "#[serde(flatten)]")
        .field_attribute("storage.Pipeline.id", "#[serde(default, skip_deserializing)]")
        .field_attribute("storage.Pipeline.metadata", "#[serde(flatten)]")
        .field_attribute("storage.Pipeline.triggers", "#[serde(default)]")
        .field_attribute("storage.PipelineStage.after", "#[serde(default)]")
        .field_attribute("storage.PipelineStage.dependencies", "#[serde(default)]")
        .field_attribute("storage.PipelineStage.outputs", "#[serde(default)]")
        .field_attribute("storage.Endpoint.id", "#[serde(default, skip_deserializing)]")
        .field_attribute("storage.Endpoint.metadata", "#[serde(flatten)]")
        .field_attribute("storage.Endpoint.input", "#[serde(default)]")
        .field_attribute("storage.Endpoint.output", "#[serde(default)]")
        .compile_protos(&["proto/storage.proto"], &["proto"])
        .context("error compiling storage proto")?;
    Ok(())
}
