// data-agents.jsx — mock agents + multi-round arena findings.
// Each finding carries its full lifecycle so the arena view can replay rounds.

const DATA_AGENTS = [
  { id: 'general',   name: 'General',     short: 'GEN', color: '#ff7a2b', icon: 'ph-sparkle',          model: 'sonnet-4.5',  desc: 'Broad correctness, logic, intent' },
  { id: 'security',  name: 'Security',    short: 'SEC', color: '#ff6b6b', icon: 'ph-shield-check',     model: 'sonnet-4.5',  desc: 'Authn, injection, secrets, PII' },
  { id: 'professor', name: 'Professor',   short: 'PRF', color: '#9b87f5', icon: 'ph-graduation-cap',   model: 'opus-4.7',    desc: 'Architecture, idioms, research-grade rigour' },
  { id: 'perf',      name: 'Performance', short: 'PRF', color: '#7f87ff', icon: 'ph-lightning',        model: 'sonnet-4.5',  desc: 'N+1, allocations, hot paths' },
  { id: 'style',     name: 'Style',       short: 'STY', color: '#4ec9a4', icon: 'ph-paint-brush',      model: 'haiku-4.5',   desc: 'Naming, idioms, dead code' },
  { id: 'tests',     name: 'Tests',       short: 'TST', color: '#ffc457', icon: 'ph-test-tube',        model: 'sonnet-4.5',  desc: 'Coverage gaps, brittle asserts' },
];

const AGENT_BY_ID = Object.fromEntries(DATA_AGENTS.map((a) => [a.id, a]));

// General-purpose models — the OTHER way to fill an arena. Same review prompt
// run across multiple frontier models; consensus surfaces what they agree on.
const DATA_MODELS = [
  { id: 'opus-47',   name: 'Opus 4.7',    short: 'OP', color: '#7f87ff', icon: 'ph-cube',          vendor: 'Anthropic', tagline: 'Deepest reasoning · slowest',  costPer1k: '$0.075', avgMs: 24000 },
  { id: 'sonnet-45', name: 'Sonnet 4.5',  short: 'SN', color: '#4ec9a4', icon: 'ph-circles-three', vendor: 'Anthropic', tagline: 'Balanced quality + speed',     costPer1k: '$0.015', avgMs: 12000 },
  { id: 'haiku-45',  name: 'Haiku 4.5',   short: 'HK', color: '#ffc457', icon: 'ph-feather',       vendor: 'Anthropic', tagline: 'Fast & cheap · good for noise', costPer1k: '$0.004', avgMs: 5000  },
  { id: 'gpt-55',    name: 'GPT-5.5',     short: 'G5', color: '#5fd970', icon: 'ph-atom',          vendor: 'OpenAI',    tagline: 'Best-in-class general',        costPer1k: '$0.060', avgMs: 18000 },
  { id: 'gemini-25', name: 'Gemini 2.5',  short: 'GM', color: '#ff7a2b', icon: 'ph-diamonds-four', vendor: 'Google',    tagline: 'Million-token context',        costPer1k: '$0.035', avgMs: 14000 },
  { id: 'grok-3',    name: 'Grok 3',      short: 'GK', color: '#ff6b6b', icon: 'ph-lightning',     vendor: 'xAI',       tagline: 'Aggressive, opinionated',      costPer1k: '$0.025', avgMs: 9000  },
];

const MODEL_BY_ID = Object.fromEntries(DATA_MODELS.map((m) => [m.id, m]));

