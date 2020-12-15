use anyhow::{Context, Result};
use hadron::Client;
use tokio::fs;
use tokio::stream::StreamExt;
use tonic::metadata::{Ascii, MetadataValue};
use tonic::Request;

#[tokio::main(core_threads = 3, max_threads = 6)]
async fn main() -> Result<()> {
    println!("creating new client");

    // Setup the client & auth token.
    let client = Client::new("http://localhost:7000")
        .await
        .context("error establishing connection to Hadron cluster")?;
    let auth_header: MetadataValue<Ascii> = "bearer test-auth".parse().context("error setting auth token")?;

    // // Sync our initial schema for the system.
    // update_schema(client.clone(), auth_header.clone()).await?;

    let mut handles = futures::stream::FuturesUnordered::new();
    for idx in 0u64..10_000u64 {
        let (client, header) = (client.clone(), auth_header.clone());
        let handle = tokio::spawn(async move {
            if let Err(err) = send_request(client, header, idx).await {
                println!("{:?}", err);
            }
        });
        handles.push(handle);
    }

    while let Some(res) = handles.next().await {
        if let Err(err) = res {
            println!("error awaiting handle\n{:?}", err);
        }
    }

    Ok(())
}

async fn send_request(mut client: Client, header: MetadataValue<Ascii>, idx: u64) -> Result<()> {
    let mut req = Request::new(hadron::StreamPubRequest {
        stream: "services".into(),
        namespace: "example".into(),
        payload: idx.to_le_bytes().to_vec(),
    });
    req.metadata_mut().insert("x-hadron-authorization", header);

    let start = tokio::time::Instant::now();
    let res = client.stream_pub(req).await.context("error from stream pub request")?;
    let duration = start.elapsed();

    println!("response from server:\nduration: {:?}\n{:?}", duration, res.into_inner());
    Ok(())
}

async fn update_schema(mut client: Client, header: MetadataValue<Ascii>) -> Result<()> {
    // Read our example schema file.
    let schema = fs::read_to_string("examples/schema.yaml")
        .await
        .context("error reading examples/schema.yaml file")?;
    let mut req = Request::new(hadron::UpdateSchemaRequest { schema });
    req.metadata_mut().insert("x-hadron-authorization", header);
    let start = tokio::time::Instant::now();
    let res = client.update_schema(req).await.context("error from update schema request")?;
    let duration = tokio::time::Instant::now().duration_since(start);
    println!("response from server:\nduration: {:?}\n{:?}", duration, res.into_inner());
    Ok(())
}
