import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createSaveMemoryTool } from "../../src/tools/save-memory.ts";

describe("save_memory tool", () => {
	let dir: string;
	let projectMemDir: string;
	let globalMemDir: string;
	let originalEnv: string | undefined;

	beforeAll(() => {
		dir = mkdtempSync(join(tmpdir(), "heddle-save-memory-"));
		projectMemDir = join(dir, "project-memory");
		globalMemDir = join(dir, "global-memory");
		mkdirSync(projectMemDir, { recursive: true });
		mkdirSync(globalMemDir, { recursive: true });
		originalEnv = process.env.HEDDLE_HOME;
		// Point global memory dir to our test dir
		process.env.HEDDLE_HOME = join(dir, "heddle-home");
		mkdirSync(join(dir, "heddle-home", "memory"), { recursive: true });
	});

	afterAll(() => {
		if (originalEnv === undefined) {
			delete process.env.HEDDLE_HOME;
		} else {
			process.env.HEDDLE_HOME = originalEnv;
		}
		rmSync(dir, { recursive: true, force: true });
	});

	test("creates MEMORY.md if it doesn't exist", async () => {
		const freshDir = join(dir, "fresh-project");
		mkdirSync(freshDir, { recursive: true });
		const tool = createSaveMemoryTool(freshDir);

		const result = await tool.execute({ content: "Remember this" });
		expect(result).toContain("Saved");

		const content = readFileSync(join(freshDir, "MEMORY.md"), "utf-8");
		expect(content).toContain("Remember this");
	});

	test("appends timestamped section to existing MEMORY.md", async () => {
		const memDir = join(dir, "append-test");
		mkdirSync(memDir, { recursive: true });
		writeFileSync(join(memDir, "MEMORY.md"), "# Existing\n\nOld content\n");
		const tool = createSaveMemoryTool(memDir);

		await tool.execute({ content: "New memory" });

		const content = readFileSync(join(memDir, "MEMORY.md"), "utf-8");
		expect(content).toContain("Old content");
		expect(content).toContain("New memory");
		// Should have ISO timestamp header
		expect(content).toMatch(/## \d{4}-\d{2}-\d{2}T/);
	});

	test("respects scope='global' vs scope='project'", async () => {
		const tool = createSaveMemoryTool(projectMemDir);

		await tool.execute({ content: "Project note", scope: "project" });
		await tool.execute({ content: "Global note", scope: "global" });

		const projectContent = readFileSync(join(projectMemDir, "MEMORY.md"), "utf-8");
		expect(projectContent).toContain("Project note");
		expect(projectContent).not.toContain("Global note");

		const globalContent = readFileSync(join(dir, "heddle-home", "memory", "MEMORY.md"), "utf-8");
		expect(globalContent).toContain("Global note");
		expect(globalContent).not.toContain("Project note");
	});

	test("default scope is 'project'", async () => {
		const defaultDir = join(dir, "default-scope");
		mkdirSync(defaultDir, { recursive: true });
		const tool = createSaveMemoryTool(defaultDir);

		await tool.execute({ content: "Default scope note" });

		const content = readFileSync(join(defaultDir, "MEMORY.md"), "utf-8");
		expect(content).toContain("Default scope note");
	});
});
