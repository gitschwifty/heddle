import { appendFileSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";
import { getHistoryPath } from "../config/paths.ts";

export interface HistoryEntry {
	timestamp: string;
	session_id: string;
	project: string;
	message_preview: string;
	content_type: "text" | "mention" | "shell";
}

export async function appendHistoryEntry(entry: HistoryEntry): Promise<void> {
	const path = getHistoryPath();
	mkdirSync(dirname(path), { recursive: true });
	appendFileSync(path, `${JSON.stringify(entry)}\n`);
}
