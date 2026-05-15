import { describe, expect, test } from "bun:test";
import { Value } from "@sinclair/typebox/value";
import {
	ApprovalModeSchema,
	HeddleConfigSchema,
	ProviderConfigSchema,
	SessionConfigSchema,
} from "../../src/config/types.ts";

describe("config/types — TypeBox schemas", () => {
	describe("ApprovalModeSchema", () => {
		test("accepts valid approval modes", () => {
			for (const mode of ["suggest", "auto-edit", "full-auto", "plan", "yolo"]) {
				expect(Value.Check(ApprovalModeSchema, mode)).toBe(true);
			}
		});

		test("rejects invalid approval modes", () => {
			expect(Value.Check(ApprovalModeSchema, "invalid")).toBe(false);
			expect(Value.Check(ApprovalModeSchema, "")).toBe(false);
			expect(Value.Check(ApprovalModeSchema, 42)).toBe(false);
		});
	});

	describe("ProviderConfigSchema", () => {
		test("accepts empty object (all optional)", () => {
			expect(Value.Check(ProviderConfigSchema, {})).toBe(true);
		});

		test("accepts full provider config", () => {
			const config = {
				model: "anthropic/claude-sonnet",
				weak_model: "openrouter/free",
				editor_model: "anthropic/claude-opus",
				max_tokens: 4096,
				temperature: 0.7,
				base_url: "http://localhost:8080",
			};
			expect(Value.Check(ProviderConfigSchema, config)).toBe(true);
		});

		test("rejects wrong types", () => {
			expect(Value.Check(ProviderConfigSchema, { temperature: "hot" })).toBe(false);
			expect(Value.Check(ProviderConfigSchema, { max_tokens: "many" })).toBe(false);
		});
	});

	describe("SessionConfigSchema", () => {
		test("accepts empty object (all optional)", () => {
			expect(Value.Check(SessionConfigSchema, {})).toBe(true);
		});

		test("accepts full session config", () => {
			const config = {
				system_prompt: "You are helpful.",
				approval_mode: "full-auto",
				instructions: ["HEDDLE.md"],
				tools: ["read_file", "glob"],
				doom_loop_threshold: 5,
				budget_limit: 1.5,
			};
			expect(Value.Check(SessionConfigSchema, config)).toBe(true);
		});

		test("rejects invalid approval_mode", () => {
			expect(Value.Check(SessionConfigSchema, { approval_mode: "banana" })).toBe(false);
		});

		test("rejects tools as bare string", () => {
			expect(Value.Check(SessionConfigSchema, { tools: "read_file" })).toBe(false);
		});

		test("rejects instructions as bare string", () => {
			expect(Value.Check(SessionConfigSchema, { instructions: "HEDDLE.md" })).toBe(false);
		});
	});

	describe("HeddleConfigSchema", () => {
		test("accepts empty object (all optional)", () => {
			expect(Value.Check(HeddleConfigSchema, {})).toBe(true);
		});

		test("accepts full config", () => {
			const config = {
				api_key: "sk-test",
				model: "anthropic/claude-sonnet",
				weak_model: "openrouter/free",
				editor_model: "anthropic/claude-opus",
				max_tokens: 4096,
				temperature: 0.7,
				base_url: "http://localhost:8080",
				system_prompt: "Be helpful.",
				approval_mode: "suggest",
				instructions: ["HEDDLE.md", "AGENTS.md"],
				tools: ["read_file", "glob", "grep"],
				doom_loop_threshold: 3,
				budget_limit: 5.0,
			};
			expect(Value.Check(HeddleConfigSchema, config)).toBe(true);
		});

		test("rejects wrong field types", () => {
			expect(Value.Check(HeddleConfigSchema, { model: 123 })).toBe(false);
			expect(Value.Check(HeddleConfigSchema, { budget_limit: "five" })).toBe(false);
		});

		test("generates valid JSON Schema", () => {
			// Verify TypeBox produces a usable JSON Schema (for headless protocol validation)
			expect(HeddleConfigSchema.type).toBe("object");
			expect(HeddleConfigSchema.properties.model.type).toBe("string");
			expect(HeddleConfigSchema.properties.tools.type).toBe("array");
		});
	});
});
