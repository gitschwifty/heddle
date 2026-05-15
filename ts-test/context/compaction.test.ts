import { describe, expect, test } from "bun:test";
import type { CompactionConfig } from "../../src/context/compaction.ts";
import {
	buildCompactionPrompt,
	compactContext,
	getCompactableMessages,
	shouldCompact,
} from "../../src/context/compaction.ts";
import { estimateTokens } from "../../src/context/pruning.ts";
import type { Provider } from "../../src/provider/types.ts";
import type { ChatCompletionResponse, Message } from "../../src/types.ts";

// ── Helpers ──────────────────────────────────────────────────────────────

function makeMessages(count: number, contentSize = 100): Message[] {
	const msgs: Message[] = [{ role: "system", content: "You are a helpful assistant." }];
	for (let i = 0; i < count; i++) {
		const role = i % 2 === 0 ? "user" : "assistant";
		const content = `Message ${i}: ${"x".repeat(contentSize)}`;
		if (role === "user") {
			msgs.push({ role: "user", content });
		} else {
			msgs.push({ role: "assistant", content, tool_calls: undefined });
		}
	}
	return msgs;
}

function makeMockProvider(summaryText: string): Provider {
	return {
		send: async (): Promise<ChatCompletionResponse> => ({
			id: "test-compaction",
			choices: [
				{
					index: 0,
					message: { role: "assistant", content: summaryText, tool_calls: undefined },
					finish_reason: "stop",
				},
			],
		}),
		stream: async function* () {
			/* not used */
		},
		with: () => makeMockProvider(summaryText),
	};
}

// ── shouldCompact ────────────────────────────────────────────────────────

describe("shouldCompact", () => {
	test("returns true when over threshold", () => {
		// Make messages big enough that estimateTokens / modelLimit >= 0.80
		const msgs = makeMessages(40, 500);
		const tokens = estimateTokens(msgs);
		// Set modelLimit so ratio is >= 0.80
		const modelLimit = Math.floor(tokens / 0.8);
		expect(shouldCompact(msgs, modelLimit)).toBe(true);
	});

	test("returns false when under threshold", () => {
		const msgs = makeMessages(5, 50);
		const tokens = estimateTokens(msgs);
		// Set modelLimit so ratio is well under 0.80
		const modelLimit = tokens * 10;
		expect(shouldCompact(msgs, modelLimit)).toBe(false);
	});

	test("respects custom config", () => {
		const msgs = makeMessages(10, 200);
		const tokens = estimateTokens(msgs);
		// With default 0.80 trigger this would be false
		const modelLimit = Math.floor(tokens / 0.5);
		expect(shouldCompact(msgs, modelLimit)).toBe(false);

		// With lower trigger it should be true
		const config: Partial<CompactionConfig> = { compactTrigger: 0.4 };
		expect(shouldCompact(msgs, modelLimit, config)).toBe(true);
	});
});

// ── getCompactableMessages ──────────────────────────────────────────────

describe("getCompactableMessages", () => {
	test("skips system message", () => {
		const msgs = makeMessages(10, 50);
		const result = getCompactableMessages(msgs);
		// Result should never include the system message (index 0)
		for (const msg of result) {
			expect(msg.role).not.toBe("system");
		}
	});

	test("skips compaction anchors", () => {
		const msgs: Message[] = [
			{ role: "system", content: "System prompt" },
			{ role: "assistant", content: "[Context Summary] Previous conversation summary.", tool_calls: undefined },
			{ role: "user", content: "Follow-up question" },
			{ role: "assistant", content: "Response to follow-up", tool_calls: undefined },
			{ role: "user", content: "Another question" },
			{ role: "assistant", content: "Another response", tool_calls: undefined },
			{ role: "user", content: "Yet another question" },
			{ role: "assistant", content: "Yet another response", tool_calls: undefined },
			{ role: "user", content: "More questions" },
			{ role: "assistant", content: "More responses", tool_calls: undefined },
		];
		const result = getCompactableMessages(msgs);
		for (const msg of result) {
			if (typeof msg.content === "string") {
				expect(msg.content.startsWith("[Context Summary]")).toBe(false);
			}
		}
	});

	test("respects protect window", () => {
		// Create messages where the last few messages are within the protect window
		const msgs = makeMessages(20, 200);
		const config: Partial<CompactionConfig> = { pruneProtect: 1000 };
		const result = getCompactableMessages(msgs, config);
		// The result should not include messages from the protected tail
		// Protected messages are those whose cumulative tokens from the end <= pruneProtect
		expect(result.length).toBeGreaterThan(0);
		expect(result.length).toBeLessThan(msgs.length - 1); // some should be protected
	});

	test("returns empty if fewer than minimum messages to compact", () => {
		const msgs = makeMessages(3, 50); // system + 3 = 4 total, only 3 non-system
		const config: Partial<CompactionConfig> = { pruneMinimum: 10 };
		const result = getCompactableMessages(msgs, config);
		expect(result).toEqual([]);
	});
});

