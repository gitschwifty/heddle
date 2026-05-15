import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createCreateTaskTool, createListTasksTool, createUpdateTaskTool } from "../../src/tools/task-tools.ts";

describe("task tools", () => {
	let dir: string;
	let originalEnv: string | undefined;

	beforeAll(() => {
		dir = mkdtempSync(join(tmpdir(), "heddle-task-tools-"));
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

	test("create_task tool returns confirmation", async () => {
		const tool = createCreateTaskTool("session-1", "/test/create-confirm");

		const result = await tool.execute({ title: "My new task" });
		expect(typeof result).toBe("string");
		expect(result).toContain("My new task");
		expect(result).toContain("pending");
	});

	test("create_task tool with details", async () => {
		const tool = createCreateTaskTool("session-1", "/test/create-details");

		const result = await tool.execute({
			title: "Task with details",
			details: "Extra info",
		});
		expect(result).toContain("Task with details");
	});

	test("create_task tool has correct name and schema", () => {
		const tool = createCreateTaskTool("s1");
		expect(tool.name).toBe("create_task");
		expect(tool.parameters).toBeTruthy();
	});

	test("update_task tool changes status", async () => {
		const projectPath = "/test/update-status";
		const createTool = createCreateTaskTool("session-1", projectPath);
		const updateTool = createUpdateTaskTool(projectPath);

		const createResult = await createTool.execute({ title: "Update me" });
		// Extract UUID from "id: <uuid>," pattern
		const idMatch = createResult.match(/id:\s*([0-9a-f-]+)/i);
		expect(idMatch).toBeTruthy();
		const taskId = idMatch![1];

		const updateResult = await updateTool.execute({
			id: taskId,
			status: "done",
		});
		expect(updateResult).toContain("done");
	});

	test("update_task tool has correct name", () => {
		const tool = createUpdateTaskTool();
		expect(tool.name).toBe("update_task");
	});

	test("list_tasks tool returns formatted output", async () => {
		const projectPath = "/test/list-formatted";
		const createTool = createCreateTaskTool("session-1", projectPath);
		const listTool = createListTasksTool("session-1", projectPath);

		await createTool.execute({ title: "Task Alpha" });
		await createTool.execute({ title: "Task Beta" });

		const result = await listTool.execute({});
		expect(result).toContain("Task Alpha");
		expect(result).toContain("Task Beta");
	});

	test("list_tasks tool returns empty message when no tasks", async () => {
		const listTool = createListTasksTool("session-1", "/test/empty-list");

		const result = await listTool.execute({});
		expect(result).toContain("No tasks");
	});

	test("list_tasks tool has correct name", () => {
		const tool = createListTasksTool("s1");
		expect(tool.name).toBe("list_tasks");
	});

	test("list_tasks flags stale session tasks", async () => {
		const projectPath = "/test/stale-flag";

		// Create task in old session
		const oldSessionTool = createCreateTaskTool("old-session", projectPath);
		await oldSessionTool.execute({ title: "Old task" });

		// Create task in current session
		const currentSessionTool = createCreateTaskTool("current-session", projectPath);
		await currentSessionTool.execute({ title: "Current task" });

		// List with current session
		const listTool = createListTasksTool("current-session", projectPath);
		const result = await listTool.execute({});

		expect(result).toMatch(/Old task.*stale|stale.*Old task/i);
		expect(result).not.toMatch(/Current task.*stale/i);
	});
});
