import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { loadConfig } from "../../src/config/loader.ts";

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
		delete process.env.HEDDLE_MODEL;
		delete process.env.OPENROUTER_API_KEY;
		delete process.env.HEDDLE_BASE_URL;
		delete process.env.HEDDLE_MAX_TOKENS;
		delete process.env.HEDDLE_TEMPERATURE;
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
			const config = loadConfig(localDir);

			expect(config.model).toBe("openrouter/free");
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
			expect(config.model).toBe("openrouter/free");
		});

		test("handles empty config file", () => {
			const globalDir = join(TEST_DIR, "empty");
			mkdirSync(globalDir, { recursive: true });
			writeFileSync(join(globalDir, "config.toml"), "");

			process.env.HEDDLE_HOME = globalDir;
			const config = loadConfig(join(TEST_DIR, "nonexistent-local"));
			expect(config.model).toBe("openrouter/free");
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

		// ── New config fields ──────────────────────────────────────────

		test("loads weak_model from TOML", () => {
			const globalDir = join(TEST_DIR, "weak-model");
			mkdirSync(globalDir, { recursive: true });
			writeFileSync(join(globalDir, "config.toml"), 'weak_model = "openrouter/free"\n');

			process.env.HEDDLE_HOME = globalDir;
			const config = loadConfig(join(TEST_DIR, "nonexistent"));
			expect(config.weakModel).toBe("openrouter/free");
		});

		test("loads editor_model from TOML", () => {
			const globalDir = join(TEST_DIR, "editor-model");
			mkdirSync(globalDir, { recursive: true });
			writeFileSync(join(globalDir, "config.toml"), 'editor_model = "anthropic/claude-opus"\n');

			process.env.HEDDLE_HOME = globalDir;
			const config = loadConfig(join(TEST_DIR, "nonexistent"));
			expect(config.editorModel).toBe("anthropic/claude-opus");
		});

		test("loads max_tokens from TOML", () => {
			const globalDir = join(TEST_DIR, "max-tokens");
			mkdirSync(globalDir, { recursive: true });
			writeFileSync(join(globalDir, "config.toml"), "max_tokens = 4096\n");

			process.env.HEDDLE_HOME = globalDir;
			const config = loadConfig(join(TEST_DIR, "nonexistent"));
			expect(config.maxTokens).toBe(4096);
		});

		test("loads temperature from TOML", () => {
			const globalDir = join(TEST_DIR, "temperature");
			mkdirSync(globalDir, { recursive: true });
			writeFileSync(join(globalDir, "config.toml"), "temperature = 0.7\n");

			process.env.HEDDLE_HOME = globalDir;
			const config = loadConfig(join(TEST_DIR, "nonexistent"));
			expect(config.temperature).toBe(0.7);
		});

		test("temperature = 0.0 is valid", () => {
			const globalDir = join(TEST_DIR, "temp-zero");
			mkdirSync(globalDir, { recursive: true });
			writeFileSync(join(globalDir, "config.toml"), "temperature = 0.0\n");

			process.env.HEDDLE_HOME = globalDir;
			const config = loadConfig(join(TEST_DIR, "nonexistent"));
			expect(config.temperature).toBe(0);
		});

		test("loads base_url from TOML", () => {
			const globalDir = join(TEST_DIR, "base-url");
			mkdirSync(globalDir, { recursive: true });
			writeFileSync(join(globalDir, "config.toml"), 'base_url = "http://localhost:8080"\n');

			process.env.HEDDLE_HOME = globalDir;
			const config = loadConfig(join(TEST_DIR, "nonexistent"));
			expect(config.baseUrl).toBe("http://localhost:8080");
		});

		test("loads doom_loop_threshold from TOML", () => {
			const globalDir = join(TEST_DIR, "doom-threshold");
			mkdirSync(globalDir, { recursive: true });
			writeFileSync(join(globalDir, "config.toml"), "doom_loop_threshold = 5\n");

			process.env.HEDDLE_HOME = globalDir;
			const config = loadConfig(join(TEST_DIR, "nonexistent"));
			expect(config.doomLoopThreshold).toBe(5);
		});

		test("loads budget_limit from TOML", () => {
			const globalDir = join(TEST_DIR, "budget");
			mkdirSync(globalDir, { recursive: true });
			writeFileSync(join(globalDir, "config.toml"), "budget_limit = 1.50\n");

			process.env.HEDDLE_HOME = globalDir;
			const config = loadConfig(join(TEST_DIR, "nonexistent"));
			expect(config.budgetLimit).toBe(1.5);
		});

		test("approval_mode = 'yolo' is accepted", () => {
			const globalDir = join(TEST_DIR, "approval-yolo");
			mkdirSync(globalDir, { recursive: true });
			writeFileSync(join(globalDir, "config.toml"), 'approval_mode = "yolo"\n');

			process.env.HEDDLE_HOME = globalDir;
			const config = loadConfig(join(TEST_DIR, "nonexistent"));
			expect(config.approvalMode).toBe("yolo");
		});

		test("approval_mode = 'suggest' is accepted", () => {
			const globalDir = join(TEST_DIR, "approval-suggest");
			mkdirSync(globalDir, { recursive: true });
			writeFileSync(join(globalDir, "config.toml"), 'approval_mode = "suggest"\n');

			process.env.HEDDLE_HOME = globalDir;
			const config = loadConfig(join(TEST_DIR, "nonexistent"));
			expect(config.approvalMode).toBe("suggest");
		});

		test("invalid approval_mode is silently dropped", () => {
			const globalDir = join(TEST_DIR, "approval-invalid");
			mkdirSync(globalDir, { recursive: true });
			writeFileSync(join(globalDir, "config.toml"), 'approval_mode = "invalid"\n');

			process.env.HEDDLE_HOME = globalDir;
			const config = loadConfig(join(TEST_DIR, "nonexistent"));
			expect(config.approvalMode).toBeUndefined();
		});

		test("instructions as TOML array of strings", () => {
			const globalDir = join(TEST_DIR, "instructions-array");
			mkdirSync(globalDir, { recursive: true });
			writeFileSync(join(globalDir, "config.toml"), 'instructions = ["HEDDLE.md", "AGENTS.md"]\n');

			process.env.HEDDLE_HOME = globalDir;
			const config = loadConfig(join(TEST_DIR, "nonexistent"));
			expect(config.instructions).toEqual(["HEDDLE.md", "AGENTS.md"]);
		});

		test("instructions as bare string is rejected (must be array)", () => {
			const globalDir = join(TEST_DIR, "instructions-string");
			mkdirSync(globalDir, { recursive: true });
			writeFileSync(join(globalDir, "config.toml"), 'instructions = "HEDDLE.md"\n');

			process.env.HEDDLE_HOME = globalDir;
			const config = loadConfig(join(TEST_DIR, "nonexistent"));
			expect(config.instructions).toBeUndefined();
		});

		test("all new fields default to undefined", () => {
			const globalDir = join(TEST_DIR, "defaults-check");
			mkdirSync(globalDir, { recursive: true });
			writeFileSync(join(globalDir, "config.toml"), "");

			process.env.HEDDLE_HOME = globalDir;
			const config = loadConfig(join(TEST_DIR, "nonexistent"));
			expect(config.weakModel).toBeUndefined();
			expect(config.editorModel).toBeUndefined();
			expect(config.maxTokens).toBeUndefined();
			expect(config.temperature).toBeUndefined();
			expect(config.baseUrl).toBeUndefined();
			expect(config.approvalMode).toBeUndefined();
			expect(config.instructions).toBeUndefined();
			expect(config.doomLoopThreshold).toBeUndefined();
			expect(config.budgetLimit).toBeUndefined();
		});

		// ── Env var overrides for new fields ──

		test("HEDDLE_BASE_URL env var overrides config", () => {
			const globalDir = join(TEST_DIR, "env-base-url");
			mkdirSync(globalDir, { recursive: true });
			writeFileSync(join(globalDir, "config.toml"), 'base_url = "http://toml-url"\n');

			process.env.HEDDLE_HOME = globalDir;
			process.env.HEDDLE_BASE_URL = "http://env-url";
			const config = loadConfig(join(TEST_DIR, "nonexistent"));
			expect(config.baseUrl).toBe("http://env-url");
		});

		test("HEDDLE_MAX_TOKENS env var overrides config", () => {
			const globalDir = join(TEST_DIR, "env-max-tokens");
			mkdirSync(globalDir, { recursive: true });

			process.env.HEDDLE_HOME = globalDir;
			process.env.HEDDLE_MAX_TOKENS = "8192";
			const config = loadConfig(join(TEST_DIR, "nonexistent"));
			expect(config.maxTokens).toBe(8192);
		});

		test("HEDDLE_TEMPERATURE env var overrides config", () => {
			const globalDir = join(TEST_DIR, "env-temperature");
			mkdirSync(globalDir, { recursive: true });

			process.env.HEDDLE_HOME = globalDir;
			process.env.HEDDLE_TEMPERATURE = "0.5";
			const config = loadConfig(join(TEST_DIR, "nonexistent"));
			expect(config.temperature).toBe(0.5);
		});

		test("HEDDLE_TEMPERATURE env var '0' is valid", () => {
			const globalDir = join(TEST_DIR, "env-temp-zero");
			mkdirSync(globalDir, { recursive: true });

			process.env.HEDDLE_HOME = globalDir;
			process.env.HEDDLE_TEMPERATURE = "0";
			const config = loadConfig(join(TEST_DIR, "nonexistent"));
			expect(config.temperature).toBe(0);
		});

		test("empty numeric env vars don't set fields", () => {
			const globalDir = join(TEST_DIR, "env-empty-numeric");
			mkdirSync(globalDir, { recursive: true });

			process.env.HEDDLE_HOME = globalDir;
			process.env.HEDDLE_MAX_TOKENS = "";
			process.env.HEDDLE_TEMPERATURE = "";
			const config = loadConfig(join(TEST_DIR, "nonexistent"));
			expect(config.maxTokens).toBeUndefined();
			expect(config.temperature).toBeUndefined();
		});

		test("non-numeric env vars don't set fields", () => {
			const globalDir = join(TEST_DIR, "env-nonnumeric");
			mkdirSync(globalDir, { recursive: true });

			process.env.HEDDLE_HOME = globalDir;
			process.env.HEDDLE_MAX_TOKENS = "abc";
			process.env.HEDDLE_TEMPERATURE = "not-a-number";
			const config = loadConfig(join(TEST_DIR, "nonexistent"));
			expect(config.maxTokens).toBeUndefined();
			expect(config.temperature).toBeUndefined();
		});
	});
});
