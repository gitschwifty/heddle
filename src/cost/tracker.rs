//! Accumulates per-turn token usage and computes totals.

use chrono::Utc;

use crate::types::Usage;

#[derive(Debug, Clone)]
pub struct TurnUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub cost: Option<f64>,
    pub cached_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub timestamp: String,
}

#[derive(Debug, Default, Clone)]
pub struct CostTracker {
    turns: Vec<TurnUsage>,
}

impl CostTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_usage(&mut self, usage: &Usage) {
        self.turns.push(TurnUsage {
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            total_tokens: usage.total_tokens,
            cost: usage.cost,
            cached_tokens: usage
                .prompt_tokens_details
                .as_ref()
                .and_then(|d| d.cached_tokens),
            reasoning_tokens: usage
                .completion_tokens_details
                .as_ref()
                .and_then(|d| d.reasoning_tokens),
            timestamp: Utc::now().to_rfc3339(),
        });
    }

    pub fn total_input_tokens(&self) -> u64 {
        self.turns.iter().map(|t| t.prompt_tokens).sum()
    }

    pub fn total_output_tokens(&self) -> u64 {
        self.turns.iter().map(|t| t.completion_tokens).sum()
    }

    pub fn total_cost(&self) -> Option<f64> {
        let with_cost: Vec<f64> = self.turns.iter().filter_map(|t| t.cost).collect();
        if with_cost.is_empty() {
            None
        } else {
            Some(with_cost.iter().sum())
        }
    }

    pub fn breakdown(&self) -> &[TurnUsage] {
        &self.turns
    }

    pub fn is_over_budget(&self, limit: f64) -> bool {
        self.total_cost().map(|c| c > limit).unwrap_or(false)
    }

    pub fn reset(&mut self) {
        self.turns.clear();
    }
}
