import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
	createTask,
	formatTasksSummary,
	getTasksPath,
	loadTasks,
	saveTasks,
	updateTask,
} from "../../src/tasks/storage.ts";

describe("tasks storage", () => {
	let dir: string;
	let originalEnv: string | undefined;

	beforeAll(() => {
		dir = mkdtempSync(join(tmpdir(), "heddle-tasks-storage-"));
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

	test("getTasksPath returns tasks.json under project dir", () => {
		const path = getTasksPath("/some/project");
		expect(path).toContain("tasks.json");
		expect(path).toContain("projects");
	});

	test("loadTasks returns empty array when file does not exist", async () => {
		const tasks = await loadTasks("/nonexistent/project");
		expect(tasks).toEqual([]);
	});

	test("createTask adds a task with correct fields", async () => {
		const projectPath = "/test/create-project";
		const task = await createTask("Write tests", "session-1", projectPath);

		expect(task.id).toBeTruthy();
		expect(task.title).toBe("Write tests");
		expect(task.status).toBe("pending");
		expect(task.session_id).toBe("session-1");
		expect(task.created).toMatch(/^\d{4}-\d{2}-\d{2}T/);
		expect(task.updated).toBe(task.created);
		expect(task.details).toBeUndefined();

		const loaded = await loadTasks(projectPath);
		expect(loaded).toHaveLength(1);
		expect(loaded[0]!.title).toBe("Write tests");
	});

	test("createTask with details stores details field", async () => {
		const task = await createTask("Detailed task", "session-2", "/test/details-project", "Some extra details");
		expect(task.details).toBe("Some extra details");
	});

	test("saveTasks and loadTasks roundtrip", async () => {
		const projectPath = "/test/roundtrip-project";
		const tasks = [
			{
				id: "abc-123",
				title: "Task A",
				status: "pending" as const,
				created: "2026-01-01T00:00:00.000Z",
				updated: "2026-01-01T00:00:00.000Z",
				session_id: "s1",
			},
			{
				id: "def-456",
				title: "Task B",
				status: "done" as const,
				created: "2026-01-02T00:00:00.000Z",
				updated: "2026-01-02T00:00:00.000Z",
				session_id: "s2",
				details: "Completed",
			},
		];

		await saveTasks(tasks, projectPath);
		const loaded = await loadTasks(projectPath);
		expect(loaded).toEqual(tasks);
	});

	test("saveTasks writes pretty-printed JSON", async () => {
		const projectPath = "/test/pretty-project";
		await saveTasks(
			[
				{
					id: "x",
					title: "T",
					status: "pending",
					created: "2026-01-01T00:00:00.000Z",
					updated: "2026-01-01T00:00:00.000Z",
					session_id: "s",
				},
			],
			projectPath,
		);

		const raw = readFileSync(getTasksPath(projectPath), "utf-8");
		// Pretty-printed JSON has newlines
		expect(raw).toContain("\n");
		expect(raw.split("\n").length).toBeGreaterThan(2);
	});

	test("updateTask modifies an existing task", async () => {
		const projectPath = "/test/update-project";
		const task = await createTask("Original title", "session-1", projectPath);

		const updated = await updateTask(task.id, { status: "in_progress", title: "New title" }, projectPath);

		expect(updated.status).toBe("in_progress");
		expect(updated.title).toBe("New title");
		// updated timestamp should be a valid ISO string (may equal created if same ms)
		expect(updated.updated).toMatch(/^\d{4}-\d{2}-\d{2}T/);
		expect(new Date(updated.updated).getTime()).toBeGreaterThanOrEqual(new Date(task.created).getTime());

		const loaded = await loadTasks(projectPath);
		expect(loaded[0]!.status).toBe("in_progress");
		expect(loaded[0]!.title).toBe("New title");
	});

	test("updateTask with details adds details to task", async () => {
		const projectPath = "/test/update-details-project";
		const task = await createTask("Task", "session-1", projectPath);

		const updated = await updateTask(task.id, { details: "Added details" }, projectPath);
		expect(updated.details).toBe("Added details");
	});

	test("updateTask throws for nonexistent task", async () => {
		const projectPath = "/test/nonexistent-update";
		expect(updateTask("nonexistent-id", { status: "done" }, projectPath)).rejects.toThrow(/not found/i);
	});

	test("formatTasksSummary groups by status", () => {
		const tasks = [
			{
				id: "1",
				title: "Pending task",
				status: "pending" as const,
				created: "2026-01-01T00:00:00.000Z",
				updated: "2026-01-01T00:00:00.000Z",
				session_id: "current",
			},
			{
				id: "2",
				title: "Done task",
				status: "done" as const,
				created: "2026-01-01T00:00:00.000Z",
				updated: "2026-01-01T00:00:00.000Z",
				session_id: "current",
			},
			{
				id: "3",
				title: "In progress task",
				status: "in_progress" as const,
				created: "2026-01-01T00:00:00.000Z",
				updated: "2026-01-01T00:00:00.000Z",
				session_id: "current",
			},
		];

		const summary = formatTasksSummary(tasks, "current");
		expect(summary).toContain("Pending task");
		expect(summary).toContain("Done task");
		expect(summary).toContain("In progress task");
	});

	test("formatTasksSummary flags stale tasks from different sessions", () => {
		const tasks = [
			{
				id: "1",
				title: "Current session task",
				status: "pending" as const,
				created: "2026-01-01T00:00:00.000Z",
				updated: "2026-01-01T00:00:00.000Z",
				session_id: "current-session",
			},
			{
				id: "2",
				title: "Old session task",
				status: "in_progress" as const,
				created: "2026-01-01T00:00:00.000Z",
				updated: "2026-01-01T00:00:00.000Z",
				session_id: "old-session",
			},
		];

		const summary = formatTasksSummary(tasks, "current-session");
		expect(summary).toMatch(/Old session task.*stale|stale.*Old session task/i);
		expect(summary).not.toMatch(/Current session task.*stale/i);
	});

	test("formatTasksSummary returns empty message for no tasks", () => {
		const summary = formatTasksSummary([], "session-1");
		expect(summary).toContain("No tasks");
	});
});
