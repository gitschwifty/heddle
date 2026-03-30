import { existsSync } from "node:fs";
import { mkdir, readdir, readFile } from "node:fs/promises";
import { join } from "node:path";
import { getFileHistoryDir } from "../config/paths.ts";

function hashContent(content: string): string {
	const hasher = new Bun.CryptoHasher("md5");
	hasher.update(content);
	return hasher.digest("hex");
}

/**
 * Back up a file's current content before it gets modified.
 * Skips if the file doesn't exist (new file being created).
 * Deduplicates against the latest backup by content hash.
 */
export async function backupFile(filePath: string, projectPath?: string): Promise<void> {
	if (!existsSync(filePath)) return;

	const content = await readFile(filePath, "utf-8");
	const hash = hashContent(content);
	const histDir = getFileHistoryDir(projectPath, filePath);

	// Check latest backup for dedup
	try {
		const files = await readdir(histDir);
		if (files.length > 0) {
			const bakFiles = files.filter((f) => f.endsWith(".bak")).sort();
			if (bakFiles.length > 0) {
				const latest = bakFiles[bakFiles.length - 1] as string;
				const latestContent = await readFile(join(histDir, latest), "utf-8");
				if (hashContent(latestContent) === hash) return;
			}
		}
	} catch {
		// Dir doesn't exist yet — will create below
	}

	await mkdir(histDir, { recursive: true });
	let ts = Date.now();
	// Ensure uniqueness by incrementing if a file with this timestamp already exists
	while (existsSync(join(histDir, `${ts}.bak`))) {
		ts++;
	}
	await Bun.write(join(histDir, `${ts}.bak`), content);
}
