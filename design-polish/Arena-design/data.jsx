// data.jsx — mock data for the easy-review prototype.
// Lifted from the source screenshots so the UI feels real.

const DATA_INBOX = [
  { id: 'i1', kind: 'merged', label: 'PR #505 merged',           sub: 'dummy commit to trigger CI',                         when: '2m',  unread: true },
  { id: 'i2', kind: 'ci-fail',label: 'CI failed on PR #498',     sub: 'fix: remove isomorphic-dompurify to fix Vercel build', when: '8m',  unread: true },
  { id: 'i3', kind: 'review', label: 'Review requested · PR #1148', sub: 'You were requested to review by @maya',           when: '14m', unread: true },
  { id: 'i4', kind: 'comment',label: 'New comment on PR #1145',  sub: '@alex: this should probably guard against undefined…', when: '32m', unread: true },
  { id: 'i5', kind: 'merged', label: 'PR #1143 merged',          sub: 'analysis-runner: Remove analysis progress polling',   when: '1h' },
  { id: 'i6', kind: 'merged', label: 'PR #1142 merged',          sub: 'virtual-device: 96 well tile around cluster. Fix off-by-one', when: '2h' },
  { id: 'i7', kind: 'merged', label: 'PR #1140 merged',          sub: 'fix: object level export missing for legacy migrations', when: '3h' },
  { id: 'i8', kind: 'merged', label: 'PR #1139 merged',          sub: 'Dur/dev 5363 add insights to assay analysis output',  when: '5h' },
  { id: 'i9', kind: 'mention',label: 'Mentioned in PR #1137',    sub: '@you can you sanity-check the variant-identity match?', when: '1d' },
];

const DATA_PROJECTS = [
  { id: 'easy-review', name: 'easy-review', count: 9, tracked: [] },
  {
    id: 'discovery',
    name: 'discovery',
    count: 32,
    tracked: [
      { id: 't1', name: 'main', repo: 'Discovery/discovery' },
      { id: 't2', name: 'claude/experiment-…', repo: 'Discovery/experiment-q…' },
      { id: 't3', name: 'claude/fix-metadat…', repo: 'worktrees/pr1132-review' },
      { id: 't4', name: 'claude/organism-sub…', repo: 'worktrees/pr1137-fixes', active: true, dot: true },
    ],
    myPRs: [
      { id: 'p1', name: 'Add swapExperimentRef…', num: '#1137', active: true, prStatus: 'draft' },
      { id: 'p2', name: 'Add default variant…',   num: '#1132', prStatus: 'review' },
      { id: 'p3', name: 'Add QC replicate…',      num: '#1099', prStatus: 'approved' },
      { id: 'p4', name: 'DEV-5067: Remove…',      num: '#1065', prStatus: 'queue' },
    ],
    toReview: [
      { id: 'r1', name: 'Dur/dev 5358 a…', num: '#1145', prStatus: 'review' },
      { id: 'r2', name: 'test: preview-fu…', num: '#1144', prStatus: 'review' },
      { id: 'r3', name: 'Schedule daily …', num: '#1141', prStatus: 'review' },
      { id: 'r4', name: 'Refactor worker…', num: '#1133', prStatus: 'declined' },
      { id: 'r5', name: 'feat: opt-in `pre…', num: '#1129', prStatus: 'draft' },
    ],
    recent: [
      { id: 'rc1', name: 'Add swapExperimentRef…', num: '#1137', prStatus: 'draft' },
      { id: 'rc2', name: 'Add QC replicate…', num: '#1099', prStatus: 'approved' },
      { id: 'rc3', name: 'Add default variant…', num: '#1132', prStatus: 'review' },
      { id: 'rc4', name: 'DEV-5067: Remove…', num: '#1065', prStatus: 'queue' },
    ],
    merged: [
      { id: 'm1', name: 'analysis-runner…', num: '#1143', prStatus: 'merged' },
      { id: 'm2', name: 'Dur/dev 5363 a…', num: '#1139', prStatus: 'merged' },
      { id: 'm3', name: 'fix: object level …', num: '#1140', prStatus: 'merged' },
      { id: 'm4', name: 'virtual-device: …', num: '#1142', prStatus: 'merged' },
    ],
  },
  { id: 'ink-booking', name: 'ink-booking', count: 26, tracked: [] },
];

