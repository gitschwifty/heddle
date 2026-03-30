import { readdir, readFile, stat, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { getFileHistoryDir } from "../config/paths.ts";
import { FileHistoryMeta } from "./meta.ts";

export interface BackupEntry {
	version: number;
	path: string;
	size: number;
}

/**
 * List backups for a file, sorted newest-first (highest version first).
 */
export async function listBackups(filePath: string, projectPath?: string): Promise<BackupEntry[]> {
	const baseDir = getFileHistoryDir(projectPath);
	const meta = new FileHistoryMeta(baseDir);
	const entry = await meta.findByPath(filePath);
	if (!entry) return [];

	const uuidDir = join(baseDir, entry.uuid);
	let files: string[];
	try {
		files = await readdir(uuidDir);
	} catch {
		return [];
	}

	const entries: BackupEntry[] = [];
	for (const f of files) {
		const match = f.match(/^v(\d+)\.bak$/);
		if (!match) continue;
		const version = Number.parseInt(match[1] as string, 10);
		const fullPath = join(uuidDir, f);
		const info = await stat(fullPath);
		entries.push({ version, path: fullPath, size: info.size });
	}

	return entries.sort((a, b) => b.version - a.version);
}

/**
 * Restore a backup to the original file path.
 */
export async function restoreBackup(filePath: string, version: number, projectPath?: string): Promise<string> {
	const baseDir = getFileHistoryDir(projectPath);
	const meta = new FileHistoryMeta(baseDir);
	const entry = await meta.findByPath(filePath);
	if (!entry) return `Error: No backup history found for ${filePath}`;

	const backupPath = join(baseDir, entry.uuid, `v${version}.bak`);

	let content: string;
	try {
		content = await readFile(backupPath, "utf-8");
	} catch {
		return `Error: Backup version ${version} not found for ${filePath}`;
	}

	await writeFile(filePath, content, "utf-8");
	return `Restored ${filePath} from backup v${version}`;
}
