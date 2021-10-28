use anyhow::{Context, Result};
use rand::prelude::*;

use crate::config::Config;
use crate::database::Database;
use crate::grpc::Event;
use crate::models::stream::Subscription;
use crate::stream::{KEY_STREAM_LAST_WRITTEN_OFFSET, PREFIX_STREAM_EVENT, PREFIX_STREAM_SUBS, PREFIX_STREAM_SUB_OFFSETS};
use crate::utils;

#[tokio::test]
async fn recover_stream_state_empty_state() -> Result<()> {
    let (config, _tmpdir) = Config::new_test()?;
    let db = Database::new(config.clone()).await?;
    let stream_tree = db.get_stream_tree().await?;

    let output = super::recover_stream_state(stream_tree).await?;

    assert!(
        output.last_written_offset == 0,
        "expected offset to be 0 got {}",
        output.last_written_offset
    );
    assert!(
        output.subscriptions.is_empty(),
        "expected subscriptions len to be 0 got {}",
        output.subscriptions.len()
    );

    Ok(())
}

#[tokio::test]
async fn recover_stream_state_with_previous_state() -> Result<()> {
    let (config, _tmpdir) = Config::new_test()?;
    let db = Database::new(config.clone()).await?;
    let stream_tree = db.get_stream_tree().await?;

    let expected_offset = setup_stream_data(&stream_tree).await?;

    let output = super::recover_stream_state(stream_tree).await?;

    assert!(
        output.last_written_offset == expected_offset,
        "expected offset to be {} got {}",
        expected_offset,
        output.last_written_offset
    );
    assert!(
        output.subscriptions.is_empty(),
        "expected subscriptions len to be 0 got {}",
        output.subscriptions.len()
    );

    Ok(())
}

#[tokio::test]
async fn recover_stream_state_with_previous_state_and_subs() -> Result<()> {
    let (config, _tmpdir) = Config::new_test()?;
    let db = Database::new(config.clone()).await?;
    let stream_tree = db.get_stream_tree().await?;

    let expected_offset = setup_stream_data(&stream_tree).await?;
    let expected_subs = setup_subs_data(&stream_tree).await?;

    let mut output = super::recover_stream_state(stream_tree).await?;
    output.subscriptions.sort_by(|a, b| a.1.cmp(&b.1));

    assert!(
        output.last_written_offset == expected_offset,
        "expected offset to be {} got {}",
        expected_offset,
        output.last_written_offset
    );
    assert_eq!(
        output.subscriptions, expected_subs,
        "expected subscriptions to match {:?}\n{:?}",
        output.subscriptions, expected_subs
    );

    Ok(())
}

/// Setup some stream data in the given DB tree returning the last written offset.
async fn setup_stream_data(db: &sled::Tree) -> Result<u64> {
    use rand::prelude::*;

    let mut batch = sled::Batch::default();
    let mut last_offset = 0;
    for offset in 0..rand::thread_rng().gen_range(50..100) {
        let event = Event::new_test(offset, "test", "empty");
        let event_bytes = utils::encode_model(&event)?;
        batch.insert(&utils::encode_byte_prefix(PREFIX_STREAM_EVENT, offset), event_bytes.as_slice());
        last_offset = offset;
    }
    batch.insert(KEY_STREAM_LAST_WRITTEN_OFFSET, &utils::encode_u64(last_offset));
    db.apply_batch(batch)
        .context("error applying batch to write test data to stream")?;
    Ok(last_offset)
}

/// Setup some subscriptions data in the given DB tree returning the set of created subs.
async fn setup_subs_data(db: &sled::Tree) -> Result<Vec<(Subscription, u64)>> {
    let mut batch = sled::Batch::default();
    let mut subs = vec![];
    for offset in 0..rand::thread_rng().gen_range(50..100) {
        let sub = Subscription { group_name: offset.to_string(), max_batch_size: 50 };
        let sub_encoded = utils::encode_model(&sub)?;
        let sub_model_key = utils::ivec_from_iter(
            PREFIX_STREAM_SUBS
                .iter()
                .copied()
                .chain(sub.group_name.as_bytes().iter().copied()),
        );
        let sub_offset_key = utils::ivec_from_iter(
            PREFIX_STREAM_SUB_OFFSETS
                .iter()
                .copied()
                .chain(sub.group_name.as_bytes().iter().copied()),
        );
        batch.insert(sub_model_key, sub_encoded.as_slice());
        batch.insert(sub_offset_key, &utils::encode_u64(offset));
        subs.push((sub, offset));
    }
    db.apply_batch(batch)
        .context("error applying batch to write test data to stream")?;
    subs.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(subs)
}
