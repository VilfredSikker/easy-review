use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::settings::{
    ConfigFieldValue, ModelUpsert, ProviderUpsert, ReviewModelGroups, UninstallOptions,
};
use crate::{
    ArenaPlan, PaintedView, Panel, PrAction, PrOpenHint, PrTarget, ResourceChanged, ReviewLine,
    TabKey,
};

/// A command the frontend sends to the backend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum Command {
    /// Settings opened: adopt a config file repaired after a failed load.
    ReloadSettings,
    /// Writes one config field, then saves.
    ApplyConfigPatch {
        key: String,
        value: ConfigFieldValue,
    },
    SetReviewFlowModels {
        groups: ReviewModelGroups,
    },
    /// Lists models from the provider CLIs into `settings`; never saved.
    /// `force` skips a fresh cache.
    RefreshAiModels {
        provider_id: Option<String>,
        force: bool,
    },
    UpsertAiProvider {
        provider: ProviderUpsert,
    },
    DeleteAiProvider {
        provider_id: String,
    },
    UpsertAiModel {
        provider_id: String,
        model: ModelUpsert,
    },
    DeleteAiModel {
        provider_id: String,
        model_id: String,
    },
    /// Lists what an uninstall would remove into `settings`.
    PreviewUninstall {
        options: UninstallOptions,
    },
    /// Removes what the options name; the app quits once it is done.
    RunUninstall {
        options: UninstallOptions,
    },
    /// Reads one repo's committed risk rules and resolves `paths` against
    /// them. The `Ack` names the rules key as `created`, and `risk(key)`
    /// holds the result. `remote_repo` (`owner/repo`) is for a target with no
    /// clone.
    LoadRisk {
        repo_root: String,
        remote_repo: Option<String>,
        paths: Vec<String>,
    },
    /// Saves the default AI provider and model, and the effort when one is
    /// named. Every pick saves: there is no session-only pick on the wire.
    SetAiSelection {
        provider_id: String,
        model_id: Option<String>,
        effort: Option<String>,
    },
    /// Saves the default effort for the default selection; none is Auto.
    SetAiEffort {
        effort: Option<String>,
    },
    /// Compares the running build with the latest release. The `Ack` comes
    /// at once; the answer lands in `session`.
    CheckAppUpdate,
    /// Mirrors a side panel the page toggled into `panels`.
    TogglePanel {
        panel: Panel,
    },
    /// Opens a Working tab on the active tab's repo.
    NewTab,
    /// Closes a tab and drops its content; the last tab stays.
    CloseTab {
        key: TabKey,
    },
    SelectTab {
        key: TabKey,
    },
    /// Moves a tab to `to_idx`; the active tab stays active.
    ReorderTabs {
        key: TabKey,
        to_idx: u32,
    },
    /// Opens a project's PR from its clone. A PR that is already open is
    /// selected. With `replace` the new tab takes the active tab's place.
    OpenPrReview {
        project_id: String,
        #[ts(type = "number")]
        pr_number: u64,
        replace: bool,
    },
    /// Opens a PR with no clone.
    OpenRemotePr {
        owner: String,
        repo: String,
        #[ts(type = "number")]
        number: u64,
        replace: bool,
    },
    /// Opens a pasted GitHub PR link, from a project's clone when one tracks
    /// its repo.
    OpenPrUrl {
        url: String,
        replace: bool,
    },
    OpenLocalBranch {
        project_id: String,
        name: String,
        replace: bool,
    },
    /// Opens a branch of a project's checkout and makes the project active.
    OpenProjectBranch {
        project_id: String,
        branch: String,
        replace: bool,
    },
    /// Asks for a folder and opens a Working tab on it. A cancelled pick
    /// changes nothing.
    OpenWorktree,
    /// Opens a Working tab on the project's root and makes it active.
    SetActiveProject {
        project_id: String,
    },
    /// Warms the open cache for a project's PR. The `Ack` comes at once.
    PrefetchPrOpen {
        project_id: String,
        #[ts(type = "number")]
        pr_number: u64,
        hint: PrOpenHint,
    },
    /// Warms the open cache for a PR with no clone. The `Ack` comes at once.
    PrefetchRemotePrOpen {
        owner: String,
        repo: String,
        #[ts(type = "number")]
        number: u64,
    },
    /// Writes the active tab's review browser; a field left out stays.
    /// `navigate` also loads `url` in the tab's webview.
    UpdateTabBrowser {
        layout: Option<crate::BrowserLayout>,
        url: Option<String>,
        navigate: bool,
        annotate: Option<bool>,
        tooltips: Option<bool>,
        split_ratio: Option<f32>,
    },
    /// Hidden, split, fullscreen, hidden: the active tab's browser layout.
    CycleTabBrowserLayout,
    /// Puts the projects in this order. Ids it does not name keep their order
    /// after the named ones; an empty list changes nothing.
    ReorderProjects {
        ordered: Vec<String>,
    },
    DeleteProject {
        project: String,
    },
    /// Lists a local branch under its project.
    AddTrackedBranch {
        project: String,
        name: String,
    },
    /// Hides a branch from its project's list. The checked-out branch stays.
    RemoveTrackedBranch {
        project: String,
        name: String,
    },
    PatchProjectReviewSettings {
        project: String,
        patch: ProjectReviewSettingsPatch,
    },
    /// Fetches every project remote's pull requests in the background; the
    /// push brings the new `prs` versions. A refresh already running absorbs it.
    RefreshPrList,
    /// Fetches one project's remote in the background; a project with no
    /// remote changes nothing.
    RefreshProjectPrList {
        project: String,
    },
    /// Hides a pull request from the project's lists.
    DismissPr {
        project: String,
        #[ts(type = "number")]
        number: u64,
    },
    UndismissPr {
        project: String,
        #[ts(type = "number")]
        number: u64,
    },
    /// Tracks a pull request, which also undoes its dismissal.
    TrackPr {
        project: String,
        #[ts(type = "number")]
        number: u64,
    },
    UntrackPr {
        project: String,
        #[ts(type = "number")]
        number: u64,
    },
    /// Saves a pull request at the front of the project's saved list, which
    /// keeps the newest 50. With no title the cached one is used.
    SavePr {
        project: String,
        #[ts(type = "number")]
        number: u64,
        title: Option<String>,
    },
    UnsavePr {
        project: String,
        #[ts(type = "number")]
        number: u64,
    },
    /// Marks one inbox item read; an id the inbox does not hold changes
    /// nothing.
    MarkInboxItemRead {
        id: String,
    },
    MarkAllInboxRead,
    MarkInboxItemsRead {
        ids: Vec<String>,
    },
    /// Removes every read inbox item.
    ClearReadInboxItems,
    /// Removes the named inbox items that are read; unread ones stay.
    ClearInboxItems {
        ids: Vec<String>,
    },
    /// Runs the PR sweep that fills the inbox, as `RefreshPrList` does.
    RefreshNotifications,
    /// Shows a test native notification; refused when the system would not
    /// show it.
    TestNativeNotification,
    /// Marks the inbox item read and opens what it names, in place of the
    /// active tab or, with `replace` false, in a new one. The `Ack` names
    /// `inbox` and `tabs`, and `created` is the tab it left active. A target
    /// no project holds is refused, and the read still lands.
    OpenInboxItem {
        id: String,
        replace: bool,
    },
    /// Warms the open cache for the inbox item's PR. The `Ack` comes at once.
    PrefetchInboxItem {
        id: String,
    },
    /// Shows a tab's diff in another view: `branch`, `unstaged`, `staged`,
    /// `history`, `conflicts`, `hidden`, `pr` (or `pr_diff`) or `tour`.
    /// `pr_number` is a PR the header detected for a tab that has none.
    SetDiffMode {
        tab: TabKey,
        mode: String,
        #[ts(type = "number | null")]
        pr_number: Option<u64>,
    },
    /// Diffs a tab against another base, which the tab keeps.
    ChangeBase {
        tab: TabKey,
        base: String,
    },
    /// Filters a tab's file list; an empty query clears the filter.
    SetFilter {
        tab: TabKey,
        query: String,
    },
    /// Reads a tab's diff again. `sync` also fetches its PR or base refs
    /// first and lines the stale mark up with the PR list, as Sync does.
    RefreshDiff {
        tab: TabKey,
        sync: bool,
    },
    /// Shows one commit of a tab's History, entering History when the tab
    /// shows another view. A remote PR's commits are display-only.
    SelectCommit {
        tab: TabKey,
        sha: String,
    },
    /// Leaves History for the view it was entered from.
    ExitHistory {
        tab: TabKey,
    },
    /// Syncs every open tab of a project's PR, after reading the PR again.
    SyncPr {
        project_id: String,
        #[ts(type = "number")]
        pr_number: u64,
    },
    /// Syncs every open tab that shows a project's branch.
    SyncBranch {
        project_id: String,
        branch: String,
    },
    /// Flips one file's reviewed mark on the tab the page painted. Refused
    /// when `tab` is no longer the active tab.
    ToggleReviewed {
        tab: TabKey,
        path: String,
    },
    /// Marks one file reviewed, or clears its mark, on the tab the page
    /// painted. Refused when `tab` is no longer the active tab.
    MarkReviewed {
        tab: TabKey,
        path: String,
        reviewed: bool,
    },
    /// Marks every file of a tour pillar reviewed, or clears them. Refused
    /// when `tab` is no longer the active tab, or when it shows another view
    /// than `view`, the one `reviewed(tab)` named when the page painted.
    BulkReviewPillar {
        tab: TabKey,
        view: PaintedView,
        pillar_id: String,
        reviewed: bool,
    },
    /// Expands a compacted file in full, or compacts it again. The tab keeps
    /// the choice across reloads.
    SetFileCompacted {
        tab: TabKey,
        path: String,
        compacted: bool,
    },
    /// Adds a question on a diff line of the tab the page painted, whichever
    /// tab is active. Refused when the tab shows another view than `view`,
    /// the one `annotations(tab)` named. `client_id` is the id of the row
    /// the page painted; the `Ack`'s `created` is the row's id.
    AddQuestion {
        tab: TabKey,
        view: PaintedView,
        target: LineTarget,
        body: String,
        side: Option<String>,
        client_id: Option<String>,
    },
    /// Adds a private note, as `AddQuestion` adds a question.
    AddNote {
        tab: TabKey,
        view: PaintedView,
        target: LineTarget,
        body: String,
        side: Option<String>,
        client_id: Option<String>,
    },
    /// Adds a comment and posts it to the tab's pull request.
    AddComment {
        tab: TabKey,
        view: PaintedView,
        target: LineTarget,
        body: String,
        side: Option<String>,
        client_id: Option<String>,
    },
    /// Anchors a question or note (`thread_kind`) on a line of a previewed
    /// document.
    /// Refused when the document's preview key in the tab is no longer
    /// `preview_key`, the one the line was picked in.
    AddDocumentThread {
        tab: TabKey,
        path: String,
        thread_kind: String,
        line: u32,
        line_end: Option<u32>,
        body: String,
        preview_key: String,
    },
    ReplyToThread {
        tab: TabKey,
        view: PaintedView,
        parent_id: String,
        body: String,
        client_id: Option<String>,
    },
    UpdateThreadMessage {
        tab: TabKey,
        view: PaintedView,
        id: String,
        body: String,
    },
    /// Deletes a thread or one reply, and any ask-AI reply still being
    /// written for it.
    DeleteThread {
        tab: TabKey,
        view: PaintedView,
        id: String,
    },
    ResolveThread {
        tab: TabKey,
        view: PaintedView,
        id: String,
    },
    /// Turns a question into a private note; with no body the question's is
    /// kept.
    PromoteToNote {
        tab: TabKey,
        view: PaintedView,
        id: String,
        body: Option<String>,
    },
    /// Posts a question or note to the pull request, then removes it.
    PromoteToComment {
        tab: TabKey,
        view: PaintedView,
        id: String,
        body: Option<String>,
    },
    /// Posts a finding to the pull request, then removes it.
    PromoteFindingToComment {
        tab: TabKey,
        view: PaintedView,
        finding_id: String,
        body: Option<String>,
    },
    /// Replies to a finding as the reviewer. An AI-assisted reply is not on
    /// the wire yet.
    ReplyToFinding {
        tab: TabKey,
        view: PaintedView,
        finding_id: String,
        body: String,
        client_id: Option<String>,
    },
    /// Removes a finding from the review, with the threads linked to it.
    DismissFinding {
        tab: TabKey,
        view: PaintedView,
        finding_id: String,
    },
    /// Marks a finding resolved, or reopens it.
    SetFindingResolved {
        tab: TabKey,
        view: PaintedView,
        finding_id: String,
        resolved: bool,
    },
    /// Deletes the threads linked to a finding, and any ask-AI reply still
    /// being written for them; the finding stays.
    RemoveFindingThread {
        tab: TabKey,
        view: PaintedView,
        finding_id: String,
    },
    UpdateFindingResponse {
        tab: TabKey,
        view: PaintedView,
        finding_id: String,
        response_id: String,
        body: String,
    },
    DeleteFindingResponse {
        tab: TabKey,
        view: PaintedView,
        finding_id: String,
        response_id: String,
    },
    /// Flips the checklist item at `index` in the checklist `annotations(tab)`
    /// holds.
    ToggleChecklistItem {
        tab: TabKey,
        view: PaintedView,
        index: u32,
    },
    /// Deletes the view bucket's `triage` or its `review` artifacts. Refused
    /// while a review flow holds the bucket.
    DeleteReviewArtifact {
        tab: TabKey,
        view: PaintedView,
        artifact: String,
    },
    DeleteDiagram {
        tab: TabKey,
        view: PaintedView,
        id: String,
    },
    /// Adds a browser annotation to the view bucket. `client_id` is the row's
    /// id, which starts `ui-`; the `Ack`'s `created` is the id it took.
    AddUiAnnotation {
        tab: TabKey,
        view: PaintedView,
        annotation: Box<NewUiAnnotation>,
    },
    /// Saves a PNG data URL as an annotation's screenshot. The `Ack`'s
    /// `created` is the file's path.
    SaveAnnotationScreenshot {
        tab: TabKey,
        view: PaintedView,
        id: String,
        data_url: String,
    },
    DeleteUiAnnotation {
        tab: TabKey,
        view: PaintedView,
        id: String,
    },
    /// Removes the browser annotations on one page, with their screenshots.
    ClearUiAnnotationsForPage {
        tab: TabKey,
        view: PaintedView,
        page_url: String,
    },
    /// Removes every browser annotation in the view bucket.
    ClearUiAnnotations {
        tab: TabKey,
        view: PaintedView,
    },
    /// Marks annotations whose element moved or went, and moves their boxes.
    UpdateUiAnnotationAnchors {
        tab: TabKey,
        view: PaintedView,
        updates: Vec<AnchorUpdate>,
    },
    /// Starts a plan on the tab's diff. The `Ack`'s `created` is the run id;
    /// a plan with groups starts one run per group, and `created` holds
    /// their ids, one per line.
    ArenaStart {
        tab: TabKey,
        plan: Box<ArenaPlan>,
        title: Option<String>,
        confirm: bool,
        agent_kind: Option<String>,
        /// None runs at the saved selection's effort.
        effort: Option<String>,
    },
    /// Dedupes the tab's expert findings and rules on them with one arbiter.
    /// The `Ack`'s `created` is the run id; none when there was nothing to
    /// validate.
    ArenaValidateFindings {
        tab: TabKey,
    },
    /// Imports a run's findings into the tab's review; all of them when
    /// `finding_ids` is none. The `Ack`'s `created` is how many it took.
    ArenaAcceptFindings {
        tab: TabKey,
        run_id: String,
        finding_ids: Option<Vec<String>>,
    },
    ArenaDelete {
        tab: TabKey,
        run_id: String,
    },
    ArenaCancel {
        tab: TabKey,
        run_id: String,
    },
    /// Records the reviewer's verdict on one finding beside the arbiter's.
    ArenaOverride {
        tab: TabKey,
        run_id: String,
        finding_id: String,
        verdict: String,
        note: String,
    },

    // ── Card asks ──
    /// Asks the AI about a thread; the answer lands as a reply in it. An
    /// empty prompt asks with the thread kind's own prompt. `jobs` shows the
    /// reply being written until it lands.
    AskAi {
        tab: TabKey,
        thread_id: String,
        prompt: String,
    },
    /// Has the AI check a finding, whose responses take the answer, or a
    /// thread, which takes it as a reply. A thread opened on a finding asks
    /// about the finding.
    ValidateWithAi {
        tab: TabKey,
        thread_id: Option<String>,
        finding_id: Option<String>,
    },
    /// Has the AI rewrite a question or note as a self-contained one.
    ElaborateWithAi {
        tab: TabKey,
        thread_id: String,
    },
    /// Asks the AI about a finding in the finding's thread, which this
    /// creates when the finding has none. Painted in `view`, so a tab that
    /// shows another view since refuses it. The `Ack`'s `created` is the
    /// thread's root id.
    AskAiOnFinding {
        tab: TabKey,
        view: PaintedView,
        finding_id: String,
        body: String,
        client_id: Option<String>,
    },

    // ── Background tasks ──
    /// Each run below queues an agent task on its tab's view; `jobs` shows
    /// it until it finishes, and its output is read through `task_log`.
    /// `scope` is `auto`, `branch`, `staged` or `unstaged`.
    RunReview {
        tab: TabKey,
        scope: String,
    },
    GenerateTour {
        tab: TabKey,
    },
    /// Ranks the repo's files for the importance rules.
    RunImportanceAgent {
        tab: TabKey,
    },
    GenerateDiagram {
        tab: TabKey,
        diagram_kind: String,
        custom_prompt: Option<String>,
    },
    RunExpertReview {
        tab: TabKey,
        scope: String,
        expert_id: String,
    },
    /// Runs `reviewers` (`general`, `professor`, `triage` or
    /// `expert:<id>`) over `paths`, or over the whole diff when empty.
    RunScopedReview {
        tab: TabKey,
        scope: String,
        paths: Vec<String>,
        reviewers: Vec<String>,
        focus_prompt: Option<String>,
    },
    /// Validates the tab's review findings and its eligible GitHub
    /// comments on `scope`.
    RunValidate {
        tab: TabKey,
        scope: String,
    },
    /// Hands a note and its replies to the agent to carry out in the tab's
    /// working tree.
    SendNoteToAgent {
        tab: TabKey,
        note_id: String,
    },
    /// Triages one PR of a project from the sidebar, with no tab open.
    RunPrTriage {
        project_id: String,
        #[ts(type = "number")]
        pr_number: u64,
    },
    /// Triages a project's branch: its open PR when it has one.
    RunBranchTriage {
        project_id: String,
        branch: String,
    },
    /// Removes a task that has not started.
    CancelQueuedTask {
        id: String,
    },
    /// Stops a running task.
    CancelRunningTask {
        id: String,
    },

    // ── The PR card ──
    /// Reads the tab's `gh stack` again into `pr(tab)`.
    RefreshStack {
        tab: TabKey,
    },
    /// Fetches the GitHub status of the tab's PR in the background; the push
    /// brings `pr_status`.
    RefreshGithubStatus {
        tab: TabKey,
    },

    // ── The GitHub senders ──
    /// Replies on a thread that is on GitHub. Painted in `view`, so a tab
    /// that shows another view since refuses it.
    ReplyToGithubThread {
        tab: TabKey,
        view: PaintedView,
        parent_id: String,
        body: String,
        client_id: Option<String>,
    },
    /// Pulls the tab's PR comments into its comments sidecar. `force` skips
    /// the hover-prefetch bundle.
    PullGithubComments {
        tab: TabKey,
        force: bool,
    },
    /// Posts a PR-wide comment on the tab's PR.
    PostGithubPrComment {
        tab: TabKey,
        body: String,
    },
    /// Posts one line comment outside any review.
    PostGithubLineComment {
        tab: TabKey,
        target: ReviewLine,
        body: String,
    },
    /// Submits a bare decision (`APPROVE`, `REQUEST_CHANGES` or `COMMENT`).
    SubmitGithubPrDecision {
        tab: TabKey,
        mode: String,
        summary: String,
    },
    /// Adds a line comment to the viewer's pending review; the page reads
    /// `pending_review(tab)` after.
    AddPendingReviewComment {
        tab: TabKey,
        target: ReviewLine,
        body: String,
    },
    RemovePendingReviewComment {
        tab: TabKey,
        comment_id: String,
    },
    /// Submits the viewer's pending review with a decision.
    SubmitPendingReview {
        tab: TabKey,
        mode: String,
        summary: String,
    },
    /// Runs a branch action on the PR the tab's card shows, when it is still
    /// `pr`. The `Ack` names `pr_status` after the refresh it runs.
    RunGithubPrAction {
        tab: TabKey,
        pr: PrTarget,
        action: PrAction,
    },
    /// Controls the review flow on `er_dir`, the bucket the page painted for
    /// `tab`. A control other than acceptance is refused when the tab's
    /// content shows another bucket. Accepting answers in `created` how
    /// many findings it took.
    ReviewFlow {
        tab: TabKey,
        er_dir: String,
        action: ReviewFlowAction,
    },
}

