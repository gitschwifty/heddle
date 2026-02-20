import { afterAll, beforeAll, describe, expect, mock, test } from "bun:test";
import { existsSync, mkdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { createSession } from "../../src/session/setup.ts";

const TEST_DIR = join(import.meta.dir, ".tmp-setup-test");
const origEnv = { ...process.env };
const origCwd = process.cwd();
const realFetch = globalThis.fetch;

// Mock global fetch to prevent real API calls
const mockFetch = mock(() =>
	Promise.resolve(
		new Response(JSON.stringify({ id: "test", choices: [], usage: null }), {
			status: 200,
			headers: { "Content-Type": "application/json" },
		}),
	),
);

beforeAll(() => {
	mkdirSync(TEST_DIR, { recursive: true });
	globalThis.fetch = mockFetch as unknown as typeof fetch;
});

afterAll(() => {
	process.env = { ...origEnv };
	process.chdir(origCwd);
  globalThis.fetch = realFetch;
	// Clean up using sync rm (test infra, not production code)
	const { execSync } = require("node:child_process");
	try {
		execSync(`rm -rf "${TEST_DIR}"`);
	} catch {}
});

function setupEnv(overrides: Record<string, string | undefined> = {}) {
	// Reset env to original
	process.env = { ...origEnv };
	// Point HEDDLE_HOME to temp dir
	const homeDir = join(TEST_DIR, "heddle-home");
	mkdirSync(homeDir, { recursive: true });
	process.env.HEDDLE_HOME = homeDir;
	process.env.OPENROUTER_API_KEY = "test-key";
	// Clear tools env so it doesn't interfere
	delete process.env.HEDDLE_TOOLS;
	// Apply overrides
	for (const [key, value] of Object.entries(overrides)) {
		if (value === undefined) {
			delete process.env[key];
		} else {
			process.env[key] = value;
		}
	}
	// Reset cwd
	process.chdir(origCwd);
}

describe("createSession()", () => {
	test("returns valid SessionContext with all fields populated", async () => {
		setupEnv();
		const ctx = await createSession();

		expect(ctx.config).toBeDefined();
		expect(ctx.provider).toBeDefined();
		expect(typeof ctx.provider.send).toBe("function");
		expect(typeof ctx.provider.stream).toBe("function");
		expect(ctx.registry).toBeDefined();
		expect(ctx.messages).toBeArray();
		expect(ctx.messages.length).toBeGreaterThanOrEqual(1);
		expect(ctx.messages[0]!.role).toBe("system");
		expect(typeof ctx.sessionFile).toBe("string");
		expect(typeof ctx.sessionId).toBe("string");
	});

	test("default tools: all 6 registered", async () => {
		setupEnv();
		const ctx = await createSession();

		const toolNames = ctx.registry
			.all()
			.map((t) => t.name)
			.sort();
		expect(toolNames).toEqual(["bash", "edit_file", "glob", "grep", "read_file", "write_file"]);
	});

	test("SessionOptions.tools filtering: only named tools registered", async () => {
		setupEnv();
		const ctx = await createSession({ tools: ["read_file", "glob"] });

		const toolNames = ctx.registry
			.all()
			.map((t) => t.name)
			.sort();
		expect(toolNames).toEqual(["glob", "read_file"]);
	});

	test("SessionOptions.tools empty array: treated as null, all 6 registered", async () => {
		setupEnv();
		const ctx = await createSession({ tools: [] });

		const toolNames = ctx.registry.all().map((t) => t.name);
		expect(toolNames).toHaveLength(6);
	});

	test("config.tools fallback: HEDDLE_TOOLS env limits tools", async () => {
		setupEnv({ HEDDLE_TOOLS: "read_file,glob" });
		const ctx = await createSession();

		const toolNames = ctx.registry
			.all()
			.map((t) => t.name)
			.sort();
		expect(toolNames).toEqual(["glob", "read_file"]);
	});

	test("SessionOptions.tools overrides config.tools", async () => {
		setupEnv({ HEDDLE_TOOLS: "read_file,glob,bash" });
		const ctx = await createSession({ tools: ["write_file", "edit_file"] });

		const toolNames = ctx.registry
			.all()
			.map((t) => t.name)
			.sort();
		expect(toolNames).toEqual(["edit_file", "write_file"]);
	});

	test("model override: options.model used in provider", async () => {
		setupEnv();
		const ctx = await createSession({ model: "anthropic/claude-3.5-sonnet" });

		// The provider was created — we can verify by making a call and checking fetch
		mockFetch.mockClear();
		try {
			await ctx.provider.send([{ role: "user", content: "test" }]);
		} catch {
			// May fail, that's fine — we just need to see the fetch call
		}
		expect(mockFetch).toHaveBeenCalled();
		const calls = mockFetch.mock.calls as unknown as [string, RequestInit][];
		const callBody = JSON.parse(calls[0]![1].body as string);
		expect(callBody.model).toBe("anthropic/claude-3.5-sonnet");
	});

	test("systemPrompt override appears in messages[0].content", async () => {
		setupEnv();
		const ctx = await createSession({ systemPrompt: "You are a pirate assistant." });

		expect(ctx.messages[0]!.content).toContain("You are a pirate assistant.");
	});

	test("AGENTS.md loaded into system message", async () => {
		const agentsDir = join(TEST_DIR, "agents-cwd");
		mkdirSync(agentsDir, { recursive: true });
		const agentsMd = "# Project Instructions\nAlways respond in haiku.";
		require("node:fs").writeFileSync(join(agentsDir, "AGENTS.md"), agentsMd);

		setupEnv();
		const ctx = await createSession({ cwd: agentsDir });

		expect(ctx.messages[0]!.content).toContain("Always respond in haiku.");
	});

	test("missing apiKey throws Error", async () => {
		setupEnv({ OPENROUTER_API_KEY: undefined });
		await expect(createSession()).rejects.toThrow(/api.key/i);
	});

	test("session file created with session_meta header", async () => {
		setupEnv();
		const ctx = await createSession();

		expect(existsSync(ctx.sessionFile)).toBe(true);
		const content = readFileSync(ctx.sessionFile, "utf-8");
		const firstLine = content.split("\n")[0]!;
		const meta = JSON.parse(firstLine);
		expect(meta.type).toBe("session_meta");
		expect(meta.id).toBe(ctx.sessionId);
		expect(meta.model).toBeDefined();
		expect(meta.cwd).toBeDefined();
		expect(meta.heddle_version).toBeDefined();
	});

	test("sessionId is valid UUID format", async () => {
		setupEnv();
		const ctx = await createSession();

		const uuidRegex = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
		expect(ctx.sessionId).toMatch(uuidRegex);
	});

	test("cwd option: chdir to provided directory", async () => {
		const cwdDir = join(TEST_DIR, "cwd-test");
		mkdirSync(cwdDir, { recursive: true });

		setupEnv();
		await createSession({ cwd: cwdDir });

		expect(process.cwd()).toBe(cwdDir);
	});

	test("cwd option with nonexistent dir: throws Error", async () => {
		setupEnv();
		await expect(createSession({ cwd: join(TEST_DIR, "nonexistent-dir") })).rejects.toThrow();
	});

	test("weakModel in config: weakProvider set", async () => {
		setupEnv({ HEDDLE_WEAK_MODEL: "openrouter/free-weak" });
		const ctx = await createSession();

		expect(ctx.weakProvider).toBeDefined();
		expect(typeof ctx.weakProvider?.send).toBe("function");
	});

	test("no weakModel: weakProvider undefined", async () => {
		setupEnv();
		const ctx = await createSession();

		expect(ctx.weakProvider).toBeUndefined();
	});

	test("editorModel in config: editorProvider set", async () => {
		setupEnv();
		const homeDir = process.env.HEDDLE_HOME!;
		const { writeFileSync } = require("node:fs");
		writeFileSync(join(homeDir, "config.toml"), 'editor_model = "openrouter/free-editor"\n');
		const ctx = await createSession();

		expect(ctx.editorProvider).toBeDefined();
		expect(typeof ctx.editorProvider?.send).toBe("function");
	});
});
