#!/usr/bin/env python3
"""File transforms around the Feature Board for the epic orchestrator.

A script cannot call ArtifactData, so the orchestrator reads with ArtifactData
(`out_dir` saves each doc as <out_dir>/<collection>/<doc_id>.json), runs this
script on the saved files, and writes what it prints with ArtifactData.
Standard library only; run with `python3 -I`. See ../SKILL.md, "The board helper".

usage:
  board.py export <saved-dir> <out-folder>
      Readable Markdown for agents: BRIEF.md, PLAN.md (with the derived status
      table), RESUME.md, LOG.md (newest first), PR-<planId>.md per sub-task, and
      <name>.md for any other epicDocs doc.
  board.py status <saved-dir>
      Print the derived status table.
  board.py log <epic id> <project> <kind> "<text>" [planId=status:pr:note ...]
      Write the JSON for an ArtifactData `set` of one new epicLog doc; print its
      path and doc id. Use - for no PR or no note.
  board.py doc <epic id> <project> <name> <markdown file> <current version or 0> "<summary>" [if_version]
      Write a batch `writes` JSON that sets epicDocs/<epic>--<name> and its
      epicDocVersions copy; print its path. <current version> is the doc's own
      `version` field; [if_version] is the database version of the epicDocs doc
      you read (required by the board when the doc already exists).
  board.py runid <task id>
      Print a new run id (run_<stamp>_session_<task id>). Start the brief of
      every agent you give that sub-task with the line "run: <run id>".
  board.py run <run id> <task id> <project> <done|failed> "<result>" "<summary>" [stages] [session id]
      Write the JSON for an ArtifactData `set` of runs/<run id>, with
      metrics.tokens summed over every subagent transcript whose brief starts
      with "run: <run id>"; print its path and doc id. [stages] is a comma list
      such as implement,review. CLAUDE_PROJECTS overrides ~/.claude/projects.
"""

import glob
import json
import os
import random
import re
import string
import sys
import tempfile
from datetime import datetime, timezone

LOG_KINDS = ("note", "decision", "lesson", "status", "owner_action")
STATUS_WORDS = ("planned", "in_progress", "in_review", "fixing", "merged", "blocked")
EMOJI = {
    "planned": "⬜ planned",
    "in_progress": "🟡 in progress",
    "in_review": "🔵 in review",
    "fixing": "🔧 fixing",
    "merged": "✅ merged",
    "blocked": "⛔ blocked",
    "wont_do": "won't do",
}
LEGEND = "⬜ planned · 🟡 in progress · 🔵 in review · 🔧 fixing · ✅ merged · ⛔ blocked"
PLANNED = {"backlog", "explored", "todo", "spec_review", "planned"}
REVIEW = {"code_review", "in_review", "merge_ready"}
NAME_RE = re.compile(r"^[a-z0-9]+(-[a-z0-9]+)*$")
BY = "orchestrator"


def die(msg):
    sys.exit(f"board.py: {msg}")


def now():
    return datetime.now(timezone.utc)


def iso(t):
    return t.strftime("%Y-%m-%dT%H:%M:%SZ")


def unwrap(raw, fallback_id):
    """A saved doc, flat or wrapped as {id, data, version}. Returns (id, fields)."""
    if isinstance(raw, dict) and isinstance(raw.get("data"), dict):
        return str(raw.get("id") or raw.get("doc_id") or fallback_id), raw["data"]
    if isinstance(raw, dict):
        return str(raw.get("id") or fallback_id), raw
    return fallback_id, {}


def load(saved):
    """Every saved doc under <saved>, grouped by collection (the parent folder name)."""
    docs = {"tasks": {}, "epicDocs": {}, "epicLog": {}}
    for root, _dirs, files in os.walk(saved):
        coll = os.path.basename(root)
        if coll not in docs:
            continue
        for f in sorted(files):
            if not f.endswith(".json"):
                continue
            with open(os.path.join(root, f), encoding="utf-8") as fh:
                doc_id, fields = unwrap(json.load(fh), f[: -len(".json")])
            docs[coll][doc_id] = fields
    return docs


def split(docs):
    tasks = docs["tasks"]
    epics = [(i, t) for i, t in tasks.items() if t.get("kind") == "epic"]
    if len(epics) != 1:
        die(f"expected one epic task (kind 'epic') in tasks/, found {len(epics)}")
    epic_id, epic = epics[0]
    subs = [(i, t) for i, t in tasks.items() if t.get("parentId") == epic_id and t.get("origin") != "follow_up"]
    subs.sort(key=lambda it: plan_key(it[1].get("planId") or it[0]))
    return epic_id, epic, subs


