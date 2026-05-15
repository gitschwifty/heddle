import { describe, expect, test } from "bun:test";
import { createBashTool } from "../../src/tools/bash.ts";

describe("bash tool (negative)", () => {
	const tool = createBashTool();

	test("returns non-zero exit code for failing command", async () => {
		const result = await tool.execute({ command: "exit 42" });
		expect(result).toContain("Exit code: 42");
	});

	test("captures stderr from failing command", async () => {
		const result = await tool.execute({ command: "echo 'bad stuff' >&2 && exit 1" });
		expect(result).toContain("STDERR");
		expect(result).toContain("bad stuff");
		expect(result).toContain("Exit code: 1");
	});

	test("returns stderr even on success (exit 0)", async () => {
		const result = await tool.execute({ command: "echo 'warning' >&2" });
		expect(result).toContain("STDERR");
		expect(result).toContain("warning");
	});

	test("handles command not found", async () => {
		const result = await tool.execute({ command: "nonexistent_command_xyz_12345" });
		expect(result).toContain("not found");
	});

	test("returns (no output) for empty command output", async () => {
		const result = await tool.execute({ command: "true" });
		expect(result).toBe("(no output)");
	});
});
