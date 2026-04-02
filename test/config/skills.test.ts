import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { createTestSandbox } from "../helpers/sandbox.ts";
import type { DiscoveryLevel } from "../../src/config/discovery.ts";
import { type Skill, loadSkillsFromDiscovery, parseFrontmatter, parseSkillFile } from "../../src/config/skills.ts";

describe("config/skills", () => {
	let sandbox: ReturnType<typeof createTestSandbox>;

	beforeAll(() => {
		sandbox = createTestSandbox("skills");
	});

	afterAll(() => {
		sandbox.cleanup();
	});

	describe("parseFrontmatter()", () => {
		test("parses valid YAML frontmatter", () => {
			const content = `---
name: My Skill
description: Does things
---
# Body content here`;
			const result = parseFrontmatter(content);
			expect(result.frontmatter.name).toBe("My Skill");
			expect(result.frontmatter.description).toBe("Does things");
			expect(result.body).toBe("# Body content here");
		});

		test("returns empty frontmatter when none present", () => {
			const content = "# Just a markdown file\nWith content";
			const result = parseFrontmatter(content);
			expect(result.frontmatter).toEqual({});
			expect(result.body).toBe(content);
		});

		test("handles malformed frontmatter gracefully", () => {
			const content = `---
this is not valid yaml: : : :
---
Body`;
			const result = parseFrontmatter(content);
			// Should not throw — returns whatever it can parse
			expect(result.body).toBe("Body");
		});

		test("handles frontmatter with no closing delimiter", () => {
			const content = `---
name: incomplete
Some body text`;
			const result = parseFrontmatter(content);
			// No closing ---, treat entire thing as body
			expect(result.frontmatter).toEqual({});
			expect(result.body).toBe(content);
		});

		test("handles empty frontmatter block", () => {
			const content = `---
---
Body only`;
			const result = parseFrontmatter(content);
			expect(result.frontmatter).toEqual({});
			expect(result.body).toBe("Body only");
		});

		test("trims body whitespace", () => {
			const content = `---
key: value
---

  Body with leading space`;
			const result = parseFrontmatter(content);
			expect(result.body.startsWith("Body")).toBe(true);
		});
	});

	describe("parseSkillFile()", () => {
		test("parses a skill file with frontmatter", () => {
			const dir = join(sandbox.project, "parse-skill");
			mkdirSync(dir, { recursive: true });
			const filePath = join(dir, "test.md");
			writeFileSync(
				filePath,
				`---
description: A test skill
---
Do the test thing`,
			);

			const level: DiscoveryLevel = {
				path: dir,
				source: "heddle",
				skills: ["test.md"],
				agents: [],
			};

			const skill = parseSkillFile(filePath, "", level);
			expect(skill.name).toBe("test");
			expect(skill.description).toBe("A test skill");
			expect(skill.content).toBe("Do the test thing");
			expect(skill.source).toBe(dir);
			expect(skill.level).toBe(level);
		});

		test("derives name from nested path with colon separator", () => {
			const dir = join(sandbox.project, "parse-nested");
			const subdir = join(dir, "foo", "bar");
			mkdirSync(subdir, { recursive: true });
			const filePath = join(subdir, "baz.md");
			writeFileSync(filePath, "nested skill content");

			const level: DiscoveryLevel = {
				path: dir,
				source: "heddle",
				skills: [],
				agents: [],
			};

			const skill = parseSkillFile(filePath, "foo/bar", level);
			expect(skill.name).toBe("foo:bar:baz");
		});

		test("uses filename as description when frontmatter lacks it", () => {
			const dir = join(sandbox.project, "parse-no-desc");
			mkdirSync(dir, { recursive: true });
			const filePath = join(dir, "deploy.md");
			writeFileSync(filePath, "Deploy instructions");

			const level: DiscoveryLevel = {
				path: dir,
				source: "heddle",
				skills: ["deploy.md"],
				agents: [],
			};

			const skill = parseSkillFile(filePath, "", level);
			expect(skill.name).toBe("deploy");
			expect(skill.description).toContain("deploy");
		});
	});

	describe("loadSkillsFromDiscovery()", () => {
		test("loads skills from multiple levels", () => {
			const deepDir = join(sandbox.project, "multi-level", "deep", ".heddle");
			const shallowDir = join(sandbox.project, "multi-level", ".heddle");
			mkdirSync(join(deepDir, "skills"), { recursive: true });
			mkdirSync(join(shallowDir, "skills"), { recursive: true });
			writeFileSync(join(deepDir, "skills", "deep.md"), "deep skill");
			writeFileSync(join(shallowDir, "skills", "shallow.md"), "shallow skill");

			const discovery = {
				levels: [
					{
						path: deepDir,
						source: "heddle" as const,
						skills: ["deep.md"],
						agents: [],
					},
					{
						path: shallowDir,
						source: "heddle" as const,
						skills: ["shallow.md"],
						agents: [],
					},
				],
			};

			const skills = loadSkillsFromDiscovery(discovery);
			const names = skills.map((s) => s.name);
			expect(names).toContain("deep");
			expect(names).toContain("shallow");
		});

		test("collision resolution: deeper .heddle/ wins", () => {
			const deepDir = join(sandbox.project, "collision", "deep", ".heddle");
			const shallowDir = join(sandbox.project, "collision", ".heddle");
			mkdirSync(join(deepDir, "skills"), { recursive: true });
			mkdirSync(join(shallowDir, "skills"), { recursive: true });
			writeFileSync(join(deepDir, "skills", "deploy.md"), "deep deploy");
			writeFileSync(join(shallowDir, "skills", "deploy.md"), "shallow deploy");

			const discovery = {
				levels: [
					{
						path: deepDir,
						source: "heddle" as const,
						skills: ["deploy.md"],
						agents: [],
					},
					{
						path: shallowDir,
						source: "heddle" as const,
						skills: ["deploy.md"],
						agents: [],
					},
				],
			};

			const skills = loadSkillsFromDiscovery(discovery);
			const deploy = skills.find((s) => s.name === "deploy");
			expect(deploy).toBeDefined();
			expect(deploy!.content).toBe("deep deploy");
		});

		test("collision resolution: .heddle/ wins over .agents/", () => {
			const heddleDir = join(sandbox.project, "collision2", ".heddle");
			const agentsDir = join(sandbox.project, "collision2", ".agents", "skills");
			mkdirSync(join(heddleDir, "skills"), { recursive: true });
			mkdirSync(agentsDir, { recursive: true });
			writeFileSync(join(heddleDir, "skills", "review.md"), "heddle review");
			writeFileSync(join(agentsDir, "review.md"), "agents review");

			const discovery = {
				levels: [
					{
						path: heddleDir,
						source: "heddle" as const,
						skills: ["review.md"],
						agents: [],
					},
					{
						path: agentsDir,
						source: "agents" as const,
						skills: ["review.md"],
						agents: [],
					},
				],
			};

			const skills = loadSkillsFromDiscovery(discovery);
			const review = skills.find((s) => s.name === "review");
			expect(review).toBeDefined();
			expect(review!.content).toBe("heddle review");
		});

		test("handles empty discovery", () => {
			const discovery = { levels: [] };
			const skills = loadSkillsFromDiscovery(discovery);
			expect(skills).toEqual([]);
		});
	});
});
