#!/usr/bin/env bun
// VERIFY fence for the guides (bd-z2d0): catches broken MDX structure the eye skips.
// Walks docs/guides/** and checks every .mdx for frontmatter, balanced code fences,
// mermaid blocks with a known diagram header, stray body h1, and JSX components with
// no import surface.
// ponytail: mermaid is header-checked only; a full parse needs mermaid-cli, add when
// diagrams actually break in CI.
import { readdirSync, readFileSync } from "node:fs";
import { join, dirname, relative } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(fileURLToPath(import.meta.url));

function walk(dir) {
  const out = [];
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, e.name);
    if (e.isDirectory()) out.push(...walk(p));
    else if (e.name.endsWith(".mdx")) out.push(p);
  }
  return out;
}

const files = walk(root).sort();

const DIAGRAMS =
  /^(flowchart|graph|sequenceDiagram|stateDiagram-v2|stateDiagram|erDiagram|classDiagram|mindmap|timeline|gantt|pie|gitGraph|journey|quadrantChart|requirementDiagram|C4Context)\b/;

function check(file) {
  const errors = [];
  const src = readFileSync(file, "utf8");
  if (!src.startsWith("---\n")) return ["missing frontmatter"];
  const close = src.indexOf("\n---\n", 4);
  if (close === -1) return ["unterminated frontmatter"];
  const body = src.slice(close + 5);

  let fence = 0;
  let inMermaid = false;
  let mermaidChecked = false;
  for (const [i, line] of body.split("\n").entries()) {
    const n = i + 1;
    if (line.startsWith("```")) {
      if (fence === 0) {
        inMermaid = line.slice(3).trim() === "mermaid";
        mermaidChecked = false;
        fence = 1;
      } else {
        if (inMermaid && !mermaidChecked) errors.push(`${n}: mermaid block without a diagram header`);
        fence = 0;
        inMermaid = false;
      }
      continue;
    }
    if (fence === 1) {
      const t = line.trim();
      if (inMermaid && !mermaidChecked && t && !t.startsWith("%%")) {
        if (!DIAGRAMS.test(t)) errors.push(`${n}: unknown mermaid header: ${t}`);
        mermaidChecked = true;
      }
      continue;
    }
    if (/^#\s/.test(line)) errors.push(`${n}: body h1; titles live in frontmatter`);
    if (/<[A-Z][A-Za-z]*[\s/>]/.test(line)) errors.push(`${n}: JSX component without an import surface`);
  }
  if (fence !== 0) errors.push("unbalanced code fence");
  return errors;
}

let failed = 0;
if (files.length === 0) {
  console.error("no .mdx guides found");
  failed = 1;
}
for (const f of files) {
  const rel = relative(root, f);
  const errors = check(f);
  if (errors.length === 0) console.log(`ok   ${rel}`);
  else { failed = 1; for (const e of errors) console.error(`FAIL ${rel}: ${e}`); }
}
process.exit(failed);
