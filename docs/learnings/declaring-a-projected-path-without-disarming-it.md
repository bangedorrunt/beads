# Declaring a projected path without quietly disarming it

The commit guard's bead-hunk rule scans the **added** lines of the staged
diff for issue-id tokens that the commit message does not cite. That is
right for work a repository performs and wrong for prose it vendors: the
guard already exempts paths declared `linguist-generated=true` in
`.gitattributes`, and beads declares its re-captured insta goldens that
way.

Three things each looked correct and each did nothing. All three cost a
commit, and none of them would have been obvious from a passing guard
check.

## 1. A dry run without `--msg-file` never exercises the rule

`toron guard check` skips bead-hunk attribution entirely when no message
is supplied — the commit-msg hook is what passes `--msg-file`. So
running the check by hand and seeing `"unattributed_bead_tokens": []`
proves nothing at all. It is the shape you get whether the exemption
works, whether the declaration is inert, and whether the guard is
definitely broken.

Always reproduce with the flag:

```bash
printf 'your subject\n' > /tmp/msg.txt
AGENT_NAME=<pin> TORON_GUARD_STRICT=1 \
  toron guard check --repo "$PWD" --project <slug> --msg-file /tmp/msg.txt
```

A guard check that cannot fail is worse than no guard check, because it
converts into evidence.

## 2. A leading slash makes the pattern inert

Git anchors a `.gitattributes` pattern that starts with `/` to the
directory holding the file. The guard's matcher does not: it compares
the pattern against the repo-relative path directly. So
`/tests/snapshots/**` matches nothing, while `tests/snapshots/**`
matches.

This is the same failure the projected-path work already fixed once
elsewhere in this guard — an exemption that is silently armed and inert.
`projected_paths_in` will happily list your pattern back at you, so the
display list is not evidence either.

## 3. Naming the example id in the declaration re-triggers the rule

`.gitattributes` is itself staged on the commit that adds it. A comment
that spells out the offending example id is therefore an added line
carrying a token, and the guard blocks the very commit meant to stop
that. Describe the id instead of quoting it.

The same trap applies to the **commit message**: citing the token to
satisfy the check is a false attribution, claiming work on a bead that
never existed. Both the comment and the message need to stay clear of
it.

## What the declaration does and does not buy

Projected paths are exempt from the bead-hunk **attribution** scan only.
They still need a lease. The guard is asking two different questions:
"whose work does this id describe" (answered by the declaration) and
"who may write this file" (answered by the reservation). Expecting the
declaration to cover both produces a confusing second block that names
the path as unreserved.

The declaration is also bounded on purpose: one that would reach compiled
source is refused outright, so it cannot be used to switch the rule off
across a crate.

## Prove the exemption, do not assume it

Three checks, all cheap:

1. With the declaration, the token is absent from
   `unattributed_bead_tokens`.
2. Without it (`git stash push -- .gitattributes`), the token is
   present. If step 1 alone is your evidence, a pattern typo looks
   exactly like a working fix.
3. A token injected into an ordinary source file is still blocked, and
   unblocked once the message cites it. Without this, "exempt the
   goldens" and "stop checking" are indistinguishable from the outside.
