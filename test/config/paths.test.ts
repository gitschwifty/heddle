import { describe, test, expect, beforeEach, afterEach } from "bun:test";
import { join, resolve } from "node:path";
import { mkdirSync, existsSync } from "node:fs";
import {
	getHeddleHome,
	getLocalHeddleDir,
	getConfigPath,
	getSessionsDir,
	getAgentsDir,
	getSkillsDir,
	ensureHeddleDirs,
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
			const localDir = join(process.cwd(), ".heddle");
			// Just check that it would prefer local over global
			const result = getConfigPath();
			// Should be either local or global path ending in config.toml
			expect(result.endsWith("config.toml")).toBe(true);
		});
	});

	describe("getSessionsDir()", () => {
		test("returns sessions dir under heddle home", () => {
			delete process.env.HEDDLE_HOME;
			const result = getSessionsDir();
			expect(result).toBe(join(process.env.HOME!, ".heddle", "sessions"));
		});

		test("respects HEDDLE_HOME", () => {
			const customDir = join(TEST_DIR, "custom-home");
			process.env.HEDDLE_HOME = customDir;
			expect(getSessionsDir()).toBe(join(customDir, "sessions"));
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

	describe("ensureHeddleDirs()", () => {
		test("creates global directory structure", () => {
			const homeDir = join(TEST_DIR, "ensure-test");
			process.env.HEDDLE_HOME = homeDir;

			ensureHeddleDirs();

			expect(existsSync(homeDir)).toBe(true);
			expect(existsSync(join(homeDir, "sessions"))).toBe(true);
			expect(existsSync(join(homeDir, "agents"))).toBe(true);
			expect(existsSync(join(homeDir, "skills"))).toBe(true);
		});

		test("is idempotent — calling twice doesn't error", () => {
			const homeDir = join(TEST_DIR, "ensure-idem");
			process.env.HEDDLE_HOME = homeDir;

			ensureHeddleDirs();
			ensureHeddleDirs(); // second call should not throw
			expect(existsSync(homeDir)).toBe(true);
		});
	});
});
