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

function deletePath(obj: any, pathStr: string) {
  const parts = pathStr.split(".");
  let cur = obj;
  for (let i = 0; i < parts.length - 1; i++) {
    if (!cur || typeof cur !== "object") return;
    cur = cur[parts[i]];
  }
  if (cur && typeof cur === "object") delete cur[parts[parts.length - 1]];
}

function stripIgnored(obj: unknown): unknown {
  const clone = JSON.parse(JSON.stringify(obj));
  for (const p of IGNORE_PATHS) deletePath(clone, p);
  return clone;
}

async function runFixtureStrict(name: string): Promise<void> {
  const inPath = path.join(FIXTURES_DIR, `${name}.in.jsonl`);
  const outPath = path.join(FIXTURES_DIR, `${name}.out.jsonl`);

  const inputLines = fs.readFileSync(inPath, "utf8").split("\n").filter(Boolean);
  const expectedLines = fs.readFileSync(outPath, "utf8").split("\n").filter(Boolean);

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

  expect(output.length).toBeGreaterThan(0);
  expect(output.length).toBe(expectedLines.length);

  for (let i = 0; i < expectedLines.length; i++) {
    const expected = stripIgnored(JSON.parse(expectedLines[i]));
    const actual = stripIgnored(JSON.parse(output[i]));
    expect(actual).toEqual(expected);
  }
}

describe("ipc fixtures", () => {
  it("normal", async () => {
    await runFixtureStrict("normal");
  });

  it("error", async () => {
    await runFixtureStrict("error");
  });

  it("cancel", async () => {
    await runFixtureStrict("cancel");
  });

  it("version mismatch", async () => {
    await runFixtureStrict("version-mismatch");
  });
});
