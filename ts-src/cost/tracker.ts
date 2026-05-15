import type { Usage } from "../types.ts";

export interface TurnUsage {
	promptTokens: number;
	completionTokens: number;
	totalTokens: number;
	cost: number | null;
	cachedTokens?: number;
	reasoningTokens?: number;
	timestamp: string;
}

export class CostTracker {
	private turns: TurnUsage[] = [];

	addUsage(usage: Usage): void {
		const turn: TurnUsage = {
			promptTokens: usage.prompt_tokens,
			completionTokens: usage.completion_tokens,
			totalTokens: usage.total_tokens,
			cost: usage.cost ?? null,
			cachedTokens: usage.prompt_tokens_details?.cached_tokens,
			reasoningTokens: usage.completion_tokens_details?.reasoning_tokens,
			timestamp: new Date().toISOString(),
		};
		this.turns.push(turn);
	}

	get totalInputTokens(): number {
		return this.turns.reduce((sum, t) => sum + t.promptTokens, 0);
	}

	get totalOutputTokens(): number {
		return this.turns.reduce((sum, t) => sum + t.completionTokens, 0);
	}

	get totalCost(): number | null {
		const withCost = this.turns.filter((t) => t.cost !== null);
		if (withCost.length === 0) return null;
		return withCost.reduce((sum, t) => sum + (t.cost as number), 0);
	}

	get breakdown(): readonly TurnUsage[] {
		return this.turns;
	}

	isOverBudget(limit: number): boolean {
		const cost = this.totalCost;
		return cost !== null && cost > limit;
	}

	reset(): void {
		this.turns = [];
	}
}
