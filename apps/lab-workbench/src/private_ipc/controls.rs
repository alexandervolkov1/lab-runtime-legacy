//! Closed CONTROL-1 messages. No operation name, Runtime arguments or retry API.
//! One request credit and one coalesced feedback frame, each <= CONTROL-HEADER.
use super::protocol::{CONTROL, HEADER, Identity, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Action {
    ProposeRetune {
        control: String,
        target: f64,
        rate: f64,
    },
    Confirm {
        ticket: u64,
    },
    Cancel {
        ticket: u64,
    },
    Acknowledge {
        ticket: u64,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Request {
    pub(crate) nonce: u64,
    pub(crate) revision: u64,
    pub(crate) action: Action,
}
pub(crate) struct Envelope {
    pub(crate) identity: Identity,
    pub(crate) request: Request,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Stage {
    #[default]
    Idle,
    Confirmation,
    Submitted,
    Accepted,
    Completed,
    Failed,
    Ambiguous,
    Reconciled,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Feedback {
    pub(crate) revision: u64,
    pub(crate) receipt: u64,
    pub(crate) enabled: bool,
    pub(crate) stage: Stage,
    pub(crate) ticket: u64,
    pub(crate) result: Option<RetuneResult>,
    pub(crate) summary: String,
    pub(crate) detail: String,
    pub(crate) problem: Option<String>,
    pub(crate) can_confirm: bool,
    pub(crate) accepted_observed: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RetuneResult {
    pub(crate) reference: String,
    pub(crate) revision: String,
    pub(crate) value: f64,
    pub(crate) target: f64,
    pub(crate) rate: f64,
}
fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec(value).map_err(|_| "control encoding")?;
    if bytes.len() > CONTROL - HEADER {
        return Err("control payload bound");
    }
    Ok(bytes)
}
impl Request {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.nonce == 0 || self.revision == 0 {
            return Err("control request identity");
        }
        match &self.action {
            Action::ProposeRetune {
                control,
                target,
                rate,
            } if control.is_empty()
                || control.len() > 512
                || !target.is_finite()
                || !rate.is_finite()
                || *rate <= 0. =>
            {
                return Err("invalid retune candidate");
            }
            Action::Confirm { ticket }
            | Action::Cancel { ticket }
            | Action::Acknowledge { ticket }
                if *ticket == 0 =>
            {
                return Err("control ticket");
            }
            _ => {}
        }
        Ok(())
    }
    pub(crate) fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        encode(self)
    }
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > CONTROL - HEADER {
            return Err("control payload bound");
        }
        let request: Self = serde_json::from_slice(bytes).map_err(|_| "closed control request")?;
        request.validate()?;
        Ok(request)
    }
}
impl Feedback {
    pub(crate) fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        encode(self)
    }
    fn validate(&self) -> Result<()> {
        if self.revision == 0
            || self.result.as_ref().is_some_and(|r| {
                r.reference.len() > 128
                    || r.revision.len() > 20
                    || !r.value.is_finite()
                    || !r.target.is_finite()
                    || !r.rate.is_finite()
            })
            || self.summary.len() > 1024
            || self.detail.len() > 1024
            || self.problem.as_ref().is_some_and(|s| s.len() > 256)
        {
            return Err("control feedback bound");
        }
        Ok(())
    }
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > CONTROL - HEADER {
            return Err("control payload bound");
        }
        let state: Self = serde_json::from_slice(bytes).map_err(|_| "closed control feedback")?;
        state.validate()?;
        Ok(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::private_ipc::protocol::{Header, Identity, Kind};
    #[test]
    fn closed_intent_codec_rejects_arbitrary_commands_and_unbounded_candidates() {
        let request = Request {
            nonce: 1,
            revision: 2,
            action: Action::ProposeRetune {
                control: "tune".into(),
                target: 42.,
                rate: 2.,
            },
        };
        assert_eq!(
            Request::decode(&request.encode().unwrap()).unwrap(),
            request
        );
        for bytes in [br#"{"nonce":1,"revision":2,"action":"mutation","op":"reference_retune","args":{}}"#.as_slice(),
            br#"{"nonce":1,"revision":2,"action":"confirm","ticket":1,"op":"anything"}"#,
            br#"{"nonce":1,"revision":2,"action":"propose_retune","control":"tune","target":1,"rate":0}"#] {
            assert!(Request::decode(bytes).is_err());
        }
        assert!(Request::decode(&vec![b' '; CONTROL]).is_err());
        let too_long = Request {
            action: Action::ProposeRetune {
                control: "x".repeat(513),
                target: 1.,
                rate: 1.,
            },
            ..request
        };
        assert!(too_long.encode().is_err());
    }
    #[test]
    fn legacy_protocol_cannot_accidentally_enable_control() {
        let identity = Identity {
            workbench: [1; 16],
            incarnation: 1,
        };
        for kind in [Kind::Hello, Kind::ControlIntent, Kind::ControlFeedback] {
            let mut bytes = [0; HEADER];
            Header::control(kind, identity, 1)
                .encode(&mut bytes)
                .unwrap();
            bytes[4..6].copy_from_slice(&1_u16.to_le_bytes());
            assert_eq!(
                Header::decode(&bytes, identity, false),
                Err("unsupported private version")
            );
        }
    }
}
