import type { Message } from "../types.ts";
import { existsSync, mkdirSync } from "node:fs";
import { appendFile } from "node:fs/promises";
import { dirname } from "node:path";

/**
 * Append a single message as a JSON line to the given JSONL file.
 * Creates parent directories and the file if they don't exist.
 */
export async function appendMessage(filePath: string, message: Message): Promise<void> {
	const dir = dirname(filePath);
	if (!existsSync(dir)) {
		mkdirSync(dir, { recursive: true });
	}
	await appendFile(filePath, JSON.stringify(message) + "\n", "utf-8");
}

/**
 * Load all messages from a JSONL session file.
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
		.map((line) => JSON.parse(line) as Message);
}
