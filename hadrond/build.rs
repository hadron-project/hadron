use anyhow::{Context, Result};

fn main() -> Result<()> {
    prost_build::Config::new()
        .out_dir("src/models/proto")
        .type_attribute(".", "#[derive(::serde::Serialize, ::serde::Deserialize)]")
        .type_attribute(".", r#"#[serde(rename_all = "camelCase")]"#)
        .field_attribute("schema.Metadata.description", "#[serde(default)]")
        .field_attribute("schema.Namespace.id", "#[serde(default, skip)]")
        .field_attribute("schema.Namespace.description", "#[serde(default)]")
        .field_attribute("schema.Stream.id", "#[serde(default, skip)]")
        .field_attribute("schema.Stream.metadata", "#[serde(flatten)]")
        .field_attribute("schema.Pipeline.id", "#[serde(default, skip)]")
        .field_attribute("schema.Pipeline.metadata", "#[serde(flatten)]")
        .field_attribute("schema.Pipeline.triggers", "#[serde(default)]")
        .field_attribute("schema.PipelineStage.after", "#[serde(default)]")
        .field_attribute("schema.PipelineStage.dependencies", "#[serde(default)]")
        .field_attribute("schema.PipelineStage.outputs", "#[serde(default)]")
        .field_attribute("schema.Endpoint.id", "#[serde(default, skip)]")
        .field_attribute("schema.Endpoint.metadata", "#[serde(flatten)]")
        .field_attribute("schema.Endpoint.input", "#[serde(default)]")
        .field_attribute("schema.Endpoint.output", "#[serde(default)]")
        .compile_protos(&["proto/schema.proto"], &["proto"])
        .context("error compiling schema proto")?;

    prost_build::Config::new()
        .out_dir("src/models/proto")
        .type_attribute(".", "#[derive(::serde::Serialize, ::serde::Deserialize)]")
        .compile_protos(&["proto/auth.proto"], &["proto"])
        .context("error compiling auth proto")?;
    Ok(())
}
