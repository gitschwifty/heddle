import { randomUUID } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { getProjectDir } from "../config/paths.ts";
import type { Task, TaskStatus } from "./types.ts";

/** Path to the tasks.json file for a project. */
export function getTasksPath(projectPath?: string): string {
	return join(getProjectDir(projectPath), "tasks.json");
}

/** Read and parse tasks.json. Returns [] if file doesn't exist. */
export async function loadTasks(projectPath?: string): Promise<Task[]> {
	const path = getTasksPath(projectPath);
	try {
		const raw = readFileSync(path, "utf-8");
		return JSON.parse(raw) as Task[];
	} catch {
		return [];
	}
}

/** Write tasks array to tasks.json (pretty-printed JSON). */
export async function saveTasks(tasks: Task[], projectPath?: string): Promise<void> {
	const path = getTasksPath(projectPath);
	const dir = join(path, "..");
	mkdirSync(dir, { recursive: true });
	writeFileSync(path, JSON.stringify(tasks, null, "\t"), "utf-8");
}

/** Create a new task with status "pending", save it, and return it. */
export async function createTask(
	title: string,
	sessionId: string,
	projectPath?: string,
	details?: string,
): Promise<Task> {
	const tasks = await loadTasks(projectPath);
	const now = new Date().toISOString();

	const task: Task = {
		id: randomUUID(),
		title,
		status: "pending",
		created: now,
		updated: now,
		session_id: sessionId,
		...(details !== undefined ? { details } : {}),
	};

	tasks.push(task);
	await saveTasks(tasks, projectPath);
	return task;
}

/** Update a task by id. Throws if not found. */
export async function updateTask(
	id: string,
	updates: Partial<Pick<Task, "status" | "title" | "details">>,
	projectPath?: string,
): Promise<Task> {
	const tasks = await loadTasks(projectPath);
	const index = tasks.findIndex((t) => t.id === id);

	if (index === -1) {
		throw new Error(`Task not found: ${id}`);
	}

	const task = tasks[index] as Task;
	if (updates.status !== undefined) task.status = updates.status;
	if (updates.title !== undefined) task.title = updates.title;
	if (updates.details !== undefined) task.details = updates.details;
	task.updated = new Date().toISOString();

	await saveTasks(tasks, projectPath);
	return task;
}

const STATUS_ORDER: TaskStatus[] = ["in_progress", "blocked", "pending", "done"];

/** Format tasks for injection into system prompt. Flags stale tasks from other sessions. */
export function formatTasksSummary(tasks: Task[], currentSessionId: string): string {
	if (tasks.length === 0) {
		return "No tasks tracked.";
	}

	const lines: string[] = [];

	for (const status of STATUS_ORDER) {
		const group = tasks.filter((t) => t.status === status);
		if (group.length === 0) continue;

		lines.push(`## ${status.replace("_", " ").toUpperCase()}`);
		for (const task of group) {
			const staleFlag = task.session_id !== currentSessionId ? " [stale]" : "";
			const detailsSuffix = task.details ? ` — ${task.details}` : "";
			lines.push(`- [${task.id}] ${task.title}${detailsSuffix}${staleFlag}`);
		}
		lines.push("");
	}

	return lines.join("\n");
}
