#!/usr/bin/env bun
// VERIFY fence for the guides (bd-z2d0). Catches broken MDX structure the eye skips.
// Walks docs/guides/** and checks every .mdx for frontmatter, balanced code fences,
// mermaid blocks with a known diagram header, stray body h1, and JSX components with
// no import surface.
//
// Extended (ADR-0004 D6 on toron.dev): a guide is evidence, not prose. Three more
// checks, all of which can fail a real page:
//
//   1. STRUCTURE  — the original set, unchanged.
//   2. COMPLETENESS — every ```bash block must be followed by a ```text block
//      showing what it printed, and the file must name at least one failure mode.
//      A command with no output is an unverified claim.
//   3. SURFACE — every command in a ```bash block must exist in the real CLI, with
//      only real flags. The surface is extracted by walking `<bin> --help`
//      recursively, so an invented flag fails here instead of shipping as truth.
//      This is the check that would have caught `br gate report --evidence` and
//      `toron workflow status`, both of which were written from memory and were
//      both wrong.
//
// Commands belonging to another tool are not this repo's to verify, so they are
// reported as skipped rather than checked. A missing own-binary is fail-closed:
// set GUIDES_SKIP_CLI=1 to bypass, which prints why.
//
// ponytail: mermaid is header-checked only; a full parse needs mermaid-cli, add when
// diagrams actually break in CI.
import { readdirSync, readFileSync, existsSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join, dirname, relative, basename } from "node:path";
import { fileURLToPath } from "node:url";

// --- per-repo setting: the one line that differs between the four copies ---------
const OWN_BIN = "br";
// ---------------------------------------------------------------------------------

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

// A failure mode is a plain-text convention rather than a component, because the
// structure check above rejects JSX without an import surface and these guides
// are rendered as plain MDX.
const FAILS = /^\s*(?:>\s*)?(?:\*\*)?if it fails/im;

// ------------------------------------------------------------------ CLI surface

function help(bin, args) {
  const r = spawnSync(bin, [...args, "--help"], { encoding: "utf8" });
  return r.status === 0 ? (r.stdout || "") : null;
}

