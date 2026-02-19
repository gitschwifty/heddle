import { beforeEach, describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { findAllAgentsMd, loadAgentsContext } from "../../src/config/agents-md.ts";

describe("findAllAgentsMd", () => {
	let tempDir: string;

	beforeEach(() => {
		tempDir = mkdtempSync(join(tmpdir(), "heddle-agents-md-"));
	});

	test("finds AGENTS.md in start directory", () => {
		const agentsPath = join(tempDir, "AGENTS.md");
		writeFileSync(agentsPath, "# Project instructions");

		const result = findAllAgentsMd(tempDir);
		expect(result).toEqual([agentsPath]);
	});

	test("finds agents.md case-insensitively", () => {
		const agentsPath = join(tempDir, "agents.md");
		writeFileSync(agentsPath, "# lowercase agents");

		const result = findAllAgentsMd(tempDir);
		expect(result).toEqual([agentsPath]);
	});

	test("finds Agents.Md mixed case", () => {
		const agentsPath = join(tempDir, "Agents.Md");
		writeFileSync(agentsPath, "# mixed case");

		const result = findAllAgentsMd(tempDir);
		expect(result).toEqual([agentsPath]);
	});

	test("finds AGENTS.md in parent directory", () => {
		const child = join(tempDir, "child");
		mkdirSync(child);
		const agentsPath = join(tempDir, "AGENTS.md");
		writeFileSync(agentsPath, "# Parent instructions");

		const result = findAllAgentsMd(child);
		expect(result).toEqual([agentsPath]);
	});

	test("finds multiple AGENTS.md files ordered farthest-first", () => {
		const child = join(tempDir, "child");
		mkdirSync(child);
		const parentAgents = join(tempDir, "AGENTS.md");
		const childAgents = join(child, "AGENTS.md");
		writeFileSync(parentAgents, "# Parent");
		writeFileSync(childAgents, "# Child");

		const result = findAllAgentsMd(child);
		expect(result).toEqual([parentAgents, childAgents]);
	});

	test("returns empty array when none exist", () => {
		const result = findAllAgentsMd(tempDir);
		expect(result).toEqual([]);
	});

	test("checks HEDDLE_HOME for AGENTS.md", () => {
		const heddleHome = mkdtempSync(join(tmpdir(), "heddle-home-"));
		const agentsPath = join(heddleHome, "AGENTS.md");
		writeFileSync(agentsPath, "# HEDDLE_HOME instructions");

		const originalHome = process.env.HEDDLE_HOME;
		try {
			process.env.HEDDLE_HOME = heddleHome;
			const result = findAllAgentsMd(tempDir);
			expect(result).toContain(agentsPath);
		} finally {
			if (originalHome) process.env.HEDDLE_HOME = originalHome;
			else delete process.env.HEDDLE_HOME;
		}
	});

	test("deduplicates HEDDLE_HOME when in walk path", () => {
		const agentsPath = join(tempDir, "AGENTS.md");
		writeFileSync(agentsPath, "# Instructions");

		const originalHome = process.env.HEDDLE_HOME;
		try {
			// Set HEDDLE_HOME to the same dir that's in the walk path
			process.env.HEDDLE_HOME = tempDir;
			const result = findAllAgentsMd(tempDir);
			// Should only appear once despite being both in walk and HEDDLE_HOME
			expect(result.filter((p) => p === agentsPath)).toHaveLength(1);
		} finally {
			if (originalHome) process.env.HEDDLE_HOME = originalHome;
			else delete process.env.HEDDLE_HOME;
		}
	});
});

describe("loadAgentsContext", () => {
	let tempDir: string;

	beforeEach(() => {
		tempDir = mkdtempSync(join(tmpdir(), "heddle-agents-ctx-"));
	});

	test("concatenates multiple AGENTS.md files", () => {
		const child = join(tempDir, "child");
		mkdirSync(child);
		writeFileSync(join(tempDir, "AGENTS.md"), "# Parent rules");
		writeFileSync(join(child, "AGENTS.md"), "# Child rules");

		const result = loadAgentsContext(child);
		expect(result).toBe("# Parent rules\n\n# Child rules");
	});

	test("returns null when no files found", () => {
		const result = loadAgentsContext(tempDir);
		expect(result).toBeNull();
	});

	test("returns single file content without extra separators", () => {
		writeFileSync(join(tempDir, "AGENTS.md"), "# Only one");

		const result = loadAgentsContext(tempDir);
		expect(result).toBe("# Only one");
	});
});
