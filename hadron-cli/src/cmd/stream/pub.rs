//! Publish data to a stream.

use anyhow::{Context, Result};
use structopt::StructOpt;

use crate::Hadron;
use hadron::{Event, WriteAck};

/// Publish data to a stream.
#[derive(StructOpt)]
#[structopt(name = "pub")]
pub struct Pub {
    /// The ID of the new event, else a UUID4 will be generated.
    #[structopt(long)]
    id: Option<String>,
    /// The source of the new event, else `hadron-cli` will be used.
    #[structopt(long)]
    source: Option<String>,
    /// The type of the new event.
    #[structopt(long)]
    r#type: String,
    /// Optional attributes to associate with the given payload.
    #[structopt(short = "o", parse(try_from_str = parse_key_val), number_of_values = 1)]
    optattrs: Vec<(String, String)>,
    /// If true, treat the data payload as a base64 encoded binary blob.
    ///
    /// When a binary blob is provided, the blob will be base64 decoded before being sent to
    /// the server. This is useful for binary types such as protobuf and the like.
    #[structopt(long)]
    binary: bool,
    /// The data payload to be published.
    data: String,
}

impl Pub {
    pub async fn run(&self, base: &Hadron) -> Result<()> {
        // Build a new client.
        tracing::info!("publishing data");
        let mut client = base.get_client().await?.publisher("hadron-cli").await?;

        let id = self.id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let source = self.source.clone().unwrap_or_else(|| "hadron-cli".into());
        let r#type = self.r#type.clone();
        let optattrs = self.optattrs.iter().cloned().collect();

        // If the given payload is binary, base64 decode it before sending it to the cluster.
        let data = if self.binary {
            base64::decode(self.data.as_str()).context("error base64 decoding given payload, controlled by -b/--binary")?
        } else {
            self.data.as_bytes().to_vec()
        };

        // Submit the request to the cluster.
        client.ready(None).await?;
        let res = client
            .publish(Event::new(id, source, r#type, data).with_optattrs(optattrs), WriteAck::All, true)
            .await
            .context("error publishing data")?;
        tracing::info!("Response: {:?}", res);
        Ok(())
    }
}

/// Parse a key-value pair from the given str.
fn parse_key_val<T, U>(s: &str) -> Result<(T, U), Box<dyn std::error::Error>>
where
    T: std::str::FromStr,
    T::Err: std::error::Error + 'static,
    U: std::str::FromStr,
    U::Err: std::error::Error + 'static,
{
    let pos = s
        .find('=')
        .ok_or_else(|| format!("invalid key=value pair: no `=` found in `{}`", s))?;
    Ok((s[..pos].parse()?, s[pos + 1..].parse()?))
}
