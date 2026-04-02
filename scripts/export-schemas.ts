/**
 * Export TypeBox schemas as JSON Schema files for taplo (TOML LSP) autocomplete.
 *
 * Usage: bun run scripts/export-schemas.ts
 */
import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { HeddleConfigSchema } from "../src/config/types.ts";
import { HooksConfigSchema } from "../src/hooks/types.ts";

const SCHEMA_DIR = resolve(import.meta.dirname ?? ".", "../schemas");

const schemas = [
	{ name: "config.schema.json", schema: HeddleConfigSchema },
	{ name: "hooks.schema.json", schema: HooksConfigSchema },
] as const;

mkdirSync(SCHEMA_DIR, { recursive: true });

for (const { name, schema } of schemas) {
	const jsonSchema = {
		$schema: "http://json-schema.org/draft-07/schema#",
		...schema,
	};
	const outPath = resolve(SCHEMA_DIR, name);
	writeFileSync(outPath, `${JSON.stringify(jsonSchema, null, "\t")}\n`);
	console.log(`Exported ${name} → ${outPath}`);
}