const DATA_FILES = [
  { path: 'packages › … › lib', kind: 'dir', depth: 0 },
  { path: 'components › property-editor', kind: 'dir', depth: 1 },
  { name: 'experiment-templa…', kind: 'file', depth: 2, comments: 1, add: 90, del: 0, ext: 'ts' },
  { name: 'experiment-te…',     kind: 'file', depth: 2, comments: 1, add: 358, del: 1, ext: 'ts' },
  { name: 'PropertyMediaEdito…', kind: 'file', depth: 2, add: 3, del: 3, ext: 'ts', active: true },
  { name: 'PropertyOrganismEdi…', kind: 'file', depth: 2, add: 3, del: 3, ext: 'ts' },
  { name: 'PropertySampleEdito…', kind: 'file', depth: 2, add: 3, del: 3, ext: 'ts' },
  { name: 'PropertyTreatmentEd…', kind: 'file', depth: 2, add: 3, del: 3, ext: 'ts' },
  { path: 'context › bulk-well-context', kind: 'dir', depth: 1 },
  { name: 'bulk-well-context.sve…', kind: 'file', depth: 2, add: 4, del: 3, ext: 'svelte' },
];

// The diff hunks shown in the main view.
// Each line: { n, kind: 'add'|'del'|'ctx'|'meta'|'spacer'|'highlight', text, n2?, comment?, focus? }
const DATA_DIFF = [
  { n: 24, kind: 'ctx',  text: '    experimentOption: ExperimentPropertyOption<TData>;' },
  { n: 25, kind: 'ctx',  text: '  }' },
  { n: 26, kind: 'ctx',  text: '' },
  { n: 27, kind: 'add',  text: '  interface SwapExperimentReferenceForGroupArgs<TData extends PropertyDataLike> {' },
  { n: 28, kind: 'add',  text: '    context: BulkWellContext;' },
  { n: 29, kind: 'add',  text: '    type: PropertyType;' },
  { n: 30, kind: 'add',  text: '    resolvedProperties: PropertyData[];' },
  { n: 31, kind: 'add',  text: '    experimentOption: ExperimentPropertyOption<TData>;' },
  { n: 32, kind: 'add',  text: '  }' },
  { n: 33, kind: 'add',  text: '' },
  { n: 34, kind: 'add',  text: '  interface ApplyExperimentOptionToExistingGroupArgs<' },
  { n: 35, kind: 'add',  text: '    TData extends PropertyDataLike,' },
  { n: 36, kind: 'add',  text: '  > {' },
  { n: 37, kind: 'add',  text: '    context: BulkWellContext;' },
  { n: 38, kind: 'add',  text: '    type: PropertyType;' },
  { n: 39, kind: 'add',  text: '    groupProperties: PropertyData[];' },
  { n: 40, kind: 'add',  text: '    experimentOption: ExperimentPropertyOption<TData>;' },
  { n: 41, kind: 'add',  text: '  }' },
  { n: 42, kind: 'add',  text: '' },
  { n: 43, kind: 'ctx',  text: '  const QUANTITY_PROPERTY_KEYS = [' },
  { n: 44, kind: 'ctx',  text: "    'volume'," },
  { n: 45, kind: 'ctx',  text: "    'volume_unit'," },
  { kind: 'meta', text: '@@ -164,6 +180,10 @@ const mergeExperimentVariantWithTemplateQuantity = <' },
  { n: 180, kind: 'ctx', text: '  } as TData;' },
  { n: 181, kind: 'ctx', text: '};' },
  { n: 182, kind: 'ctx', text: '' },
  { n: 183, kind: 'add', text: '  // Resolves a still-variable template group against an experiment option by', focus: true },
  { n: 184, kind: 'add', text: '  // variant-identity matching. Each templateProperty must be a variable slot', focus: true },
  { n: 185, kind: 'add', text: '  // (carrying `variableProperties`); once a group has been resolved to set data,', focus: true },
  { n: 186, kind: 'add', text: '  // use `swapExperimentReferenceForGroup` instead.', focus: true },
  { kind: 'comment', range: '183–186', kind2: 'question', author: 'you', when: '3m', text: 'Is this comment relevant?' },
  { n: 187, kind: 'add', text: '  export const resolveExperimentPropertyGroup = <TData extends PropertyDataLike>({' },
  { n: 188, kind: 'add', text: '    context,' },
  { n: 189, kind: 'add', text: '    type,' },
  { kind: 'meta', text: '@@ -267,3 +287,73 @@ export const resolveExperimentPropertyGroup = <TData extends PropertyDataLike>({' },
  { n: 287, kind: 'ctx', text: '' },
  { n: 288, kind: 'ctx', text: '    context.deduplicateProperties(type);' },
  { n: 289, kind: 'ctx', text: '  };' },
  { n: 290, kind: 'add', text: '' },
  { n: 291, kind: 'add', text: '  // Swaps the entity reference and display name on every property in a group' },
  { n: 292, kind: 'add', text: '  // that has already been resolved to set data. Existing subproperties' },
  { n: 293, kind: 'add', text: '  // (concentration, volume, control_type, dilution, etc.) and well assignments' },
  { n: 294, kind: 'add', text: '  // are preserved verbatim. Use this when the user picks a different experiment' },
  { n: 295, kind: 'add', text: '  // option for a group that is no longer in its variable-template state — the' },
  { n: 296, kind: 'add', text: '  // variant-matching logic in `resolveExperimentPropertyGroup` does not apply' },
];