// Walk `<bin> --help` → `Commands:` block → recurse. Clap prints subcommands as
// two-space indented names, and flags one per line under `Options:`.
//
// Keys are the path WITHOUT the binary name (root is ""), so the caller must
// strip the leading token before descending. Getting that wrong is silent: the
// walk breaks at the first token and every command reports clean. The probe in
// the test suite exists because that bug shipped once.
function extractSurface(bin) {
  const commands = new Map(); // path (space-joined, no binary) → { flags, children }
  const queue = [[]];
  const seen = new Set();
  while (queue.length > 0) {
    const path = queue.shift();
    const key = path.join(" ");
    if (seen.has(key) || path.length > 3) continue;
    seen.add(key);
    const text = help(bin, path);
    if (text === null) continue;

    // long flag → does it take a value? (`--project <PROJECT>` does)
    const flags = new Map();
    const shorts = new Map();
    const shortLong = new Map();
    for (const line of text.split("\n")) {
      const m = line.match(/^\s+(?:-(\w),\s+)?--([a-z0-9][a-z0-9-]*)(.*)$/);
      if (!m) continue;
      const takesValue = /<[^>]+>/.test(m[3]);
      flags.set(m[2], takesValue);
      if (m[1]) {
        shorts.set(m[1], takesValue);
        shortLong.set(m[1], m[2]);
      }
    }

    // Positional arguments, read off the clap usage line: `Usage: toron slot
    // acquire [OPTIONS] --slot <SLOT> --project <PROJECT>` takes none, while
    // `Usage: toron mail whois <PROJECT> <NAME>` takes two. Strip the flag/value
    // pairs first, then count what is left. The reason this matters: a chain
    // check sees `toron slot acquire cargo` as a legal leaf call, but clap
    // rejects it at runtime, so a wrong literal positional shipped in the
    // guides and nothing failed. `[OPTIONS]` / `[COMMAND]` are not positionals.
    // A missing usage line reads as "unknown", and unknown is kept permissive:
    // this rule must never manufacture drift out of a help text we cannot parse.
    // Two more facts off the same usage line: which flags are REQUIRED (clap
    // prints optional ones bracketed: `[--holder <PIN>]`) and how many
    // positionals are required. A guide that omits a required flag is a command
    // the reader cannot run — `toron bench` with no CASE fails at runtime while
    // every token in it resolves. Bracketed regions are dropped before the parse
    // so `[--holder <PIN>]` cannot read as required.
    const usageLine = text.split("\n").find((l) => /^Usage:\s/.test(l));
    let positionals = 1;
    let requiredPositionals = 0;
    const requiredFlags = new Set();
    if (usageLine) {
      // Required half: drop the bracketed (optional) regions, then drop
      // `--flag <VALUE>` pairs. What is left is what a caller MUST supply.
      const required = usageLine.replace(/\[[^\]]*\]/g, " ");
      requiredPositionals = (
        required.replace(/--[a-z0-9-]+\s*[= ]\s*<[^>]+>/g, " ").match(/<[^>]+>/g) || []
      ).length;
      for (const m of required.matchAll(/--([a-z0-9][a-z0-9-]*)\s*[= ]\s*<[^>]+>/g)) {
        requiredFlags.add(m[1]);
      }
      // Optional half: a bracketed region that is not a flag and not the generic
      // `[OPTIONS]`/`[COMMAND]` is an optional positional (`[PATH]`), which still
      // makes the node accept one. That count is what the literal-positional rule
      // and the missing-positional rule both need.
      const optionalPositionals = (usageLine.match(/\[[^\]]*\]/g) || []).filter(
        (seg) =>
          !/^\[\s*-/.test(seg) && !/^\[(OPTIONS|COMMAND|FLAGS?)\]$/i.test(seg),
      ).length;
      positionals = requiredPositionals + optionalPositionals;
    }

    const children = new Set();
    const lines = text.split("\n");
    const start = lines.findIndex((l) => /^Commands:\s*$/.test(l));
    if (start !== -1) {
      for (let i = start + 1; i < lines.length; i++) {
        if (/^\S/.test(lines[i])) break; // next top-level section
        // A subcommand line is two-space indented. The description is OPTIONAL:
        // `br comments` prints `  add   ` with nothing after it, and requiring a
        // description silently dropped the child, which turned a legal
        // `br comments add --message` into a reported unknown flag.
        const m = lines[i].match(/^\s{2}([a-z][a-z0-9-]*)(?:\s{2,}\S|\s*$)/);
        if (m && m[1] !== "help") children.add(m[1]);
      }
    }

    commands.set(key, {
      flags,
      shorts,
      shortLong,
      children,
      positionals,
      requiredFlags,
      requiredPositionals,
    });
    for (const child of children) queue.push([...path, child]);
  }
  return commands;
}

// Split one shell line on already-joined continuations, honouring quotes, so a
// path with a space or a `#` inside a string does not confuse the tokenizer.
function tokenize(line) {
  const tokens = [];
  let cur = "";
  let quote = null;
  for (const ch of line) {
    if (quote) {
      if (ch === quote) quote = null;
      else cur += ch;
      continue;
    }
    if (ch === '"' || ch === "'") { quote = ch; continue; }
    if (ch === " " || ch === "\t") {
      if (cur) { tokens.push(cur); cur = ""; }
      continue;
    }
    cur += ch;
  }
  if (cur) tokens.push(cur);
  return tokens;
}

// Join backslash continuations, then keep only the commands a reader would run:
// env assignments, `$` prompts, pipes, and redirections are all stripped away.
function commandLines(block) {
  const joined = block.replace(/\\\n\s*/g, " ");
  const out = [];
  for (const raw of joined.split("\n")) {
    let line = raw.trim();
    if (!line || line.startsWith("#") || line.startsWith("%%")) continue;
    line = line.replace(/^\$\s*/, "");
    // Split off pipes/redirections, but a `|` inside a placeholder is part of the
    // value, not a shell pipe: `--crews <squad|profile>` was being truncated at
    // the pipe, which made the required `--as` that followed look missing.
    line = line
      .replace(/<[^>]*>/g, (m) => m.replace(/\|/g, "/"))
      .split(/\|\||&&|\||;/)[0]
      .trim();
    let tokens = tokenize(line);
    while (tokens.length > 0 && /^[A-Z_][A-Z0-9_]*=/.test(tokens[0])) tokens.shift();
    // Drop a trailing `# comment`. A `#` mid-token (inside a quoted value) is
    // not a comment and survives, because it never starts its own token.
    const hash = tokens.findIndex((t) => t.startsWith("#"));
    if (hash !== -1) tokens = tokens.slice(0, hash);
    // Unwrap synopsis brackets (`[--check]`, `[--holder <Pin>]`) instead of
    // dropping the tokens: dropping `[--path` would leave its value looking like
    // a stray positional. Placeholders survive to verifyCommand, which knows the
    // difference between `<thread-id>` (the reader's input) and a literal.
    tokens = tokens
      .map((t) => t.replace(/^\[+/, "").replace(/\]+$/, ""))
      .filter((t) => t.length > 0);
    if (tokens.length === 0) continue;
    out.push({ text: line, tokens });
  }
  return out;
}

