import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { mkdirSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { resolveDiscovery } from "../../src/config/discovery.ts";
import { createTestSandbox } from "../helpers/sandbox.ts";

describe("config/discovery", () => {
	let sandbox: ReturnType<typeof createTestSandbox>;

	beforeAll(() => {
		sandbox = createTestSandbox("discovery");
	});

	afterAll(() => {
		sandbox.cleanup();
	});

	describe("resolveDiscovery()", () => {
		test("finds .heddle/ in cwd", () => {
			const heddleDir = join(sandbox.project, ".heddle");
			mkdirSync(heddleDir, { recursive: true });

			const result = resolveDiscovery(sandbox.project, sandbox.home);
			const heddleLevels = result.levels.filter((l) => l.source === "heddle");
			expect(heddleLevels.length).toBeGreaterThanOrEqual(1);
			expect(heddleLevels.some((l) => l.path === heddleDir)).toBe(true);
		});

		test("walks up and finds .heddle/ at multiple levels", () => {
			// sandbox.project is inside sandbox.root, let's create nested dirs
			const deep = join(sandbox.project, "a", "b", "c");
			mkdirSync(deep, { recursive: true });

			// .heddle at project level (already exists from previous test)
			const projectHeddle = join(sandbox.project, ".heddle");
			mkdirSync(projectHeddle, { recursive: true });

			// .heddle at project/a level
			const midHeddle = join(sandbox.project, "a", ".heddle");
			mkdirSync(midHeddle, { recursive: true });

			const result = resolveDiscovery(deep, sandbox.home);
			const heddlePaths = result.levels.filter((l) => l.source === "heddle").map((l) => l.path);

			// Deepest first
			expect(heddlePaths.indexOf(midHeddle)).toBeLessThan(heddlePaths.indexOf(projectHeddle));
		});

		test("stops walking at homeDir", () => {
			// Use a project dir under home so the walk actually passes through home
			const projectUnderHome = join(sandbox.home, "projects", "myproject");
			mkdirSync(projectUnderHome, { recursive: true });
			// Create .heddle above home — should not be found
			const aboveHome = join(sandbox.root, ".heddle-above");
			mkdirSync(aboveHome, { recursive: true });

			// Set HEDDLE_HOME to something nonexistent to avoid it adding extra levels
			const origHH = process.env.HEDDLE_HOME;
			process.env.HEDDLE_HOME = join(sandbox.root, "nonexistent-hh");

			const result = resolveDiscovery(projectUnderHome, sandbox.home);
			const paths = result.levels.map((l) => l.path);
			expect(paths).not.toContain(aboveHome);

			if (origHH) process.env.HEDDLE_HOME = origHH;
			else process.env.HEDDLE_HOME = sandbox.heddleHome;
		});

		test("finds .agents/skills/ at repo root", () => {
			// Create a .git dir in project to make it a repo root
			mkdirSync(join(sandbox.project, ".git"), { recursive: true });
			const agentsSkills = join(sandbox.project, ".agents", "skills");
			mkdirSync(agentsSkills, { recursive: true });
			writeFileSync(join(agentsSkills, "test.md"), "# Test skill");

			const result = resolveDiscovery(sandbox.project, sandbox.home);
			const agentsLevels = result.levels.filter((l) => l.source === "agents");
			expect(agentsLevels).toHaveLength(1);
			expect(agentsLevels[0]!.path).toBe(agentsSkills);
		});

		test("handles /etc/heddle graceful failure (EACCES)", () => {
			// This should not throw even if /etc/heddle doesn't exist or is inaccessible
			const result = resolveDiscovery(sandbox.project, sandbox.home);
			// Just verify it doesn't throw and returns a valid result
			expect(result.levels).toBeInstanceOf(Array);
		});

		test("follows symlinks via statSync", () => {
			const realDir = join(sandbox.root, "real-heddle");
			mkdirSync(join(realDir, "skills"), { recursive: true });
			writeFileSync(join(realDir, "skills", "linked.md"), "# Linked skill");

			const linkTarget = join(sandbox.project, "linked-project");
			mkdirSync(linkTarget, { recursive: true });
			symlinkSync(realDir, join(linkTarget, ".heddle"));

			const result = resolveDiscovery(linkTarget, sandbox.home);
			const heddleLevels = result.levels.filter((l) => l.source === "heddle");
			expect(heddleLevels.some((l) => l.skills.includes("linked.md"))).toBe(true);
		});

		test("content priority: deepest .heddle/ first", () => {
			const deep = join(sandbox.project, "deep-priority");
			mkdirSync(join(deep, ".heddle", "skills"), { recursive: true });
			mkdirSync(join(sandbox.project, ".heddle", "skills"), {
				recursive: true,
			});
			writeFileSync(join(deep, ".heddle", "skills", "deep.md"), "deep skill");
			writeFileSync(join(sandbox.project, ".heddle", "skills", "shallow.md"), "shallow skill");

			const result = resolveDiscovery(deep, sandbox.home);
			const heddleLevels = result.levels.filter((l) => l.source === "heddle");
			// First heddle level should be the deepest one
			expect(heddleLevels[0]!.path).toBe(join(deep, ".heddle"));
		});

		test("includes HEDDLE_HOME level", () => {
			// HEDDLE_HOME is set by sandbox
			mkdirSync(join(sandbox.heddleHome, "skills"), { recursive: true });
			writeFileSync(join(sandbox.heddleHome, "skills", "global.md"), "global");

			const result = resolveDiscovery(sandbox.project, sandbox.home);
			const heddleLevels = result.levels.filter((l) => l.source === "heddle");
			expect(heddleLevels.some((l) => l.path === sandbox.heddleHome)).toBe(true);
		});

		test("finds config.toml in levels", () => {
			const dir = join(sandbox.project, "config-test");
			const heddleDir = join(dir, ".heddle");
			mkdirSync(heddleDir, { recursive: true });
			writeFileSync(join(heddleDir, "config.toml"), "model = 'test'");

			const result = resolveDiscovery(dir, sandbox.home);
			const level = result.levels.find((l) => l.path === heddleDir);
			expect(level?.config).toBe(join(heddleDir, "config.toml"));
		});

		test("handles .git file (worktree) for repo root detection", () => {
			const worktreeDir = join(sandbox.root, "worktree-project");
			mkdirSync(worktreeDir, { recursive: true });
			// .git as a file (worktree format)
			writeFileSync(join(worktreeDir, ".git"), "gitdir: /some/path/.git/worktrees/branch");
			const agentsSkills = join(worktreeDir, ".agents", "skills");
			mkdirSync(agentsSkills, { recursive: true });
			writeFileSync(join(agentsSkills, "wt.md"), "worktree skill");

			const result = resolveDiscovery(worktreeDir, sandbox.home);
			const agentsLevels = result.levels.filter((l) => l.source === "agents");
			expect(agentsLevels).toHaveLength(1);
		});

		test("handles permission errors gracefully", () => {
			// Nonexistent startDir should not throw
			const result = resolveDiscovery(join(sandbox.root, "nonexistent"), sandbox.home);
			expect(result.levels).toBeInstanceOf(Array);
		});
	});
});
