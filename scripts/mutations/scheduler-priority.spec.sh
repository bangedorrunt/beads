# shellcheck shell=bash
# Sourced by scripts/verify-mutations.sh.
# shellcheck disable=SC2034

# The scheduler's priority contribution, and the two guards that were stacked on
# it. bd-s6b found `4_i32.saturating_sub(issue.priority.0.clamp(0, 4))`: with the
# clamp bounding priority to [0,4], the subtraction cannot leave [0,4], so the
# saturating op could never saturate. It was dead, and no test could ever have
# seen it — which is precisely why the clamp it shadowed had no coverage at all.
# The probe that established that is recorded below as history, not as a live
# mutation; the guard it names no longer exists.

CARGO_ARGS=(--lib)
TEST_FILTER=(an_out_of_range_priority)

# The clamp removed. An unbounded priority is what a row another tool wrote
# carries — validation rejects it, but `issues.jsonl` is a partial source of
# truth and the scheduler reads what is there.
mutation "P1  the priority clamp removed" \
    file=src/cli/commands/scheduler.rs \
    expect=an_out_of_range_priority_scores_as_the_nearest_legal_one \
    find='i64::from(4 - issue.priority.0.clamp(0, 4))' \
    replace='i64::from(4 - issue.priority.0)'

# The contribution inverted. P0 is the most urgent and so must score highest,
# which means the contribution is `4 - priority`. A test that only checks
# clamping would pass either way, so this is the assertion doing its job.
mutation "P3  the contribution direction inverted" \
    file=src/cli/commands/scheduler.rs \
    expect=an_out_of_range_priority_scores_as_the_nearest_legal_one \
    find='i64::from(4 - issue.priority.0.clamp(0, 4))' \
    replace='i64::from(issue.priority.0.clamp(0, 4) - 4)'