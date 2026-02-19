import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { type HeddleConfig, loadConfig } from "../../src/config/loader.ts";

const TEST_DIR = join(import.meta.dir, ".tmp-loader-test");

function cleanup() {
	try {
		const { execSync } = require("node:child_process");
		execSync(`rm -rf "${TEST_DIR}"`);
	} catch {}
}

describe("config/loader", () => {
	const origEnv = { ...process.env };

	beforeEach(() => {
		cleanup();
		mkdirSync(TEST_DIR, { recursive: true });
	});

	afterEach(() => {
		process.env = { ...origEnv };
		cleanup();
	});

	describe("loadConfig()", () => {
		test("returns defaults when no config files exist", () => {
			const globalDir = join(TEST_DIR, "no-config-global");
			const localDir = join(TEST_DIR, "no-config-local");
			mkdirSync(globalDir, { recursive: true });

			process.env.HEDDLE_HOME = globalDir;
			delete process.env.OPENROUTER_API_KEY;
			delete process.env.HEDDLE_MODEL;
			const config = loadConfig(localDir);

			expect(config.model).toBe("moonshotai/kimi-k2.5");
			expect(config.apiKey).toBeUndefined();
		});

		test("loads global config.toml", () => {
			const globalDir = join(TEST_DIR, "global-only");
			mkdirSync(globalDir, { recursive: true });
			writeFileSync(join(globalDir, "config.toml"), 'model = "anthropic/claude-sonnet"\n');

			process.env.HEDDLE_HOME = globalDir;
			const config = loadConfig(join(TEST_DIR, "nonexistent-local"));

			expect(config.model).toBe("anthropic/claude-sonnet");
		});

		test("local config.toml overrides global", () => {
			const globalDir = join(TEST_DIR, "merge-global");
			const localDir = join(TEST_DIR, "merge-local");
			mkdirSync(globalDir, { recursive: true });
			mkdirSync(localDir, { recursive: true });

			writeFileSync(join(globalDir, "config.toml"), 'model = "global-model"\nsystem_prompt = "global prompt"\n');
			writeFileSync(join(localDir, "config.toml"), 'model = "local-model"\n');

			process.env.HEDDLE_HOME = globalDir;
			const config = loadConfig(localDir);

			expect(config.model).toBe("local-model");
			expect(config.systemPrompt).toBe("global prompt");
		});

		test("env vars override config files", () => {
			const globalDir = join(TEST_DIR, "env-override");
			mkdirSync(globalDir, { recursive: true });
			writeFileSync(join(globalDir, "config.toml"), 'model = "file-model"\n');

			process.env.HEDDLE_HOME = globalDir;
			process.env.HEDDLE_MODEL = "env-model";
			process.env.OPENROUTER_API_KEY = "env-key";

			const config = loadConfig(join(TEST_DIR, "nonexistent-local"));

			expect(config.model).toBe("env-model");
			expect(config.apiKey).toBe("env-key");
		});

		test("handles malformed TOML gracefully", () => {
			const globalDir = join(TEST_DIR, "malformed");
			mkdirSync(globalDir, { recursive: true });
			writeFileSync(join(globalDir, "config.toml"), "this is not valid toml [[[");

			process.env.HEDDLE_HOME = globalDir;
			// Should not throw — returns defaults
			const config = loadConfig(join(TEST_DIR, "nonexistent-local"));
			expect(config.model).toBe("moonshotai/kimi-k2.5");
		});

		test("handles empty config file", () => {
			const globalDir = join(TEST_DIR, "empty");
			mkdirSync(globalDir, { recursive: true });
			writeFileSync(join(globalDir, "config.toml"), "");

			process.env.HEDDLE_HOME = globalDir;
			const config = loadConfig(join(TEST_DIR, "nonexistent-local"));
			expect(config.model).toBe("moonshotai/kimi-k2.5");
		});

		test("loads api_key from config file", () => {
			const globalDir = join(TEST_DIR, "api-key");
			mkdirSync(globalDir, { recursive: true });
			writeFileSync(join(globalDir, "config.toml"), 'api_key = "sk-from-config"\n');

			process.env.HEDDLE_HOME = globalDir;
			delete process.env.OPENROUTER_API_KEY;

			const config = loadConfig(join(TEST_DIR, "nonexistent-local"));
			expect(config.apiKey).toBe("sk-from-config");
		});
	});
});
