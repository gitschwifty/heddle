import { describe, expect, test } from "bun:test";
import { MetricsCollector } from "../../src/usage/collector.ts";

describe("MetricsCollector", () => {
	test("initial metrics are all zeros", () => {
		const collector = new MetricsCollector();
		const m = collector.metrics;
		expect(m.messageCount).toEqual({ user: 0, assistant: 0 });
		expect(m.toolCalls).toEqual({});
		expect(m.errors).toEqual({ tool: 0, provider: 0 });
		expect(m.tokens).toEqual({ input: 0, output: 0 });
		expect(m.turns).toBe(0);
	});

	test("onAssistantMessage increments assistant count", () => {
		const collector = new MetricsCollector();
		collector.onAssistantMessage();
		collector.onAssistantMessage();
		expect(collector.metrics.messageCount.assistant).toBe(2);
		expect(collector.metrics.messageCount.user).toBe(0);
	});

	test("onUserMessage increments user count and turns", () => {
		const collector = new MetricsCollector();
		collector.onUserMessage();
		collector.onUserMessage();
		collector.onUserMessage();
		expect(collector.metrics.messageCount.user).toBe(3);
		expect(collector.metrics.turns).toBe(3);
	});

	test("onToolCall tracks per-tool counts", () => {
		const collector = new MetricsCollector();
		collector.onToolCall("read");
		collector.onToolCall("write");
		collector.onToolCall("read");
		collector.onToolCall("read");
		collector.onToolCall("write");
		expect(collector.metrics.toolCalls).toEqual({ read: 3, write: 2 });
	});

	test("onError tracks tool errors", () => {
		const collector = new MetricsCollector();
		collector.onError("tool");
		collector.onError("tool");
		expect(collector.metrics.errors).toEqual({ tool: 2, provider: 0 });
	});

	test("onError tracks provider errors", () => {
		const collector = new MetricsCollector();
		collector.onError("provider");
		expect(collector.metrics.errors).toEqual({ tool: 0, provider: 1 });
	});

	test("onError tracks mixed error sources", () => {
		const collector = new MetricsCollector();
		collector.onError("tool");
		collector.onError("provider");
		collector.onError("tool");
		expect(collector.metrics.errors).toEqual({ tool: 2, provider: 1 });
	});

	test("onUsage accumulates tokens", () => {
		const collector = new MetricsCollector();
		collector.onUsage({ prompt_tokens: 100, completion_tokens: 50 });
		collector.onUsage({ prompt_tokens: 200, completion_tokens: 75 });
		expect(collector.metrics.tokens).toEqual({ input: 300, output: 125 });
	});

	test("full session scenario", () => {
		const collector = new MetricsCollector();

		// Turn 1: user sends message, assistant responds with tool call
		collector.onUserMessage();
		collector.onUsage({ prompt_tokens: 100, completion_tokens: 50 });
		collector.onAssistantMessage();
		collector.onToolCall("read");

		// Turn 2: user sends follow-up, assistant responds, tool errors
		collector.onUserMessage();
		collector.onUsage({ prompt_tokens: 200, completion_tokens: 100 });
		collector.onAssistantMessage();
		collector.onToolCall("edit");
		collector.onError("tool");

		// Provider error during turn
		collector.onError("provider");

		const m = collector.metrics;
		expect(m.messageCount).toEqual({ user: 2, assistant: 2 });
		expect(m.toolCalls).toEqual({ read: 1, edit: 1 });
		expect(m.errors).toEqual({ tool: 1, provider: 1 });
		expect(m.tokens).toEqual({ input: 300, output: 150 });
		expect(m.turns).toBe(2);
	});

	test("metrics returns a snapshot (not a live reference)", () => {
		const collector = new MetricsCollector();
		const before = collector.metrics;
		collector.onUserMessage();
		collector.onToolCall("read");
		const after = collector.metrics;
		expect(before.turns).toBe(0);
		expect(after.turns).toBe(1);
		expect(before.toolCalls).toEqual({});
		expect(after.toolCalls).toEqual({ read: 1 });
	});
});
