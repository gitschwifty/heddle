import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { rmSync } from "node:fs";
import { mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createGlobTool } from "../../src/tools/glob.ts";

describe("glob tool (negative)", () => {
	let dir: string;
	const tool = createGlobTool();

	beforeEach(async () => {
		dir = await mkdtemp(join(tmpdir(), "heddle-glob-"));
	});

	afterEach(() => {
		rmSync(dir, { recursive: true, force: true });
	});

	test("returns no-match message for pattern with no hits", async () => {
		const result = await tool.execute({ pattern: "*.nonexistent_ext", path: dir });
		expect(result).toContain("No files matched");
	});

	test("returns no-match for empty directory", async () => {
		const result = await tool.execute({ pattern: "*", path: dir });
		expect(result).toContain("No files matched");
	});

	test("handles nonexistent directory path", async () => {
		const result = await tool.execute({
			pattern: "*.ts",
			path: join(dir, "does-not-exist"),
		});
		// Should either error or return no matches
		expect(result).toMatch(/No files matched|Error/);
	});
});
