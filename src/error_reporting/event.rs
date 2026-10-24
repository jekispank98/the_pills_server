use chrono::{SecondsFormat, Utc};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use uuid::Uuid;

/// Values shared by every event of this process.
#[derive(Debug, Clone)]
pub struct EventDefaults {
    pub release: String,
    pub environment: String,
    pub server_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Error,
    Fatal,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub function: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub in_app: bool,
}

#[derive(Debug, Clone)]
pub struct Exception {
    pub kind: String,
    pub value: String,
    /// Innermost call first, as a backtrace prints it.
    pub frames: Vec<Frame>,
}

#[derive(Debug, Clone)]
pub struct Event {
    pub level: Level,
    pub logger: String,
    pub message: String,
    pub fingerprint: Vec<String>,
    pub transaction: Option<String>,
    pub user_id: Option<String>,
    pub tags: BTreeMap<String, String>,
    pub extra: Map<String, Value>,
    pub exception: Option<Exception>,
}

impl Event {
    pub fn to_envelope(&self, defaults: &EventDefaults) -> Vec<u8> {
        let event_id = Uuid::new_v4().simple().to_string();
        let payload = self.to_json(&event_id, defaults);
        let payload = serde_json::to_vec(&payload).unwrap_or_default();
        let header = json!({
            "event_id": event_id,
            "sent_at": Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        });
        let item_header = json!({ "type": "event", "length": payload.len() });

        let mut envelope = Vec::with_capacity(payload.len() + 256);
        envelope.extend_from_slice(header.to_string().as_bytes());
        envelope.push(b'\n');
        envelope.extend_from_slice(item_header.to_string().as_bytes());
        envelope.push(b'\n');
        envelope.extend_from_slice(&payload);
        envelope.push(b'\n');
        envelope
    }

    fn to_json(&self, event_id: &str, defaults: &EventDefaults) -> Value {
        let mut event = json!({
            "event_id": event_id,
            "timestamp": Utc::now().timestamp_micros() as f64 / 1_000_000.0,
            "platform": "native",
            "level": match self.level {
                Level::Error => "error",
                Level::Fatal => "fatal",
            },
            "logger": self.logger,
            "message": self.message,
            "fingerprint": self.fingerprint,
            "release": defaults.release,
            "environment": defaults.environment,
            "tags": self.tags,
            "extra": self.extra,
            "contexts": {
                "os": { "name": std::env::consts::OS },
                "runtime": { "name": "rust" },
            },
        });
        let object = event.as_object_mut().expect("event is an object");
        if let Some(server_name) = &defaults.server_name {
            object.insert("server_name".into(), json!(server_name));
        }
        if let Some(transaction) = &self.transaction {
            object.insert("transaction".into(), json!(transaction));
        }
        if let Some(user_id) = &self.user_id {
            object.insert("user".into(), json!({ "id": user_id }));
        }
        if let Some(exception) = &self.exception {
            let frames: Vec<Value> = exception
                .frames
                .iter()
                .rev()
                .map(|frame| {
                    json!({
                        "function": frame.function,
                        "filename": frame.file,
                        "lineno": frame.line,
                        "in_app": frame.in_app,
                    })
                })
                .collect();
            object.insert(
                "exception".into(),
                json!({
                    "values": [{
                        "type": exception.kind,
                        "value": exception.value,
                        "mechanism": { "type": "panic", "handled": false },
                        "stacktrace": { "frames": frames },
                    }]
                }),
            );
        }
        event
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> EventDefaults {
        EventDefaults {
            release: "pills_server_test@0.1.0".into(),
            environment: "production".into(),
            server_name: Some("api-1".into()),
        }
    }

    fn parse_envelope(envelope: &[u8]) -> (Value, Value, Value) {
        let text = std::str::from_utf8(envelope).unwrap();
        let mut lines = text.lines();
        let header = serde_json::from_str(lines.next().unwrap()).unwrap();
        let item_header: Value = serde_json::from_str(lines.next().unwrap()).unwrap();
        let payload_line = lines.next().unwrap();
        assert_eq!(item_header["length"], payload_line.len());
        (
            header,
            item_header,
            serde_json::from_str(payload_line).unwrap(),
        )
    }

    #[test]
    fn envelope_has_headers_and_event() {
        let event = Event {
            level: Level::Error,
            logger: "pills_server_test::error".into(),
            message: "database query failed".into(),
            fingerprint: vec![
                "pills_server_test::error".into(),
                "database query failed".into(),
            ],
            transaction: Some("GET /persons/{id}".into()),
            user_id: Some("7d1f".into()),
            tags: BTreeMap::from([("http.method".into(), "GET".into())]),
            extra: Map::from_iter([("error".into(), json!("pool timed out"))]),
            exception: None,
        };

        let (header, item_header, payload) = parse_envelope(&event.to_envelope(&defaults()));

        assert_eq!(item_header["type"], "event");
        assert_eq!(header["event_id"], payload["event_id"]);
        assert_eq!(payload["level"], "error");
        assert_eq!(payload["message"], "database query failed");
        assert_eq!(payload["transaction"], "GET /persons/{id}");
        assert_eq!(payload["user"]["id"], "7d1f");
        assert_eq!(payload["release"], "pills_server_test@0.1.0");
        assert_eq!(payload["environment"], "production");
        assert_eq!(payload["server_name"], "api-1");
        assert_eq!(payload["tags"]["http.method"], "GET");
        assert_eq!(payload["extra"]["error"], "pool timed out");
        assert!(payload.get("exception").is_none());
    }

    #[test]
    fn panic_frames_are_sent_oldest_first() {
        let frame = |function: &str| Frame {
            function: function.into(),
            file: None,
            line: None,
            in_app: true,
        };
        let event = Event {
            level: Level::Fatal,
            logger: "panic".into(),
            message: "boom".into(),
            fingerprint: vec!["panic".into()],
            transaction: None,
            user_id: None,
            tags: BTreeMap::new(),
            extra: Map::new(),
            exception: Some(Exception {
                kind: "panic".into(),
                value: "boom".into(),
                frames: vec![frame("inner"), frame("outer")],
            }),
        };

        let (_, _, payload) = parse_envelope(&event.to_envelope(&defaults()));
        let exception = &payload["exception"]["values"][0];

        assert_eq!(payload["level"], "fatal");
        assert_eq!(exception["mechanism"]["handled"], false);
        assert_eq!(exception["stacktrace"]["frames"][0]["function"], "outer");
        assert_eq!(exception["stacktrace"]["frames"][1]["function"], "inner");
        assert!(payload.get("user").is_none());
    }
}
