// User-facing text, comments, and docs follow the same rules as the drafts: no contractions
// and no em dashes. This scans the source tree with the same patterns as checker.rs.
import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const ROOT = join(__dirname, "..");
const CONTRACTION = /\b(?:\w+n['’]t|\w+['’](?:re|ve|ll|m|d)|(?:it|that|there|let|he|she|what)['’]s)\b/gi;

// Files whose job is to contain broken examples (checker tests, fake replies, test inputs).
const ALLOWED = new Set([
  "src-tauri/src/checker.rs",
  "src-tauri/src/drafts.rs",
  "src-tauri/src/engine.rs",
  "src-tauri/src/bin/fakeclaude.rs",
  "tests/popup-state.test.ts",
  "tests/copy-rules.test.ts",
  "tests/messages.json",
]);

function files(dir: string): string[] {
  const out: string[] = [];
  for (const e of readdirSync(dir)) {
    if (["node_modules", "target", "dist", "gen", ".git", "spec", "icons", "reports"].includes(e)) continue;
    const p = join(dir, e);
    if (statSync(p).isDirectory()) out.push(...files(p));
    else if (/\.(ts|mjs|rs|html|css|md|txt|json|nsh)$/.test(e) && !/package-lock|Cargo\.lock|SPEC\.md|M1-timing-report/.test(e)) out.push(p);
  }
  return out;
}

describe("copy rules", () => {
  const all = files(ROOT).filter((f) => !ALLOWED.has(relative(ROOT, f).replace(/\\/g, "/")));

  it("scans a real set of files", () => {
    expect(all.length).toBeGreaterThan(25);
  });

  it("has no contractions or em dashes", () => {
    const hits: string[] = [];
    for (const f of all) {
      readFileSync(f, "utf8")
        .split("\n")
        .forEach((line, i) => {
          for (const m of line.match(CONTRACTION) ?? []) hits.push(`${relative(ROOT, f)}:${i + 1} ${m}`);
          if (line.includes("—")) hits.push(`${relative(ROOT, f)}:${i + 1} em dash`);
        });
    }
    expect(hits).toEqual([]);
  });
});
