use std::sync::Arc;

use anyhow::{Context, Result};
use rand::prelude::*;

use crate::config::Config;
use crate::database::Database;
use crate::grpc::Event;
use crate::models::stream::Subscription;
use crate::stream::{KEY_STREAM_LAST_WRITTEN_OFFSET, PREFIX_STREAM_EVENT, PREFIX_STREAM_SUBS, PREFIX_STREAM_SUB_OFFSETS, PREFIX_STREAM_TS};
use crate::utils;
use hadron_core::crd::StreamRetentionPolicy;

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

    let expected_offset = setup_stream_data(&stream_tree).await?.1;

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
    assert!(
        output.first_timestamp_opt.is_some(),
        "expected first_timestamp_opt to be populated, got None"
    );

    Ok(())
}

#[tokio::test]
async fn recover_stream_state_with_previous_state_and_subs() -> Result<()> {
    let (config, _tmpdir) = Config::new_test()?;
    let db = Database::new(config.clone()).await?;
    let stream_tree = db.get_stream_tree().await?;

    let expected_offset = setup_stream_data(&stream_tree).await?.1;
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

#[tokio::test]
async fn compact_stream_noop_with_empty_tree() -> Result<()> {
    let (config, _tmpdir) = Config::new_test()?;
    let db = Database::new(config.clone()).await?;
    let stream_tree = db.get_stream_tree().await?;

    let earliest_timestamp_opt = super::compact_stream(config, stream_tree.clone(), None)
        .await
        .context("unexpected error from compaction")?;

    assert!(
        earliest_timestamp_opt.is_none(),
        "expected compaction to return None, got {:?}",
        earliest_timestamp_opt
    );
    let count = stream_tree.scan_prefix(PREFIX_STREAM_EVENT).count();
    assert_eq!(
        count, 0,
        "mismatch number of stream events after compaction, got {} expected {}",
        count, 0
    );

    Ok(())
}

#[tokio::test]
async fn compact_stream_noop_retention_policy_retain() -> Result<()> {
    let (mut config, _tmpdir) = Config::new_test()?;
    Arc::make_mut(&mut config).retention_policy.strategy = StreamRetentionPolicy::Retain;
    let db = Database::new(config.clone()).await?;
    let stream_tree = db.get_stream_tree().await?;
    let last_offset = setup_stream_data(&stream_tree).await?.1;

    let earliest_timestamp_opt = super::compact_stream(config, stream_tree.clone(), None)
        .await
        .context("unexpected error from compaction")?;

    assert!(
        earliest_timestamp_opt.is_none(),
        "expected compaction to return None, got {:?}",
        earliest_timestamp_opt
    );
    let count = stream_tree.scan_prefix(PREFIX_STREAM_EVENT).count();
    assert_eq!(
        count,
        last_offset as usize + 1,
        "mismatch number of stream events after compaction, got {} expected {}",
        count,
        last_offset + 1
    );

    Ok(())
}

#[tokio::test]
async fn compact_stream_deletes_all_data() -> Result<()> {
    let (mut config, _tmpdir) = Config::new_test()?;
    Arc::make_mut(&mut config).retention_policy.retention_seconds = Some(0); // Should ensure that all data is removed.
    let db = Database::new(config.clone()).await?;
    let stream_tree = db.get_stream_tree().await?;
    let last_offset = setup_stream_data(&stream_tree).await?.1;

    let earliest_timestamp_opt = super::compact_stream(config, stream_tree.clone(), None)
        .await
        .context("unexpected error from compaction")?;

    assert!(
        last_offset > 50,
        "expected at least offset 50 from setup_stream_data, got {}",
        last_offset
    );
    assert!(
        earliest_timestamp_opt.is_none(),
        "expected compaction to return None, got {:?}",
        earliest_timestamp_opt
    );
    let count = stream_tree.scan_prefix(PREFIX_STREAM_EVENT).count();
    assert_eq!(
        count, 0,
        "mismatch number of stream events after compaction, got {} expected {}",
        count, 0
    );

    Ok(())
}

#[tokio::test]
async fn compact_stream_deletes_only_old_data() -> Result<()> {
    let (config, _tmpdir) = Config::new_test()?;
    let db = Database::new(config.clone()).await?;
    let stream_tree = db.get_stream_tree().await?;
    let (last_ts, last_offset) = setup_stream_data(&stream_tree).await?;
    let ts_two_weeks_ago = (chrono::Utc::now() - chrono::Duration::weeks(2)).timestamp_millis();
    let old_offset = last_offset / 2;
    stream_tree
        .insert(
            &utils::encode_byte_prefix_i64(PREFIX_STREAM_TS, ts_two_weeks_ago),
            &utils::encode_u64(old_offset),
        )
        .context("error inserting fake timestamp index record for compaction test")?;

    let earliest_timestamp = super::compact_stream(config, stream_tree.clone(), None)
        .await
        .context("unexpected error from compaction")?
        .context("expected next earliest timestamp record to be returned")?;

    assert_eq!(
        earliest_timestamp.0, last_ts,
        "expected earliest timestamp to be {}, got {}",
        earliest_timestamp.0, last_ts
    );
    assert_eq!(
        earliest_timestamp.1, last_offset,
        "expected earliest timestamp offset to be {}, got {}",
        earliest_timestamp.1, last_offset
    );
    let event_count = stream_tree.scan_prefix(PREFIX_STREAM_EVENT).count();
    let expected_count = (old_offset..last_offset).count(); // `..` because old_offset is deleted.
    assert_eq!(
        event_count, expected_count,
        "mismatch number of stream events after compaction, got {} expected {}",
        event_count, expected_count
    );
    let ts_count = stream_tree.scan_prefix(PREFIX_STREAM_TS).count();
    assert_eq!(ts_count, 1, "expected 1 timestamp index entry, got {}", ts_count);

    Ok(())
}

/// Setup some stream data in the given DB tree returning the last written offset.
async fn setup_stream_data(db: &sled::Tree) -> Result<(i64, u64)> {
    let mut batch = sled::Batch::default();
    let mut last_offset = 0;
    let ts = chrono::Utc::now().timestamp_millis();
    for offset in 0..rand::thread_rng().gen_range(50..100) {
        let event = Event::new_test(offset, "test", "empty");
        let event_bytes = utils::encode_model(&event)?;
        batch.insert(&utils::encode_byte_prefix(PREFIX_STREAM_EVENT, offset), event_bytes.as_slice());
        last_offset = offset;
    }
    batch.insert(KEY_STREAM_LAST_WRITTEN_OFFSET, &utils::encode_u64(last_offset));
    batch.insert(&utils::encode_byte_prefix_i64(PREFIX_STREAM_TS, ts), &utils::encode_u64(last_offset));
    db.apply_batch(batch)
        .context("error applying batch to write test data to stream")?;
    Ok((ts, last_offset))
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
