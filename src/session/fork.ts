import { randomUUID } from "node:crypto";
import { dirname, join } from "node:path";
import { loadSessionMeta } from "./jsonl.ts";

export interface ForkResult {
	sessionFile: string;
	sessionId: string;
}

/**
 * Fork a session file. Creates a new session file in the same directory
 * with a new UUID and forked_from pointing to the original session.
 * If upToMessage is set, only copies that many messages from the source.
 */
export async function forkSession(sourceFile: string, options?: { upToMessage?: number }): Promise<ForkResult> {
	const meta = await loadSessionMeta(sourceFile);
	if (!meta) {
		throw new Error(`Cannot fork: no session_meta found in ${sourceFile}`);
	}

	const content = await Bun.file(sourceFile).text();
	const allLines = content
		.trim()
		.split("\n")
		.filter((l) => l.trim());

	// Separate meta line from message lines
	const messageLines: string[] = [];
	for (const line of allLines) {
		try {
			const parsed = JSON.parse(line);
			if (parsed.type === "session_meta") continue;
			if ("role" in parsed) {
				messageLines.push(line);
			}
		} catch {
			// skip unparseable lines
		}
	}

	const newId = randomUUID();
	const sessionDir = dirname(sourceFile);
	const newFile = join(sessionDir, `${newId}.jsonl`);

	// Write new meta with forked_from
	const newMeta = {
		...meta,
		id: newId,
		created: new Date().toISOString(),
		forked_from: meta.id,
	};

	const linesToCopy = options?.upToMessage != null ? messageLines.slice(0, options.upToMessage) : messageLines;

	const output = `${[JSON.stringify(newMeta), ...linesToCopy].join("\n")}\n`;
	await Bun.write(newFile, output);

	return { sessionFile: newFile, sessionId: newId };
}