def plan_key(pid):
    m = re.match(r"^[A-Za-z]*(\d+)(.*)$", str(pid))
    return (0, int(m.group(1)), m.group(2)) if m else (1, 0, str(pid))


def ci_state(task):
    ci = task.get("ci")
    return ci.get("state") if isinstance(ci, dict) else ci


def derive(task):
    """The status word for a sub-task, from the board's fields (never stored)."""
    st = task.get("status")
    if st == "done":
        return "wont_do" if task.get("resolution") == "wont_do" else "merged"
    if st == "on_hold" or task.get("blockedBy") or task.get("pipelineStop"):
        return "blocked"
    if (st == "in_progress" and task.get("prUrl")) or ci_state(task) == "failure":
        return "fixing"
    if st in REVIEW:
        return "in_review"
    if st == "in_progress":
        return "in_progress"
    return "planned"


def pr_cell(task):
    url = task.get("prUrl") or ""
    m = re.search(r"/pull/(\d+)", url)
    return f"[#{m.group(1)}]({url})" if m else (url or "")


def cell(v):
    if isinstance(v, list):
        v = ", ".join(str(x) for x in v)
    return str(v or "").replace("|", "/").replace("\n", " ")


def status_table(subs):
    rows = [
        f"**Legend:** {LEGEND}",
        "",
        "| PR | Title | Lane | Model | Status | PR link | Depends on |",
        "| --- | --- | --- | --- | --- | --- | --- |",
    ]
    for tid, t in subs:
        rows.append(
            "| "
            + " | ".join(
                cell(x)
                for x in (
                    t.get("planId") or tid,
                    t.get("title"),
                    t.get("lane"),
                    t.get("model"),
                    EMOJI[derive(t)],
                    pr_cell(t),
                    t.get("dependsOn"),
                )
            )
            + " |"
        )
    return "\n".join(rows) + "\n"


def render_items(value):
    """decisions / answers / questions in whatever shape the board holds them."""
    if not value:
        return "_None._\n"
    if isinstance(value, dict):
        value = [{"q": k, "a": v} for k, v in value.items()]
    if not isinstance(value, list):
        return f"{value}\n"
    out = []
    for item in value:
        if isinstance(item, dict):
            head = item.get("q") or item.get("question") or item.get("title") or item.get("id") or ""
            body = item.get("a") or item.get("answer") or item.get("decision") or item.get("text") or ""
            if not head and not body:
                body = json.dumps(item, ensure_ascii=False)
            out.append(f"- **{head}** {body}" if head else f"- {body}")
        else:
            out.append(f"- {item}")
    return "\n".join(out) + "\n"


def write(path, text):
    with open(path, "w", encoding="utf-8") as fh:
        fh.write(text if text.endswith("\n") else text + "\n")


def cmd_export(saved, out):
    docs = load(saved)
    epic_id, epic, subs = split(docs)
    os.makedirs(out, exist_ok=True)
    edocs = {d.get("name") or i.split("--", 1)[-1]: d for i, d in docs["epicDocs"].items() if d.get("epic") in (None, epic_id)}
    written = []

    def doc_file(fname, name, missing):
        d = edocs.pop(name, None)
        body = d.get("body", "") if d else missing
        title = d.get("title") if d else None
        head = f"# {title}\n\n" if title and not body.lstrip().startswith("# ") else ""
        write(os.path.join(out, fname), head + body)
        written.append(fname)

    doc_file("BRIEF.md", "brief", "# Agent brief\n\n_No `brief` doc on the epic yet._")
    doc_file("RESUME.md", "resume", "# How to resume\n\n_No `resume` doc on the epic yet._")

    plan = [f"# {epic.get('title') or epic_id} ({epic_id})", ""]
    if epic.get("release"):
        plan += [f"Release branch: `{epic['release']}`. Every sub-task PR targets it.", ""]
    plan += ["## Status", "", status_table(subs), "## Plan", "", epic.get("spec") or "_No spec._", ""]
    plan += ["## Owner decisions", "", "Where a decision and the plan disagree, the decision wins.", ""]
    plan += [render_items(epic.get("decisions")), "## Answers", "", render_items(epic.get("answers"))]
    notes = edocs.pop("plan-notes", None)
    if notes:
        plan += ["## Plan notes", "", notes.get("body", "")]
    write(os.path.join(out, "PLAN.md"), "\n".join(plan))
    written.append("PLAN.md")

    entries = [e for e in docs["epicLog"].values() if e.get("epic") in (None, epic_id)]
    entries.sort(key=lambda e: e.get("at") or "", reverse=True)
    log = ["# Build log (newest first)", ""]
    for e in entries:
        log.append(f"- {e.get('at', '')} [{e.get('kind', 'note')}] {e.get('text', '')}")
        for it in e.get("items") or []:
            log.append(
                f"  - {it.get('planId', '')}: {EMOJI.get(it.get('status'), it.get('status', ''))}"
                + (f" #{it['pr']}" if it.get("pr") else "")
                + (f" {it['note']}" if it.get("note") else "")
            )
    write(os.path.join(out, "LOG.md"), "\n".join(log))
    written.append("LOG.md")

    for tid, t in subs:
        pid = t.get("planId") or tid
        meta = [
            f"# {pid}: {t.get('title', '')}",
            "",
            f"- Task: `{tid}`",
            f"- Lane: {t.get('lane') or '-'} · Model: {t.get('model') or '-'} · Status: {EMOJI[derive(t)]}",
            f"- Depends on: {cell(t.get('dependsOn')) or '-'} · Stacks on: {cell(t.get('stackOn')) or '-'}",
            f"- Branch: {t.get('branch') or '-'} · PR: {t.get('prUrl') or '-'}",
            "",
            t.get("spec") or "_No spec._",
        ]
        fname = f"PR-{re.sub(r'[^A-Za-z0-9_.-]', '_', str(pid))}.md"
        write(os.path.join(out, fname), "\n".join(meta))
        written.append(fname)

    for name, d in sorted(edocs.items()):
        fname = f"{name}.md"
        write(os.path.join(out, fname), (f"# {d['title']}\n\n" if d.get("title") else "") + d.get("body", ""))
        written.append(fname)

    for f in written:
        print(os.path.join(out, f))


