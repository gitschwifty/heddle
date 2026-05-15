import { describe, expect, test } from "bun:test";
import { Type } from "@sinclair/typebox";
import { createBashTool } from "../../src/tools/bash.ts";
import { ToolRegistry } from "../../src/tools/registry.ts";
import type { HeddleTool } from "../../src/tools/types.ts";

describe("bash tool abort signal", () => {
	const tool = createBashTool();

	test("returns early if signal already aborted", async () => {
		const ac = new AbortController();
		ac.abort();
		const result = await tool.execute({ command: "echo hello" }, { signal: ac.signal });
		expect(result).toBe("Error: Aborted");
	});

	test("kills running process on abort", async () => {
		const ac = new AbortController();
		// Start a long-running command
		const resultPromise = tool.execute({ command: "sleep 30" }, { signal: ac.signal });
		// Abort after a short delay
		setTimeout(() => ac.abort(), 100);
		const start = Date.now();
		const result = await resultPromise;
		const elapsed = Date.now() - start;
		// Should resolve quickly (well under 30s)
		expect(elapsed).toBeLessThan(5000);
		expect(result).toBe("Error: Aborted");
	});

	test("works normally without signal", async () => {
		const result = await tool.execute({ command: "echo hello" });
		expect(result).toBe("hello\n");
	});

	test("works normally with non-aborted signal", async () => {
		const ac = new AbortController();
		const result = await tool.execute({ command: "echo hello" }, { signal: ac.signal });
		expect(result).toBe("hello\n");
	});
});

describe("registry passes signal to tool", () => {
	test("signal is forwarded to tool.execute", async () => {
		let receivedSignal: AbortSignal | undefined;
		const tool: HeddleTool = {
			name: "test_signal",
			description: "test",
			parameters: Type.Object({ text: Type.String() }),
			execute: async (_params, options) => {
				receivedSignal = options?.signal;
				return "ok";
			},
		};

		const registry = new ToolRegistry();
		registry.register(tool);

		const ac = new AbortController();
		await registry.execute("test_signal", JSON.stringify({ text: "hi" }), { signal: ac.signal });

		expect(receivedSignal).toBe(ac.signal);
	});

	test("registry works without signal (backward compatible)", async () => {
		let called = false;
		const tool: HeddleTool = {
			name: "test_no_signal",
			description: "test",
			parameters: Type.Object({ text: Type.String() }),
			execute: async () => {
				called = true;
				return "ok";
			},
		};

		const registry = new ToolRegistry();
		registry.register(tool);

		const result = await registry.execute("test_no_signal", JSON.stringify({ text: "hi" }));
		expect(called).toBe(true);
		expect(result).toBe("ok");
	});
});