// Descend from the root, one bare token at a time. A bare token under a command
// that HAS subcommands must name one of them; under a leaf it is a positional
// argument and is allowed. Flags are checked against the node they apply to, and
// a value-taking flag consumes its value so `--project torondev` does not look
// like a subcommand named `torondev`.
function verifyCommand(tokens, surface) {
  const errors = [];
  const path = [];
  const seenFlags = new Set();
  let positionalsSeen = 0;
  let node = surface.get("");
  if (!node) return errors;

  for (let i = 1; i < tokens.length; i++) {
    const t = tokens[i];
    if (t === "--") break;

    if (t.startsWith("-")) {
      const long = t.match(/^--([a-z0-9][a-z0-9-]*)/);
      const short = !long && t.match(/^-([A-Za-z])\b/);
      let takesValue = false;
      const where = `${OWN_BIN} ${path.join(" ")}`.trimEnd();
      if (long) {
        if (!node.flags.has(long[1])) {
          errors.push(`unknown flag --${long[1]} on \`${where}\``);
        } else {
          takesValue = node.flags.get(long[1]);
          seenFlags.add(long[1]);
        }
      } else if (short) {
        if (!node.shorts.has(short[1])) {
          errors.push(`unknown flag -${short[1]} on \`${where}\``);
        } else {
          takesValue = node.shorts.get(short[1]);
          // A short spelling satisfies the required long flag it aliases.
          seenFlags.add(short[1]);
          const aliased = node.shortLong.get(short[1]);
          if (aliased) seenFlags.add(aliased);
        }
      }
      if (takesValue && !t.includes("=")) i++;
      continue;
    }

    // A placeholder (`<run-id>`, `<file.yaml>`) is the reader's to fill in. It
    // cannot be judged as a subcommand name, but it still needs a positional
    // slot in the usage line to be legal here, so it is not a free pass: a
    // synopsis line pasted into a bash fence still reads as a claim about what
    // the command takes.
    if (/^<[^>]+>$/.test(t)) {
      if (node.positionals === 0) {
        const where = `${OWN_BIN} ${path.join(" ")}`.trimEnd();
        errors.push(`unexpected argument \`${t}\` on \`${where}\` (its usage takes none)`);
      }
      // It fills a positional slot, so it counts toward the required ones:
      // `mail whois <project> <name>` supplies the two the usage asks for even
      // though neither token is a literal.
      positionalsSeen++;
      continue;
    }

    if (node.children.size > 0) {
      if (node.children.has(t)) {
        path.push(t);
        node = surface.get(path.join(" "));
      } else {
        const where = `${OWN_BIN} ${path.join(" ")}`.trimEnd();
        errors.push(
          `unknown subcommand \`${t}\` under \`${where}\`` +
            ` (known: ${[...node.children].sort().join(", ")})`,
        );
        // Stop here: the node is unresolved, so every remaining token would be
        // judged against the wrong help text and pile cascade noise onto one
        // real defect (`mail thread <thread-id>` reported two errors, not one).
        break;
      }
      continue;
    }
    // Leaf. A placeholder (`<file.yaml>`) is the reader's to fill in and is always
    // fine. A literal is a claim about the real surface, so it is allowed only
    // where the usage line shows a positional — this is the `slot acquire cargo`
    // class, which clap rejects at runtime and a chain-only check cannot see.
    if (node.positionals === 0 && !/^<[^>]+>$/.test(t)) {
      const where = `${OWN_BIN} ${path.join(" ")}`.trimEnd();
      errors.push(`unexpected positional \`${t}\` on \`${where}\` (its usage takes none)`);
    }
    positionalsSeen++;
  }

  // Missing required arguments are drift too: every token resolves and the
  // reader's copy still fails at runtime (`toron bench` with no CASE). Skipped
  // when the walk already failed, so one defect reports one error instead of a
  // cascade judged against the wrong help text.
  if (errors.length === 0 && node) {
    const where = `${OWN_BIN} ${path.join(" ")}`.trimEnd();
    for (const f of node.requiredFlags || []) {
      if (!seenFlags.has(f)) errors.push(`missing required flag --${f} for \`${where}\``);
    }
    const needed = (node.requiredPositionals || 0) - positionalsSeen;
    if (needed > 0) {
      errors.push(
        `missing required argument(s) for \`${where}\` — its usage takes ${node.requiredPositionals}`,
      );
    }
  }
  return errors;
}