def cmd_status(saved):
    _epic_id, _epic, subs = split(load(saved))
    sys.stdout.write(status_table(subs))


def out_path(prefix):
    fd, path = tempfile.mkstemp(prefix=prefix, suffix=".json", dir=os.environ.get("BOARD_OUT"))
    os.close(fd)
    return path


def parse_item(epic, arg):
    if "=" not in arg:
        die(f"item '{arg}' is not planId=status:pr:note")
    pid, rest = arg.split("=", 1)
    parts = rest.split(":", 2) + ["-", "-"]
    status, pr, note = parts[0], parts[1], parts[2]
    if status not in STATUS_WORDS:
        die(f"status '{status}' for {pid} is not one of {', '.join(STATUS_WORDS)}")
    item = {"planId": pid, "task": f"{epic}--{pid.lower()}", "status": status}
    if pr not in ("-", ""):
        item["pr"] = int(pr) if pr.isdigit() else pr
    if note not in ("-", ""):
        item["note"] = note
    return item


def cmd_log(epic, project, kind, text, items):
    if kind not in LOG_KINDS:
        die(f"kind '{kind}' is not one of {', '.join(LOG_KINDS)}")
    if items and kind != "status":
        die("items are only for kind 'status'")
    t = now()
    suffix = "".join(random.choice(string.ascii_lowercase) for _ in range(4))
    doc_id = f"{epic}--{t.strftime('%Y%m%d%H%M%S')}-{suffix}"
    doc = {"epic": epic, "project": project, "at": iso(t), "by": BY, "kind": kind, "text": text}
    if items:
        doc["items"] = [parse_item(epic, a) for a in items]
    path = out_path("epiclog-")
    write(path, json.dumps(doc, ensure_ascii=False, indent=2))
    print(path)
    print(f"collection=epicLog doc_id={doc_id}")


def cmd_doc(epic, project, name, md, current, summary, if_version=None):
    if not NAME_RE.match(name) or len(name) > 60:
        die(f"name '{name}' must be kebab-case, at most 60 characters")
    if not str(current).isdigit():
        die("current version must be a whole number (0 for a new doc)")
    with open(md, encoding="utf-8") as fh:
        body = fh.read()
    m = re.match(r"^# (.+)$", body, re.M)
    title = m.group(1).strip() if m else name.replace("-", " ").capitalize()
    version = int(current) + 1
    at = iso(now())
    entry = {
        "op": "set",
        "collection": "epicDocs",
        "doc_id": f"{epic}--{name}",
        "data": {
            "epic": epic,
            "project": project,
            "name": name,
            "title": title,
            "body": body,
            "version": version,
            "updatedAt": at,
            "by": BY,
        },
    }
    if if_version:
        entry["if_version"] = int(if_version)
    elif int(current) > 0:
        print("warning: the epicDocs doc exists; pass its database version as if_version", file=sys.stderr)
    copy = {
        "op": "set",
        "collection": "epicDocVersions",
        "doc_id": f"{epic}--{name}--v{version}",
        "data": {"epic": epic, "name": name, "version": version, "body": body, "at": at, "by": BY, "summary": summary},
    }
    path = out_path("epicdoc-")
    write(path, json.dumps({"writes": [entry, copy]}, ensure_ascii=False, indent=2))
    print(path)


