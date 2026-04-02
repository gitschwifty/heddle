import type { Provider } from "../provider/types.ts";
import type { Message } from "../types.ts";
import { estimateTokens } from "./pruning.ts";

export interface CompactionConfig {
	/** Ratio of estimateTokens/modelLimit that triggers compaction (default: 0.80) */
	compactTrigger: number;
	/** Token count to protect at end of conversation (default: 40000) */
	pruneProtect: number;
	/** Minimum compactable messages required before compacting (default: 4) */
	pruneMinimum: number;
	/** Buffer ratio — reserved space after compaction (default: 0.50) */
	compactBuffer: number;
}

export interface CompactionStats {
	messagesRemoved: number;
	tokensBefore: number;
	tokensAfter: number;
}

const DEFAULTS: CompactionConfig = {
	compactTrigger: 0.8,
	pruneProtect: 40000,
	pruneMinimum: 4,
	compactBuffer: 0.5,
};

function resolveConfig(partial?: Partial<CompactionConfig>): CompactionConfig {
	return { ...DEFAULTS, ...partial };
}

/** Returns true if context usage ratio exceeds the compact trigger threshold. */
export function shouldCompact(messages: Message[], modelLimit: number, config?: Partial<CompactionConfig>): boolean {
	const cfg = resolveConfig(config);
	const tokens = estimateTokens(messages);
	return tokens / modelLimit >= cfg.compactTrigger;
}

/**
 * Returns the slice of messages eligible for compaction.
 * Skips: system (index 0), compaction anchors, and the protected tail window.
 * Returns empty if fewer than pruneMinimum messages are eligible.
 */
export function getCompactableMessages(messages: Message[], config?: Partial<CompactionConfig>): Message[] {
	const cfg = resolveConfig(config);

	// Walk backward to find protection boundary
	let accumulated = 0;
	let protectionBoundary = messages.length;
	for (let i = messages.length - 1; i >= 1; i--) {
		accumulated += Math.ceil(JSON.stringify(messages[i]).length / 4);
		if (accumulated > cfg.pruneProtect) {
			protectionBoundary = i + 1;
			break;
		}
	}

	// Collect compactable messages: skip system (index 0), skip anchors, stop at protection boundary
	const compactable: Message[] = [];
	for (let i = 1; i < protectionBoundary; i++) {
		const msg = messages[i] as Message | undefined;
		if (!msg) continue;
		if (typeof msg.content === "string" && msg.content.startsWith("[Context Summary]")) {
			continue;
		}
		compactable.push(msg);
	}

	if (compactable.length < cfg.pruneMinimum) {
		return [];
	}

	return compactable;
}

/**
 * Builds a prompt asking a model to summarize the given messages.
 * Preserves: key decisions, file paths, tool results, errors.
 */
export function buildCompactionPrompt(messages: Message[]): string {
	const lines: string[] = [];
	for (const msg of messages) {
		const content = typeof msg.content === "string" ? msg.content : "(no content)";
		lines.push(`[${msg.role}]: ${content}`);
	}

	return `Summarize the following conversation, preserving:
- Key decisions made
- File paths mentioned
- Tool results and their outcomes
- Errors encountered and how they were resolved

Be concise but complete. Do not lose important context.

---
${lines.join("\n")}
---

Provide a structured summary:`;
}

/**
 * Compact conversation context by summarizing older messages via a weak model.
 * Mutates the messages array in place.
 * Depth cap: if an existing [Context Summary] anchor exists, it's included in what gets
 * summarized but we don't recurse further.
 */
export async function compactContext(
	messages: Message[],
	weakProvider: Provider,
	_modelLimit: number,
	config?: Partial<CompactionConfig>,
): Promise<CompactionStats> {
	const tokensBefore = estimateTokens(messages);
	const compactable = getCompactableMessages(messages, config);

	if (compactable.length === 0) {
		return { messagesRemoved: 0, tokensBefore, tokensAfter: tokensBefore };
	}

	// Include any existing anchor in the compaction set (depth cap = 1)
	const toSummarize: Message[] = [];
	for (let i = 1; i < messages.length; i++) {
		const msg = messages[i] as Message | undefined;
		if (!msg) continue;
		if (compactable.includes(msg)) {
			toSummarize.push(msg);
		} else if (typeof msg.content === "string" && msg.content.startsWith("[Context Summary]")) {
			toSummarize.push(msg);
		}
	}

	const prompt = buildCompactionPrompt(toSummarize);
	const response = await weakProvider.send([
		{ role: "system", content: "You are a conversation summarizer. Be concise and preserve key context." },
		{ role: "user", content: prompt },
	]);

	const summaryContent = response.choices[0]?.message?.content ?? "No summary generated.";

	// Find indices of messages to remove (compactable + any existing anchors in the compactable zone)
	const indicesToRemove = new Set<number>();
	for (let i = 1; i < messages.length; i++) {
		const msg = messages[i] as Message | undefined;
		if (!msg) continue;
		if (compactable.includes(msg)) {
			indicesToRemove.add(i);
		} else if (typeof msg.content === "string" && msg.content.startsWith("[Context Summary]")) {
			// Only remove anchors that are in the pre-protection zone
			const cfg = resolveConfig(config);
			let accumulated = 0;
			let protectionBoundary = messages.length;
			for (let j = messages.length - 1; j >= 1; j--) {
				accumulated += Math.ceil(JSON.stringify(messages[j]).length / 4);
				if (accumulated > cfg.pruneProtect) {
					protectionBoundary = j + 1;
					break;
				}
			}
			if (i < protectionBoundary) {
				indicesToRemove.add(i);
			}
		}
	}

	// Find insertion point — right after system message (index 1)
	const summaryMessage: Message = {
		role: "assistant",
		content: `[Context Summary] ${summaryContent}`,
	};

	// Remove compacted messages and insert summary
	// Work from high indices to low to preserve indices
	const sortedIndices = [...indicesToRemove].sort((a, b) => b - a);
	for (const idx of sortedIndices) {
		messages.splice(idx, 1);
	}

	// Insert summary after system message
	messages.splice(1, 0, summaryMessage);

	const tokensAfter = estimateTokens(messages);

	return {
		messagesRemoved: indicesToRemove.size,
		tokensBefore,
		tokensAfter,
	};
}
