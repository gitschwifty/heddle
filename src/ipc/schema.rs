//! Best-effort wire validation: try to deserialize as either request or
//! response; either succeeding means the message is structurally valid.

use serde_json::Value;

use super::types::{IpcRequest, IpcResponse};

pub fn validate_ipc_message(msg: &Value) -> bool {
    serde_json::from_value::<IpcRequest>(msg.clone()).is_ok()
        || serde_json::from_value::<IpcResponse>(msg.clone()).is_ok()
}
