import { describe, expect, test } from "bun:test";
import { CostTracker } from "../../src/cost/tracker.ts";
import type { Usage } from "../../src/types.ts";

function makeUsage(overrides: Partial<Usage> = {}): Usage {
	return {
		prompt_tokens: 100,
		completion_tokens: 50,
		total_tokens: 150,
		...overrides,
	};
}

describe("CostTracker", () => {
	test("empty tracker returns zeros and null cost", () => {
		const tracker = new CostTracker();
		expect(tracker.totalInputTokens).toBe(0);
		expect(tracker.totalOutputTokens).toBe(0);
		expect(tracker.totalCost).toBeNull();
		expect(tracker.breakdown).toEqual([]);
	});

	test("addUsage accumulates input tokens", () => {
		const tracker = new CostTracker();
		tracker.addUsage(makeUsage({ prompt_tokens: 100 }));
		tracker.addUsage(makeUsage({ prompt_tokens: 200 }));
		expect(tracker.totalInputTokens).toBe(300);
	});

	test("addUsage accumulates output tokens", () => {
		const tracker = new CostTracker();
		tracker.addUsage(makeUsage({ completion_tokens: 50 }));
		tracker.addUsage(makeUsage({ completion_tokens: 75 }));
		expect(tracker.totalOutputTokens).toBe(125);
	});

	test("totalCost sums cost fields", () => {
		const tracker = new CostTracker();
		tracker.addUsage(makeUsage({ cost: 0.001 }));
		tracker.addUsage(makeUsage({ cost: 0.002 }));
		expect(tracker.totalCost).toBeCloseTo(0.003);
	});

	test("totalCost returns null when all costs are null", () => {
		const tracker = new CostTracker();
		tracker.addUsage(makeUsage()); // no cost field
		tracker.addUsage(makeUsage());
		expect(tracker.totalCost).toBeNull();
	});

	test("totalCost returns sum of non-null costs (mixed null/number)", () => {
		const tracker = new CostTracker();
		tracker.addUsage(makeUsage({ cost: 0.005 }));
		tracker.addUsage(makeUsage()); // no cost
		tracker.addUsage(makeUsage({ cost: 0.003 }));
		expect(tracker.totalCost).toBeCloseTo(0.008);
	});

	test("isOverBudget returns true when over limit", () => {
		const tracker = new CostTracker();
		tracker.addUsage(makeUsage({ cost: 1.5 }));
		expect(tracker.isOverBudget(1.0)).toBe(true);
	});

	test("isOverBudget returns false when under limit", () => {
		const tracker = new CostTracker();
		tracker.addUsage(makeUsage({ cost: 0.5 }));
		expect(tracker.isOverBudget(1.0)).toBe(false);
	});

	test("isOverBudget returns false when cost is null", () => {
		const tracker = new CostTracker();
		tracker.addUsage(makeUsage());
		expect(tracker.isOverBudget(1.0)).toBe(false);
	});

	test("reset clears all data", () => {
		const tracker = new CostTracker();
		tracker.addUsage(makeUsage({ cost: 0.01 }));
		tracker.addUsage(makeUsage({ cost: 0.02 }));
		tracker.reset();
		expect(tracker.totalInputTokens).toBe(0);
		expect(tracker.totalOutputTokens).toBe(0);
		expect(tracker.totalCost).toBeNull();
		expect(tracker.breakdown).toEqual([]);
	});

	test("breakdown returns readonly turn array", () => {
		const tracker = new CostTracker();
		tracker.addUsage(makeUsage({ cost: 0.01 }));
		const breakdown = tracker.breakdown;
		expect(breakdown).toHaveLength(1);
		expect(breakdown[0].promptTokens).toBe(100);
		expect(breakdown[0].completionTokens).toBe(50);
		expect(breakdown[0].totalTokens).toBe(150);
		expect(breakdown[0].cost).toBe(0.01);
		expect(breakdown[0].timestamp).toMatch(/^\d{4}-\d{2}-\d{2}T/);
	});

	test("handles cached_tokens and reasoning_tokens in usage details", () => {
		const tracker = new CostTracker();
		tracker.addUsage(
			makeUsage({
				cost: 0.01,
				prompt_tokens_details: { cached_tokens: 50 },
				completion_tokens_details: { reasoning_tokens: 20 },
			}),
		);
		const turn = tracker.breakdown[0];
		expect(turn.cachedTokens).toBe(50);
		expect(turn.reasoningTokens).toBe(20);
	});
});