// Counts written in the guides are claims about the emitted catalog: "38 tools"
// is only true until the binary changes. Read the catalog back and fail on any
// number that no longer matches, so a hand-typed count cannot age silently.
// Skipped loudly when the binary cannot emit the catalog.
function catalogCountDrift(entries) {
  const r = spawnSync(OWN_BIN, ["catalog", "--json"], { encoding: "utf8" });
  if (r.status !== 0 || !r.stdout) {
    console.error(`skip catalog counts: \`${OWN_BIN} catalog --json\` did not emit (exit ${r.status})`);
    return;
  }
  let catalog;
  try {
    catalog = JSON.parse(r.stdout);
  } catch {
    console.error(`skip catalog counts: \`${OWN_BIN} catalog --json\` did not emit JSON`);
    return;
  }
  const real = {
    tools: catalog.tools?.length ?? 0,
    resources: catalog.resources?.length ?? 0,
  };
  for (const { rel, body } of entries) {
    for (const [n, line] of body.split("\n").entries()) {
      for (const [what, want] of Object.entries(real)) {
        const m = line.match(new RegExp(`(\\d+)\\s+${what}\\b`));
        if (m && Number(m[1]) !== want) {
          console.error(
            `FAIL ${rel}: ${n + 1}: the guide says \`${m[0]}\` and \`${OWN_BIN} catalog --json\` emits ${want} — in \`${line.trim().slice(0, 100)}\``,
          );
          failed = 1;
        }
      }
    }
  }
}

// --------------------------------------------------------------- block walking

function blocks(body) {
  const out = [];
  let cur = null;
  for (const [i, line] of body.split("\n").entries()) {
    if (line.startsWith("```")) {
      if (cur) { out.push(cur); cur = null; continue; }
      cur = { lang: line.slice(3).trim().split(/\s+/)[0], startLine: i + 1, lines: [] };
      continue;
    }
    if (cur) cur.lines.push(line);
  }
  if (cur) out.push({ ...cur, unterminated: true });
  return out;
}

