import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { loadAgentDefinitions, parseAgentFile } from "../../src/agents/loader.ts";
import type { DiscoveryResult } from "../../src/config/discovery.ts";
import { createTestSandbox } from "../helpers/sandbox.ts";

describe("agents/loader", () => {
	let sandbox: ReturnType<typeof createTestSandbox>;

	beforeAll(() => {
		sandbox = createTestSandbox("agents-loader");
	});

	afterAll(() => {
		sandbox.cleanup();
	});

	describe("parseAgentFile()", () => {
		test("parses a valid agent file with all fields", () => {
			const filePath = join(sandbox.project, "full-agent.md");
			writeFileSync(
				filePath,
				`---
name: researcher
description: Research-focused agent
model: openrouter/google/gemini-2.5-pro
tools:
  - read
  - glob
  - grep
---

You are a research agent. Read files and report findings.

Never modify files.
`,
			);

			const result = parseAgentFile(filePath);
			expect(result).not.toBeNull();
			expect(result!.name).toBe("researcher");
			expect(result!.description).toBe("Research-focused agent");
			expect(result!.model).toBe("openrouter/google/gemini-2.5-pro");
			expect(result!.tools).toEqual(["read", "glob", "grep"]);
			expect(result!.systemPrompt).toContain("You are a research agent.");
			expect(result!.systemPrompt).toContain("Never modify files.");
			expect(result!.source).toBe(filePath);
		});

		test("parses with minimal fields (name only in frontmatter)", () => {
			const filePath = join(sandbox.project, "minimal-agent.md");
			writeFileSync(
				filePath,
				`---
name: helper
---

Help the user.
`,
			);

			const result = parseAgentFile(filePath);
			expect(result).not.toBeNull();
			expect(result!.name).toBe("helper");
			expect(result!.description).toBe("");
			expect(result!.model).toBeUndefined();
			expect(result!.tools).toBeUndefined();
			expect(result!.systemPrompt).toContain("Help the user.");
			expect(result!.source).toBe(filePath);
		});

		test("derives name from filename when not in frontmatter", () => {
			const filePath = join(sandbox.project, "code-reviewer.md");
			writeFileSync(
				filePath,
				`---
description: Reviews code
---

Review code carefully.
`,
			);

			const result = parseAgentFile(filePath);
			expect(result).not.toBeNull();
			expect(result!.name).toBe("code-reviewer");
			expect(result!.description).toBe("Reviews code");
		});

		test("derives name from filename with no frontmatter at all", () => {
			const filePath = join(sandbox.project, "simple-bot.md");
			writeFileSync(filePath, "Just a system prompt, no frontmatter.\n");

			const result = parseAgentFile(filePath);
			expect(result).not.toBeNull();
			expect(result!.name).toBe("simple-bot");
			expect(result!.description).toBe("");
			expect(result!.systemPrompt).toContain("Just a system prompt, no frontmatter.");
		});

		test("returns null for nonexistent file", () => {
			const result = parseAgentFile(join(sandbox.project, "does-not-exist.md"));
			expect(result).toBeNull();
		});

		test("returns null for empty file", () => {
			const filePath = join(sandbox.project, "empty-agent.md");
			writeFileSync(filePath, "");

			const result = parseAgentFile(filePath);
			expect(result).toBeNull();
		});

		test("returns null for file with only whitespace", () => {
			const filePath = join(sandbox.project, "whitespace-agent.md");
			writeFileSync(filePath, "   \n\n  \n");

			const result = parseAgentFile(filePath);
			expect(result).toBeNull();
		});

		test("handles file with frontmatter but empty body", () => {
			const filePath = join(sandbox.project, "no-body-agent.md");
			writeFileSync(
				filePath,
				`---
name: headless
description: No system prompt
---
`,
			);

			const result = parseAgentFile(filePath);
			expect(result).not.toBeNull();
			expect(result!.name).toBe("headless");
			expect(result!.systemPrompt).toBe("");
		});

		test("handles malformed YAML frontmatter gracefully", () => {
			const filePath = join(sandbox.project, "bad-yaml.md");
			writeFileSync(
				filePath,
				`---
name: [broken
  yaml: {{{}
---

Some body.
`,
			);

			const result = parseAgentFile(filePath);
			// Should return null or handle gracefully (not throw)
			expect(result).toBeNull();
		});
	});

	describe("loadAgentDefinitions()", () => {
		test("returns empty map when no agents exist", () => {
			const discovery: DiscoveryResult = {
				levels: [
					{
						path: join(sandbox.project, "empty-level"),
						source: "heddle",
						skills: [],
						agents: [],
					},
				],
			};

			const result = loadAgentDefinitions(discovery);
			expect(result.size).toBe(0);
		});

		test("loads agents from a single discovery level", () => {
			const levelPath = join(sandbox.project, "single-level");
			const agentsDir = join(levelPath, "agents");
			mkdirSync(agentsDir, { recursive: true });
			writeFileSync(
				join(agentsDir, "writer.md"),
				`---
name: writer
description: Writing agent
model: gpt-4o
tools:
  - write
  - edit
---

You are a writing agent.
`,
			);

			const discovery: DiscoveryResult = {
				levels: [
					{
						path: levelPath,
						source: "heddle",
						skills: [],
						agents: ["writer.md"],
					},
				],
			};

			const result = loadAgentDefinitions(discovery);
			expect(result.size).toBe(1);
			expect(result.has("writer")).toBe(true);
			const writer = result.get("writer")!;
			expect(writer.description).toBe("Writing agent");
			expect(writer.model).toBe("gpt-4o");
			expect(writer.tools).toEqual(["write", "edit"]);
			expect(writer.systemPrompt).toContain("You are a writing agent.");
		});

		test("project-level agent overrides global agent with same name", () => {
			// Global level (last in the array = shallowest)
			const globalPath = join(sandbox.project, "global-level");
			const globalAgents = join(globalPath, "agents");
			mkdirSync(globalAgents, { recursive: true });
			writeFileSync(
				join(globalAgents, "reviewer.md"),
				`---
name: reviewer
description: Global reviewer
model: gpt-3.5-turbo
---

Global review prompt.
`,
			);

			// Project level (first in the array = deepest)
			const projectPath = join(sandbox.project, "project-level");
			const projectAgents = join(projectPath, "agents");
			mkdirSync(projectAgents, { recursive: true });
			writeFileSync(
				join(projectAgents, "reviewer.md"),
				`---
name: reviewer
description: Project reviewer
model: claude-sonnet-4-20250514
---

Project-specific review prompt.
`,
			);

			// Discovery levels are deepest-first
			const discovery: DiscoveryResult = {
				levels: [
					{
						path: projectPath,
						source: "heddle",
						skills: [],
						agents: ["reviewer.md"],
					},
					{
						path: globalPath,
						source: "heddle",
						skills: [],
						agents: ["reviewer.md"],
					},
				],
			};

			const result = loadAgentDefinitions(discovery);
			expect(result.size).toBe(1);
			const reviewer = result.get("reviewer")!;
			expect(reviewer.description).toBe("Project reviewer");
			expect(reviewer.model).toBe("claude-sonnet-4-20250514");
			expect(reviewer.systemPrompt).toContain("Project-specific review prompt.");
		});

		test("merges agents from multiple levels without collision", () => {
			const globalPath = join(sandbox.project, "merge-global");
			const globalAgents = join(globalPath, "agents");
			mkdirSync(globalAgents, { recursive: true });
			writeFileSync(
				join(globalAgents, "coder.md"),
				`---
name: coder
description: Global coder
---

Code things.
`,
			);

			const projectPath = join(sandbox.project, "merge-project");
			const projectAgents = join(projectPath, "agents");
			mkdirSync(projectAgents, { recursive: true });
			writeFileSync(
				join(projectAgents, "tester.md"),
				`---
name: tester
description: Test runner
---

Run tests.
`,
			);

			const discovery: DiscoveryResult = {
				levels: [
					{
						path: projectPath,
						source: "heddle",
						skills: [],
						agents: ["tester.md"],
					},
					{
						path: globalPath,
						source: "heddle",
						skills: [],
						agents: ["coder.md"],
					},
				],
			};

			const result = loadAgentDefinitions(discovery);
			expect(result.size).toBe(2);
			expect(result.has("coder")).toBe(true);
			expect(result.has("tester")).toBe(true);
		});

		test("skips malformed agent files without crashing", () => {
			const levelPath = join(sandbox.project, "skip-bad");
			const agentsDir = join(levelPath, "agents");
			mkdirSync(agentsDir, { recursive: true });
			writeFileSync(
				join(agentsDir, "good.md"),
				`---
name: good
description: Works fine
---

Good agent.
`,
			);
			writeFileSync(join(agentsDir, "bad.md"), "");

			const discovery: DiscoveryResult = {
				levels: [
					{
						path: levelPath,
						source: "heddle",
						skills: [],
						agents: ["good.md", "bad.md"],
					},
				],
			};

			const result = loadAgentDefinitions(discovery);
			expect(result.size).toBe(1);
			expect(result.has("good")).toBe(true);
		});

		test("handles level with missing agents directory gracefully", () => {
			const levelPath = join(sandbox.project, "no-agents-dir");
			mkdirSync(levelPath, { recursive: true });
			// agents dir doesn't exist, but agents array lists a file

			const discovery: DiscoveryResult = {
				levels: [
					{
						path: levelPath,
						source: "heddle",
						skills: [],
						agents: ["phantom.md"],
					},
				],
			};

			const result = loadAgentDefinitions(discovery);
			expect(result.size).toBe(0);
		});
	});
});