// ── buildCompactionPrompt ───────────────────────────────────────────────

describe("buildCompactionPrompt", () => {
	test("includes message content", () => {
		const msgs: Message[] = [
			{ role: "user", content: "What is the capital of France?" },
			{ role: "assistant", content: "The capital of France is Paris.", tool_calls: undefined },
		];
		const prompt = buildCompactionPrompt(msgs);
		expect(prompt).toContain("capital of France");
		expect(prompt).toContain("Paris");
	});
});

// ── compactContext ───────────────────────────────────────────────────────

describe("compactContext", () => {
	test("replaces messages with summary", async () => {
		const msgs = makeMessages(20, 200);
		const provider = makeMockProvider("This is a summary of the conversation.");
		const modelLimit = estimateTokens(msgs) * 2;

		// Set low protect so most messages are compactable
		const config: Partial<CompactionConfig> = { pruneProtect: 500, pruneMinimum: 2 };
		const stats = await compactContext(msgs, provider, modelLimit, config);

		expect(stats.messagesRemoved).toBeGreaterThan(0);
		// Should have a summary message
		const summaryMsg = msgs.find((m) => typeof m.content === "string" && m.content.startsWith("[Context Summary]"));
		expect(summaryMsg).toBeDefined();
		expect(summaryMsg!.role).toBe("assistant");
	});

	test("depth cap — handles existing summary anchor", async () => {
		const pad = "x".repeat(500);
		const msgs: Message[] = [
			{ role: "system", content: "System prompt" },
			{ role: "assistant", content: `[Context Summary] Old summary of first part. ${pad}`, tool_calls: undefined },
			{ role: "user", content: `Question after first compaction ${pad}` },
			{ role: "assistant", content: `Answer after first compaction ${pad}`, tool_calls: undefined },
			{ role: "user", content: `Another question ${pad}` },
			{ role: "assistant", content: `Another answer ${pad}`, tool_calls: undefined },
			{ role: "user", content: `Third question ${pad}` },
			{ role: "assistant", content: `Third answer ${pad}`, tool_calls: undefined },
			{ role: "user", content: "Fourth question" },
			{ role: "assistant", content: "Fourth answer", tool_calls: undefined },
		];
		const provider = makeMockProvider("Combined summary including old context.");
		const modelLimit = 999999;
		// Protect only the last ~100 tokens worth of messages (the last 2 short messages)
		const config: Partial<CompactionConfig> = { pruneProtect: 100, pruneMinimum: 2 };

		const stats = await compactContext(msgs, provider, modelLimit, config);

		// The old summary should be included in what gets compacted
		expect(stats.messagesRemoved).toBeGreaterThan(0);
		// There should be exactly one summary anchor now
		const summaries = msgs.filter((m) => typeof m.content === "string" && m.content.startsWith("[Context Summary]"));
		expect(summaries.length).toBe(1);
	});

	test("returns correct stats", async () => {
		const msgs = makeMessages(20, 200);
		const tokensBefore = estimateTokens(msgs);
		const provider = makeMockProvider("Short summary.");
		const modelLimit = tokensBefore * 2;
		const config: Partial<CompactionConfig> = { pruneProtect: 500, pruneMinimum: 2 };

		const stats = await compactContext(msgs, provider, modelLimit, config);

		expect(stats.tokensBefore).toBe(tokensBefore);
		expect(stats.tokensAfter).toBeLessThan(stats.tokensBefore);
		expect(stats.messagesRemoved).toBeGreaterThan(0);
	});
});
