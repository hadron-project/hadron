use crate::grpc::stream::Event;
#[cfg(test)]
use crate::grpc::stream::EventPartition;

impl Event {
    #[cfg(test)]
    pub fn new_test<T: ToString>(id: T, source: &str, r#type: &str, partition: Option<EventPartition>) -> Self {
        Self {
            id: id.to_string(),
            source: source.into(),
            specversion: "1.0".into(),
            r#type: r#type.into(),
            partition,
            optattrs: Default::default(),
            data: Default::default(),
        }
    }
}
