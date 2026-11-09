export function state(vars: Record<string, any>): string {
  const dep = Boolean(vars.w1_has_open_dep);
  const st = String(vars.w1_status ?? "open");
  const defer = String(vars.w1_defer ?? "none");

  const lowerWaveActive =
    (st === "open" || st === "in_progress") &&
    !dep &&
    (defer === "none" || defer === "expired");
  const minWave = lowerWaveActive ? 1 : 999;
  const w1Ready =
    (st === "open" || (st === "deferred" && defer !== "none")) &&
    !dep &&
    (defer === "none" || defer === "expired") &&
    1 <= minWave;
  const w2Ready = 2 <= minWave;
  const blockedJson = dep
    ? st !== "closed" && (st === "blocked" || dep)
    : false;

  if (w1Ready && w2Ready) {
    return `WAVE BYPASS: w1=${st}(defer=${defer}) in ready AND w2 in ready`;
  }
  if (st === "blocked" && !blockedJson) {
    return `DARK BLOCKED: w1=blocked(dep=0) missing from br blocked --json`;
  }
  if (st === "closed") {
    return "W1 Closed (W2 unlocked in ready)";
  }
  if (dep) {
    return "W1 Blocked by open dep | ready=[w2]";
  }
  return `w1=${st} (defer=${defer}) | ready=[${[
    w1Ready ? "w1" : "",
    w2Ready ? "w2" : "",
  ]
    .filter(Boolean)
    .join(",")}]`;
}

export function step(
  action: string,
  _from: Record<string, any>,
  _to: Record<string, any>,
): string | null {
  if (action === "Done") return null;
  return action;
}
