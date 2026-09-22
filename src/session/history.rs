//! Repair interrupted tool batches without replaying effects or claiming success.

use crate::types::{Message, ToolMessage};

/// Supply missing results before the next non-tool message. Existing results
/// and call IDs are preserved; repeated recovery is a no-op.
pub fn recover_interrupted_tools(messages: &mut Vec<Message>) {
    let mut index = 0;
    while index < messages.len() {
        let calls = match &messages[index] {
            Message::Assistant(message) => message.tool_calls.as_deref().unwrap_or_default(),
            _ => {
                index += 1;
                continue;
            }
        };
        let mut end = index + 1;
        while matches!(messages.get(end), Some(Message::Tool(_))) {
            end += 1;
        }
        let missing: Vec<_> = calls.iter().filter(|call| {
            !messages[index + 1..end].iter().any(|message| {
                matches!(message, Message::Tool(result) if result.tool_call_id == call.id)
            })
        }).map(|call| Message::Tool(ToolMessage {
            tool_call_id: call.id.clone(),
            content: "Error: Tool execution was interrupted; outcome unknown. The tool was not automatically replayed. Verify any side effects before retrying.".into(),
        })).collect();
        let count = missing.len();
        messages.splice(end..end, missing);
        index = end + count;
    }
}

/// Repair even when an event consumer drops a suspended agent stream.
pub(crate) struct HistoryRecovery<'a>(pub &'a mut Vec<Message>);

impl Drop for HistoryRecovery<'_> {
    fn drop(&mut self) {
        recover_interrupted_tools(self.0);
    }
}
