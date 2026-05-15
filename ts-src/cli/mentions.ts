import { readdirSync, statSync } from "node:fs";
import { extname, resolve } from "node:path";

export interface MentionResult {
	/** The user message with @paths replaced by just the path (no @ prefix) */
	cleanedInput: string;
	/** File contents to append to the message */
	injectedFiles: Array<{ path: string; content: string; lines: number }>;
	/** Paths that couldn't be resolved */
	errors: string[];
}

const MENTION_RE = /@(\S+)/g;

function looksLikePath(token: string): boolean {
	return token.includes("/") || token.includes(".");
}

export async function resolveMentions(input: string, cwd: string): Promise<MentionResult> {
	const injectedFiles: MentionResult["injectedFiles"] = [];
	const errors: string[] = [];
	const seen = new Set<string>();

	let cleanedInput = input;
	const matches: string[] = [];

	for (const m of input.matchAll(MENTION_RE)) {
		const token = m[1];
		if (token === undefined || !looksLikePath(token)) continue;
		matches.push(token);
	}

	for (const token of matches) {
		cleanedInput = cleanedInput.replace(`@${token}`, token);
		const resolved = resolve(cwd, token);
		if (seen.has(resolved)) continue;
		seen.add(resolved);

		try {
			const stat = statSync(resolved);
			if (stat.isDirectory()) {
				const entries = readdirSync(resolved);
				const content = entries.join("\n");
				injectedFiles.push({ path: resolved, content, lines: entries.length });
			} else {
				const content = await Bun.file(resolved).text();
				const lines = content.split("\n").length;
				injectedFiles.push({ path: resolved, content, lines });
			}
		} catch {
			errors.push(`Not found: ${resolved}`);
		}
	}

	return { cleanedInput, injectedFiles, errors };
}

export function buildMentionMessage(input: string, files: MentionResult["injectedFiles"]): string {
	const blocks = files.map((f) => {
		const ext = extname(f.path).slice(1);
		const fence = ext ? `\`\`\`${ext}` : "```";
		return `\`${f.path}\`:\n${fence}\n${f.content}\n\`\`\``;
	});

	return `${input}\n\n---\nReferenced files:\n\n${blocks.join("\n\n")}`;
}
