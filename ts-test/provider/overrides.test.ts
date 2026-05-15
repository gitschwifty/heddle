import { describe, expect, test } from "bun:test";
import { validateOverrides } from "../../src/provider/overrides.ts";

describe("validateOverrides()", () => {
	test("accepts valid fields", () => {
		const result = validateOverrides({
			model: "anthropic/claude-sonnet",
			temperature: 0.7,
			max_tokens: 4096,
		});
		expect(result.model).toBe("anthropic/claude-sonnet");
		expect(result.temperature).toBe(0.7);
		expect(result.max_tokens).toBe(4096);
	});

	test("empty overrides returns empty result", () => {
		const result = validateOverrides({});
		expect(Object.keys(result)).toHaveLength(0);
	});

	test("rejects temperature outside [0, 2]", () => {
		expect(validateOverrides({ temperature: -0.1 }).temperature).toBeUndefined();
		expect(validateOverrides({ temperature: 2.1 }).temperature).toBeUndefined();
		expect(validateOverrides({ temperature: 0 }).temperature).toBe(0);
		expect(validateOverrides({ temperature: 2 }).temperature).toBe(2);
	});

	test("rejects negative max_tokens", () => {
		expect(validateOverrides({ max_tokens: -1 }).max_tokens).toBeUndefined();
		expect(validateOverrides({ max_tokens: 0 }).max_tokens).toBeUndefined();
		expect(validateOverrides({ max_tokens: 1.5 }).max_tokens).toBeUndefined();
		expect(validateOverrides({ max_tokens: 100 }).max_tokens).toBe(100);
	});

	test("validates session_id max 128 chars", () => {
		expect(validateOverrides({ session_id: "short" }).session_id).toBe("short");
		expect(validateOverrides({ session_id: "a".repeat(128) }).session_id).toBe("a".repeat(128));
		expect(validateOverrides({ session_id: "a".repeat(129) }).session_id).toBeUndefined();
	});

	test("validates reasoning nested object", () => {
		const result = validateOverrides({
			reasoning: {
				effort: "high",
				max_tokens: 2000,
				excluded: false,
				summary: "concise",
			},
		});
		expect(result.reasoning?.effort).toBe("high");
		expect(result.reasoning?.max_tokens).toBe(2000);
		expect(result.reasoning?.excluded).toBe(false);
		expect(result.reasoning?.summary).toBe("concise");
	});

	test("rejects invalid reasoning effort", () => {
		const result = validateOverrides({ reasoning: { effort: "invalid" } });
		expect(result.reasoning).toBeUndefined();
	});

	test("rejects invalid reasoning summary", () => {
		const result = validateOverrides({ reasoning: { summary: "verbose" } });
		expect(result.reasoning).toBeUndefined();
	});

	test("validates reasoning.max_tokens must be positive integer", () => {
		const result = validateOverrides({ reasoning: { max_tokens: -10 } });
		expect(result.reasoning).toBeUndefined();
	});

	test("validates route values", () => {
		expect(validateOverrides({ route: "fallback" }).route).toBe("fallback");
		expect(validateOverrides({ route: "sort" }).route).toBe("sort");
		expect(validateOverrides({ route: "invalid" }).route).toBeUndefined();
	});

	test("validates models array", () => {
		expect(validateOverrides({ models: ["a", "b"] }).models).toEqual(["a", "b"]);
		expect(validateOverrides({ models: "not-array" }).models).toBeUndefined();
	});

	test("passes through complex objects", () => {
		const result = validateOverrides({
			response_format: { type: "json_object" },
			tool_choice: "auto",
			provider: { order: ["openai"] },
		});
		expect(result.response_format).toEqual({ type: "json_object" });
		expect(result.tool_choice).toBe("auto");
		expect(result.provider).toEqual({ order: ["openai"] });
	});

	test("warns on unknown keys via debug", () => {
		// We can't easily capture debug output, but verify it doesn't throw
		const result = validateOverrides({ unknown_field: "value", temperature: 0.5 });
		expect(result.temperature).toBe(0.5);
		expect((result as Record<string, unknown>).unknown_field).toBeUndefined();
	});

	test("numeric fields pass through", () => {
		const result = validateOverrides({
			top_p: 0.9,
			seed: 42,
			frequency_penalty: 0.5,
			presence_penalty: -0.5,
		});
		expect(result.top_p).toBe(0.9);
		expect(result.seed).toBe(42);
		expect(result.frequency_penalty).toBe(0.5);
		expect(result.presence_penalty).toBe(-0.5);
	});

	test("stop as string", () => {
		expect(validateOverrides({ stop: "\n" }).stop).toBe("\n");
	});

	test("stop as string array", () => {
		expect(validateOverrides({ stop: ["\n", "END"] }).stop).toEqual(["\n", "END"]);
	});
});
