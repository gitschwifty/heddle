import { readdirSync } from "node:fs";
import { join } from "node:path";
import { getProjectSessionsDir } from "../config/paths.ts";
import { loadSessionMeta } from "./jsonl.ts";

export interface SessionInfo {
	id: string;
	name: string | undefined;
	created: string;
	model: string;
	cwd: string;
	messageCount: number;
	firstUserMessage: string | undefined;
	forkedFrom: string | undefined;
}

/**
 * List all sessions in the given (or default) sessions directory.
 * Parses each .jsonl file's session_meta, counts messages, extracts first user message.
 * Returns sorted by created descending.
 */
export async function listSessions(sessionDir?: string): Promise<SessionInfo[]> {
	const dir = sessionDir ?? getProjectSessionsDir();

	let files: string[];
	try {
		files = readdirSync(dir).filter((f) => f.endsWith(".jsonl"));
	} catch {
		return [];
	}

	const sessions: SessionInfo[] = [];

	for (const file of files) {
		const filePath = join(dir, file);
		const meta = await loadSessionMeta(filePath);
		if (!meta) continue;

		const content = await Bun.file(filePath).text();
		const lines = content
			.trim()
			.split("\n")
			.filter((l) => l.trim())
			.map((l) => {
				try {
					return JSON.parse(l) as Record<string, unknown>;
				} catch {
					return null;
				}
			})
			.filter((obj): obj is Record<string, unknown> => obj !== null);

		const messages = lines.filter((obj) => "role" in obj);
		const firstUser = messages.find((obj) => obj.role === "user");
		let firstUserMessage: string | undefined;
		if (firstUser && typeof firstUser.content === "string") {
			firstUserMessage = firstUser.content.length > 100 ? firstUser.content.slice(0, 100) : firstUser.content;
		}

		// Look for session_name marker (last one wins)
		let name: string | undefined = meta.name as string | undefined;
		for (const line of lines) {
			if (line.type === "session_name" && typeof line.name === "string") {
				name = line.name;
			}
		}

		sessions.push({
			id: meta.id,
			name,
			created: meta.created,
			model: meta.model,
			cwd: meta.cwd,
			messageCount: messages.length,
			firstUserMessage,
			forkedFrom: (meta.forked_from as string) ?? undefined,
		});
	}

	sessions.sort((a, b) => b.created.localeCompare(a.created));
	return sessions;
}

/**
 * Find a session file by target. If target is empty/undefined, returns the most recent.
 * If target looks like a UUID (or prefix), match by id. Otherwise try name lookup.
 * Returns file path string or null.
 */
export async function findSession(target: string | undefined, sessionDir?: string): Promise<string | null> {
	const dir = sessionDir ?? getProjectSessionsDir();

	let files: string[];
	try {
		files = readdirSync(dir).filter((f) => f.endsWith(".jsonl"));
	} catch {
		return null;
	}

	// Load all metas
	const entries: { meta: { id: string; name?: string; created: string }; filePath: string }[] = [];
	for (const file of files) {
		const filePath = join(dir, file);
		const meta = await loadSessionMeta(filePath);
		if (!meta) continue;

		// Also scan for session_name markers
		let name: string | undefined = meta.name as string | undefined;
		const content = await Bun.file(filePath).text();
		const lines = content
			.trim()
			.split("\n")
			.filter((l) => l.trim());
		for (const line of lines) {
			try {
				const parsed = JSON.parse(line);
				if (parsed.type === "session_name" && typeof parsed.name === "string") {
					name = parsed.name;
				}
			} catch {}
		}

		entries.push({ meta: { id: meta.id, name, created: meta.created }, filePath });
	}

	if (!target || target.trim() === "") {
		// Return most recent
		if (entries.length === 0) return null;
		entries.sort((a, b) => b.meta.created.localeCompare(a.meta.created));
		return entries[0]?.filePath ?? null;
	}

	// Try UUID match (exact or prefix)
	const byId = entries.find((e) => e.meta.id === target || e.meta.id.startsWith(target));
	if (byId) return byId.filePath;

	// Try name match
	const byName = entries.find((e) => e.meta.name === target);
	if (byName) return byName.filePath;

	return null;
}