// Top branch tabs (cmd+click to open new ones — simulated).
const DATA_BRANCH_TABS = [
  { id: 'b1', label: 'claude/organism-sub…', repo: 'main', active: true, dirty: false, comments: 1 },
  { id: 'b2', label: 'claude/fix-metadat…', repo: 'pr1132', dirty: true, comments: 3 },
  { id: 'b3', label: 'Add QC replicate…', repo: 'pr1099', comments: 0 },
];

// Branch context details for the active tab.
const DATA_BRANCH = {
  name: 'claude/organism-subproperty-mixup-LgMlO',
  short: 'claude/organism-sub…',
  base: 'main',
  reviewed: '4/7 reviewed',
  changes: { add: 464, del: 16 },
  pr: '#1137',
  status: 'draft',
  reviewRequired: true,
  mergeable: true,
  ci: { passed: 7, total: 7 },
  comments: 1,
  reviews: 0,
  worktree: 'worktrees/pr1137-fixes',
  description: 'Adds swapExperimentReferenceForGroup …',
};

// AI Review (currently empty — opportunity to make empty state feel intentional)
const DATA_AI_REVIEW = {
  fresh: true,
  high: 0, med: 0, low: 0,
  body: "No findings written. Inspect the `.er/` folder to see raw review output, or re-run the review skill.",
};

// Questions feed (private, machine-local notes)
const DATA_QUESTIONS = [
  {
    id: 'q1',
    file: 'experiment-template-resolution.ts',
    lines: '183–186',
    text: 'Is this comment relevant?',
    note: 'Questions stay on your machine. Use them for personal review notes or routing to an AI assistant.',
  },
];

// Commits on the active branch — used by the FilesRail commit picker.
const DATA_COMMITS = [
  { sha: 'a7c91b2', msg: 'Add swapExperimentReferenceForGroup helper', author: 'V', when: '3m',  files: 4, add: 90,  del: 0,  pushed: true },
  { sha: 'f3e0d44', msg: 'Apply experiment option to existing groups', author: 'V', when: '14m', files: 3, add: 248, del: 12, pushed: true },
  { sha: '9d2b1ce', msg: 'Refactor template-quantity merge to TData generic', author: 'V', when: '47m', files: 2, add: 76, del: 4, pushed: true },
  { sha: '4ac7f80', msg: 'Wire BulkWellContext into resolver', author: 'V', when: '2h',  files: 2, add: 38, del: 0, pushed: true },
  { sha: '1e8a09d', msg: 'WIP: experiment template resolution stub', author: 'V', when: '4h',  files: 1, add: 12, del: 0, pushed: false },
];

Object.assign(window, {
  DATA_INBOX, DATA_PROJECTS, DATA_FILES, DATA_DIFF,
  DATA_BRANCH_TABS, DATA_BRANCH, DATA_AI_REVIEW, DATA_QUESTIONS,
  DATA_COMMITS,
});
