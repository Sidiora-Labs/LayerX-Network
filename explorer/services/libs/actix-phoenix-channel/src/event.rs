use std::borrow::Cow;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ChannelEvent {
    pub(crate) topic: Cow<'static, str>,
    pub(crate) event: Cow<'static, str>,
    pub(crate) payload: Value,

    #[serde(rename = "ref")]
    pub(crate) reference: Option<Cow<'static, str>>,

    #[serde(rename = "join_ref")]
    pub(crate) join_reference: Option<Cow<'static, str>>,
}

impl ChannelEvent {
    pub fn build_reply(
        &self,
        event: impl Into<Cow<'static, str>>,
        payload: &impl Serialize,
    ) -> ChannelEvent {
        ChannelEvent {
            topic: self.topic.clone(),
            event: event.into(),
            payload: match serde_json::to_value(payload).expect("payload should be serializable") {
                Value::Null => Value::Object(Default::default()),
                other => other,
            },
            reference: self.reference.clone(),
            join_reference: self.join_reference.clone(),
        }
    }

    pub(crate) fn serialize(&self) -> serde_json::Result<String> {
        serde_json::to_string(&(
            &self.join_reference,
            &self.reference,
            &self.topic,
            &self.event,
            &self.payload,
        ))
    }

    pub(crate) fn deserialize(string: &str) -> serde_json::Result<Self> {
        let (join_reference, reference, topic, event, payload): (
            Option<String>,
            Option<String>,
            String,
            String,
            Value,
        ) = serde_json::from_str(string)?;
        Ok(Self {
            join_reference: join_reference.map(Into::into),
            reference: reference.map(Into::into),
            topic: topic.into(),
            event: event.into(),
            payload,
        })
    }

    pub fn topic(&self) -> &str {
        &self.topic
    }

    pub fn event(&self) -> &str {
        &self.event
    }

    pub fn payload(&self) -> &Value {
        &self.payload
    }

    pub fn reference(&self) -> Option<&str> {
        self.reference.as_deref()
    }

    pub fn new(
        topic: impl Into<Cow<'static, str>>,
        event: impl Into<Cow<'static, str>>,
        payload: &impl Serialize,
    ) -> Self {
        Self {
            topic: topic.into(),
            event: event.into(),
            payload: match serde_json::to_value(payload).unwrap() {
                Value::Null => Value::Object(Default::default()),
                other => other,
            },
            reference: None,
            join_reference: None,
        }
    }

    pub fn is_system_event(&self) -> bool {
        self.topic == "phoenix" || self.event == "phx_join" || self.event == "phx_leave"
    }
}

impl<T, E> From<(T, E)> for ChannelEvent
where
    T: Into<Cow<'static, str>>,
    E: Into<Cow<'static, str>>,
{
    fn from(te: (T, E)) -> Self {
        let (topic, event) = te;
        Self::new(topic, event, &())
    }
}

impl<T, E, P> From<(T, E, P)> for ChannelEvent
where
    T: Into<Cow<'static, str>>,
    E: Into<Cow<'static, str>>,
    P: Serialize,
{
    fn from(tep: (T, E, P)) -> Self {
        let (topic, event, payload) = tep;
        Self::new(topic, event, &payload)
    }
}
