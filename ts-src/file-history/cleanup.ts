import { readdir, stat, unlink } from "node:fs/promises";
import { join } from "node:path";
import { getFileHistoryDir } from "../config/paths.ts";

export interface FileHistoryCleanupConfig {
	maxSize: number; // bytes, default 100MB
	projectPath?: string;
}

interface CleanupStats {
	filesRemoved: number;
	bytesFreed: number;
}

const DEFAULT_MAX_SIZE = 100 * 1024 * 1024;

/**
 * Clean up old file history backups.
 * If total size exceeds maxSize, removes oldest versions first.
 */
export async function runFileHistoryCleanup(config?: Partial<FileHistoryCleanupConfig>): Promise<CleanupStats> {
	const maxSize = config?.maxSize ?? DEFAULT_MAX_SIZE;
	const baseDir = getFileHistoryDir(config?.projectPath);

	const stats: CleanupStats = { filesRemoved: 0, bytesFreed: 0 };

	let entries: string[];
	try {
		entries = await readdir(baseDir);
	} catch {
		return stats;
	}

	// Collect all backup files across all UUID dirs
	interface BackupInfo {
		path: string;
		version: number;
		size: number;
	}
	const allBackups: BackupInfo[] = [];

	for (const entry of entries) {
		if (entry === "meta.json") continue;
		const dirPath = join(baseDir, entry);
		let dirStat: Awaited<ReturnType<typeof stat>>;
		try {
			dirStat = await stat(dirPath);
		} catch {
			continue;
		}
		if (!dirStat.isDirectory()) continue;

		let files: string[];
		try {
			files = await readdir(dirPath);
		} catch {
			continue;
		}

		for (const f of files) {
			const match = f.match(/^v(\d+)\.bak$/);
			if (!match) continue;
			const version = Number.parseInt(match[1] as string, 10);
			const fullPath = join(dirPath, f);
			const info = await stat(fullPath);
			allBackups.push({ path: fullPath, version, size: info.size });
		}
	}

	// If total size exceeds maxSize, remove oldest versions first
	let totalSize = allBackups.reduce((sum, b) => sum + b.size, 0);
	if (totalSize > maxSize) {
		allBackups.sort((a, b) => a.version - b.version); // oldest first
		for (const backup of allBackups) {
			if (totalSize <= maxSize) break;
			await unlink(backup.path);
			stats.filesRemoved++;
			stats.bytesFreed += backup.size;
			totalSize -= backup.size;
		}
	}

	return stats;
}
