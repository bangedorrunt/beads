# A mutation harness that restores with `mv` leaves a stale binary

**Bead:** beads_rust-t1wfl (the 682-failure suite recovery).

When you prove a test can fail by mutating its source, you need the
mutated build to be the one that runs, and you need the unmutated build
back afterwards. The obvious script shape gets the second half wrong:

```bash
cp "$file" "$file.bak"
python3 - "$file" <<'PY'   # apply the mutation
...
PY
cargo test -p beads --test "$bin"          # rebuilds: file mtime is now
mv "$file.bak" "$file"                     # restore
```

`cp` stamps the backup with the time of the copy. `mv` installs it with
that stamp intact. So the restored file is **older than the artifact
cargo just built**, cargo considers the target fresh, and it never
recompiles. The next `cargo test` runs the *mutated* binary against the
*restored* source and reports green.

That is the worst possible failure mode for a mutation harness: it does
not look like a broken harness, it looks like a vacuous test. The
mutation that "stayed green" reads as evidence about the test when it is
evidence about the build.

The tell is a `git diff` that shows the correct source next to a test
failure that contradicts it. Here the committed source said
`--type custom_type` while the failure printed
`"issue_type":"bug"` — the shape the mutation had produced.

Fix: `touch` after restoring, or restore with `cat "$file.bak" > "$file"`
so the content write stamps a fresh mtime. Either way, assert the
invariant explicitly — after the last mutation, check that `git diff`
for the mutated paths is what you expect before you trust a run.

## The second trap: `--test` takes a target name, not a path

`cargo test --test tests/e2e_stale.rs` does not name a target; it fails
to find one and prints an error. A harness that reads that error as
"the test did not fail" reports every mutation green. Grep for
`test result: FAILED` specifically, and treat "no `test result:` line at
all" as a harness error rather than a pass.

Both traps fail in the same direction, which is why a mutation harness
should be built to be obviously broken when it is broken: anchor
amiguity must abort the run, a missing result line must not count as
green, and the restored tree must be verified rather than assumed.
