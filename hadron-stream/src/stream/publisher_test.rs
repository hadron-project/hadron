use anyhow::{Context, Result};
use rand::prelude::*;
use tokio::sync::watch;

use crate::config::Config;
use crate::database::Database;
use crate::error::AppError;
use crate::grpc::{Event, StreamPublishRequest};
use crate::models::stream::Subscription;
use crate::utils;

#[tokio::test]
async fn publish_data_frame_err_with_empty_batch() -> Result<()> {
    let (config, _tmpdir) = Config::new_test()?;
    let db = Database::new(config.clone()).await?;
    let stream_tree = db.get_stream_tree().await?;
    let mut current_offset = 0u64;
    let (tx, rx) = watch::channel(current_offset);
    let req = StreamPublishRequest { batch: vec![], fsync: true, ack: 0 };

    let res = super::StreamCtl::publish_data_frame(&stream_tree, &mut current_offset, &tx, req).await;

    assert!(res.is_err(), "expected an error to be returned");
    let err = res.unwrap_err();
    let app_err = err.downcast::<AppError>().context("unexpected error type")?;
    assert!(
        matches!(app_err, AppError::InvalidInput(val) if val == "entries batch was empty, no-op"),
        "unexpected error returned",
    );

    Ok(())
}

#[tokio::test]
async fn publish_data_frame() -> Result<()> {
    let (config, _tmpdir) = Config::new_test()?;
    let db = Database::new(config.clone()).await?;
    let stream_tree = db.get_stream_tree().await?;
    let mut current_offset = 0u64;
    let (tx, rx) = watch::channel(current_offset);

    let mut req = StreamPublishRequest { batch: vec![], fsync: true, ack: 0 };
    let (mut expected_events, expected_offset) =
        (1u64..rand::thread_rng().gen_range(50u64..100u64)).fold((vec![], 0u64), |(mut events, _), offset| {
            let event = Event::new_test(offset, "test", "empty");
            req.batch.push(event.clone());
            events.push(event);
            (events, offset)
        });
    expected_events.sort_by(|a, b| a.id.cmp(&b.id));

    let last_offset = super::StreamCtl::publish_data_frame(&stream_tree, &mut current_offset, &tx, req).await?;

    let last_watcher_offset = *rx.borrow();
    assert!(
        last_offset == expected_offset,
        "expected offset to be {} got {}",
        expected_offset,
        last_offset
    );
    assert!(
        last_watcher_offset == expected_offset,
        "expected watcher offset to be {} got {}",
        expected_offset,
        last_watcher_offset
    );

    let mut events = vec![];
    for kv_res in stream_tree.iter() {
        let (_, val) = kv_res.context("error reading data from stream in test")?;
        let val: Event = utils::decode_model(&val)?;
        events.push(val);
    }
    events.sort_by(|a, b| a.id.cmp(&b.id));
    assert_eq!(
        events, expected_events,
        "unexpected data on stream\nexpected: {:?}\ngot: {:?}",
        expected_events, events,
    );

    Ok(())
}
