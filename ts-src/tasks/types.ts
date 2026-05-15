export type TaskStatus = "pending" | "in_progress" | "done" | "blocked";

export interface Task {
	id: string;
	title: string;
	status: TaskStatus;
	created: string; // ISO timestamp
	updated: string; // ISO timestamp
	session_id: string;
	details?: string;
}
