import { describe, expect, test } from "bun:test";
import { Type } from "@sinclair/typebox";
import { ToolRegistry } from "../../src/tools/registry.ts";
import type { HeddleTool } from "../../src/tools/types.ts";

function makeTool(name: string, fn?: (params: unknown) => Promise<string>): HeddleTool {
	return {
		name,
		description: `Test tool: ${name}`,
		parameters: Type.Object({
			input: Type.String(),
		}),
		execute: fn ?? (async (params) => `executed ${name} with ${JSON.stringify(params)}`),
	};
}

describe("ToolRegistry", () => {
	test("register a tool and look it up by name", () => {
		const registry = new ToolRegistry();
		const tool = makeTool("echo");
		registry.register(tool);

		expect(registry.get("echo")).toBe(tool);
	});

	test("get returns undefined for unknown tool", () => {
		const registry = new ToolRegistry();
		expect(registry.get("nonexistent")).toBeUndefined();
	});

	test("list all registered tools", () => {
		const registry = new ToolRegistry();
		registry.register(makeTool("alpha"));
		registry.register(makeTool("beta"));

		const all = registry.all();
		expect(all).toHaveLength(2);
		expect(all.map((t) => t.name)).toEqual(["alpha", "beta"]);
	});

	test("generate OpenAI-format tool definitions", () => {
		const registry = new ToolRegistry();
		registry.register(makeTool("read_file"));

		const defs = registry.definitions();
		expect(defs).toHaveLength(1);
		expect(defs[0]?.type).toBe("function");
		expect(defs[0]?.function.name).toBe("read_file");
		expect(defs[0]?.function.description).toBe("Test tool: read_file");
		expect(defs[0]?.function.parameters).toBeDefined();
	});

	test("execute a tool by name with JSON string args", async () => {
		const registry = new ToolRegistry();
		registry.register(makeTool("greet"));

		const result = await registry.execute("greet", '{"input":"world"}');
		expect(result).toBe('executed greet with {"input":"world"}');
	});

	test("execute throws on unknown tool name", async () => {
		const registry = new ToolRegistry();

		expect(registry.execute("missing", "{}")).rejects.toThrow("Unknown tool: missing");
	});

	test("execute returns error string on invalid JSON args", async () => {
		const registry = new ToolRegistry();
		registry.register(makeTool("test_tool"));

		const result = await registry.execute("test_tool", "not-json");
		expect(result).toContain("Invalid JSON");
	});

	test("execute returns error string when tool throws", async () => {
		const registry = new ToolRegistry();
		const failTool = makeTool("fail", async () => {
			throw new Error("boom");
		});
		registry.register(failTool);

		const result = await registry.execute("fail", '{"input":"x"}');
		expect(result).toContain("boom");
	});

	test("prevents duplicate registration", () => {
		const registry = new ToolRegistry();
		registry.register(makeTool("dupe"));

		expect(() => registry.register(makeTool("dupe"))).toThrow("already registered");
	});
});
