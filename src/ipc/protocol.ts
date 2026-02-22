import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

declare const __PROTOCOL_VERSION__: string | undefined;

function loadProtocolVersion(): string {
	if (process.env.HEDDLE_PROTOCOL_VERSION) {
		return process.env.HEDDLE_PROTOCOL_VERSION.trim();
	}
	// Injected at compile time via --define in build scripts
	if (typeof __PROTOCOL_VERSION__ !== "undefined") {
		return __PROTOCOL_VERSION__;
	}
	// Dev mode: read from file
	const __dirname = dirname(fileURLToPath(import.meta.url));
	const versionPath = join(__dirname, "..", "..", "PROTOCOL_VERSION");
	return readFileSync(versionPath, "utf-8").trim();
}

export const PROTOCOL_VERSION = loadProtocolVersion();

export function parseSemver(version: string): { major: number; minor: number; patch: number } {
	const parts = version.split(".");
	return {
		major: Number.parseInt(parts[0] ?? "0", 10),
		minor: Number.parseInt(parts[1] ?? "0", 10),
		patch: Number.parseInt(parts[2] ?? "0", 10),
	};
}

export function checkCompatibility(clientVersion: string): {
	compatible: boolean;
	level: "exact" | "patch" | "minor" | "major" | "incompatible";
	warn?: string;
} {
	const server = parseSemver(PROTOCOL_VERSION);
	const client = parseSemver(clientVersion);

	if (server.major !== client.major) {
		return {
			compatible: false,
			level: "incompatible",
			warn: `Major version mismatch: client=${clientVersion}, server=${PROTOCOL_VERSION}`,
		};
	}

	if (server.minor !== client.minor) {
		return {
			compatible: true,
			level: "minor",
			warn: `Minor version mismatch: client=${clientVersion}, server=${PROTOCOL_VERSION}`,
		};
	}

	if (server.patch !== client.patch) {
		return { compatible: true, level: "patch" };
	}

	return { compatible: true, level: "exact" };
}