// ── Single-agent review (replaces dropdown). Each agent has its own findings.
// `severity`: high | med | low. Same `id` across agents means "same underlying issue
// found by multiple agents" — we use this in the arena to merge them.
const DATA_AGENT_REVIEWS = {
  general: {
    status: 'fresh', ranAt: '2 min ago', durationMs: 18400, findings: [
      { id: 'F-undef-sev',    file: 'variant-warning-copy.ts',          line: 42, severity: 'high', text: 'Fallback returns undefined when severity is missing — callers expect a string.' },
      { id: 'F-mapkey',       file: 'variant-warning-copy.ts',          line: 67, severity: 'high', text: 'Mapped key uses raw user input — risks collision with reserved _mismatchType.' },
      { id: 'F-opt-prefix',   file: 'MediaCombobox.svelte',             line: 40, severity: 'med',  text: '_option prefix signals unused but the param is used — drop the underscore.' },
      { id: 'F-shadow-opts',  file: 'experiment-template-resolution.ts',line: 18, severity: 'low',  text: 'Variable opts shadows outer scope — readability nit.' },
    ],
  },
  security: {
    status: 'fresh', ranAt: '2 min ago', durationMs: 24100, findings: [
      { id: 'F-mapkey',       file: 'variant-warning-copy.ts',          line: 67, severity: 'high', text: 'User-controlled key flows into property access without sanitisation — prototype pollution surface.' },
      { id: 'F-injection',    file: 'experiment-template-resolution.ts',line: 91, severity: 'high', text: 'Template name interpolated into a raw query string — switch to parameterised binding.' },
      { id: 'F-log-secret',   file: 'analysis-runner.ts',               line: 204,severity: 'med',  text: 'Auth token logged on resolver failure path.' },
    ],
  },
  perf: {
    status: 'fresh', ranAt: '2 min ago', durationMs: 9800, findings: [
      { id: 'F-nplusone',     file: 'PropertyMediaEditor.svelte',       line: 88, severity: 'med',  text: 'N+1 fetch in render loop — each row triggers a separate getMediaPreview call.' },
      { id: 'F-realloc',      file: 'experiment-template-resolution.ts',line: 230,severity: 'low',  text: 'New array allocated per call in hot path; pre-size or reuse.' },
    ],
  },
  style: {
    status: 'fresh', ranAt: '2 min ago', durationMs: 6400, findings: [
      { id: 'F-opt-prefix',   file: 'MediaCombobox.svelte',             line: 40, severity: 'low',  text: 'Underscore-prefixed parameter is read on line 47 — convention says drop the prefix.' },
      { id: 'F-shadow-opts',  file: 'experiment-template-resolution.ts',line: 18, severity: 'low',  text: 'Inner opts shadows the parameter of the same name.' },
      { id: 'F-deadcomment',  file: 'experiment-template-resolution.ts',line: 183,severity: 'low',  text: 'Block comment restates the function signature — redundant.' },
      { id: 'F-misnamed',     file: 'PropertyOrganismEditor.svelte',    line: 12, severity: 'low',  text: 'Local variable named "data" — be specific.' },
      { id: 'F-trailing-ws',  file: 'PropertySampleEditor.svelte',      line: 56, severity: 'low',  text: 'Trailing whitespace on three lines.' },
    ],
  },
  tests: {
    status: 'fresh', ranAt: '2 min ago', durationMs: 12200, findings: [
      { id: 'F-no-test',      file: 'variant-warning-copy.ts',          line: 42, severity: 'med',  text: 'swapExperimentReferenceForGroup has no tests for the undefined-severity branch.' },
    ],
  },
  professor: {
    status: 'fresh', ranAt: '2 min ago', durationMs: 22600, findings: [
      { id: 'F-prof-coupling', file: 'experiment-template-resolution.ts', line: 18, severity: 'high', text: 'Resolver function couples three concerns — fetching, normalising, and mapping. Split for testability and reuse.' },
      { id: 'F-prof-naming',   file: 'variant-warning-copy.ts',          line: 12, severity: 'med',  text: 'swapExperimentReferenceForGroup conflates two operations (swap + apply). Rename or split into composable helpers.' },
      { id: 'F-prof-invariant',file: 'PropertyMediaEditor.svelte',       line: 88, severity: 'high', text: 'No documented invariant between `media.kind` and `previewState`. Future refactors will silently break the contract.' },
      { id: 'F-prof-doc',      file: 'analysis-runner.ts',               line: 204, severity: 'low', text: 'Public API lacks a docstring describing the failure-mode behaviour.' },
    ],
  },
};

// ── Arena: multi-round consensus run. Each `arenaFinding` aggregates one underlying
// issue across however many agents raised it, then tracks votes per round.
//
// `verdict` is the final state. `rounds` is the trail showing how we got there.

