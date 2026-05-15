use heddle::cost::tracker::CostTracker;
use heddle::types::{CompletionTokenDetails, PromptTokenDetails, Usage};

mod common;

fn usage(prompt: u64, completion: u64, cost: Option<f64>) -> Usage {
    Usage {
        prompt_tokens: prompt,
        completion_tokens: completion,
        total_tokens: prompt + completion,
        cost,
        ..Default::default()
    }
}

#[test]
fn empty_tracker_zeros_and_null_cost() {
    let t = CostTracker::new();
    assert_eq!(t.total_input_tokens(), 0);
    assert_eq!(t.total_output_tokens(), 0);
    assert_eq!(t.total_cost(), None);
    assert!(t.breakdown().is_empty());
}

#[test]
fn add_usage_accumulates_input_tokens() {
    let mut t = CostTracker::new();
    t.add_usage(&usage(100, 50, None));
    t.add_usage(&usage(200, 50, None));
    assert_eq!(t.total_input_tokens(), 300);
}

#[test]
fn add_usage_accumulates_output_tokens() {
    let mut t = CostTracker::new();
    t.add_usage(&usage(100, 50, None));
    t.add_usage(&usage(100, 75, None));
    assert_eq!(t.total_output_tokens(), 125);
}

#[test]
fn total_cost_sums_cost_fields() {
    let mut t = CostTracker::new();
    t.add_usage(&usage(100, 50, Some(0.001)));
    t.add_usage(&usage(100, 50, Some(0.002)));
    let c = t.total_cost().unwrap();
    assert!((c - 0.003).abs() < 1e-9);
}

#[test]
fn total_cost_null_when_all_costs_null() {
    let mut t = CostTracker::new();
    t.add_usage(&usage(100, 50, None));
    t.add_usage(&usage(100, 50, None));
    assert_eq!(t.total_cost(), None);
}

#[test]
fn total_cost_sums_non_null_with_mixed() {
    let mut t = CostTracker::new();
    t.add_usage(&usage(100, 50, Some(0.005)));
    t.add_usage(&usage(100, 50, None));
    t.add_usage(&usage(100, 50, Some(0.003)));
    let c = t.total_cost().unwrap();
    assert!((c - 0.008).abs() < 1e-9);
}

#[test]
fn over_budget_when_over_limit() {
    let mut t = CostTracker::new();
    t.add_usage(&usage(100, 50, Some(1.5)));
    assert!(t.is_over_budget(1.0));
}

#[test]
fn not_over_budget_when_under() {
    let mut t = CostTracker::new();
    t.add_usage(&usage(100, 50, Some(0.5)));
    assert!(!t.is_over_budget(1.0));
}

#[test]
fn not_over_budget_when_cost_null() {
    let mut t = CostTracker::new();
    t.add_usage(&usage(100, 50, None));
    assert!(!t.is_over_budget(1.0));
}

#[test]
fn reset_clears_data() {
    let mut t = CostTracker::new();
    t.add_usage(&usage(100, 50, Some(0.01)));
    t.add_usage(&usage(100, 50, Some(0.02)));
    t.reset();
    assert_eq!(t.total_input_tokens(), 0);
    assert_eq!(t.total_output_tokens(), 0);
    assert_eq!(t.total_cost(), None);
    assert!(t.breakdown().is_empty());
}

#[test]
fn breakdown_contents() {
    let mut t = CostTracker::new();
    t.add_usage(&usage(100, 50, Some(0.01)));
    let b = t.breakdown();
    assert_eq!(b.len(), 1);
    assert_eq!(b[0].prompt_tokens, 100);
    assert_eq!(b[0].completion_tokens, 50);
    assert_eq!(b[0].total_tokens, 150);
    assert_eq!(b[0].cost, Some(0.01));
    assert!(b[0].timestamp.starts_with("20"));
}

#[test]
fn cached_and_reasoning_tokens_passed_through() {
    let mut t = CostTracker::new();
    t.add_usage(&Usage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
        cost: Some(0.01),
        prompt_tokens_details: Some(PromptTokenDetails {
            cached_tokens: Some(50),
            cache_write_tokens: None,
        }),
        completion_tokens_details: Some(CompletionTokenDetails {
            reasoning_tokens: Some(20),
        }),
    });
    let turn = &t.breakdown()[0];
    assert_eq!(turn.cached_tokens, Some(50));
    assert_eq!(turn.reasoning_tokens, Some(20));
}
