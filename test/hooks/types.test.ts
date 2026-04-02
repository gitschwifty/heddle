import { Value } from "@sinclair/typebox/value";
import { describe, expect, test } from "bun:test";
import {
	HookDefinitionSchema,
	HookEventSchema,
	HookMatchersSchema,
	HookModeSchema,
	HooksConfigSchema,
} from "../../src/hooks/types.ts";

describe("HookEventSchema", () => {
	test("accepts all valid event names", () => {
		for (const event of ["session_start", "session_end", "pre_prompt", "pre_tool", "post_tool", "post_turn", "error"]) {
			expect(Value.Check(HookEventSchema, event)).toBe(true);
		}
	});

	test("rejects invalid event names", () => {
		expect(Value.Check(HookEventSchema, "invalid")).toBe(false);
		expect(Value.Check(HookEventSchema, "")).toBe(false);
		expect(Value.Check(HookEventSchema, 123)).toBe(false);
	});
});

describe("HookModeSchema", () => {
	test("accepts valid modes", () => {
		expect(Value.Check(HookModeSchema, "interactive")).toBe(true);
		expect(Value.Check(HookModeSchema, "headless")).toBe(true);
		expect(Value.Check(HookModeSchema, "both")).toBe(true);
	});

	test("rejects invalid modes", () => {
		expect(Value.Check(HookModeSchema, "cli")).toBe(false);
	});
});

describe("HookMatchersSchema", () => {
	test("accepts empty object (no matchers)", () => {
		expect(Value.Check(HookMatchersSchema, {})).toBe(true);
	});

	test("accepts tool as string", () => {
		expect(Value.Check(HookMatchersSchema, { tool: "read" })).toBe(true);
	});

	test("accepts tool as array", () => {
		expect(Value.Check(HookMatchersSchema, { tool: ["read", "write"] })).toBe(true);
	});

	test("accepts glob matchers", () => {
		expect(
			Value.Check(HookMatchersSchema, {
				match_path: "**/*.ts",
				match_args: "*secret*",
				match_input: "*deploy*",
			}),
		).toBe(true);
	});
});

describe("HookDefinitionSchema", () => {
	test("accepts minimal hook (command only)", () => {
		const hook = { command: "echo hello" };
		expect(Value.Check(HookDefinitionSchema, hook)).toBe(true);
	});

	test("applies defaults", () => {
		const hook = Value.Default(HookDefinitionSchema, { command: "echo hello" });
		expect(hook).toEqual({
			command: "echo hello",
			timeout: 10000,
			mode: "both",
			async: false,
		});
	});

	test("accepts full hook definition", () => {
		const hook = {
			command: "notify.sh",
			timeout: 5000,
			mode: "interactive",
			async: true,
			matchers: { tool: "bash", match_path: "**/*.ts" },
		};
		expect(Value.Check(HookDefinitionSchema, hook)).toBe(true);
	});

	test("rejects hook without command", () => {
		expect(Value.Check(HookDefinitionSchema, { timeout: 5000 })).toBe(false);
	});
});

describe("HooksConfigSchema", () => {
	test("accepts empty config", () => {
		expect(Value.Check(HooksConfigSchema, {})).toBe(true);
	});

	test("accepts config with hooks per event", () => {
		const config = {
			pre_tool: [{ command: "lint.sh" }],
			post_tool: [{ command: "log.sh" }, { command: "notify.sh", async: true }],
		};
		expect(Value.Check(HooksConfigSchema, config)).toBe(true);
	});

	test("rejects invalid event key", () => {
		const config = {
			invalid_event: [{ command: "echo" }],
		};
		// Record type allows any string key, but HooksConfig uses specific event keys
		// This tests the typed version we export
		expect(Value.Check(HooksConfigSchema, config)).toBe(false);
	});
});
