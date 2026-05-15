import { Type } from "@sinclair/typebox";
import { createTask, formatTasksSummary, loadTasks, updateTask } from "../tasks/storage.ts";
import type { HeddleTool } from "./types.ts";

const TaskStatusSchema = Type.Union(
	[Type.Literal("pending"), Type.Literal("in_progress"), Type.Literal("done"), Type.Literal("blocked")],
	{ description: "Task status" },
);

const CreateTaskParams = Type.Object({
	title: Type.String({ description: "Title of the task" }),
	details: Type.Optional(Type.String({ description: "Additional details about the task" })),
});

const UpdateTaskParams = Type.Object({
	id: Type.String({ description: "ID of the task to update" }),
	status: Type.Optional(TaskStatusSchema),
	title: Type.Optional(Type.String({ description: "New title for the task" })),
	details: Type.Optional(Type.String({ description: "Updated details for the task" })),
});

const ListTasksParams = Type.Object({});

/** Factory: creates a create_task tool. */
export function createCreateTaskTool(sessionId: string, projectPath?: string): HeddleTool {
	return {
		name: "create_task",
		description: "Create a new task to track work across sessions. Tasks persist through context compaction.",
		parameters: CreateTaskParams,
		async execute(params: unknown): Promise<string> {
			const { title, details } = params as {
				title: string;
				details?: string;
			};
			const task = await createTask(title, sessionId, projectPath, details);
			return `Created task: "${task.title}" (id: ${task.id}, status: ${task.status})`;
		},
	};
}

/** Factory: creates an update_task tool. */
export function createUpdateTaskTool(projectPath?: string): HeddleTool {
	return {
		name: "update_task",
		description: "Update an existing task's status, title, or details.",
		parameters: UpdateTaskParams,
		async execute(params: unknown): Promise<string> {
			const { id, status, title, details } = params as {
				id: string;
				status?: "pending" | "in_progress" | "done" | "blocked";
				title?: string;
				details?: string;
			};
			try {
				const task = await updateTask(id, { status, title, details }, projectPath);
				return `Updated task: "${task.title}" (id: ${task.id}, status: ${task.status})`;
			} catch (err) {
				return `Error: ${err instanceof Error ? err.message : String(err)}`;
			}
		},
	};
}

/** Factory: creates a list_tasks tool. */
export function createListTasksTool(sessionId: string, projectPath?: string): HeddleTool {
	return {
		name: "list_tasks",
		description: "List all tracked tasks, grouped by status.",
		parameters: ListTasksParams,
		async execute(): Promise<string> {
			const tasks = await loadTasks(projectPath);
			return formatTasksSummary(tasks, sessionId);
		},
	};
}
