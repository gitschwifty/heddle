import { readdir, stat, unlink } from "node:fs/promises";
import { join } from "node:path";
import { getFileHistoryDir } from "../config/paths.ts";

export interface FileHistoryCleanupConfig {
	maxAge: number; // ms, default 30 days
	maxSize: number; // bytes, default 100MB
	projectPath?: string;
}

interface CleanupStats {
	filesRemoved: number;
	bytesFreed: number;
}

const DEFAULT_MAX_AGE = 30 * 24 * 60 * 60 * 1000;
const DEFAULT_MAX_SIZE = 100 * 1024 * 1024;

/**
 * Clean up old file history backups.
 * Removes files older than maxAge, then trims by maxSize (oldest first).
 */
export async function runFileHistoryCleanup(config?: Partial<FileHistoryCleanupConfig>): Promise<CleanupStats> {
	const maxAge = config?.maxAge ?? DEFAULT_MAX_AGE;
	const maxSize = config?.maxSize ?? DEFAULT_MAX_SIZE;
	const baseDir = getFileHistoryDir(config?.projectPath);

	const stats: CleanupStats = { filesRemoved: 0, bytesFreed: 0 };

	let projectDirs: string[];
	try {
		projectDirs = await readdir(baseDir);
	} catch {
		return stats;
	}

	// Collect all backup files across all file history dirs
	interface BackupInfo {
		path: string;
		timestamp: number;
		size: number;
	}
	const allBackups: BackupInfo[] = [];

	for (const subDir of projectDirs) {
		const dirPath = join(baseDir, subDir);
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
			if (!f.endsWith(".bak")) continue;
			const ts = Number.parseInt(f.replace(".bak", ""), 10);
			if (Number.isNaN(ts)) continue;
			const fullPath = join(dirPath, f);
			const info = await stat(fullPath);
			allBackups.push({ path: fullPath, timestamp: ts, size: info.size });
		}
	}

	const now = Date.now();

	// Phase 1: Remove files older than maxAge
	const remaining: BackupInfo[] = [];
	for (const backup of allBackups) {
		if (now - backup.timestamp > maxAge) {
			await unlink(backup.path);
			stats.filesRemoved++;
			stats.bytesFreed += backup.size;
		} else {
			remaining.push(backup);
		}
	}

	// Phase 2: If total size exceeds maxSize, remove oldest first
	let totalSize = remaining.reduce((sum, b) => sum + b.size, 0);
	if (totalSize > maxSize) {
		remaining.sort((a, b) => a.timestamp - b.timestamp); // oldest first
		for (const backup of remaining) {
			if (totalSize <= maxSize) break;
			await unlink(backup.path);
			stats.filesRemoved++;
			stats.bytesFreed += backup.size;
			totalSize -= backup.size;
		}
	}

	return stats;
}