const DATA_ARENA_RUN = {
  id: 'arena-2026-05-27-001',
  status: 'complete',          // pending | running | complete
  startedAt: '2 min ago',
  durationMs: 47800,
  agents: ['general', 'security', 'perf', 'style', 'tests'],
  rounds: [
    { n: 1, name: 'Propose',  desc: 'Every agent reviews independently' },
    { n: 2, name: 'Cross-check', desc: 'Agents validate or challenge each other' },
    { n: 3, name: 'Resolve',  desc: 'Conflicts arbitrated; final truth set' },
  ],
};

// One row per *underlying* issue. `agents` = which agents raised it in round 1.
// `votes[round]` = each agent's vote that round (keep | drop | merge | abstain).
// `verdict` = final state. `severity` may CHANGE across rounds (escalated/lowered).
const DATA_ARENA_FINDINGS = [
  {
    id: 'F-mapkey',
    file: 'variant-warning-copy.ts',
    line: 67,
    title: 'Mapped key uses raw user input',
    text: 'User-controlled key flows into property access without sanitisation — risk of prototype pollution / collision with reserved fields like _mismatchType.',
    raisedBy: ['general', 'security'],
    severityByRound: { 1: 'high', 2: 'high', 3: 'high' },
    verdict: 'kept',              // kept | dropped | merged | escalated
    confidence: 0.94,
    rationale: 'Two agents independently flagged it; security strengthened the framing. Style and Perf abstained (not in scope). No dissent.',
    rounds: [
      { n: 1, log: [
        { agent: 'general',  vote: 'propose', note: 'Mapped key uses raw user input — risks collision with reserved _mismatchType.' },
        { agent: 'security', vote: 'propose', note: 'User-controlled key flows into property access without sanitisation.' },
      ]},
      { n: 2, log: [
        { agent: 'general',  vote: 'keep',  note: 'Security framing is more accurate; defer to that wording.' },
        { agent: 'security', vote: 'keep',  note: 'Confirmed exploitable; reproducer in scratchpad.' },
        { agent: 'perf',     vote: 'abstain' },
        { agent: 'style',    vote: 'abstain' },
        { agent: 'tests',    vote: 'flag',  note: 'No test covers the malicious-key path.' },
      ]},
      { n: 3, log: [
        { agent: 'general',  vote: 'keep',  note: 'Merged wording from Security.' },
      ]},
    ],
  },
  {
    id: 'F-injection',
    file: 'experiment-template-resolution.ts',
    line: 91,
    title: 'Template name interpolated into raw SQL',
    text: 'Template name is interpolated directly into a query string. Switch to parameterised binding.',
    raisedBy: ['security'],
    severityByRound: { 1: 'high', 2: 'high', 3: 'high' },
    verdict: 'kept',
    confidence: 0.88,
    rationale: 'Only Security raised it; cross-check confirmed the call site is reachable from a user-facing form. General upgraded confidence on review.',
    rounds: [
      { n: 1, log: [
        { agent: 'security', vote: 'propose', note: 'Template name interpolated into a raw query string — switch to parameterised binding.' },
      ]},
      { n: 2, log: [
        { agent: 'general',  vote: 'keep',    note: 'Reachable from user form; agree it is exploitable.' },
        { agent: 'security', vote: 'keep' },
        { agent: 'perf',     vote: 'abstain' },
        { agent: 'style',    vote: 'abstain' },
      ]},
    ],
  },
  {
    id: 'F-undef-sev',
    file: 'variant-warning-copy.ts',
    line: 42,
    title: 'Fallback returns undefined for missing severity',
    text: 'Callers expect a string; returning undefined breaks downstream string ops.',
    raisedBy: ['general', 'tests'],
    severityByRound: { 1: 'high', 2: 'med', 3: 'med' },
    verdict: 'kept',
    confidence: 0.81,
    rationale: 'Real bug, but downgraded after Tests showed the only caller already null-checks. Still ships as Medium.',
    rounds: [
      { n: 1, log: [
        { agent: 'general', vote: 'propose', note: 'Fallback returns undefined when severity is missing — callers expect a string.' },
        { agent: 'tests',   vote: 'propose', note: 'No tests for the undefined-severity branch.' },
      ]},
      { n: 2, log: [
        { agent: 'general', vote: 'lower',   note: 'Only one caller; it null-checks. Downgrade to Medium.' },
        { agent: 'tests',   vote: 'keep' },
        { agent: 'security',vote: 'abstain' },
        { agent: 'perf',    vote: 'abstain' },
      ]},
    ],
  },
  {
    id: 'F-opt-prefix',
    file: 'MediaCombobox.svelte',
    line: 40,
    title: 'Underscore prefix on used parameter',
    text: '_option signals unused but is read on line 47.',
    raisedBy: ['general', 'style'],
    severityByRound: { 1: 'med', 2: 'low', 3: 'low' },
    verdict: 'merged',
    confidence: 0.72,
    rationale: 'Same issue raised twice. General had it as Medium, Style as Low. Merged into the Style finding (lower severity wins for cosmetic issues).',
    rounds: [
      { n: 1, log: [
        { agent: 'general', vote: 'propose', note: '_option prefix signals unused but the param is used — drop the underscore.' },
        { agent: 'style',   vote: 'propose', note: 'Underscore-prefixed parameter is read on line 47.' },
      ]},
      { n: 2, log: [
        { agent: 'general', vote: 'merge',   note: 'Duplicate of Style finding; merge.' },
        { agent: 'style',   vote: 'keep' },
      ]},
    ],
  },
  {
    id: 'F-nplusone',
    file: 'PropertyMediaEditor.svelte',
    line: 88,
    title: 'N+1 fetch in render loop',
    text: 'Each row triggers a separate getMediaPreview call.',
    raisedBy: ['perf'],
    severityByRound: { 1: 'med', 2: 'med', 3: 'med' },
    verdict: 'kept',
    confidence: 0.77,
    rationale: 'Only Perf flagged. General cross-checked and confirmed; suggested batch endpoint exists.',
    rounds: [
      { n: 1, log: [
        { agent: 'perf', vote: 'propose', note: 'N+1 fetch in render loop.' },
      ]},
      { n: 2, log: [
        { agent: 'general', vote: 'keep', note: 'Confirmed. Batch endpoint exists at /media/preview-batch.' },
        { agent: 'perf',    vote: 'keep' },
      ]},
    ],
  },
  {
    id: 'F-log-secret',
    file: 'analysis-runner.ts',
    line: 204,
    title: 'Auth token logged on failure path',
    text: 'Resolver failure path logs the bearer token.',
    raisedBy: ['security'],
    severityByRound: { 1: 'med', 2: 'high', 3: 'high' },
    verdict: 'escalated',
    confidence: 0.91,
    rationale: 'General escalated severity — logs flow to a shared sink. Now High.',
    rounds: [
      { n: 1, log: [
        { agent: 'security', vote: 'propose', note: 'Auth token logged on resolver failure path.' },
      ]},
      { n: 2, log: [
        { agent: 'general',  vote: 'escalate', note: 'Logs ship to Datadog — anyone with log access can replay. Bump to High.' },
        { agent: 'security', vote: 'keep' },
      ]},
    ],
  },
  {
    id: 'F-shadow-opts',
    file: 'experiment-template-resolution.ts',
    line: 18,
    title: 'Inner opts shadows outer scope',
    text: 'Readability nit.',
    raisedBy: ['general', 'style'],
    severityByRound: { 1: 'low', 2: 'low', 3: 'low' },
    verdict: 'kept',
    confidence: 0.65,
    rationale: 'Both agents agreed; merged with no debate.',
    rounds: [
      { n: 1, log: [
        { agent: 'general', vote: 'propose', note: 'Variable opts shadows outer scope.' },
        { agent: 'style',   vote: 'propose', note: 'Inner opts shadows the parameter of the same name.' },
      ]},
      { n: 2, log: [
        { agent: 'style',   vote: 'keep' },
        { agent: 'general', vote: 'merge', note: 'Same issue.' },
      ]},
    ],
  },
  {
    id: 'F-realloc',
    file: 'experiment-template-resolution.ts',
    line: 230,
    title: 'Array realloc in hot path',
    text: 'New array allocated per call; pre-size or reuse.',
    raisedBy: ['perf'],
    severityByRound: { 1: 'low', 2: 'low' },
    verdict: 'dropped',
    confidence: 0.42,
    rationale: 'General challenged — this is not on any measured hot path. Perf conceded; dropped.',
    rounds: [
      { n: 1, log: [
        { agent: 'perf', vote: 'propose', note: 'New array allocated per call in hot path.' },
      ]},
      { n: 2, log: [
        { agent: 'general', vote: 'drop', note: 'Profiler shows this path runs <10x per session. Not hot.' },
        { agent: 'perf',    vote: 'drop', note: 'Concede; dropping.' },
      ]},
    ],
  },
  {
    id: 'F-deadcomment',
    file: 'experiment-template-resolution.ts',
    line: 183,
    title: 'Block comment restates function signature',
    text: 'Redundant.',
    raisedBy: ['style'],
    severityByRound: { 1: 'low', 2: 'low' },
    verdict: 'dropped',
    confidence: 0.38,
    rationale: 'General disputed — the comment describes intent, not signature. Style withdrew.',
    rounds: [
      { n: 1, log: [
        { agent: 'style', vote: 'propose', note: 'Block comment restates the function signature.' },
      ]},
      { n: 2, log: [
        { agent: 'general', vote: 'drop', note: 'Comment describes when to use vs. its sibling — that is intent, not signature.' },
        { agent: 'style',   vote: 'drop', note: 'Withdrawn.' },
      ]},
    ],
  },
  {
    id: 'F-misnamed',
    file: 'PropertyOrganismEditor.svelte',
    line: 12,
    title: 'Local named "data"',
    text: 'Be specific.',
    raisedBy: ['style'],
    severityByRound: { 1: 'low', 2: 'low' },
    verdict: 'dropped',
    confidence: 0.31,
    rationale: 'General challenged — "data" is the established name in the surrounding 12 components. Style conceded.',
    rounds: [
      { n: 1, log: [
        { agent: 'style', vote: 'propose', note: 'Local variable named "data" — be specific.' },
      ]},
      { n: 2, log: [
        { agent: 'general', vote: 'drop', note: 'Convention across the codebase; cosmetic at best.' },
        { agent: 'style',   vote: 'drop' },
      ]},
    ],
  },
  {
    id: 'F-trailing-ws',
    file: 'PropertySampleEditor.svelte',
    line: 56,
    title: 'Trailing whitespace',
    text: 'Three lines.',
    raisedBy: ['style'],
    severityByRound: { 1: 'low', 2: 'low' },
    verdict: 'dropped',
    confidence: 0.20,
    rationale: 'Formatter catches it. Auto-dropped (below confidence threshold).',
    rounds: [
      { n: 1, log: [
        { agent: 'style', vote: 'propose', note: 'Trailing whitespace on three lines.' },
      ]},
      { n: 2, log: [
        { agent: 'style', vote: 'drop', note: 'Formatter handles. Withdrawn.' },
      ]},
    ],
  },
];

