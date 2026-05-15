import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { loadMemoryContext } from "../../src/memory/loader.ts";

describe("loadMemoryContext", () => {
	let dir: string;
	let originalEnv: string | undefined;
	let projectPath: string;

	beforeAll(() => {
		dir = mkdtempSync(join(tmpdir(), "heddle-memory-loader-"));
		originalEnv = process.env.HEDDLE_HOME;
		process.env.HEDDLE_HOME = dir;
		projectPath = join(dir, "test-project");
		mkdirSync(projectPath, { recursive: true });
	});

	afterAll(() => {
		if (originalEnv === undefined) {
			delete process.env.HEDDLE_HOME;
		} else {
			process.env.HEDDLE_HOME = originalEnv;
		}
		rmSync(dir, { recursive: true, force: true });
	});

	test("returns null when no memory files exist", () => {
		const result = loadMemoryContext(projectPath);
		expect(result).toBeNull();
	});

	test("loads global memory only", () => {
		const globalMemDir = join(dir, "memory");
		mkdirSync(globalMemDir, { recursive: true });
		writeFileSync(join(globalMemDir, "MEMORY.md"), "Global notes here");

		const result = loadMemoryContext(projectPath);
		expect(result).toContain("## Global Memory");
		expect(result).toContain("Global notes here");
		expect(result).not.toContain("## Project Memory");

		// cleanup
		rmSync(globalMemDir, { recursive: true, force: true });
	});

	test("loads project memory only", () => {
		const projMemDir = join(dir, "projects", "-test-project", "memory");
		mkdirSync(projMemDir, { recursive: true });
		writeFileSync(join(projMemDir, "MEMORY.md"), "Project notes here");

		const result = loadMemoryContext("/test-project");
		expect(result).toContain("## Project Memory");
		expect(result).toContain("Project notes here");
		expect(result).not.toContain("## Global Memory");

		// cleanup
		rmSync(join(dir, "projects"), { recursive: true, force: true });
	});

	test("loads both and concatenates global-first", () => {
		const globalMemDir = join(dir, "memory");
		mkdirSync(globalMemDir, { recursive: true });
		writeFileSync(join(globalMemDir, "MEMORY.md"), "Global stuff");

		const projMemDir = join(dir, "projects", "-test-project", "memory");
		mkdirSync(projMemDir, { recursive: true });
		writeFileSync(join(projMemDir, "MEMORY.md"), "Project stuff");

		const result = loadMemoryContext("/test-project");
		expect(result).not.toBeNull();
		const globalIdx = result!.indexOf("## Global Memory");
		const projectIdx = result!.indexOf("## Project Memory");
		expect(globalIdx).toBeLessThan(projectIdx);
		expect(result).toContain("Global stuff");
		expect(result).toContain("Project stuff");

		// cleanup
		rmSync(globalMemDir, { recursive: true, force: true });
		rmSync(join(dir, "projects"), { recursive: true, force: true });
	});

	test("handles empty MEMORY.md files", () => {
		const globalMemDir = join(dir, "memory");
		mkdirSync(globalMemDir, { recursive: true });
		writeFileSync(join(globalMemDir, "MEMORY.md"), "");

		const result = loadMemoryContext(projectPath);
		expect(result).toBeNull();

		// cleanup
		rmSync(globalMemDir, { recursive: true, force: true });
	});
});
