//! Focused execution feedback is never final acceptance evidence.
use super::*;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Feedback {
    pub id: String,
    pub criterion: String,
    pub attempt: u8,
    pub fingerprint: String,
    pub check: Check,
}
