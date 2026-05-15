import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { loadPlan } from "../../src/plans/storage.ts";
import { createSavePlanTool } from "../../src/tools/save-plan.ts";

describe("save_plan tool", () => {
	let dir: string;
	let originalEnv: string | undefined;

	beforeAll(() => {
		dir = mkdtempSync(join(tmpdir(), "heddle-save-plan-tool-"));
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

	test("has correct name and description", () => {
		const tool = createSavePlanTool("sess-1", "test-model");
		expect(tool.name).toBe("save_plan");
		expect(tool.description).toBeTruthy();
	});

	test("execute saves plan and returns confirmation", async () => {
		const tool = createSavePlanTool("sess-tool", "tool-model");
		const result = await tool.execute({
			name: "tool-plan",
			content: "# Tool Plan\n\nThis is a plan from the tool.",
		});

		expect(typeof result).toBe("string");
		expect(result).toContain("tool-plan");

		// Verify the plan was actually saved
		const plan = await loadPlan("tool-plan");
		expect(plan).not.toBeNull();
		expect(plan!.content).toContain("This is a plan from the tool.");
		expect(plan!.meta.model).toBe("tool-model");
		expect(plan!.meta.session_id).toBe("sess-tool");
	});

	test("execute without model still saves plan", async () => {
		const tool = createSavePlanTool("sess-nomodel");
		const result = await tool.execute({
			name: "no-model-plan",
			content: "Plan without model.",
		});

		expect(result).toContain("no-model-plan");

		const plan = await loadPlan("no-model-plan");
		expect(plan).not.toBeNull();
		expect(plan!.meta.model).toBeUndefined();
	});

	test("parameters schema requires name and content", () => {
		const tool = createSavePlanTool("sess-1");
		const schema = tool.parameters as Record<string, unknown>;
		expect(schema).toHaveProperty("properties");
		const props = schema.properties as Record<string, unknown>;
		expect(props).toHaveProperty("name");
		expect(props).toHaveProperty("content");
		const required = schema.required as string[];
		expect(required).toContain("name");
		expect(required).toContain("content");
	});
});
