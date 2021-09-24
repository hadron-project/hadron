use super::stream::Event;

impl Event {
    /// Create a new event.
    pub fn new(id: String, source: String, r#type: String, data: Vec<u8>) -> Self {
        Self {
            id,
            source,
            specversion: "1.0".into(),
            r#type,
            data,
            ..Default::default()
        }
    }

    /// Set the `specversion` field of the event, defaults to `"1.0"`.
    pub fn with_specversion(mut self, specversion: String) -> Self {
        self.specversion = specversion;
        self
    }

    /// Set the `optattrs` field of the event.
    ///
    /// **Warning:** this will overwrite any previously set optional or extension attributes.
    pub fn with_optattrs(mut self, optattrs: std::collections::HashMap<String, String>) -> Self {
        self.optattrs = optattrs;
        self
    }

    /// Set the optional `datacontenttype` field of the event.
    pub fn with_datacontenttype(mut self, datacontenttype: String) -> Self {
        self.optattrs.insert("datacontenttype".into(), datacontenttype);
        self
    }

    /// Get the optional `datacontenttype` field of the event.
    pub fn get_datacontenttype(&self) -> Option<&String> {
        self.optattrs.get("datacontenttype")
    }

    /// Set the optional `dataschema` field of the event.
    pub fn with_dataschema(mut self, dataschema: String) -> Self {
        self.optattrs.insert("dataschema".into(), dataschema);
        self
    }

    /// Get the optional `dataschema` field of the event.
    pub fn get_dataschema(&self) -> Option<&String> {
        self.optattrs.get("dataschema")
    }

    /// Set the optional `subject` field of the event.
    pub fn with_subject(mut self, subject: String) -> Self {
        self.optattrs.insert("subject".into(), subject);
        self
    }

    /// Get the optional `subject` field of the event.
    pub fn get_subject(&self) -> Option<&String> {
        self.optattrs.get("subject")
    }

    /// Set the optional `time` field of the event.
    pub fn with_time(mut self, time: String) -> Self {
        self.optattrs.insert("time".into(), time);
        self
    }

    /// Get the optional `time` field of the event.
    pub fn get_time(&self) -> Option<&String> {
        self.optattrs.get("time")
    }

    /// Set an optional extension attribute on the event.
    pub fn with_extattr(mut self, key: String, val: String) -> Self {
        self.optattrs.insert(key, val);
        self
    }

    /// Get an optional extension attribute of the event.
    pub fn get_extattr(&self, key: &str) -> Option<&String> {
        self.optattrs.get(key)
    }
}
