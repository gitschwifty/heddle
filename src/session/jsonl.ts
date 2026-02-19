import { existsSync, mkdirSync } from "node:fs";
import { appendFile } from "node:fs/promises";
import { dirname } from "node:path";
import type { Message } from "../types.ts";

export interface SessionMeta {
	type: "session_meta";
	id: string;
	cwd: string;
	model: string;
	created: string;
	heddle_version: string;
	[key: string]: unknown;
}

/** Ensure parent dirs exist, then append a raw JSON line. */
async function appendLine(filePath: string, data: Record<string, unknown>): Promise<void> {
	const dir = dirname(filePath);
	if (!existsSync(dir)) {
		mkdirSync(dir, { recursive: true });
	}
	await appendFile(filePath, JSON.stringify(data) + "\n", "utf-8");
}

/** Write the session_meta header as the first line of a new session file. */
export async function writeSessionMeta(filePath: string, meta: SessionMeta): Promise<void> {
	await appendLine(filePath, meta);
}

/**
 * Append a single message as a JSON line with a timestamp.
 * Creates parent directories and the file if they don't exist.
 */
export async function appendMessage(filePath: string, message: Message): Promise<void> {
	await appendLine(filePath, {
		...message,
		timestamp: new Date().toISOString(),
	});
}

/**
 * Load all messages from a JSONL session file.
 * Skips non-message lines (session_meta, compaction markers, etc.).
 * Returns an empty array if the file doesn't exist or is empty.
 */
export async function loadSession(filePath: string): Promise<Message[]> {
	if (!existsSync(filePath)) {
		return [];
	}

	const content = await Bun.file(filePath).text();
	if (!content.trim()) {
		return [];
	}

	return content
		.trim()
		.split("\n")
		.filter((line) => line.trim().length > 0)
		.map((line) => JSON.parse(line) as Record<string, unknown>)
		.filter((obj) => obj.type !== "session_meta") as Message[];
}

/**
 * Load the session_meta header from a session file.
 * Returns null if the file doesn't exist or has no session_meta line.
 */
export async function loadSessionMeta(filePath: string): Promise<SessionMeta | null> {
	if (!existsSync(filePath)) {
		return null;
	}

	const content = await Bun.file(filePath).text();
	const firstLine = content.split("\n")[0]?.trim();
	if (!firstLine) return null;

	try {
		const parsed = JSON.parse(firstLine);
		if (parsed.type === "session_meta") return parsed as SessionMeta;
	} catch {}
	return null;
}
