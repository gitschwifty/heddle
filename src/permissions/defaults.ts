/** Default deny rules shipped in ~/.heddle/config.toml on first run. */
export const DEFAULT_DENY_RULES: string[] = [
	"Write(.env*)",
	"Write(*.pem)",
	"Write(*.key)",
	"Write(*credentials*)",
	"Bash(rm *)",
	"Bash(rm)",
	"Bash(sudo *)",
	"Bash(chmod *)",
	"Write(.heddle/config.toml)",
];

/** Generate the [permissions] TOML section for a default config file. */
export function generateDefaultPermissionsToml(): string {
	const denyEntries = DEFAULT_DENY_RULES.map((r) => `  "${r}",`).join("\n");
	return `[permissions]\ndeny = [\n${denyEntries}\n]\n`;
}
