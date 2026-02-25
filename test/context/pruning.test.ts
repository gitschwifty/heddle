import { describe, expect, test } from "bun:test";
import { estimateTokens, pruneToolResults } from "../../src/context/pruning.ts";
import type { Message } from "../../src/types.ts";

function systemMsg(content: string): Message {
	return { role: "system", content };
}

function userMsg(content: string): Message {
	return { role: "user", content };
}

function assistantMsg(content: string): Message {
	return { role: "assistant", content };
}

function toolMsg(id: string, content: string): Message {
	return { role: "tool", tool_call_id: id, content };
}

describe("estimateTokens", () => {
	test("returns roughly length/4", () => {
		const messages: Message[] = [userMsg("hello world")];
		const serialized = JSON.stringify(messages);
		const expected = Math.ceil(serialized.length / 4);
		expect(estimateTokens(messages)).toBe(expected);
	});

	test("returns 0 for empty array", () => {
		expect(estimateTokens([])).toBe(0);
	});
});

describe("pruneToolResults", () => {
	test("no-op when below threshold", () => {
		const messages: Message[] = [systemMsg("sys"), userMsg("hi"), assistantMsg("hello")];
		const count = pruneToolResults(messages, { pruneThresholdTokens: 999999 });
		expect(count).toBe(0);
	});

	test("prunes old tool messages beyond protection window", () => {
		const longContent = "x".repeat(1000);
		const messages: Message[] = [
			systemMsg("system"),
			userMsg("q1"),
			assistantMsg("a1"),
			toolMsg("t1", longContent),
			userMsg("q2"),
			assistantMsg("a2"),
			toolMsg("t2", longContent),
			userMsg("recent"),
			assistantMsg("recent response"),
		];
		const count = pruneToolResults(messages, {
			pruneThresholdTokens: 1,
			protectWindowTokens: 50,
		});
		expect(count).toBeGreaterThan(0);
		const earlyTool = messages[3]!;
		expect(earlyTool.role).toBe("tool");
		if (earlyTool.role === "tool") {
			expect(earlyTool.content).toStartWith("[pruned");
		}
	});

	test("preserves system message at index 0", () => {
		const longContent = "x".repeat(1000);
		const messages: Message[] = [
			systemMsg(longContent),
			userMsg("q"),
			assistantMsg("a"),
			toolMsg("t1", longContent),
			userMsg("recent"),
		];
		pruneToolResults(messages, { pruneThresholdTokens: 1, protectWindowTokens: 50 });
		expect(messages[0]!.role).toBe("system");
		if (messages[0]!.role === "system") {
			expect(messages[0]!.content).toBe(longContent);
		}
	});

	test("preserves messages in protection window", () => {
		const longContent = "x".repeat(1000);
		const messages: Message[] = [
			systemMsg("sys"),
			userMsg("old"),
			toolMsg("t1", longContent),
			userMsg("recent question"),
			assistantMsg("recent answer"),
			toolMsg("t2", "recent-tool-result"),
		];
		pruneToolResults(messages, { pruneThresholdTokens: 1, protectWindowTokens: 500 });
		const lastTool = messages[5]!;
		if (lastTool.role === "tool") {
			expect(lastTool.content).toBe("recent-tool-result");
		}
	});

	test("returns count of pruned messages", () => {
		const longContent = "x".repeat(1000);
		const messages: Message[] = [
			systemMsg("sys"),
			toolMsg("t1", longContent),
			toolMsg("t2", longContent),
			toolMsg("t3", longContent),
			userMsg("recent"),
			assistantMsg("recent"),
		];
		const count = pruneToolResults(messages, { pruneThresholdTokens: 1, protectWindowTokens: 50 });
		expect(count).toBe(3);
	});

	test('replaces content with "[pruned — original: N chars]" placeholder', () => {
		const content = "a".repeat(500);
		const messages: Message[] = [
			systemMsg("sys"),
			toolMsg("t1", content),
			userMsg("recent q"),
			assistantMsg("recent a"),
		];
		pruneToolResults(messages, { pruneThresholdTokens: 1, protectWindowTokens: 50 });
		const pruned = messages[1]!;
		if (pruned.role === "tool") {
			expect(pruned.content).toBe(`[pruned — original: ${content.length} chars]`);
		}
	});

	test("mutates messages in place", () => {
		const content = "x".repeat(500);
		const messages: Message[] = [systemMsg("sys"), toolMsg("t1", content), userMsg("recent"), assistantMsg("recent")];
		const original = messages[1];
		pruneToolResults(messages, { pruneThresholdTokens: 1, protectWindowTokens: 50 });
		expect(messages[1]).toBe(original);
		const msg = messages[1]!;
		if (msg.role === "tool") {
			expect(msg.content).toStartWith("[pruned");
		}
	});

	test("custom options (different protectWindowTokens, pruneThresholdTokens)", () => {
		const content = "x".repeat(2000);
		const messages: Message[] = [
			systemMsg("sys"),
			toolMsg("t1", content),
			userMsg("q"),
			assistantMsg("a"),
			toolMsg("t2", content),
			userMsg("recent"),
		];
		expect(pruneToolResults(messages, { pruneThresholdTokens: 999999 })).toBe(0);

		const messages2: Message[] = [systemMsg("sys"), toolMsg("t1", content), userMsg("recent")];
		expect(pruneToolResults(messages2, { pruneThresholdTokens: 1, protectWindowTokens: 999999 })).toBe(0);
	});

	test("idempotent — re-running doesn't re-prune already-pruned messages", () => {
		const content = "x".repeat(500);
		const messages: Message[] = [
			systemMsg("sys"),
			toolMsg("t1", content),
			toolMsg("t2", content),
			userMsg("recent"),
			assistantMsg("recent"),
		];
		const opts = { pruneThresholdTokens: 1, protectWindowTokens: 50 };
		const count1 = pruneToolResults(messages, opts);
		expect(count1).toBe(2);

		const count2 = pruneToolResults(messages, opts);
		expect(count2).toBe(0);
	});

	test("handles conversation with no tool messages (returns 0)", () => {
		const messages: Message[] = [
			systemMsg("sys"),
			userMsg("hello"),
			assistantMsg("hi"),
			userMsg("how are you"),
			assistantMsg("good"),
		];
		const count = pruneToolResults(messages, { pruneThresholdTokens: 1, protectWindowTokens: 50 });
		expect(count).toBe(0);
	});
});
