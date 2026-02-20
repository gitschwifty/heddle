import fs from "node:fs";
import path from "node:path";
import readline from "node:readline";
import { spawn } from "node:child_process";
import { describe, it, expect } from "bun:test";
import { validateIpcMessage } from "../../src/ipc/schema";

const FIXTURES_DIR = path.resolve(process.cwd(), "test/ipc/fixtures");

const IGNORE_PATHS: string[] = [
  "session_id",
  "timestamp",
  "usage.prompt_tokens",
  "usage.completion_tokens",
  "usage.total_tokens",
];

function stripIgnored(obj: unknown): unknown {
  if (Array.isArray(obj)) return obj.map(stripIgnored);
  if (obj && typeof obj === "object") {
    const out: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(obj)) {
      const pathKey = k;
      if (IGNORE_PATHS.includes(pathKey)) continue;
      out[k] = stripIgnored(v);
    }
    return out;
  }
  return obj;
}

async function runFixtureStrict(fixture: string): Promise<void> {
  const fixturePath = path.join(FIXTURES_DIR, fixture);
  const inputLines = fs.readFileSync(fixturePath, "utf8").split("\n").filter(Boolean);

  const child = spawn("bun", ["run", "headless"], {
    cwd: process.cwd(),
    stdio: ["pipe", "pipe", "inherit"],
  });

  const rl = readline.createInterface({ input: child.stdout });

  for (const line of inputLines) {
    child.stdin.write(line + "\n");
  }

  const output: string[] = [];
  const timeoutMs = 5000;
  const start = Date.now();

  for await (const line of rl) {
    output.push(line);
    const msg = JSON.parse(line);
    expect(validateIpcMessage(msg)).toBe(true);

    if (Date.now() - start > timeoutMs) {
      child.kill();
      throw new Error("fixture timeout");
    }
  }

  child.stdin.end();

  // Compare outputs line-by-line with ignored paths stripped.
  expect(output.length).toBeGreaterThan(0);
  for (let i = 0; i < Math.min(output.length, inputLines.length); i++) {
    const expected = stripIgnored(JSON.parse(inputLines[i]));
    const actual = stripIgnored(JSON.parse(output[i]));
    expect(actual).toEqual(expected);
  }
}

describe("ipc fixtures", () => {
  it("normal", async () => {
    await runFixtureStrict("normal.jsonl");
  });

  it("error", async () => {
    await runFixtureStrict("error.jsonl");
  });

  it("cancel", async () => {
    await runFixtureStrict("cancel.jsonl");
  });

  it("version mismatch", async () => {
    await runFixtureStrict("version-mismatch.jsonl");
  });
});
