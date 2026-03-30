import { readdir, readFile, stat, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { getFileHistoryDir } from "../config/paths.ts";

export interface BackupEntry {
	timestamp: number;
	path: string;
	size: number;
}

/**
 * List backups for a file, sorted newest-first.
 */
export async function listBackups(filePath: string, projectPath?: string): Promise<BackupEntry[]> {
	const histDir = getFileHistoryDir(projectPath, filePath);
	let files: string[];
	try {
		files = await readdir(histDir);
	} catch {
		return [];
	}

	const entries: BackupEntry[] = [];
	for (const f of files) {
		if (!f.endsWith(".bak")) continue;
		const ts = Number.parseInt(f.replace(".bak", ""), 10);
		if (Number.isNaN(ts)) continue;
		const fullPath = join(histDir, f);
		const info = await stat(fullPath);
		entries.push({ timestamp: ts, path: fullPath, size: info.size });
	}

	return entries.sort((a, b) => b.timestamp - a.timestamp);
}

/**
 * Restore a backup to the original file path.
 */
export async function restoreBackup(filePath: string, timestamp: number, projectPath?: string): Promise<string> {
	const histDir = getFileHistoryDir(projectPath, filePath);
	const backupPath = join(histDir, `${timestamp}.bak`);

	let content: string;
	try {
		content = await readFile(backupPath, "utf-8");
	} catch {
		return `Error: Backup not found for timestamp ${timestamp}`;
	}

	await writeFile(filePath, content, "utf-8");
	return `Restored ${filePath} from backup ${timestamp}`;
}
