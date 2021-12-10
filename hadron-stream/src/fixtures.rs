use anyhow::{Context, Result};
use rand::prelude::*;

use crate::grpc::{Event, EventPartition};
use crate::models::stream::Subscription;
use crate::stream::{KEY_STREAM_LAST_WRITTEN_OFFSET, PREFIX_STREAM_EVENT, PREFIX_STREAM_SUBS, PREFIX_STREAM_SUB_OFFSETS, PREFIX_STREAM_TS};
use crate::utils;

/// Setup some stream data in the given DB tree returning the last written offset.
pub async fn setup_stream_data(db: &sled::Tree) -> Result<(i64, u64)> {
    let mut batch = sled::Batch::default();
    let mut last_offset = 0;
    let ts = time::OffsetDateTime::now_utc().unix_timestamp();
    for offset in 0..rand::thread_rng().gen_range(50..100) {
        let event = Event::new_test(offset, "test", "empty", Some(EventPartition { partition: 0, offset }));
        let event_bytes = utils::encode_model(&event)?;
        batch.insert(&utils::encode_byte_prefix(PREFIX_STREAM_EVENT, offset), event_bytes.as_slice());
        last_offset = offset;
    }
    batch.insert(KEY_STREAM_LAST_WRITTEN_OFFSET, &utils::encode_u64(last_offset));
    batch.insert(&utils::encode_byte_prefix_i64(PREFIX_STREAM_TS, ts), &utils::encode_u64(last_offset));
    db.apply_batch(batch).context("error applying batch to write test data to stream")?;
    Ok((ts, last_offset))
}

/// Setup some subscriptions data in the given DB tree returning the set of created subs.
pub async fn setup_subs_data(db: &sled::Tree) -> Result<Vec<(Subscription, u64)>> {
    let mut batch = sled::Batch::default();
    let mut subs = vec![];
    for offset in 0..rand::thread_rng().gen_range(50..100) {
        let sub = Subscription {
            group_name: offset.to_string(),
            max_batch_size: 50,
        };
        let sub_encoded = utils::encode_model(&sub)?;
        let sub_model_key = utils::ivec_from_iter(PREFIX_STREAM_SUBS.iter().copied().chain(sub.group_name.as_bytes().iter().copied()));
        let sub_offset_key = utils::ivec_from_iter(PREFIX_STREAM_SUB_OFFSETS.iter().copied().chain(sub.group_name.as_bytes().iter().copied()));
        batch.insert(sub_model_key, sub_encoded.as_slice());
        batch.insert(sub_offset_key, &utils::encode_u64(offset));
        subs.push((sub, offset));
    }
    db.apply_batch(batch).context("error applying batch to write test data to stream")?;
    subs.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(subs)
}
