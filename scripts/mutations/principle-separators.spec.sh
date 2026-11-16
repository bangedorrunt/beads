#!/usr/bin/env bash
# Sourced by scripts/verify-mutations.sh.
# bd-nwfj7: the em dash is canonical, but ` - ` and `: ` are what people type.
# All three must parse to one citation, and a refusal must name all of them.

CARGO_ARGS=(--lib)
TEST_FILTER=(bd_principle_separator_tests)

UPDATE=src/cli/commands/update.rs

mutation "P1  only the em dash parses again" \
    file=$UPDATE \
    expect=all_three_separators_parse_to_the_same_citation \
    find='pub(crate) const PRINCIPLE_SEPARATORS: [&str; 3] = ["—", " - ", ": "];' \
    replace='pub(crate) const PRINCIPLE_SEPARATORS: [&str; 1] = ["—"];'

mutation "P2  the refusal stops naming the ascii forms" \
    file=$UPDATE \
    expect=the_refusal_names_the_accepted_forms \
    find="(also accepted: 'name - decision'," \
    replace="(em dash only,"
