import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { getPlansDir, listPlans, loadPlan, savePlan } from "../../src/plans/storage.ts";

describe("plans storage", () => {
	let dir: string;
	let originalEnv: string | undefined;

	beforeAll(() => {
		dir = mkdtempSync(join(tmpdir(), "heddle-plans-storage-"));
		originalEnv = process.env.HEDDLE_HOME;
		process.env.HEDDLE_HOME = dir;
	});

	afterAll(() => {
		if (originalEnv === undefined) {
			delete process.env.HEDDLE_HOME;
		} else {
			process.env.HEDDLE_HOME = originalEnv;
		}
		rmSync(dir, { recursive: true, force: true });
	});

	test("getPlansDir returns plans/ under project dir", () => {
		const plansDir = getPlansDir("/some/project");
		expect(plansDir).toContain("projects");
		expect(plansDir).toEndWith("/plans");
	});

	test("savePlan writes a file and returns the path", async () => {
		const filePath = await savePlan("my-plan", "# My Plan\n\nDo stuff.", {
			model: "gpt-4",
			sessionId: "sess-1",
		});

		expect(filePath).toContain("my-plan.md");
		const raw = readFileSync(filePath, "utf-8");
		expect(raw).toContain("# My Plan");
		expect(raw).toContain("Do stuff.");
	});

	test("savePlan writes correct frontmatter", async () => {
		const filePath = await savePlan("frontmatter-test", "Plan body here.", {
			model: "claude-3",
			sessionId: "sess-fm",
		});

		const raw = readFileSync(filePath, "utf-8");
		expect(raw).toMatch(/^---\n/);
		expect(raw).toContain("model: claude-3");
		expect(raw).toContain("session_id: sess-fm");
		expect(raw).toMatch(/created: \d{4}-\d{2}-\d{2}T/);
		// Frontmatter ends with ---
		expect(raw).toMatch(/---\n[\s\S]*---\n/);
	});

	test("savePlan with no model omits model from frontmatter", async () => {
		const filePath = await savePlan("no-model", "Content.", {
			sessionId: "sess-nm",
		});

		const raw = readFileSync(filePath, "utf-8");
		expect(raw).not.toContain("model:");
		expect(raw).toContain("session_id: sess-nm");
	});

	test("loadPlan roundtrips content and metadata", async () => {
		await savePlan("roundtrip", "Roundtrip body content.", {
			model: "test-model",
			sessionId: "sess-rt",
		});

		const plan = await loadPlan("roundtrip");
		expect(plan).not.toBeNull();
		expect(plan!.name).toBe("roundtrip");
		expect(plan!.content).toContain("Roundtrip body content.");
		expect(plan!.meta.model).toBe("test-model");
		expect(plan!.meta.session_id).toBe("sess-rt");
		expect(plan!.meta.created).toMatch(/^\d{4}-\d{2}-\d{2}T/);
	});

	test("loadPlan returns null for nonexistent plan", async () => {
		const plan = await loadPlan("does-not-exist");
		expect(plan).toBeNull();
	});

	test("listPlans returns saved plans with previews", async () => {
		// Save two plans with distinct project paths to avoid cross-test pollution
		const projectPath = "/test/list-project";
		await savePlan(
			"list-alpha",
			"Alpha plan first line.\nSecond line.",
			{
				sessionId: "sess-la",
			},
			projectPath,
		);
		await savePlan(
			"list-beta",
			"Beta plan first line.",
			{
				model: "m",
				sessionId: "sess-lb",
			},
			projectPath,
		);

		const plans = await listPlans(projectPath);
		expect(plans.length).toBeGreaterThanOrEqual(2);

		const alpha = plans.find((p) => p.name === "list-alpha");
		expect(alpha).toBeDefined();
		expect(alpha!.preview).toContain("Alpha plan first line.");

		const beta = plans.find((p) => p.name === "list-beta");
		expect(beta).toBeDefined();
		expect(beta!.created).toMatch(/^\d{4}-\d{2}-\d{2}T/);
	});

	test("listPlans returns empty array when no plans exist", async () => {
		const plans = await listPlans("/nonexistent/project");
		expect(plans).toEqual([]);
	});

	test("plan name is sanitized against path traversal", async () => {
		const filePath = await savePlan("../../../etc/passwd", "evil content", {
			sessionId: "sess-evil",
		});

		// Should not escape the plans directory
		const plansDir = getPlansDir();
		expect(filePath.startsWith(plansDir)).toBe(true);
		// Should not contain ..
		expect(filePath).not.toContain("..");
	});

	test("plan name with slashes is sanitized", async () => {
		const filePath = await savePlan("foo/bar/baz", "slash content", {
			sessionId: "sess-slash",
		});

		const plansDir = getPlansDir();
		expect(filePath.startsWith(plansDir)).toBe(true);
		// The filename itself should be flat (no subdirectories created)
		const filename = filePath.slice(plansDir.length + 1);
		expect(filename).not.toContain("/");
	});
});