def first_text(path):
    """The text of a transcript's first message, or ''."""
    with open(path, encoding="utf-8", errors="replace") as fh:
        for line in fh:
            try:
                c = json.loads(line)["message"]["content"]
            except Exception:
                continue
            if isinstance(c, list):
                c = "".join(x.get("text", "") for x in c if isinstance(x, dict))
            return c if isinstance(c, str) else ""
    return ""


def transcripts_for(run_id):
    root = os.environ.get("CLAUDE_PROJECTS") or os.path.expanduser("~/.claude/projects")
    pat = re.compile(r"run: " + re.escape(run_id) + r"(\s|$)")
    files = glob.glob(os.path.join(root, "*", "*", "subagents", "*.jsonl"))
    return sorted((f for f in files if pat.match(first_text(f).lstrip())), key=os.path.getmtime)


def run_tokens(files):
    """(tokens or None, model or None, earliest timestamp or None) over the transcripts."""
    tot = {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "turns": 0}
    first, models, start = None, {}, None
    for f in files:
        by = {}
        with open(f, encoding="utf-8", errors="replace") as fh:
            for i, line in enumerate(fh):
                try:
                    d = json.loads(line)
                except Exception:
                    continue
                ts = d.get("timestamp") if isinstance(d, dict) else None
                if isinstance(ts, str) and (start is None or ts < start):
                    start = ts
                m = d.get("message") if isinstance(d, dict) else None
                u = m.get("usage") if isinstance(m, dict) else None
                if u:
                    by[m.get("id") or i] = u
                    if m.get("model"):
                        models[m["model"]] = models.get(m["model"], 0) + 1
        for u in by.values():
            tot["input"] += u.get("input_tokens") or 0
            tot["output"] += u.get("output_tokens") or 0
            tot["cacheRead"] += u.get("cache_read_input_tokens") or 0
            tot["cacheWrite"] += u.get("cache_creation_input_tokens") or 0
        tot["turns"] += len(by)
        if by and first is None:
            first = next(iter(by.values()))
    if first is None:
        return None, None, start
    tot["firstContext"] = sum(first.get(k) or 0 for k in ("input_tokens", "cache_read_input_tokens", "cache_creation_input_tokens"))
    return tot, max(models, key=models.get) if models else None, start


def cmd_runid(task):
    print(f"run_{now().strftime('%Y%m%d%H%M%S')}_session_{task}")


def cmd_run(run_id, task, project, status, result, summary, stages="", group=""):
    if status not in ("done", "failed"):
        die("status must be done or failed")
    tokens, model, start = run_tokens(transcripts_for(run_id))
    at = iso(now())
    metrics = {"model": model, "tokens": tokens}
    if tokens is None:
        metrics["note"] = f"no subagent transcript starts with 'run: {run_id}'"
    doc = {
        "kind": "session",
        "trigger": "session",
        "by": "session",
        "task": task,
        "project": project,
        "status": status,
        "startedAt": start or at,
        "finishedAt": at,
        "updatedAt": at,
        "progress": 100,
        "result": result,
        "summary": summary,
        "stages": [x for x in stages.split(",") if x],
        "metrics": metrics,
    }
    if group:
        doc["group"] = {"id": group}
    path = out_path("run-")
    write(path, json.dumps(doc, ensure_ascii=False, indent=2))
    print(path)
    print(f"collection=runs doc_id={run_id}")


def main(argv):
    if not argv:
        die("no command; see the usage at the top of this file")
    cmd, args = argv[0], argv[1:]
    if cmd == "export" and len(args) == 2:
        return cmd_export(*args)
    if cmd == "status" and len(args) == 1:
        return cmd_status(*args)
    if cmd == "log" and len(args) >= 4:
        return cmd_log(args[0], args[1], args[2], args[3], args[4:])
    if cmd == "doc" and len(args) in (6, 7):
        return cmd_doc(*args)
    if cmd == "runid" and len(args) == 1:
        return cmd_runid(*args)
    if cmd == "run" and 6 <= len(args) <= 8:
        return cmd_run(*args)
    die(f"bad arguments for '{cmd}'; see the usage at the top of this file")


if __name__ == "__main__":
    main(sys.argv[1:])
