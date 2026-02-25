import type { Message } from "../types.ts";

export interface PruningOptions {
	/** Estimated tokens to protect at end of conversation (default: 40000) */
	protectWindowTokens?: number;
	/** Only prune if total estimated tokens exceed this (default: 20000) */
	pruneThresholdTokens?: number;
}

const DEFAULT_PROTECT_WINDOW = 40000;
const DEFAULT_PRUNE_THRESHOLD = 20000;

/** Simple char/4 heuristic for token estimation. */
export function estimateTokens(messages: Message[]): number {
	if (messages.length === 0) return 0;
	return Math.ceil(JSON.stringify(messages).length / 4);
}

/**
 * Prune old tool message contents to reduce context size.
 * Mutates messages in place. Returns count of messages pruned.
 */
export function pruneToolResults(messages: Message[], options?: PruningOptions): number {
	const protectWindow = options?.protectWindowTokens ?? DEFAULT_PROTECT_WINDOW;
	const threshold = options?.pruneThresholdTokens ?? DEFAULT_PRUNE_THRESHOLD;

	if (estimateTokens(messages) < threshold) return 0;

	// Walk backward to find protection boundary
	let accumulated = 0;
	let protectionBoundary = 1; // default: everything is protected (prune nothing)
	for (let i = messages.length - 1; i >= 1; i--) {
		accumulated += Math.ceil(JSON.stringify(messages[i]).length / 4);
		if (accumulated > protectWindow) {
			protectionBoundary = i + 1;
			break;
		}
	}

	// Walk forward from index 1 (skip system) up to protection boundary
	let count = 0;
	for (let i = 1; i < protectionBoundary; i++) {
		const msg = messages[i] as Message | undefined;
		if (msg?.role === "tool" && !msg.content.startsWith("[pruned")) {
			const originalLength = msg.content.length;
			msg.content = `[pruned — original: ${originalLength} chars]`;
			count++;
		}
	}

	return count;
}
