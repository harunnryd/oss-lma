use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ErrorCode;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "EventType", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ControlMessage {
    Start {
        #[serde(rename = "CallId")]
        call_id: String,
        #[serde(rename = "SamplingRate")]
        sampling_rate: usize,
        #[serde(rename = "DiarizeSystemChannel")]
        diarize_system_channel: bool,
        #[serde(rename = "DiarizeMicChannel")]
        diarize_mic_channel: bool,
    },
    Pause {
        #[serde(rename = "CallId")]
        call_id: String,
    },
    Resume {
        #[serde(rename = "CallId")]
        call_id: String,
    },
    End {
        #[serde(rename = "CallId")]
        call_id: String,
    },
}

impl ControlMessage {
    pub fn start(
        call_id: impl Into<String>,
        sampling_rate: usize,
        diarize_mic_channel: bool,
    ) -> Self {
        Self::Start {
            call_id: call_id.into(),
            sampling_rate,
            diarize_system_channel: false,
            diarize_mic_channel,
        }
    }

    pub fn pause(call_id: impl Into<String>) -> Self {
        Self::Pause {
            call_id: call_id.into(),
        }
    }

    pub fn resume(call_id: impl Into<String>) -> Self {
        Self::Resume {
            call_id: call_id.into(),
        }
    }

    pub fn end(call_id: impl Into<String>) -> Self {
        Self::End {
            call_id: call_id.into(),
        }
    }

    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).expect("control messages are serializable")
    }
}

#[derive(Deserialize)]
#[serde(tag = "EventType")]
enum IncomingMessage {
    #[serde(rename = "ERROR")]
    Error {
        #[serde(rename = "CallId")]
        call_id: String,
        #[serde(rename = "Code")]
        code: ErrorCode,
        #[serde(rename = "Context", default = "empty_context")]
        context: Value,
    },
    #[serde(other)]
    Other,
}

fn empty_context() -> Value {
    Value::Object(Default::default())
}

pub fn parse_error(raw: &str) -> Option<(String, ErrorCode, Value)> {
    match serde_json::from_str(raw).ok()? {
        IncomingMessage::Error {
            call_id,
            code,
            context,
        } => Some((call_id, code, context)),
        IncomingMessage::Other => None,
    }
}

pub fn parse_meeting_event(raw: &str) -> Option<Value> {
    let event: Value = serde_json::from_str(raw).ok()?;
    let object = event.as_object()?;
    (object.get("EventType")?.as_str()? == "ADD_TRANSCRIPT_SEGMENT").then_some(())?;
    for field in ["CallId", "SegmentId", "Channel", "Transcript"] {
        object.get(field)?.as_str()?;
    }
    for field in ["StartTime", "EndTime"] {
        object.get(field)?.as_f64()?;
    }
    object.get("IsPartial")?.as_bool()?;
    Some(event)
}

#[cfg(test)]
#[test]
fn parses_a_transcript_envelope_without_losing_pascal_case_fields() {
    let raw = r#"{"EventType":"ADD_TRANSCRIPT_SEGMENT","CallId":"call-1","SegmentId":"s1","Channel":"CALLER","StartTime":0.0,"EndTime":1.0,"Transcript":"partial","IsPartial":true}"#;
    let event = parse_meeting_event(raw).expect("transcript envelope is accepted");
    assert_eq!(event["SegmentId"], "s1");
    assert_eq!(event["IsPartial"], true);
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::ControlMessage;

    fn validate_event(value: &Value) {
        let mut schema: Value =
            serde_json::from_str(include_str!("../../../contracts/events.schema.json"))
                .expect("event schema parses");
        schema["$defs"]
            .as_object_mut()
            .expect("definitions")
            .remove("Error");
        schema["oneOf"]
            .as_array_mut()
            .expect("events")
            .retain(|event| event["$ref"] != "#/$defs/Error");
        let validator = jsonschema::validator_for(&schema).expect("schema compiles");
        assert!(
            validator.is_valid(value),
            "event must satisfy events.schema.json: {value}"
        );
    }

    #[test]
    fn control_messages_use_pascal_case_wire_contract() {
        let call_id = "123e4567-e89b-12d3-a456-426614174000";
        let events = [
            ControlMessage::start(call_id, 48_000, true).to_json(),
            ControlMessage::pause(call_id).to_json(),
            ControlMessage::resume(call_id).to_json(),
            ControlMessage::end(call_id).to_json(),
        ];
        for event in events {
            validate_event(&event);
            assert!(event.get("call_id").is_none());
        }
    }
}
