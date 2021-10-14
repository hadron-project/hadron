use crate::grpc::stream::Event;

impl Event {
    #[cfg(test)]
    pub fn new_test<T: ToString>(id: T, source: &str, r#type: &str) -> Self {
        Self {
            id: id.to_string(),
            source: source.into(),
            specversion: "1.0".into(),
            r#type: r#type.into(),
            optattrs: Default::default(),
            data: Default::default(),
        }
    }
}
