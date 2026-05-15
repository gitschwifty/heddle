import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { rmSync } from "node:fs";
import { mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createReadTool } from "../../src/tools/read.ts";

describe("read_file (negative)", () => {
	let dir: string;
	const tool = createReadTool();

	beforeEach(async () => {
		dir = await mkdtemp(join(tmpdir(), "heddle-read-"));
	});

	afterEach(() => {
		rmSync(dir, { recursive: true, force: true });
	});

	test("returns error for nonexistent file", async () => {
		const result = await tool.execute({ file_path: join(dir, "nope.txt") });
		expect(result).toContain("Error");
		expect(result).toContain("nope.txt");
	});

	test("returns error for a directory path", async () => {
		const result = await tool.execute({ file_path: dir });
		expect(result).toContain("Error");
	});

	test("returns error for empty path", async () => {
		const result = await tool.execute({ file_path: "" });
		expect(result).toContain("Error");
	});
});
