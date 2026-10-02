#!/usr/bin/env bash
# Prove that named tests can actually fail, by breaking the code they cover.
#
# A test nobody has seen fail is not evidence. This takes a spec of mutations —
# each one a single edit that breaks one decision a test claims to cover —
# applies them one at a time, and reports what each one proved:
#
#   RED            the named test went red, so it really exercises that decision
#   VACUOUS        the suite stayed green: that test cannot fail
#   BUILD-BROKEN   the mutation did not compile, so nothing was proved
#   RED-OTHER      the suite went red but not on the named test
#   NEEDLE-MISS    the find string was not in the file
#
# Usage:
#   scripts/verify-mutations.sh --self-test                 # no cargo, instant
#   scripts/verify-mutations.sh scripts/mutations/<name>.spec.sh --cargo mbx
#
# The spec is a shell file of `mutation` calls, so there is no parser layer and
# the needles stay greppable:
#
#   CARGO_ARGS=(-p chiebukuro --lib)
#   TEST_FILTER=(cli::services_cmd::tests)
#   mutation "M1  threshold inverted" \
#     file=src/cli/services_cmd.rs \
#     expect=a_recent_queue_is_a_gauge_not_an_alarm \
#     find='b.oldest_age_secs > BACKLOG_STALE_SECS' \
#     replace='b.oldest_age_secs < BACKLOG_STALE_SECS'
#
# Three ways the first version of this got it wrong, all fixed here, all of them
# able to report a confident wrong answer:
#
#   * It matched cargo's own trailing `error: test failed, to rerun pass ...`
#     after a *successful* compile, so every genuine RED was filed as
#     BUILD-BROKEN. Only "could not compile" and "error[E" are build breaks.
#   * It restored mutated files preserving mtime, and the build cache then
#     served a binary compiled from the mutation, so the post-restore control
#     reported a verdict about code that was not on disk. Every write and every
#     restore touches the file.
#   * It was killed mid-run and left a mutation applied to the tree. Restore
#     runs from a signal trap and from a final trap.
#
# Bash 3.2 compatible: /bin/bash on a stock macOS box is 3.2 even where a newer
# one is first on PATH, and no other script in this repo needs bash 4.

set -euo pipefail

