import { describe, expect, test } from "bun:test";
import { Value } from "@sinclair/typebox/value";
import { StreamChunk, Usage } from "../src/types.ts";

describe("Usage schema", () => {
	test("validates standard 3 fields", () => {
		const usage = { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 };
		expect(Value.Check(Usage, usage)).toBe(true);
	});

	test("accepts optional OR-specific cost field", () => {
		const usage = { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15, cost: 0.0012 };
		expect(Value.Check(Usage, usage)).toBe(true);
	});

	test("accepts optional prompt_tokens_details", () => {
		const usage = {
			prompt_tokens: 100,
			completion_tokens: 50,
			total_tokens: 150,
			prompt_tokens_details: {
				cached_tokens: 80,
				cache_write_tokens: 20,
			},
		};
		expect(Value.Check(Usage, usage)).toBe(true);
	});

	test("accepts optional completion_tokens_details", () => {
		const usage = {
			prompt_tokens: 100,
			completion_tokens: 50,
			total_tokens: 150,
			completion_tokens_details: {
				reasoning_tokens: 30,
			},
		};
		expect(Value.Check(Usage, usage)).toBe(true);
	});

	test("accepts all optional fields together", () => {
		const usage = {
			prompt_tokens: 100,
			completion_tokens: 50,
			total_tokens: 150,
			cost: 0.005,
			prompt_tokens_details: { cached_tokens: 80 },
			completion_tokens_details: { reasoning_tokens: 30 },
		};
		expect(Value.Check(Usage, usage)).toBe(true);
	});

	test("rejects missing required fields", () => {
		expect(Value.Check(Usage, { prompt_tokens: 10, completion_tokens: 5 })).toBe(false);
		expect(Value.Check(Usage, {})).toBe(false);
	});
});

describe("StreamChunk schema", () => {
	test("validates without usage field", () => {
		const chunk = {
			id: "chatcmpl-test",
			choices: [{ index: 0, delta: { content: "hello" }, finish_reason: null }],
		};
		expect(Value.Check(StreamChunk, chunk)).toBe(true);
	});

	test("accepts optional usage field (final SSE chunk)", () => {
		const chunk = {
			id: "chatcmpl-test",
			choices: [{ index: 0, delta: {}, finish_reason: "stop" }],
			usage: { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15, cost: 0.001 },
		};
		expect(Value.Check(StreamChunk, chunk)).toBe(true);
	});
});