function check(file, surface) {
  const errors = [];
  const warns = [];
  const src = readFileSync(file, "utf8");
  if (!src.startsWith("---\n")) return { errors: ["missing frontmatter"], warns };
  const close = src.indexOf("\n---\n", 4);
  if (close === -1) return { errors: ["unterminated frontmatter"], warns };
  const body = src.slice(close + 5);

  // --- 1. structure (unchanged behaviour) ---
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
    // JSX-looking prose is a defect in these repos (the guides render as plain
    // markdown), but a placeholder is not JSX. `<Pin>`, `<ID>` and `<NAME>` are
    // legitimate inside code spans and after flags, so those regions come out
    // before the test; only a bare component name left in prose trips it.
    const prose = line
      .replace(/`[^`]*`/g, "")
      .replace(/--[a-z0-9-]+\s+<[A-Za-z0-9_.-]+>/g, "");
    if (/<[A-Z][A-Za-z]*[\s/>]/.test(prose)) errors.push(`${n}: JSX component without an import surface`);
  }
  if (fence !== 0) errors.push("unbalanced code fence");

  // --- 2. completeness ---
  const bs = blocks(body);
  const bashBlocks = bs.filter((b) => b.lang === "bash");
  for (const [idx, block] of bashBlocks.entries()) {
    const after = bs.filter((b) => b.startLine > block.startLine);
    const nextBash = bashBlocks[idx + 1];
    const between = after.filter((b) => !nextBash || b.startLine < nextBash.startLine);
    if (!between.some((b) => b.lang === "text")) {
      errors.push(`${block.startLine}: bash block with no \`\`\`text block after it — a command with no shown output is an unverified claim`);
    }
  }
  if (bashBlocks.length > 0 && !FAILS.test(body)) {
    errors.push("no failure mode; add an `If it fails:` paragraph to each step that can fail");
  }

  // --- 3. surface ---
  if (surface) {
    for (const block of bashBlocks) {
      for (const cmd of commandLines(block.lines.join("\n"))) {
        if (cmd.tokens[0] !== OWN_BIN) {
          warns.push(`${block.startLine}: skipped \`${cmd.tokens[0]}\` (not this repo's binary)`);
          continue;
        }
        for (const e of verifyCommand(cmd.tokens, surface)) {
          errors.push(`${block.startLine}: ${e} — in \`${cmd.text}\``);
        }
      }
    }
  }

  return { errors, warns };
}

// ------------------------------------------------------------------------ main

let failed = 0;
let surface = null;
if (process.env.GUIDES_SKIP_CLI === "1") {
  console.error(`SKIP surface check (GUIDES_SKIP_CLI=1): ${OWN_BIN} commands are NOT verified`);
} else {
  const probe = spawnSync(OWN_BIN, ["--help"], { encoding: "utf8" });
  if (probe.error || probe.status !== 0) {
    console.error(
      `FAIL cannot run \`${OWN_BIN} --help\`, so guide commands cannot be verified.\n` +
        `     install the binary, or set GUIDES_SKIP_CLI=1 and say why in the report.`,
    );
    process.exit(1);
  }
  surface = extractSurface(OWN_BIN);

  // Self-test: a gate that cannot fail is theater. This one shipped broken once
  // (the binary name was never stripped, so every command reported clean) and
  // the only reason it was caught is that a deliberate probe was run by hand.
  // Now the check refuses to run if it cannot reject an impossible command.
  const probeErrors = verifyCommand(
    [OWN_BIN, "__definitely-not-a-subcommand", "--__definitely-not-a-flag"],
    surface,
  );
  if (probeErrors.length === 0) {
    console.error(
      `FAIL the surface check is not biting: it accepted ${OWN_BIN} __definitely-not-a-subcommand.\n` +
        `     refuse to trust this run. Fix extractSurface/verifyCommand first.`,
    );
    process.exit(1);
  }

  // Second probe: a leaf whose usage takes no positional must reject a literal
  // one. The chain check alone walks `slot acquire` happily and never looks at
  // `cargo`, which is exactly how a wrong literal shipped green.
  const bareLeaf = [...surface.entries()].find(
    ([, n]) => n.children.size === 0 && n.positionals === 0,
  );
  if (!bareLeaf) {
    console.error(`skip self-test: ${OWN_BIN} has no positional-free leaf to probe`);
  } else {
    const probePath = bareLeaf[0].split(" ").filter(Boolean);
    const positionalProbe = verifyCommand([OWN_BIN, ...probePath, "bogus-positional"], surface);
    if (!positionalProbe.some((e) => e.includes("unexpected positional"))) {
      console.error(
        `FAIL the positional rule is not biting: it accepted ${OWN_BIN} ${probePath.join(" ")} bogus-positional.\n` +
          `     refuse to trust this run. Fix the usage-line parse in extractSurface first.`,
      );
      process.exit(1);
    }
  }
  console.log(`surface  ${OWN_BIN}: ${surface.size} command paths (self-test ok)`);
}

