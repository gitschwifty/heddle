import { describe, expect, test } from "bun:test";
import { Value } from "@sinclair/typebox/value";
import { HeddleConfigSchema } from "../../src/config/types.ts";
import { HooksConfigSchema } from "../../src/hooks/types.ts";

describe("config/schema-export — JSON Schema generation", () => {
	// Get the schema objects as they'd be exported (with $schema added)
	const configSchema = {
		$schema: "http://json-schema.org/draft-07/schema#",
		...HeddleConfigSchema,
	};
	const hooksSchema = {
		$schema: "http://json-schema.org/draft-07/schema#",
		...HooksConfigSchema,
	};

	describe("config schema is valid JSON Schema", () => {
		test("serializes to valid JSON", () => {
			const json = JSON.stringify(configSchema);
			expect(() => JSON.parse(json)).not.toThrow();
		});

		test("has $schema draft-07 identifier", () => {
			expect(configSchema.$schema).toBe("http://json-schema.org/draft-07/schema#");
		});

		test("is type object", () => {
			expect(configSchema.type).toBe("object");
		});

		test("contains expected top-level fields", () => {
			const props = Object.keys(configSchema.properties);
			for (const field of ["model", "api_key", "system_prompt", "features", "permissions", "hooks"]) {
				expect(props).toContain(field);
			}
		});

		test("optional fields are not required", () => {
			// All fields in HeddleConfigSchema are optional, so required should be
			// absent or empty
			const required = configSchema.required ?? [];
			expect(required).toEqual([]);
		});
	});

	describe("hooks schema is valid JSON Schema", () => {
		test("serializes to valid JSON", () => {
			const json = JSON.stringify(hooksSchema);
			expect(() => JSON.parse(json)).not.toThrow();
		});

		test("has $schema draft-07 identifier", () => {
			expect(hooksSchema.$schema).toBe("http://json-schema.org/draft-07/schema#");
		});

		test("is type object", () => {
			expect(hooksSchema.type).toBe("object");
		});

		test("contains expected hook event fields", () => {
			const props = Object.keys(hooksSchema.properties);
			for (const field of [
				"session_start",
				"session_end",
				"pre_prompt",
				"pre_tool",
				"post_tool",
				"post_turn",
				"error",
			]) {
				expect(props).toContain(field);
			}
		});

		test("has additionalProperties false", () => {
			expect(hooksSchema.additionalProperties).toBe(false);
		});
	});

	describe("validation against schema", () => {
		test("known-good config validates", () => {
			const goodConfig = {
				api_key: "sk-test-123",
				model: "anthropic/claude-sonnet",
				system_prompt: "You are a helpful assistant.",
				features: {
					history: true,
					hooks: false,
				},
				permissions: {
					allow: ["read_file", "glob"],
					deny: ["bash"],
				},
				hooks: {
					pre_tool: [
						{
							command: "echo hello",
							timeout: 5000,
						},
					],
				},
			};
			expect(Value.Check(HeddleConfigSchema, goodConfig)).toBe(true);
		});

		test("known-bad config fails validation", () => {
			const badConfig = {
				model: 12345, // should be string
				temperature: "hot", // should be number
				features: "all", // should be object
			};
			expect(Value.Check(HeddleConfigSchema, badConfig)).toBe(false);
		});
	});

	describe("schema file output", () => {
		test("config schema round-trips through JSON stringify/parse", () => {
			const serialized = JSON.stringify(configSchema, null, "\t");
			const parsed = JSON.parse(serialized);
			expect(parsed.$schema).toBe("http://json-schema.org/draft-07/schema#");
			expect(parsed.type).toBe("object");
			expect(parsed.properties.model.type).toBe("string");
		});

		test("hooks schema round-trips through JSON stringify/parse", () => {
			const serialized = JSON.stringify(hooksSchema, null, "\t");
			const parsed = JSON.parse(serialized);
			expect(parsed.$schema).toBe("http://json-schema.org/draft-07/schema#");
			expect(parsed.type).toBe("object");
			expect(parsed.properties.pre_tool).toBeDefined();
		});
	});
});
