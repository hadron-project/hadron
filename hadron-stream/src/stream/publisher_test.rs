use anyhow::{Context, Result};
use rand::prelude::*;
use tokio::sync::watch;

use crate::config::Config;
use crate::database::Database;
use crate::error::AppError;
use crate::grpc::{Event, EventPartition, StreamPublishRequest};
use crate::stream::{KEY_STREAM_LAST_WRITTEN_OFFSET, PREFIX_STREAM_EVENT};
use crate::utils;

use super::PREFIX_STREAM_TS;

#[tokio::test]
async fn publish_data_frame_err_with_empty_batch() -> Result<()> {
    let (config, _tmpdir) = Config::new_test()?;
    let db = Database::new(config.clone()).await?;
    let stream_tree = db.get_stream_tree().await?;
    let (mut current_offset, mut earliest_timestamp) = (0u64, None);
    let (tx, rx) = watch::channel(current_offset);
    let req = StreamPublishRequest { batch: vec![], fsync: true, ack: 0 };

    let res = super::StreamCtl::publish_data_frame(&stream_tree, &mut current_offset, 0, &mut earliest_timestamp, &tx, req).await;

    let last_watcher_offset = *rx.borrow();
    assert_eq!(last_watcher_offset, current_offset, "expected watcher offset to be {} got {}", current_offset, last_watcher_offset);
    assert!(res.is_err(), "expected an error to be returned");
    let err = res.unwrap_err();
    let app_err = err.downcast::<AppError>().context("unexpected error type")?;
    assert!(matches!(app_err, AppError::InvalidInput(val) if val == "entries batch was empty, no-op"), "unexpected error returned");
    assert!(earliest_timestamp.is_none(), "expected earliest_timestamp to remain `None`, got {:?}", earliest_timestamp);

    Ok(())
}

#[tokio::test]
async fn publish_data_frame() -> Result<()> {
    let (config, _tmpdir) = Config::new_test()?;
    let db = Database::new(config.clone()).await?;
    let stream_tree = db.get_stream_tree().await?;
    let (mut current_offset, mut earliest_timestamp, partition) = (0u64, None, 7);
    let (tx, rx) = watch::channel(current_offset);
    let expected_ts_min = time::OffsetDateTime::now_utc().unix_timestamp() - 5;

    let mut req = StreamPublishRequest { batch: vec![], fsync: true, ack: 0 };
    let (mut expected_events, expected_offset) = (1u64..rand::thread_rng().gen_range(50u64..100u64)).fold((vec![], 0u64), |(mut events, _), offset| {
        let event = Event::new_test(offset, "test", "empty", Some(EventPartition { partition, offset }));
        req.batch.push(event.clone());
        events.push(event);
        (events, offset)
    });
    expected_events.sort_by(|a, b| a.id.cmp(&b.id));

    let last_offset = super::StreamCtl::publish_data_frame(&stream_tree, &mut current_offset, partition, &mut earliest_timestamp, &tx, req).await?;

    // Check emitted info on last offset.
    let last_watcher_offset = *rx.borrow();
    assert!(last_offset == expected_offset, "expected offset to be {} got {}", expected_offset, last_offset);
    assert!(last_watcher_offset == expected_offset, "expected watcher offset to be {} got {}", expected_offset, last_watcher_offset);

    // Check all written events.
    let mut events = vec![];
    for kv_res in stream_tree.scan_prefix(PREFIX_STREAM_EVENT) {
        let (offset, val) = kv_res.context("error reading data from stream in test")?;
        let offset = utils::decode_u64(&offset[1..])?;
        let val: Event = utils::decode_model(&val)?;
        // Ensure partition/offset info has been updated as part of the write.
        assert_eq!(
            Some(partition),
            val.partition.as_ref().map(|prtn| prtn.partition),
            "expected event partition to be {}, got {:?}",
            partition,
            val.partition,
        );
        assert_eq!(
            Some(offset),
            val.partition.as_ref().map(|prtn| prtn.offset),
            "expected event offset to be {}, got {:?}",
            offset,
            val.partition,
        );
        events.push(val);
    }
    events.sort_by(|a, b| a.id.cmp(&b.id));
    assert_eq!(events, expected_events, "unexpected data on stream\nexpected: {:?}\ngot: {:?}", expected_events, events);

    // Check storage for the last offset key.
    let db_offset_ivec = stream_tree
        .get(KEY_STREAM_LAST_WRITTEN_OFFSET)
        .context("error fetching last written offset from storage")?
        .context("no value found for last written offset")?;
    let db_offset = utils::decode_u64(&db_offset_ivec)?;
    assert_eq!(db_offset, expected_offset, "expected db last written offset to be {} got {}", expected_offset, db_offset);

    // Check for secondary timestamp index.
    let ts_idx = stream_tree
        .scan_prefix(PREFIX_STREAM_TS)
        .try_fold(vec![], |mut acc, kv_res| -> Result<Vec<(i64, u64)>> {
            let (key, val) = kv_res.context("error scanning stream timestamp index")?;
            let ts = utils::decode_i64(&key[1..])?;
            let offset = utils::decode_u64(&val)?;
            acc.push((ts, offset));
            Ok(acc)
        })
        .context("error reading stream timestamp index")?;
    assert_eq!(ts_idx.len(), 1, "expected one timestamp index entry, got {}", ts_idx.len());
    assert_eq!(
        ts_idx[0].1, expected_offset,
        "expected timestamp index entry to point to offset {} got {}",
        expected_offset, ts_idx[0].1
    );
    assert!(ts_idx[0].0 > expected_ts_min, "expected stream index entry timestamp {} to be > {}", expected_ts_min, ts_idx[0].0);
    assert!(earliest_timestamp.is_some(), "expected earliest_timestamp to be updated");
    assert_eq!(
        earliest_timestamp.map(|val| val.0),
        Some(ts_idx[0].0),
        "expected earliest_timestamp to be {:?}, got {:?}",
        ts_idx[0].0,
        earliest_timestamp.map(|val| val.0)
    );
    assert_eq!(
        earliest_timestamp.map(|val| val.1),
        Some(ts_idx[0].1),
        "expected earliest_timestamp to be {:?}, got {:?}",
        ts_idx[0].1,
        earliest_timestamp.map(|val| val.1)
    );

    Ok(())
}
