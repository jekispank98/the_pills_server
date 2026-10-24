use super::backtrace;
use super::event::{Event, EventDefaults, Exception, Level};
use super::transport::Transport;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;
use tracing::Subscriber;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

/// Target of the event the panic hook emits.
pub const PANIC_TARGET: &str = "panic";
const PANIC_DELIVERY_TIMEOUT: Duration = Duration::from_secs(3);
const FILTERED: &str = "[Filtered]";
const SENSITIVE_KEYS: [&str; 6] = [
    "password",
    "token",
    "secret",
    "email",
    "authorization",
    "cookie",
];

/// Turns every ERROR event into a Sentry-protocol event, with the fields of
/// the enclosing spans (request method, route, user id) attached.
pub struct ErrorReportingLayer {
    transport: Transport,
    defaults: EventDefaults,
    panic_delivery_timeout: Duration,
}

impl ErrorReportingLayer {
    pub fn new(transport: Transport, defaults: EventDefaults) -> Self {
        Self {
            transport,
            defaults,
            panic_delivery_timeout: PANIC_DELIVERY_TIMEOUT,
        }
    }
}

#[derive(Default)]
struct SpanFields(Map<String, Value>);

impl<S> Layer<S> for ErrorReportingLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let mut fields = SpanFields::default();
        attrs.record(&mut JsonVisitor(&mut fields.0));
        span.extensions_mut().insert(fields);
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        if let Some(fields) = span.extensions_mut().get_mut::<SpanFields>() {
            values.record(&mut JsonVisitor(&mut fields.0));
        }
    }

    fn on_event(&self, event: &tracing::Event<'_>, ctx: Context<'_, S>) {
        let metadata = event.metadata();
        if *metadata.level() != tracing::Level::ERROR {
            return;
        }

        let mut fields = Map::new();
        event.record(&mut JsonVisitor(&mut fields));
        let mut span_fields = Map::new();
        if let Some(scope) = ctx.event_scope(event) {
            for span in scope.from_root() {
                if let Some(SpanFields(values)) = span.extensions().get::<SpanFields>() {
                    span_fields.extend(values.clone());
                }
            }
        }

        let is_panic = metadata.target() == PANIC_TARGET;
        let report = build_event(metadata.target(), fields, span_fields, is_panic);
        let envelope = report.to_envelope(&self.defaults);
        if is_panic {
            self.transport
                .send_and_wait(envelope, self.panic_delivery_timeout);
        } else {
            self.transport.send(envelope);
        }
    }
}

fn build_event(
    target: &str,
    mut fields: Map<String, Value>,
    span_fields: Map<String, Value>,
    is_panic: bool,
) -> Event {
    let message = take_string(&mut fields, "message").unwrap_or_else(|| target.to_string());
    let method = string_field(&span_fields, "method");
    let route = string_field(&span_fields, "route").or_else(|| string_field(&span_fields, "path"));
    let user_id = string_field(&span_fields, "user_id");

    let mut tags = BTreeMap::new();
    if let Some(method) = &method {
        tags.insert("http.method".to_string(), method.clone());
    }
    if let Some(route) = &route {
        tags.insert("http.route".to_string(), route.clone());
    }
    let transaction = match (&method, &route) {
        (Some(method), Some(route)) => Some(format!("{method} {route}")),
        _ => None,
    };

    let (exception, fingerprint) = if is_panic {
        let location = take_string(&mut fields, "location").unwrap_or_default();
        let frames = take_string(&mut fields, "backtrace")
            .map(|text| backtrace::parse(&text))
            .unwrap_or_default();
        let exception = Exception {
            kind: "panic".to_string(),
            value: message.clone(),
            frames,
        };
        (Some(exception), vec!["panic".to_string(), location])
    } else {
        (None, vec![target.to_string(), message.clone()])
    };

    let mut extra = Map::new();
    for (key, value) in fields {
        let value = if is_sensitive(&key) {
            json!(FILTERED)
        } else {
            value
        };
        extra.insert(key, value);
    }

    Event {
        level: if is_panic { Level::Fatal } else { Level::Error },
        logger: target.to_string(),
        message,
        fingerprint,
        transaction,
        user_id,
        tags,
        extra,
        exception,
    }
}

fn is_sensitive(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    SENSITIVE_KEYS
        .iter()
        .any(|sensitive| key.contains(sensitive))
}

