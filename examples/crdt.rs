//! A demonstration of the CRDT system in action.
//!
//! The main idea being demonstrated here is the process various clients connecting to different
//! nodes within a cluster and writing events to the same stream.
//!
//! This simulation uses a single producer, but uses an RNG to randomly select the node to handle
//! the write operation.
//!
//! As nodes receive write requests, they also broadcast out their new data for replication to
//! their peers.

use std::{
    sync::mpsc,
    thread,
};

use ditto::{List, ListState};
use rand::distributions::{Distribution, Uniform};
use serde_derive::{Serialize, Deserialize};
use serde_json;
use sled;

const NODE1: u32 = 1;
const NODE2: u32 = 2;
const NODE3: u32 = 3;

fn main() {
    let (chan1, chan2, chan3) = (mpsc::channel(), mpsc::channel(), mpsc::channel());
    let j1 = Node::spawn(NODE1, chan1.1, chan2.0.clone(), chan3.0.clone());
    let j2 = Node::spawn(NODE2, chan2.1, chan1.0.clone(), chan3.0.clone());
    let j3 = Node::spawn(NODE3, chan3.1, chan1.0.clone(), chan2.0.clone());
    let chans: [mpsc::Sender<Event>; 3] = [chan1.0, chan2.0, chan3.0];

    // Build RNG.
    let between = Uniform::from(NODE1..=NODE3);
    let mut rng = rand::thread_rng();
    for msg in 0..1000 {
        let target_node = between.sample(&mut rng);
        let node_chan = &chans[target_node as usize - 1];
        node_chan.send(Event::new_payload(msg, target_node)).expect("Expected send to succeed.");
    }

    thread::sleep_ms(1000 * 7);
    chans[0].clone().send(Event::Done).expect("Expected send to succeed.");
    chans[1].clone().send(Event::Done).expect("Expected send to succeed.");
    chans[2].clone().send(Event::Done).expect("Expected send to succeed.");

    let state1 = j1.join().expect("Failed to join node1 thread.");
    let state2 = j2.join().expect("Failed to join node2 thread.");
    let state3 = j3.join().expect("Failed to join node3 thread.");

    assert_eq!(state1, state2);
    assert_eq!(state2, state3);
    assert_eq!(state1, state3);
}

enum Event {
    Payload(Payload),
    Replicate(ListState<'static, Payload>),
    Done,
}

impl Event {
    pub fn new_payload(eventnum: u64, targetid: u32) -> Self {
        Event::Payload(Payload{eventnum, targetid})
    }

    pub fn new_repliacte(payload: ListState<'static, Payload>) -> Self {
        Event::Replicate(payload)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Payload {
    pub eventnum: u64,
    pub targetid: u32,
}

struct Node {
    id: u32,
    input: mpsc::Receiver<Event>,
    peers: [mpsc::Sender<Event>; 2],
    state: List<Payload>,
    db: sled::Db,
}

impl Node {
    pub fn spawn(id: u32, input: mpsc::Receiver<Event>, peer0: mpsc::Sender<Event>, peer1: mpsc::Sender<Event>) -> thread::JoinHandle<Vec<u64>> {
        let initial: List<Payload> = List::from(vec![]);
        let serialized = serde_json::to_string(&initial.state()).expect("Expected to be able to serialize initial data.");
        let decoded = serde_json::from_str(&serialized).expect("Expected to be able to deserialize initial data.");
        let state: List<Payload> = List::from_state(decoded, Some(id)).expect("Expected to be able to create new state instance from deserialized data.");
        let db = sled::Db::start_default(format!("./target/db{}", id)).expect("Failed to open database.");

        let node = Node{id, input, peers: [peer0, peer1], state, db};
        thread::spawn(move || {
            node.run()
        })
    }

    fn run(mut self) -> Vec<u64> {
        loop {
            let event = self.input.recv().unwrap();
            match event {
                Event::Done => {
                    println!("Crunching final state for node {}", self.id);
                    return self.state.local_value().into_iter().map(|payload: Payload| payload.eventnum).collect();
                },
                Event::Payload(payload) => self.handle_payload(payload),
                Event::Replicate(payload) => self.handle_replication(payload),
            }
        }
    }

    fn handle_payload(&mut self, payload: Payload) {
        let op = self.state.push(payload).expect("Expected to be able to push new state in handle_payload.");
        let elem = op.inserted_element().unwrap();
        let _key = elem.uid.to_vlq();
        for peer in &self.peers {
            peer.send(Event::new_repliacte(self.state.clone_state())).expect("Expected to be able to send replication event.");
        }
    }

    fn handle_replication(&mut self, payload: ListState<'static, Payload>) {
        self.state.merge(payload).expect("Expected to be able to merge replicated state.");
    }
}
