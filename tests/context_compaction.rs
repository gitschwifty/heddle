use heddle::context::compaction::{compact_context, get_compactable_messages, CompactionConfig};
use heddle::types::{Message, SystemMessage, UserMessage};

mod common;
use common::{text_response, MockProvider};

fn history() -> Vec<Message> {
    vec![
        Message::System(SystemMessage {
            content: "system".into(),
        }),
        Message::User(UserMessage {
            content: "retain this decision".into(),
        }),
        Message::User(UserMessage {
            content: "recent question".into(),
        }),
    ]
}

#[test]
fn protection_threshold_preserves_recent_context() {
    let messages = history();
    let total: u64 = messages[1..]
        .iter()
        .map(|m| serde_json::to_string(m).unwrap().len().div_ceil(4) as u64)
        .sum();
    for protect in [total, total + 1] {
        assert!(get_compactable_messages(
            &messages,
            CompactionConfig {
                prune_protect: protect,
                prune_minimum: 1,
                ..Default::default()
            }
        )
        .is_empty());
    }
    assert_eq!(
        get_compactable_messages(
            &messages,
            CompactionConfig {
                prune_protect: total - 1,
                prune_minimum: 1,
                ..Default::default()
            }
        ),
        vec![1]
    );
}

#[tokio::test]
async fn unusable_summaries_preserve_original_history() {
    let mut absent = text_response("ignored");
    absent.choices.clear();
    let mut null = text_response("ignored");
    null.choices[0].message.content = None;
    for response in [absent, null, text_response(""), text_response(" \n\t")] {
        let mut messages = history();
        let before = serde_json::to_value(&messages).unwrap();
        let provider = MockProvider::new().push_response(response);
        let result = compact_context(
            &mut messages,
            provider.as_ref(),
            1000,
            CompactionConfig {
                prune_protect: 0,
                prune_minimum: 1,
                ..Default::default()
            },
        )
        .await;
        assert!(result.is_err(), "unusable summary must fail");
        assert_eq!(serde_json::to_value(messages).unwrap(), before);
    }
}

#[tokio::test]
async fn usable_summary_replaces_eligible_history() {
    let mut messages = history();
    let provider = MockProvider::new().push_response(text_response("Kept decision"));
    let stats = compact_context(
        &mut messages,
        provider.as_ref(),
        1000,
        CompactionConfig {
            prune_protect: 0,
            prune_minimum: 1,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(stats.messages_removed, 2);
    assert_eq!(messages.len(), 2);
    assert_eq!(
        messages[1].content_str(),
        Some("[Context Summary] Kept decision")
    );
}
