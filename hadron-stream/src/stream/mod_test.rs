use std::sync::Arc;

use anyhow::{Context, Result};

use crate::config::Config;
use crate::database::Database;
use crate::fixtures;
use crate::stream::{PREFIX_STREAM_EVENT, PREFIX_STREAM_TS};
use crate::utils;
use hadron_core::crd::StreamRetentionPolicy;

#[tokio::test]
async fn recover_stream_state_empty_state() -> Result<()> {
    let (config, _tmpdir) = Config::new_test()?;
    let db = Database::new(config.clone()).await?;
    let stream_tree = db.get_stream_tree().await?;

    let output = super::recover_stream_state(stream_tree).await?;

    assert!(output.last_written_offset == 0, "expected offset to be 0 got {}", output.last_written_offset);
    assert!(output.subscriptions.is_empty(), "expected subscriptions len to be 0 got {}", output.subscriptions.len());

    Ok(())
}

#[tokio::test]
async fn recover_stream_state_with_previous_state() -> Result<()> {
    let (config, _tmpdir) = Config::new_test()?;
    let db = Database::new(config.clone()).await?;
    let stream_tree = db.get_stream_tree().await?;

    let expected_offset = fixtures::setup_stream_data(&stream_tree).await?.1;

    let output = super::recover_stream_state(stream_tree).await?;

    assert!(
        output.last_written_offset == expected_offset,
        "expected offset to be {} got {}",
        expected_offset,
        output.last_written_offset
    );
    assert!(output.subscriptions.is_empty(), "expected subscriptions len to be 0 got {}", output.subscriptions.len());
    assert!(output.first_timestamp_opt.is_some(), "expected first_timestamp_opt to be populated, got None");

    Ok(())
}

#[tokio::test]
async fn recover_stream_state_with_previous_state_and_subs() -> Result<()> {
    let (config, _tmpdir) = Config::new_test()?;
    let db = Database::new(config.clone()).await?;
    let stream_tree = db.get_stream_tree().await?;

    let expected_offset = fixtures::setup_stream_data(&stream_tree).await?.1;
    let expected_subs = fixtures::setup_subs_data(&stream_tree).await?;

    let mut output = super::recover_stream_state(stream_tree).await?;
    output.subscriptions.sort_by(|a, b| a.1.cmp(&b.1));

    assert!(
        output.last_written_offset == expected_offset,
        "expected offset to be {} got {}",
        expected_offset,
        output.last_written_offset
    );
    assert_eq!(output.subscriptions, expected_subs, "expected subscriptions to match {:?}\n{:?}", output.subscriptions, expected_subs);

    Ok(())
}

#[tokio::test]
async fn compact_stream_noop_with_empty_tree() -> Result<()> {
    let (config, _tmpdir) = Config::new_test()?;
    let db = Database::new(config.clone()).await?;
    let stream_tree = db.get_stream_tree().await?;

    let earliest_timestamp_opt = super::compact_stream(config, stream_tree.clone(), None).await.context("unexpected error from compaction")?;

    assert!(earliest_timestamp_opt.is_none(), "expected compaction to return None, got {:?}", earliest_timestamp_opt);
    let count = stream_tree.scan_prefix(PREFIX_STREAM_EVENT).count();
    assert_eq!(count, 0, "mismatch number of stream events after compaction, got {} expected {}", count, 0);

    Ok(())
}

#[tokio::test]
async fn compact_stream_noop_retention_policy_retain() -> Result<()> {
    let (mut config, _tmpdir) = Config::new_test()?;
    Arc::make_mut(&mut config).retention_policy.strategy = StreamRetentionPolicy::Retain;
    let db = Database::new(config.clone()).await?;
    let stream_tree = db.get_stream_tree().await?;
    let last_offset = fixtures::setup_stream_data(&stream_tree).await?.1;

    let earliest_timestamp_opt = super::compact_stream(config, stream_tree.clone(), None).await.context("unexpected error from compaction")?;

    assert!(earliest_timestamp_opt.is_none(), "expected compaction to return None, got {:?}", earliest_timestamp_opt);
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
    let (last_ts, last_offset) = fixtures::setup_stream_data(&stream_tree).await?;
    let ts_two_weeks_ago = (time::OffsetDateTime::now_utc() - time::Duration::weeks(2)).unix_timestamp();
    stream_tree
        .insert(&utils::encode_byte_prefix_i64(PREFIX_STREAM_TS, ts_two_weeks_ago), &utils::encode_u64(last_offset))
        .context("error inserting older timestamp index record for compaction test")?;
    stream_tree
        .remove(&utils::encode_byte_prefix_i64(PREFIX_STREAM_TS, last_ts))
        .context("error removing original timestamp record for compaction test setup")?;

    let earliest_timestamp_opt = super::compact_stream(config, stream_tree.clone(), None).await.context("unexpected error from compaction")?;

    assert!(last_offset >= 49, "expected at least offset 49 from fixtures::setup_stream_data, got {}", last_offset);
    assert!(earliest_timestamp_opt.is_none(), "expected compaction to return None, got {:?}", earliest_timestamp_opt);
    let count = stream_tree.scan_prefix(PREFIX_STREAM_EVENT).count();
    assert_eq!(count, 0, "mismatch number of stream events after compaction, got {} expected {}", count, 0);

    Ok(())
}

#[tokio::test]
async fn compact_stream_deletes_only_old_data() -> Result<()> {
    let (config, _tmpdir) = Config::new_test()?;
    let db = Database::new(config.clone()).await?;
    let stream_tree = db.get_stream_tree().await?;
    let (last_ts, last_offset) = fixtures::setup_stream_data(&stream_tree).await?;
    let ts_two_weeks_ago = (time::OffsetDateTime::now_utc() - time::Duration::weeks(2)).unix_timestamp();
    let old_offset = last_offset / 2;
    stream_tree
        .insert(&utils::encode_byte_prefix_i64(PREFIX_STREAM_TS, ts_two_weeks_ago), &utils::encode_u64(old_offset))
        .context("error inserting fake timestamp index record for compaction test")?;

    let earliest_timestamp = super::compact_stream(config, stream_tree.clone(), None)
        .await
        .context("unexpected error from compaction")?
        .context("expected next earliest timestamp record to be returned")?;

    assert_eq!(earliest_timestamp.0, last_ts, "expected earliest timestamp to be {}, got {}", earliest_timestamp.0, last_ts);
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

#[test]
fn calculate_initial_compaction_delay_returns_delta_under_30_min() {
    let expected_seconds = time::OffsetDateTime::now_utc().unix_timestamp() - (60 * 25);
    let expected_output = time::Duration::seconds(expected_seconds);
    let output = super::calculate_initial_compaction_delay(Some(60 * 25));
    assert_eq!(output, expected_output, "unexpected duration returned, expected {:?}, got {:?}", expected_output, output);
}