if (files.length === 0) {
  console.error("no .mdx guides found");
  failed = 1;
}
const bodies = [];
for (const f of files) {
  const rel = relative(root, f);
  const { errors, warns } = check(f, surface);
  for (const w of warns) console.error(`skip ${rel}: ${w}`);
  if (errors.length === 0) console.log(`ok   ${rel}`);
  else { failed = 1; for (const e of errors) console.error(`FAIL ${rel}: ${e}`); }
  bodies.push({ rel, body: readFileSync(f, "utf8") });
}
// ---------------------------------------------------------------- index coverage
//
// A guide nobody can reach is a guide nobody reads. `index.mdx` is this
// repository's own navigation for `docs/guides/`, and a guide that exists but
// has no row there is invisible to anyone who arrives through the index.
//
// This is the same class of drift the surface check above hunts: a file on
// disk that the world cannot see. It shipped uncaught here for a long time,
// which is why it exists. toron was missing tool-authoring, flywheel was
// missing integration, jev-w2-enable, judgment-packs, and stack, and both
// indexes were internally consistent while omitting a third of a plane's
// documentation in one case.
//
// Pure function over plain data so the self-test can prove it bites.
function indexGaps(guideNames, indexText) {
  const linked = new Set(
    [...indexText.matchAll(/\]\(\.\/([a-z0-9-]+)(?:\.mdx)?\)/g)].map((m) => m[1]),
  );
  return guideNames.filter((name) => !linked.has(name)).sort();
}

// The other direction. A row pointing at a page that does not exist is a dead
// link in the index, which is the same reader-facing fault as a missing row:
// the index promises a guide and cannot deliver it. Checking only one
// direction would have let the negative test above pass with a bogus row added.
function indexDanglingLinks(guideNames, indexText) {
  const present = new Set(guideNames);
  return [...indexText.matchAll(/\]\(\.\/([a-z0-9-]+)(?:\.mdx)?\)/g)]
    .map((m) => m[1])
    .filter((name) => !present.has(name))
    .sort();
}

// The extension is optional on purpose: two of the four repositories write
// `./quick-start` and two write `./quick-start.mdx`, and both resolve. A check
// that rejected the second spelling would have been a gate reporting style
// errors as missing documentation.
if (indexGaps(["a", "b"], "| [a](./a) | x |\n| [b](./b.mdx) | y |\n").length !== 0) {
  console.error("FAIL the index-coverage check rejects a valid link, refusing to trust this run");
  process.exit(1);
}
if (!indexGaps(["a", "b"], "| [a](./a) | x |\n").includes("b")) {
  console.error("FAIL the index-coverage check misses an unlisted guide, refusing to trust this run");
  process.exit(1);
}
if (indexDanglingLinks(["a", "b"], "| [a](./a) | x |\n| [b](./b) | y |\n").length !== 0) {
  console.error("FAIL the dangling-link check rejects valid rows, refusing to trust this run");
  process.exit(1);
}
if (!indexDanglingLinks(["a"], "| [a](./a) | x |\n| [ghost](./ghost) | y |\n").includes("ghost")) {
  console.error("FAIL the dangling-link check misses a row with no page, refusing to trust this run");
  process.exit(1);
}

const indexFile = join(root, "index.mdx");
if (!existsSync(indexFile)) {
  console.error(`FAIL ${relative(root, indexFile)} is absent, so no guide is reachable from the index`);
  failed = 1;
} else {
  const gaps = indexGaps(
    files.map((f) => basename(f, ".mdx")).filter((n) => n !== "index"),
    readFileSync(indexFile, "utf8"),
  );
  const dangling = indexDanglingLinks(
    files.map((f) => basename(f, ".mdx")).filter((n) => n !== "index"),
    readFileSync(indexFile, "utf8"),
  );
  if (dangling.length > 0) {
    failed = 1;
    for (const name of dangling) {
      console.error(
        `FAIL docs/guides/index.mdx links ./${name}, and there is no docs/guides/${name}.mdx. remove the row or add the guide.`,
      );
    }
  }
  if (gaps.length > 0) {
    failed = 1;
    for (const name of gaps) {
      console.error(
        `FAIL docs/guides/${name}.mdx is not listed in docs/guides/index.mdx, so it is unreachable from the guides index. add a row: | [${name}](./${name}) | when to read it |`,
      );
    }
  } else {
    const total = files.filter((f) => basename(f) !== "index.mdx").length;
    console.log(`index  ${total} guide(s) listed in docs/guides/index.mdx`);
  }
}

if (surface) catalogCountDrift(bodies);
process.exit(failed);
