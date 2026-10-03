#!/usr/bin/env bash
# Fixture-facing wrapper around the real `br` binary.
#
# `br create` is fail-closed (ADR-0001): a seed needs a description, a VERIFY
# command, and — at P<=2 — a principles citation. Every `corrupt.sh` here
# predates that gate and only cares about the workspace state it plants, so
# the 59 seed creates across the suite were all failing at the plant step
# with exit 4 before they could plant anything.
#
# The Rust harnesses solve this at their single argv funnel
# (`tests/common/cli.rs::gate_create_args`). This is the shell equivalent:
# one choke point, no per-fixture edits, and a fixture that DOES spell out
# its brief keeps its own values.
#
# Everything except `create` is passed through byte-for-byte.

set -uo pipefail

real_bin="${BEADS_UNDER_TEST_BIN:?BEADS_UNDER_TEST_BIN must name the real br binary}"

# Only the FIRST non-flag token is the subcommand; value-taking globals
# (--db <path> etc.) must not be mistaken for one.
subcommand=""
expect_value=0
for arg in "$@"; do
    if [ "$expect_value" -eq 1 ]; then
        expect_value=0
        continue
    fi
    case "$arg" in
        --db|--actor|--lock-timeout) expect_value=1; continue ;;
        -*) continue ;;
        *) subcommand="$arg"; break ;;
    esac
done

if [ "$subcommand" != "create" ]; then
    exec "$real_bin" "$@"
fi

has_flag() {
    local needle="$1"; shift
    for arg in "$@"; do
        [ "$arg" = "$needle" ] && return 0
        case "$arg" in "$needle"=*) return 0 ;; esac
    done
    return 1
}

# `--ac judgment` IS the declaration that the bead carries no VERIFY, and it
# conflicts with --verify by design. A seed passing it is specifying the
# verify slot deliberately, so the gate must not fill it in.
ac_is_judgment=0
expect_value=0
for arg in "$@"; do
    if [ "$expect_value" -eq 1 ]; then
        expect_value=0
        [ "$arg" = "judgment" ] && ac_is_judgment=1
        continue
    fi
    case "$arg" in
        --ac) expect_value=1 ;;
        --ac=judgment) ac_is_judgment=1 ;;
    esac
done

# -p/--priority accepts 0-4 and P0-P4; default is 2.
create_priority() {
    local raw="" i=0
    local -a argv_copy=("$@")
    for ((i = 0; i < ${#argv_copy[@]}; i++)); do
        local a="${argv_copy[$i]}"
        case "$a" in
            -p|--priority) raw="${argv_copy[$((i + 1))]:-}" ;;
            -p=*|--priority=*) raw="${a#*=}" ;;
            --priority*) raw="${a#--priority}" ;;
        esac
    done
    local digits="${raw//[Pp]/}"
    [[ "$digits" =~ ^[0-4]$ ]] && printf '%s' "$digits" || printf '2'
}

brief_args=()
if ! has_flag --description "$@" && ! has_flag -d "$@" \
   && ! has_flag --body "$@" && ! has_flag --description-file "$@"; then
    brief_args+=(--description "doctor fixture seed: br create is fail-closed")
fi
if ! has_flag --verify "$@" && [ "$ac_is_judgment" -eq 0 ]; then
    brief_args+=(--verify "br list")
fi
if [ "$(create_priority "$@")" -le 2 ] && ! has_flag --principle "$@"; then
    brief_args+=(--principle "prove-it-works — doctor fixture seed")
fi

if [ "${#brief_args[@]}" -eq 0 ]; then
    exec "$real_bin" "$@"
fi
exec "$real_bin" "$@" "${brief_args[@]}"