CARGO_BIN="${CARGO_BIN:-cargo}"
REPO="."
SELF_PATH="$0"
case "$SELF_PATH" in
    /*) ;;
    *) SELF_PATH="$PWD/$SELF_PATH" ;;
esac

VERDICT_RED="RED"
VERDICT_VACUOUS="VACUOUS"
VERDICT_BUILD="BUILD-BROKEN"
VERDICT_OTHER="RED-OTHER"
VERDICT_MISS="NEEDLE-MISS"
VERDICT_NO_RESULTS="NO-RESULTS"
VERDICT_DEAD="DEAD-CONFIRMED"
VERDICT_ALIVE="STILL-LOAD-BEARING"

# Parallel indexed arrays; bash 3.2 has no associative ones.
M_LABEL=()
M_FILE=()
M_FIND=()
M_REPLACE=()
M_EXPECT=()
# 1 when this mutation must NOT change behaviour: it exists to prove a guard is
# dead, and green is the answer rather than a failure.
M_GREEN=()

# Set by the spec before sourcing, defaulted after. Deliberately left UNSET
# rather than initialised empty: under `set -u`, bash 3.2 treats `${arr[@]}` on
# an empty array as unbound, so an empty initialiser turns a legality check into
# a crash. `${name+set}` on the name itself is the portable test.

# --- the harness proper ---------------------------------------------------

# Name what one mutation's test run proved.
#
# `expect` is the test that must go red. A suite that fails without naming it
# is not a pass: the mutation broke something else, or broke the build in a way
# we did not anticipate, and either way the named test was not shown to detect
# the change.
classify() {
    local out="$1" expect="$2"
    case "$out" in
        *"could not compile"* | *"error[E"*) printf '%s\n' "$VERDICT_BUILD"; return ;;
    esac
    # Cargo failing before it runs anything -- a bad package name, a missing
    # toolchain -- prints no `test result:` line at all. That is NOT a passing
    # suite, and treating it as one made a broken command report "control green"
    # and every mutation VACUOUS: the harness agreeing with itself while
    # measuring nothing.
    case "$out" in
        *"test result:"*) ;;
        *) printf '%s\n' "$VERDICT_NO_RESULTS"; return ;;
    esac
    case "$out" in
        *"test result: FAILED"*)
            case "$out" in
                *"$expect"*) printf '%s\n' "$VERDICT_RED" ;;
                *) printf '%s\n' "$VERDICT_OTHER" ;;
            esac
            ;;
        *) printf '%s\n' "$VERDICT_VACUOUS" ;;
    esac
}

# Called by the spec. Named key=value arguments so a 17-mutation spec reads as
# prose instead of five positional slots nobody can hold in their head.
mutation() {
    local label="$1" file="" find="" replace="" expect="" green=0
    shift
    while [ "$#" -gt 0 ]; do
        case "$1" in
            file=*) file="${1#file=}" ;;
            find=*) find="${1#find=}" ;;
            replace=*) replace="${1#replace=}" ;;
            expect=*) expect="${1#expect=}" ;;
            expect_green) green=1 ;;
            expect_green=*) green="${1#expect_green=}" ;;
            *) echo "unknown mutation argument: $1" >&2; return 2 ;;
        esac
        shift
    done
    for pair in "file:$file" "find:$find" "replace:$replace" "expect:$expect"; do
        if [ -z "${pair#*:}" ]; then
            echo "mutation '$label' is missing a ${pair%%:*}= argument" >&2
            return 2
        fi
    done
    M_LABEL+=("$label")
    M_FILE+=("$file")
    M_FIND+=("$find")
    M_REPLACE+=("$replace")
    M_EXPECT+=("$expect")
    M_GREEN+=("$green")
}

load_spec() {
    local spec="$1"
    [ -f "$spec" ] || { echo "no such spec: $spec" >&2; exit 2; }
    # shellcheck disable=SC1090
    . "$spec"
    # Default only when the spec left it empty. A flag saying "the spec did not
    # set this" was wrong the moment a spec set a DIFFERENT value: the harness
    # overwrote it with its own default and ran the wrong package, which fails
    # with no `test result:` line and therefore read as a passing control.
    if [ -z "${CARGO_ARGS+set}" ]; then
        CARGO_ARGS=(-p chiebukuro --lib)
    fi
    [ -n "${TEST_FILTER+set}" ] || {
        echo "the spec needs TEST_FILTER: a mutation with no filter runs the whole" >&2
        echo "suite once per mutation, which is how a run turns into hours" >&2
        exit 2
    }
}

# Every file any mutation touches, deduplicated, relative to $REPO.
touched_files() {
    local seen="" i f
    for i in "${!M_FILE[@]}"; do
        f="${M_FILE[$i]}"
        case "$seen" in
            *"|$f|"*) ;;
            *) seen="$seen|$f|"; printf '%s\n' "$f" ;;
        esac
    done
}

SNAP_DIR=""
SNAP_LIST=""
snapshot_save() {
    SNAP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/verify-mutations.XXXXXX")"
    SNAP_LIST=""
    local f base
    while IFS= read -r f; do
        [ -n "$f" ] || continue
        [ -f "$REPO/$f" ] || { echo "spec names a file that does not exist: $f" >&2; exit 2; }
        base="$SNAP_DIR/$(printf '%s' "$f" | tr '/' '_')"
        # cp, never cp -p: a copy that carries mtime lets the build cache serve
        # a binary compiled from the mutation.
        cp "$REPO/$f" "$base"
        SNAP_LIST="$SNAP_LIST$f"$'\n'
    done < <(touched_files)
}

snapshot_restore() {
    local f
    while IFS= read -r f; do
        [ -n "$f" ] || continue
        cp "$SNAP_DIR/$(printf '%s' "$f" | tr '/' '_')" "$REPO/$f"
        touch "$REPO/$f"
    done <<<"$SNAP_LIST"
}

snapshot_discard() {
    [ -n "$SNAP_DIR" ] && rm -rf "$SNAP_DIR"
    SNAP_DIR=""
}

# Run the suite. A non-zero exit is data here, not a failure of the harness, so
# `set -e` is lifted around it rather than letting the exit propagate.
run_tests() {
    set +e
    (cd "$REPO" && "$CARGO_BIN" test "${CARGO_ARGS[@]}" -- "${TEST_FILTER[@]}" 2>&1)
    set -e
}

# --- self-test ------------------------------------------------------------

# Counts failures into SELF_FAILURES; prints one line per case.
SELF_FAILURES=0
expect_verdict() {
    local got="$1" want="$2" why="$3"
    if [ "$got" = "$want" ]; then
        printf '  ok    %-12s %s\n' "$got" "$why"
    else
        printf '  FAIL  got %s, want %s — %s\n' "$got" "$want" "$why"
        SELF_FAILURES=$((SELF_FAILURES + 1))
    fi
}

self_test() {
    SELF_FAILURES=0

    # A failing suite still prints `error:`. That is the trap: it must read RED.
    local compiled_and_failed
    compiled_and_failed="$(cat <<'EOF'
running 1 test
test result: FAILED. 3 passed; 1 failed
failures:
    cli::services_cmd::tests::the_worker_is_not_claiming
error: test failed, to rerun pass `-p chiebukuro --lib`
EOF
)"
    local build_broke
    build_broke="$(cat <<'EOF'
error: argument never used
  --> src/x.rs:107:17
error: could not compile `chiebukuro` (lib test) due to 1 previous error
EOF
)"
    local all_green="test result: ok. 944 passed; 0 failed; 0 ignored"
    # Cargo never got as far as running anything. This is the case that made a
    # broken package name look like a passing control.
    local never_ran
    never_ran="$(cat <<'EOF'
error: package ID specification `chiebukuro` did not match any packages
EOF
)"
    local failed_elsewhere
    failed_elsewhere="$(cat <<'EOF'
test result: FAILED. 8 passed; 1 failed
failures:
    some::other::test
error: test failed, to rerun pass `-p chiebukuro --lib`
EOF
)"

    expect_verdict "$(classify "$compiled_and_failed" the_worker_is_not_claiming)" \
        "$VERDICT_RED" "a real failure must not be mistaken for a broken build"
    expect_verdict "$(classify "$compiled_and_failed" some_other_test)" \
        "$VERDICT_OTHER" "red without the named test proves nothing about it"
    expect_verdict "$(classify "$build_broke" the_worker_is_not_claiming)" \
        "$VERDICT_BUILD" "a compile error is not a test result"
    expect_verdict "$(classify "$(printf 'error[E0308]: mismatched types\ncould not compile')" anything)" \
        "$VERDICT_BUILD" "a coded rustc error is a build break"
    expect_verdict "$(classify "$all_green" the_worker_is_not_claiming)" \
        "$VERDICT_VACUOUS" "a green suite means the test cannot fail"
    expect_verdict "$(classify "$never_ran" the_worker_is_not_claiming)" \
        "$VERDICT_NO_RESULTS" "a cargo invocation that ran no tests is not a passing suite"
    expect_verdict "$(classify "" the_worker_is_not_claiming)" \
        "$VERDICT_NO_RESULTS" "empty output is not a passing suite either"
    expect_verdict "$(classify "$failed_elsewhere" the_worker_is_not_claiming)" \
        "$VERDICT_OTHER" "a different test failing does not clear this one"

    # A harness that cannot restore is worse than no harness: it leaves the
    # tree sabotaged. Prove the snapshot round-trips and does not carry a stamp.
    local tmp f backup oldref restored carried copy2_preserves
    tmp="$(mktemp -d "${TMPDIR:-/tmp}/vm-self.XXXXXX")"
    mkdir -p "$tmp/repo/src"
    printf 'original\n' >"$tmp/repo/src/a.rs"
    REPO="$tmp/repo"
    f="src/a.rs"
    M_FILE=("$f")
    snapshot_save
    printf 'MUTATED\n' >"$tmp/repo/src/a.rs"
    # Age the backup the way a preserving copy would stamp it, and keep a
    # reference carrying the same old stamp to compare against. If restore
    # carried it through, the cache would serve a binary built from the
    # mutation and the next control would report about code not on disk.
    backup="$SNAP_DIR/src_a.rs"
    oldref="$tmp/old-ref"
    touch -t 202001010000 "$backup" "$oldref"
    snapshot_restore
    restored="$(cat "$tmp/repo/src/a.rs")"
    # `-nt` is strictly newer: a carried stamp leaves the target exactly equal to
    # the old stamp, which is not newer than the reference, so it reads carried.
    if [ "$tmp/repo/src/a.rs" -nt "$oldref" ]; then carried=no; else carried=yes; fi
    # The converse, so the check above cannot pass by accident: a copy that does
    # preserve mtime really does leave the target stale.
    cp -p "$backup" "$tmp/repo/src/a.rs"
    if [ "$tmp/repo/src/a.rs" -nt "$oldref" ]; then
        copy2_preserves=no
    else
        copy2_preserves=yes
    fi
    snapshot_discard
    rm -rf "$tmp"
    REPO="."

    if [ "$restored" = "original" ]; then
        printf '  ok    %-12s %s\n' restore "the file comes back byte-for-byte"
    else
        printf '  FAIL  restore read %s\n' "$restored"
        SELF_FAILURES=$((SELF_FAILURES + 1))
    fi
    if [ "$carried" = yes ]; then
        printf '  FAIL  mtime    restore carries the backup stamp; a stale artifact survives it\n'
        SELF_FAILURES=$((SELF_FAILURES + 1))
    elif [ "$copy2_preserves" = no ]; then
        printf '  FAIL  mtime    cp -p did not preserve the stamp, so the check is measuring nothing\n'
        SELF_FAILURES=$((SELF_FAILURES + 1))
    else
        printf '  ok    %-12s %s\n' mtime "restore drops the backup stamp; cp -p keeps it, so the check discriminates"
    fi

    # The spec-loading path. A harness that ignores a spec's CARGO_ARGS runs the
    # wrong package, cargo errors before running a test, and the result reads as
    # a passing control — so this case exists to keep a spec's own arguments.
    local tmpspec
    tmpspec="$(mktemp "${TMPDIR:-/tmp}/vm-spec.XXXXXX")"
    cat >"$tmpspec" <<'SPEC'
CARGO_ARGS=(--lib)
TEST_FILTER=(some_test)
mutation "x" file=src/a.rs find=a replace=b expect=c
SPEC
    unset CARGO_ARGS TEST_FILTER
    if load_spec "$tmpspec" 2>/dev/null; then
        if [ "${CARGO_ARGS[*]}" = "--lib" ]; then
            printf '  ok    %-12s %s\n' spec "the CARGO_ARGS a spec sets survives the default"
        else
            printf '  FAIL  spec      CARGO_ARGS became [%s], expected [--lib]\n' "${CARGO_ARGS[*]}"
            SELF_FAILURES=$((SELF_FAILURES + 1))
        fi
    else
        printf '  FAIL  spec      load_spec rejected a valid spec\n'
        SELF_FAILURES=$((SELF_FAILURES + 1))
    fi
    rm -f "$tmpspec"
    # The self-test exits straight after this, so unsetting beats saving.
    unset CARGO_ARGS TEST_FILTER

    # And end to end: a cargo that never runs a test must NOT report a green
    # control. This is the bug that made a wrong package name look like a
    # passing suite on every one of seventeen mutations.
    local tmpbin tmprepo
    tmpbin="$(mktemp -d "${TMPDIR:-/tmp}/vm-bin.XXXXXX")"
    tmprepo="$(mktemp -d "${TMPDIR:-/tmp}/vm-repo.XXXXXX")"
    mkdir -p "$tmprepo/src"
    printf 'x\n' >"$tmprepo/src/a.rs"
    cat >"$tmpbin/fakecargo" <<'STUB'
#!/usr/bin/env bash
echo "error: package ID specification \`nope\` did not match any packages" >&2
exit 101
STUB
    chmod +x "$tmpbin/fakecargo"
    cat >"$tmprepo/spec.sh" <<'SPEC'
CARGO_ARGS=(--lib)
TEST_FILTER=(some_test)
mutation "x" file=src/a.rs find=x replace=y expect=some_test
SPEC
    local rc=0
    # $SELF_PATH, not $0: this runs from a different cwd and a relative $0
    # would not resolve, which would make the case pass for the wrong reason.
    local outfile="$tmpbin/out.txt"
    : >"$outfile"
    (cd "$tmprepo" && bash "$SELF_PATH" spec.sh --repo "$tmprepo" --cargo "$tmpbin/fakecargo") >"$outfile" 2>&1 || rc=$?
    # Assert on the MESSAGE as well as the exit status. Exit non-zero on its own
    # proves nothing: with the control check deleted the harness still fails, one
    # step later, because no mutation can be RED. Naming the guard is what makes
    # this case able to see that particular removal.
    if [ "$rc" -ne 0 ] && grep -q "control is not green" "$outfile"; then
        printf '  ok    %-12s %s\n' control "a cargo that ran no tests is refused at the control"
    elif [ "$rc" -eq 0 ]; then
        printf '  FAIL  control  the harness reported success while cargo ran nothing\n'
        SELF_FAILURES=$((SELF_FAILURES + 1))
    else
        printf '  FAIL  control  the harness failed, but not at the control check\n'
        SELF_FAILURES=$((SELF_FAILURES + 1))
    fi
    rm -rf "$tmpbin" "$tmprepo"

    if [ "$SELF_FAILURES" -ne 0 ]; then
        printf '\nself-test FAILED: %s case(s)\n' "$SELF_FAILURES"
        return 1
    fi
    printf '\nself-test pass\n'
}

# --- main -----------------------------------------------------------------

on_signal() {
    snapshot_restore
    snapshot_discard
    echo "interrupted; working copy restored" >&2
    exit 130
}

usage() {
    sed -n '2,30p' "$0" | sed 's/^# \{0,1\}//'
}

main() {
    local spec=""
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --self-test) self_test; exit $? ;;
            --cargo) CARGO_BIN="$2"; shift ;;
            --repo) REPO="$2"; shift ;;
            -h | --help) usage; exit 0 ;;
            -*) echo "unknown flag: $1" >&2; usage >&2; exit 2 ;;
            *) spec="$1" ;;
        esac
        shift
    done
    if [ -z "$spec" ]; then
        echo "a spec file is required unless --self-test is given" >&2
        usage >&2
        exit 2
    fi

    load_spec "$spec"

    trap on_signal INT TERM
    snapshot_save
    trap on_signal EXIT

    printf 'control (unmutated): ...\n'
    local before
    before="$(run_tests)"
    if [ "$(classify "$before" "")" != "$VERDICT_VACUOUS" ]; then
        echo "  control is not green — fix that before trusting any verdict" >&2
        printf '%s\n' "$before" | tail -40 >&2
        snapshot_restore
        snapshot_discard
        trap - EXIT
        exit 1
    fi
    echo "control green"

    local i verdict proven=0 dead=0 alive=0 total="${#M_LABEL[@]}"
    for i in $(seq 0 $((total - 1))); do
        snapshot_restore
        if ! grep -qF -- "${M_FIND[$i]}" "$REPO/${M_FILE[$i]}"; then
            printf '%-52s %s\n' "${M_LABEL[$i]}" "$VERDICT_MISS"
            continue
        fi
        # -0 slurps the file so a needle spanning lines still matches; /e
        # evaluates the replacement as a string, so a $ or \ in it is literal
        # rather than a backreference.
        NEEDLE="${M_FIND[$i]}" REPLACEMENT="${M_REPLACE[$i]}" \
            perl -0pi -e 's/\Q$ENV{NEEDLE}\E/$ENV{REPLACEMENT}/e' "$REPO/${M_FILE[$i]}"
        touch "$REPO/${M_FILE[$i]}"
        verdict="$(classify "$(run_tests)" "${M_EXPECT[$i]}")"
        # A green-expected mutation asks the opposite question: this guard is
        # supposed to be dead, so a green suite is the ANSWER and a red one is
        # the finding. Scoring it like a normal mutation would report the whole
        # point of it as a failing test.
        if [ "${M_GREEN[$i]}" = 1 ]; then
            case "$verdict" in
                "$VERDICT_VACUOUS") verdict="$VERDICT_DEAD"; dead=$((dead + 1)) ;;
                "$VERDICT_RED") verdict="$VERDICT_ALIVE" ;;
            esac
            printf '%-52s %-13s (guard is dead)\n' "${M_LABEL[$i]}" "$verdict"
            continue
        fi
        printf '%-52s %-13s (%s)\n' "${M_LABEL[$i]}" "$verdict" "${M_EXPECT[$i]}"
        case "$verdict" in
            "$VERDICT_RED") proven=$((proven + 1)) ;;
            *)
                echo "    !! not proof"
                case "$verdict" in
                    "$VERDICT_VACUOUS") echo "       the suite stayed green; this test cannot fail" ;;
                    "$VERDICT_BUILD") echo "       the mutation did not build; the needle is probably not arity-preserving" ;;
                    "$VERDICT_OTHER") echo "       a different test went red; the named one did not" ;;
                    "$VERDICT_NO_RESULTS") echo "       cargo ran no tests at all; the spec's CARGO_ARGS is probably wrong" ;;
                esac
                ;;
        esac
    done

    snapshot_restore
    local after restored_ok
    after="$(run_tests)"
    [ "$(classify "$after" "")" = "$VERDICT_VACUOUS" ] && restored_ok=yes || restored_ok=no
    printf '\ncontrol after restore: %s\n' "$([ "$restored_ok" = yes ] && echo GREEN || echo RED)"
    snapshot_discard
    trap - EXIT

    if [ "$dead" -gt 0 ] || [ "$alive" -gt 0 ]; then
        printf '%s guard(s) confirmed dead, %s still load-bearing\n' "$dead" "$alive"
    fi
    printf '%s/%s mutations proved red\n' "$proven" "$total"
    [ "$restored_ok" = yes ] && [ "$proven" -eq "$total" ]
}

main "$@"