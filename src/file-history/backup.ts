import { existsSync } from "node:fs";
import { mkdir, readFile } from "node:fs/promises";
import { join } from "node:path";
import { getFileHistoryDir } from "../config/paths.ts";
import { FileHistoryMeta } from "./meta.ts";

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
	const baseDir = getFileHistoryDir(projectPath);
	const meta = new FileHistoryMeta(baseDir);
	const entry = await meta.getOrCreate(filePath);
	const uuidDir = join(baseDir, entry.uuid);

	// Check latest backup for dedup
	if (entry.versions > 0) {
		try {
			const latestPath = join(uuidDir, `v${entry.versions}.bak`);
			const latestContent = await readFile(latestPath, "utf-8");
			if (hashContent(latestContent) === hash) return;
		} catch {
			// Latest backup missing — proceed with new backup
		}
	}

	await mkdir(uuidDir, { recursive: true });
	const nextVersion = entry.versions + 1;
	await Bun.write(join(uuidDir, `v${nextVersion}.bak`), content);
	await meta.incrementVersion(entry.uuid);
}