// Convenience: precomputed roll-ups
const ARENA_STATS = (() => {
  const verdicts = { kept: 0, escalated: 0, merged: 0, dropped: 0 };
  let proposed = 0;
  const perAgentProposed = {};
  const perAgentKept = {};
  DATA_AGENTS.forEach((a) => { perAgentProposed[a.id] = 0; perAgentKept[a.id] = 0; });
  DATA_ARENA_FINDINGS.forEach((f) => {
    verdicts[f.verdict] = (verdicts[f.verdict] || 0) + 1;
    proposed += f.raisedBy.length;
    f.raisedBy.forEach((id) => { perAgentProposed[id]++; });
    if (f.verdict !== 'dropped') f.raisedBy.forEach((id) => { perAgentKept[id]++; });
  });
  return {
    proposed, total: DATA_ARENA_FINDINGS.length,
    finalCount: verdicts.kept + verdicts.escalated + verdicts.merged,
    verdicts, perAgentProposed, perAgentKept,
  };
})();

Object.assign(window, {
  DATA_AGENTS, AGENT_BY_ID, DATA_AGENT_REVIEWS,
  DATA_MODELS, MODEL_BY_ID,
  DATA_ARENA_RUN, DATA_ARENA_FINDINGS, ARENA_STATS,
});
