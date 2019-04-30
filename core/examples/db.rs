use chrono::prelude::*;
use serde_derive::{Serialize, Deserialize};
use serde_json;
use sled;

fn main() {
    let _db = sled::Db::start_default("./target/db-testing-persistence").expect("Failed to open database.");
    let db = _db.open_tree("/region/stream/").expect("Failed to open DB namespace for '/region/stream/'.");

    let initial: List<Payload> = List::from(vec![]);
    let serialized = serde_json::to_string(&initial.state()).expect("Failed to serialize initial data.");
    let decoded = serde_json::from_str(&serialized).expect("Failed to deserialize initial data.");
    let mut state: List<Payload> = List::from_state(decoded, Some(1)).expect("Failed to create new state instance from deserialized data.");

    for _ in 0..900 {
        let id = _db.generate_id().expect("Failed to generate an ID.");
        let op = state.push(Payload{id, ts: Utc::now().timestamp_millis(), uid: None, data: vec![]}).expect("Failed to push new state.");
        let elem = op.inserted_element().unwrap();
        let key = elem.uid.to_string();
        let payload = serde_json::to_vec(&elem.value).unwrap();

        db.set(key, payload).expect("Failed to insert into database.");
    }

    // OBSERVATIONS:
    // - keeping the data ordered in the database is important.
    // - inasmuch as absolute ordering is required for persistent streams, CRDTs may not be as hot
    //   for this usecase as originally anticipated.
    // - data will never be removed from a stream, by definition it is not allowed in this
    //   system. Though, perhaps raft can still be used just as a cluster membership system. What
    //   would we need a log for if everything is append only? If everything went through a proper
    //   raft log, it would still need to be replicated. Instead, we can just have the

    // // Check DB len.
    // println!("Db len: {}", db.len());
    // let (last_key, last_val) = db.iter().nth(0).unwrap().unwrap();
    // let lkey = String::from_utf8(last_key).expect("decode key");
    // let lval: Payload = serde_json::from_slice(&last_val).expect("decode val");
    // println!("Last key,val: {:?}: {:?}", lkey, lval);
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Payload {
    /// The ID is the key used to uniquely identify this message on its stream.
    pub id: u64,

    /// The milliseconds epoch timestamp when this message was persisted.
    pub ts: i64,

    /// The user supplied unique ID for this message, used only for ID checking streams.
    ///
    /// This mechanism is primarily intended for use in, though not limited to, deduplication. For
    /// streams which perform ID checking on inbound messages, if the user supplied ID has already
    /// been used on the stream, then an error will be returned identifying the issue.
    ///
    /// This is particularly powerful in that if a client receives a duplicate ID error, they can
    /// be sure that they've already published the particular message to the stream (perhaps due
    /// to retry logic, or the like). This is, by necesity, based upon the user's ID generation
    /// system though. Generally speaking, using a valid UUID4 generation pattern should suffice.
    pub uid: Option<String>,

    /// This represents whatever data the user supplied for being written to the stream.
    pub data: Vec<u8>,
}