/// What a `ReviewFlow` command does to the flow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ReviewFlowAction {
    /// Reviews `scope` (`current`, `pr` or a git scope) on the tab.
    Start {
        scope: String,
    },
    /// Starts the Deep plan the user approved.
    Confirm,
    Decline,
    /// Re-runs an interrupted flow from its record.
    Rerun,
    Cancel,
    /// Runs the lenses triage chose as Standard finders instead of Deep.
    RunStandard,
    AcceptFindings {
        run_id: String,
        finding_ids: Vec<String>,
    },
}

/// A browser annotation as the page submits it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct NewUiAnnotation {
    pub url: String,
    pub selector: Option<String>,
    /// x, y, width and height in page pixels.
    pub bbox: [f64; 4],
    /// Width and height of the pane the box was drawn in.
    pub viewport: [u32; 2],
    pub text: String,
    pub screenshot_data_url: Option<String>,
    pub element_context: Option<String>,
    /// The DOM context as JSON text, as `UiAnnotation` carries it.
    pub dom_context_json: Option<String>,
    pub client_id: Option<String>,
}

/// One annotation's anchor after the page looked for its element again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AnchorUpdate {
    pub id: String,
    /// Whether the element is still where the annotation points.
    pub fresh: bool,
    pub new_box: Option<[f64; 4]>,
}

/// The diff line a thread anchors to, as the page painted it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LineTarget {
    pub file: String,
    pub hunk_idx: u32,
    pub line_num: Option<u32>,
    pub line_num_end: Option<u32>,
}

/// The review settings a patch changes; a field left empty stays as it is.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct ProjectReviewSettingsPatch {
    pub auto_triage_max_diff_kb: Option<u32>,
    pub review_ignore_glob_add: Option<String>,
    /// The index of the glob to remove.
    pub review_ignore_glob_remove: Option<u32>,
}

/// A command was applied.
///
/// It names the versions the command moved, so the frontend refetches them
/// as soon as the answer arrives instead of waiting for the push.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Ack {
    pub changed: Vec<ResourceChanged>,
    /// The id of what a create command made; empty for every other command.
    pub created: Option<String>,
}
