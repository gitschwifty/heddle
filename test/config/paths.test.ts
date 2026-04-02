import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { existsSync, mkdirSync } from "node:fs";
import { join, resolve } from "node:path";
import {
	encodePath,
	ensureHeddleDirs,
	findRepoRoot,
	getAgentsDir,
	getConfigPath,
	getHeddleHome,
	getLocalHeddleDir,
	getProjectDir,
	getProjectSessionsDir,
	getSkillsDir,
	getSystemHeddleDir,
	walkUpHeddleDirs,
} from "../../src/config/paths.ts";

const TEST_DIR = join(import.meta.dir, ".tmp-paths-test");

function cleanup() {
	try {
		const { execSync } = require("node:child_process");
		execSync(`rm -rf "${TEST_DIR}"`);
	} catch {}
}

describe("config/paths", () => {
	const origEnv = { ...process.env };

	beforeEach(() => {
		cleanup();
		mkdirSync(TEST_DIR, { recursive: true });
	});

	afterEach(() => {
		process.env = { ...origEnv };
		cleanup();
	});

	describe("getHeddleHome()", () => {
		test("returns ~/.heddle by default", () => {
			delete process.env.HEDDLE_HOME;
			const home = getHeddleHome();
			expect(home).toBe(join(process.env.HOME!, ".heddle"));
		});

		test("respects HEDDLE_HOME env var (absolute path)", () => {
			const customDir = join(TEST_DIR, "custom-home");
			process.env.HEDDLE_HOME = customDir;
			expect(getHeddleHome()).toBe(customDir);
		});

		test("resolves HEDDLE_HOME relative to cwd", () => {
			process.env.HEDDLE_HOME = ".heddle-dev";
			const result = getHeddleHome();
			expect(result).toBe(resolve(process.cwd(), ".heddle-dev"));
		});
	});

	describe("getLocalHeddleDir()", () => {
		test("returns .heddle in cwd", () => {
			expect(getLocalHeddleDir()).toBe(join(process.cwd(), ".heddle"));
		});
	});

	describe("getConfigPath()", () => {
		test("returns local config.toml when it exists", () => {
			const result = getConfigPath();
			expect(result.endsWith("config.toml")).toBe(true);
		});
	});

	describe("encodePath()", () => {
		test("encodes absolute path with dashes", () => {
			expect(encodePath("/home/user/repos/heddle")).toBe("-home-user-repos-heddle");
		});

		test("encodes path with trailing slash", () => {
			expect(encodePath("/home/user/repos/heddle/")).toBe("-home-user-repos-heddle");
		});

		test("handles single segment", () => {
			expect(encodePath("/tmp")).toBe("-tmp");
		});
	});

	describe("getProjectDir()", () => {
		test("returns project dir under heddle home", () => {
			delete process.env.HEDDLE_HOME;
			const cwd = process.cwd();
			const encoded = encodePath(cwd);
			expect(getProjectDir()).toBe(join(process.env.HOME!, ".heddle", "projects", encoded));
		});

		test("accepts explicit path argument", () => {
			delete process.env.HEDDLE_HOME;
			const result = getProjectDir("/home/user/repos/heddle");
			expect(result).toBe(join(process.env.HOME!, ".heddle", "projects", "-home-user-repos-heddle"));
		});

		test("respects HEDDLE_HOME", () => {
			const customDir = join(TEST_DIR, "custom-home");
			process.env.HEDDLE_HOME = customDir;
			const result = getProjectDir("/foo/bar");
			expect(result).toBe(join(customDir, "projects", "-foo-bar"));
		});
	});

	describe("getProjectSessionsDir()", () => {
		test("returns sessions dir under project dir", () => {
			delete process.env.HEDDLE_HOME;
			const cwd = process.cwd();
			const encoded = encodePath(cwd);
			expect(getProjectSessionsDir()).toBe(join(process.env.HOME!, ".heddle", "projects", encoded, "sessions"));
		});

		test("respects HEDDLE_HOME", () => {
			const customDir = join(TEST_DIR, "custom-home");
			process.env.HEDDLE_HOME = customDir;
			const encoded = encodePath(process.cwd());
			expect(getProjectSessionsDir()).toBe(join(customDir, "projects", encoded, "sessions"));
		});
	});

	describe("getAgentsDir()", () => {
		test("returns agents dir under heddle home", () => {
			delete process.env.HEDDLE_HOME;
			expect(getAgentsDir()).toBe(join(process.env.HOME!, ".heddle", "agents"));
		});
	});

	describe("getSkillsDir()", () => {
		test("returns skills dir under heddle home", () => {
			delete process.env.HEDDLE_HOME;
			expect(getSkillsDir()).toBe(join(process.env.HOME!, ".heddle", "skills"));
		});
	});

	describe("walkUpHeddleDirs()", () => {
		test("finds .heddle/ dirs walking up from startDir to homeDir", () => {
			const home = join(TEST_DIR, "walk-home");
			const deep = join(home, "a", "b", "c");
			mkdirSync(deep, { recursive: true });
			mkdirSync(join(home, "a", ".heddle"), { recursive: true });
			mkdirSync(join(home, "a", "b", ".heddle"), { recursive: true });

			const result = walkUpHeddleDirs(deep, home);
			// Deepest first
			expect(result[0]).toBe(join(home, "a", "b", ".heddle"));
			expect(result[1]).toBe(join(home, "a", ".heddle"));
		});

		test("returns empty array when no .heddle/ dirs exist", () => {
			const home = join(TEST_DIR, "walk-empty");
			const project = join(home, "project");
			mkdirSync(project, { recursive: true });
			// Ensure HEDDLE_HOME doesn't contribute extra dirs
			process.env.HEDDLE_HOME = join(TEST_DIR, "nonexistent-hh");

			const result = walkUpHeddleDirs(project, home);
			expect(result).toEqual([]);
		});

		test("stops at homeDir (does not go above)", () => {
			const root = join(TEST_DIR, "walk-stop");
			const home = join(root, "home");
			const project = join(home, "project");
			mkdirSync(project, { recursive: true });
			// .heddle above home should not be found
			mkdirSync(join(root, ".heddle"), { recursive: true });
			// Prevent HEDDLE_HOME from adding extra dirs
			process.env.HEDDLE_HOME = join(TEST_DIR, "nonexistent-hh-stop");

			const result = walkUpHeddleDirs(project, home);
			expect(result.some((p) => p.includes(join(root, ".heddle")))).toBe(false);
		});

		test("includes .heddle/ at homeDir itself", () => {
			const home = join(TEST_DIR, "walk-athome");
			const project = join(home, "project");
			mkdirSync(project, { recursive: true });
			mkdirSync(join(home, ".heddle"), { recursive: true });

			const result = walkUpHeddleDirs(project, home);
			expect(result).toContain(join(home, ".heddle"));
		});

		test("includes HEDDLE_HOME if different from walk path", () => {
			const home = join(TEST_DIR, "walk-hh");
			const project = join(home, "project");
			const customHome = join(TEST_DIR, "walk-hh-custom");
			mkdirSync(project, { recursive: true });
			mkdirSync(customHome, { recursive: true });
			process.env.HEDDLE_HOME = customHome;

			const result = walkUpHeddleDirs(project, home);
			expect(result).toContain(customHome);
		});
	});

	describe("findRepoRoot()", () => {
		test("finds directory containing .git dir", () => {
			const repo = join(TEST_DIR, "repo-dir");
			const sub = join(repo, "src", "lib");
			mkdirSync(sub, { recursive: true });
			mkdirSync(join(repo, ".git"), { recursive: true });

			const result = findRepoRoot(sub);
			expect(result).toBe(repo);
		});

		test("finds directory containing .git file (worktree)", () => {
			const repo = join(TEST_DIR, "repo-file");
			const sub = join(repo, "src");
			mkdirSync(sub, { recursive: true });
			const { writeFileSync } = require("node:fs");
			writeFileSync(join(repo, ".git"), "gitdir: /some/path/.git/worktrees/branch");

			const result = findRepoRoot(sub);
			expect(result).toBe(repo);
		});

		test("returns undefined when no .git found", () => {
			// Use tmpdir-based path to avoid walking up into the real repo
			const { tmpdir } = require("node:os");
			const { realpathSync } = require("node:fs");
			const tmpBase = realpathSync(tmpdir());
			const noRepo = join(tmpBase, "heddle-no-repo-test");
			mkdirSync(noRepo, { recursive: true });

			const result = findRepoRoot(noRepo);
			expect(result).toBeUndefined();
		});
	});

	describe("getSystemHeddleDir()", () => {
		test("returns /etc/heddle", () => {
			expect(getSystemHeddleDir()).toBe("/etc/heddle");
		});
	});

	describe("ensureHeddleDirs()", () => {
		test("creates global and project directory structure", () => {
			const homeDir = join(TEST_DIR, "ensure-test");
			process.env.HEDDLE_HOME = homeDir;

			ensureHeddleDirs();

			expect(existsSync(homeDir)).toBe(true);
			expect(existsSync(join(homeDir, "agents"))).toBe(true);
			expect(existsSync(join(homeDir, "skills"))).toBe(true);
			// Project-specific dirs
			const encoded = encodePath(process.cwd());
			const projectDir = join(homeDir, "projects", encoded);
			expect(existsSync(join(projectDir, "sessions"))).toBe(true);
		});

		test("is idempotent — calling twice doesn't error", () => {
			const homeDir = join(TEST_DIR, "ensure-idem");
			process.env.HEDDLE_HOME = homeDir;

			ensureHeddleDirs();
			ensureHeddleDirs();
			expect(existsSync(homeDir)).toBe(true);
		});
	});
});
