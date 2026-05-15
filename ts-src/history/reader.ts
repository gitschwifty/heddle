import { existsSync, readFileSync } from "node:fs";
import { getHistoryPath } from "../config/paths.ts";
import type { HistoryEntry } from "./writer.ts";

export interface LoadHistoryOptions {
	limit?: number;
	search?: string;
}

export async function loadHistory(options?: LoadHistoryOptions): Promise<HistoryEntry[]> {
	const path = getHistoryPath();
	if (!existsSync(path)) return [];

	const content = readFileSync(path, "utf-8");
	const lines = content.trim().split("\n").filter(Boolean);

	let entries: HistoryEntry[] = [];
	for (const line of lines) {
		try {
			entries.push(JSON.parse(line));
		} catch {
			// skip malformed lines
		}
	}

	if (options?.search) {
		const term = options.search.toLowerCase();
		entries = entries.filter((e) => e.message_preview.toLowerCase().includes(term));
	}

	if (options?.limit) {
		entries = entries.slice(-options.limit);
	}

	return entries;
}
