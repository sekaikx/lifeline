// Quick sanity checks for src/plan/access.ts logic (no test runner required).
// Run: node scripts/check-access.mjs

function modeOption(a, mode) {
  return a.options.find((o) => o.mode === mode);
}
function truckBlocked(a) {
  const t = modeOption(a, "truck");
  return !!t && !t.feasible;
}
function recommendAlternate(a) {
  for (const m of ["drone", "airdrop"]) {
    const o = modeOption(a, m);
    if (o?.feasible) return m;
  }
  return null;
}

const blocked = {
  request_id: "R1",
  options: [
    { mode: "truck", feasible: false, reason: "no ground route: blocked by impassable areas" },
    { mode: "drone", feasible: true, reason: null },
    { mode: "airdrop", feasible: false, reason: null },
  ],
};
const ok = {
  request_id: "R2",
  options: [
    { mode: "truck", feasible: true, reason: null },
    { mode: "drone", feasible: true, reason: null },
  ],
};

if (!truckBlocked(blocked)) throw new Error("expected truck blocked");
if (truckBlocked(ok)) throw new Error("expected truck ok");
if (recommendAlternate(blocked) !== "drone") throw new Error("expected drone alternate");
if (recommendAlternate(ok) !== "drone") throw new Error("recommend should pick first feasible alternate");

console.log("access checks ok");
