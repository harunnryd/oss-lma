use serde::Serialize;
use serde_json::Value;

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