fn take_string(fields: &mut Map<String, Value>, key: &str) -> Option<String> {
    match fields.remove(key)? {
        Value::String(text) => Some(text),
        other => Some(other.to_string()),
    }
}

fn string_field(fields: &Map<String, Value>, key: &str) -> Option<String> {
    match fields.get(key)? {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::String(_) | Value::Null => None,
        other => Some(other.to_string()),
    }
}

struct JsonVisitor<'a>(&'a mut Map<String, Value>);

impl Visit for JsonVisitor<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.0
            .insert(field.name().to_string(), json!(format!("{value:?}")));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().to_string(), json!(value));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.0.insert(field.name().to_string(), json!(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.0.insert(field.name().to_string(), json!(value));
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.0.insert(field.name().to_string(), json!(value));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.0.insert(field.name().to_string(), json!(value));
    }

    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        self.0
            .insert(field.name().to_string(), json!(value.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::super::transport::Message;
    use super::*;
    use std::sync::mpsc::{Receiver, sync_channel};
    use tracing_subscriber::layer::SubscriberExt;

    fn capture(emit: impl FnOnce()) -> Vec<Value> {
        let (queue, receiver) = sync_channel(10);
        let mut layer = ErrorReportingLayer::new(
            Transport::from_queue(queue),
            EventDefaults {
                release: "test@1".into(),
                environment: "test".into(),
                server_name: None,
            },
        );
        layer.panic_delivery_timeout = Duration::from_millis(10);
        let subscriber = tracing_subscriber::registry().with(layer);
        tracing::subscriber::with_default(subscriber, emit);
        payloads(receiver)
    }

    fn payloads(receiver: Receiver<Message>) -> Vec<Value> {
        receiver
            .try_iter()
            .filter_map(|message| match message {
                Message::Envelope { body, .. } => {
                    let text = String::from_utf8(body).unwrap();
                    Some(serde_json::from_str(text.lines().nth(2).unwrap()).unwrap())
                }
                Message::Flush(_) => None,
            })
            .collect()
    }

    #[test]
    fn only_errors_are_reported() {
        let events = capture(|| {
            tracing::info!("started");
            tracing::warn!("slow");
            tracing::error!(error = "pool timed out", "database query failed");
        });

        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["message"], "database query failed");
        assert_eq!(events[0]["extra"]["error"], "pool timed out");
        assert_eq!(events[0]["level"], "error");
    }

    #[test]
    fn request_span_gives_route_and_user() {
        let events = capture(|| {
            let span = tracing::info_span!(
                "request",
                method = "GET",
                route = "/persons/{id}",
                path = "/persons/5f0c",
                user_id = tracing::field::Empty,
            );
            let _entered = span.enter();
            tracing::Span::current().record("user_id", "7d1f");
            tracing::error!("database query failed");
        });

        assert_eq!(events[0]["transaction"], "GET /persons/{id}");
        assert_eq!(events[0]["tags"]["http.route"], "/persons/{id}");
        assert_eq!(events[0]["user"]["id"], "7d1f");
        assert_eq!(
            events[0]["fingerprint"],
            json!([module_path!(), "database query failed"])
        );
    }

    #[test]
    fn sensitive_fields_are_filtered() {
        let events = capture(|| {
            tracing::error!(
                email = "user@example.com",
                refresh_token = "abc",
                "failed to send"
            );
        });

        assert_eq!(events[0]["extra"]["email"], FILTERED);
        assert_eq!(events[0]["extra"]["refresh_token"], FILTERED);
    }

    #[test]
    fn panic_event_is_fatal_with_frames() {
        let events = capture(|| {
            tracing::error!(
                target: PANIC_TARGET,
                location = "src/handlers/persons.rs:42:9",
                backtrace = "   0: pills_server_test::handlers::persons::get_person\n             at ./src/handlers/persons.rs:42:9",
                "index out of bounds"
            );
        });
        let exception = &events[0]["exception"]["values"][0];

        assert_eq!(events[0]["level"], "fatal");
        assert_eq!(exception["value"], "index out of bounds");
        assert_eq!(exception["stacktrace"]["frames"][0]["lineno"], 42);
        assert_eq!(
            events[0]["fingerprint"],
            json!(["panic", "src/handlers/persons.rs:42:9"])
        );
        assert!(events[0]["extra"].get("backtrace").is_none());
    }
}
