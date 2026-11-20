#!/usr/bin/env python3
"""Tests for board.py on small fixtures. Run: python3 -I .claude/skills/epic-orchestration/scripts/test_board.py"""

import json
import os
import re
import subprocess
import sys
import tempfile
import unittest

BOARD = os.path.join(os.path.dirname(os.path.abspath(__file__)), "board.py")
EPIC = "ink-90"


def run(*args, env=None):
    res = subprocess.run(
        [sys.executable, "-I", BOARD, *args], capture_output=True, text=True, env={**os.environ, **(env or {})}
    )
    return res.returncode, res.stdout, res.stderr


def text(path):
    with open(path, encoding="utf-8") as fh:
        return fh.read()


def save(root, coll, doc_id, data, wrapped=False):
    os.makedirs(os.path.join(root, coll), exist_ok=True)
    body = {"id": doc_id, "version": 3, "data": data} if wrapped else data
    with open(os.path.join(root, coll, f"{doc_id}.json"), "w", encoding="utf-8") as fh:
        json.dump(body, fh)


def sub(key, plan_id, status, **extra):
    return f"{EPIC}--{key}", {
        "parentId": EPIC,
        "origin": "subtask",
        "planId": plan_id,
        "title": f"Work {plan_id}",
        "lane": "db",
        "model": "sonnet",
        "status": status,
        "spec": f"Spec of {plan_id}.",
        **extra,
    }


class BoardTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.saved = os.path.join(self.tmp.name, "saved")
        save(
            self.saved,
            "tasks",
            EPIC,
            {
                "kind": "epic",
                "title": "Consent",
                "release": "release/0.1.4",
                "spec": "Scope and design.",
                "decisions": [{"q": "Q1 PDFs?", "a": "Yes, keep them."}],
            },
            wrapped=True,
        )
        fixtures = [
            sub("p10", "P10", "planned"),
            sub("p2", "P2", "in_progress"),
            sub("p3", "P3", "in_progress", prUrl="https://github.com/o/r/pull/12"),
            sub("p4", "P4", "code_review", prUrl="https://github.com/o/r/pull/13"),
            sub("p5", "P5", "code_review", prUrl="https://github.com/o/r/pull/14", ci={"state": "failure"}),
            sub("p6", "P6", "done", resolution="completed", prUrl="https://github.com/o/r/pull/9"),
            sub("p7", "P7", "todo", blockedBy=["ink-1"]),
            sub("p1", "P1", "on_hold"),
        ]
        for doc_id, data in fixtures:
            save(self.saved, "tasks", doc_id, data)
        save(self.saved, "tasks", "ink-91", {"parentId": EPIC, "origin": "follow_up", "title": "Later"})
        save(self.saved, "epicDocs", f"{EPIC}--brief", {"epic": EPIC, "name": "brief", "title": "Agent brief", "body": "Rules.", "version": 2})
        save(self.saved, "epicDocs", f"{EPIC}--resume", {"epic": EPIC, "name": "resume", "body": "# How to resume\n\nStart here.", "version": 1})
        save(self.saved, "epicDocs", f"{EPIC}--owner-actions", {"epic": EPIC, "name": "owner-actions", "body": "Set the secret.", "version": 1})
        save(self.saved, "epicLog", f"{EPIC}--20261117100000-abcd", {"epic": EPIC, "at": "2026-11-17T10:00:00Z", "kind": "note", "text": "older"})
        save(
            self.saved,
            "epicLog",
            f"{EPIC}--20261118100000-efgh",
            {"epic": EPIC, "at": "2026-11-18T10:00:00Z", "kind": "status", "text": "newer", "items": [{"planId": "P6", "status": "merged", "pr": 9}]},
        )

    def tearDown(self):
        self.tmp.cleanup()

    def statuses(self, table):
        out = {}
        for line in table.splitlines():
            cells = [c.strip() for c in line.split("|")]
            if len(cells) > 6 and re.match(r"^P\d+$", cells[1]):
                out[cells[1]] = cells[5]
        return out

    def test_status_is_derived_and_ordered(self):
        code, out, err = run("status", self.saved)
        self.assertEqual(code, 0, err)
        st = self.statuses(out)
        self.assertEqual(list(st), ["P1", "P2", "P3", "P4", "P5", "P6", "P7", "P10"])
        self.assertEqual(
            st,
            {
                "P1": "⛔ blocked",
                "P2": "🟡 in progress",
                "P3": "🔧 fixing",
                "P4": "🔵 in review",
                "P5": "🔧 fixing",
                "P6": "✅ merged",
                "P7": "⛔ blocked",
                "P10": "⬜ planned",
            },
        )
        self.assertIn("[#13](https://github.com/o/r/pull/13)", out)
        self.assertNotIn("Later", out)

    def test_export_writes_agent_files(self):
        out_dir = os.path.join(self.tmp.name, "agents")
        code, out, err = run("export", self.saved, out_dir)
        self.assertEqual(code, 0, err)
        files = sorted(os.listdir(out_dir))
        self.assertEqual(
            files,
            ["BRIEF.md", "LOG.md", "PLAN.md", "PR-P1.md", "PR-P10.md", "PR-P2.md", "PR-P3.md", "PR-P4.md", "PR-P5.md", "PR-P6.md", "PR-P7.md", "RESUME.md", "owner-actions.md"],
        )
        read = lambda f: text(os.path.join(out_dir, f))
        self.assertEqual(read("BRIEF.md"), "# Agent brief\n\nRules.\n")
        self.assertEqual(read("RESUME.md"), "# How to resume\n\nStart here.\n")
        log = read("LOG.md")
        self.assertLess(log.index("newer"), log.index("older"))
        self.assertIn("P6: ✅ merged #9", log)
        plan = read("PLAN.md")
        self.assertIn("release/0.1.4", plan)
        self.assertIn("Scope and design.", plan)
        self.assertIn("- **Q1 PDFs?** Yes, keep them.", plan)
        self.assertIn("| P4 | Work P4 | db | sonnet | 🔵 in review |", plan)
        self.assertIn("Spec of P3.", read("PR-P3.md"))
        self.assertIn("Task: `ink-90--p3`", read("PR-P3.md"))

    def test_log_writes_one_entry(self):
        code, out, err = run("log", EPIC, "ink-booking", "status", "P3 merged", "P3=merged:12:clean", "P4=in_review", env={"BOARD_OUT": self.tmp.name})
        self.assertEqual(code, 0, err)
        path, ids = out.strip().splitlines()
        self.assertRegex(ids, r"^collection=epicLog doc_id=ink-90--\d{14}-[a-z]{4}$")
        doc = json.loads(text(path))
        self.assertRegex(doc["at"], r"^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ$")
        self.assertEqual(ids.split("--")[1][:14], re.sub(r"\D", "", doc["at"]))
        del doc["at"]
        self.assertEqual(
            doc,
            {
                "epic": EPIC,
                "project": "ink-booking",
                "by": "orchestrator",
                "kind": "status",
                "text": "P3 merged",
                "items": [
                    {"planId": "P3", "task": "ink-90--p3", "status": "merged", "pr": 12, "note": "clean"},
                    {"planId": "P4", "task": "ink-90--p4", "status": "in_review"},
                ],
            },
        )

    def test_log_refuses_bad_input(self):
        self.assertNotEqual(run("log", EPIC, "ink-booking", "gossip", "x")[0], 0)
        self.assertNotEqual(run("log", EPIC, "ink-booking", "note", "x", "P1=merged")[0], 0)
        self.assertNotEqual(run("log", EPIC, "ink-booking", "status", "x", "P1=done")[0], 0)

    def test_doc_writes_a_versioned_batch(self):
        md = os.path.join(self.tmp.name, "brief.md")
        with open(md, "w", encoding="utf-8") as fh:
            fh.write("# Agent brief\n\nNew rules.\n")
        code, out, err = run("doc", EPIC, "ink-booking", "brief", md, "2", "Adds rules", "7", env={"BOARD_OUT": self.tmp.name})
        self.assertEqual(code, 0, err)
        writes = json.loads(text(out.strip()))["writes"]
        self.assertEqual([(w["op"], w["collection"], w["doc_id"]) for w in writes], [("set", "epicDocs", "ink-90--brief"), ("set", "epicDocVersions", "ink-90--brief--v3")])
        self.assertEqual(writes[0]["if_version"], 7)
        self.assertNotIn("if_version", writes[1])
        self.assertEqual(writes[0]["data"]["version"], 3)
        self.assertEqual(writes[0]["data"]["title"], "Agent brief")
        self.assertEqual(writes[1]["data"]["summary"], "Adds rules")
        self.assertEqual(writes[1]["data"]["body"], "# Agent brief\n\nNew rules.\n")

    def test_doc_refuses_bad_names(self):
        md = os.path.join(self.tmp.name, "x.md")
        open(md, "w", encoding="utf-8").close()
        self.assertNotEqual(run("doc", EPIC, "ink-booking", "Bad_Name", md, "0", "s")[0], 0)
        self.assertNotEqual(run("doc", EPIC, "ink-booking", "a" * 61, md, "0", "s")[0], 0)

    def test_runid_names_the_task(self):
        code, out, _ = run("runid", "ink-90--p07")
        self.assertEqual(code, 0)
        self.assertRegex(out.strip(), r"^run_\d{14}_session_ink-90--p07$")

    def test_run_sums_every_subagent_brief_for_the_run(self):
        root = os.path.join(self.tmp.name, "projects")
        agents = os.path.join(root, "proj", "sess", "subagents")
        os.makedirs(agents)

        def transcript(name, brief, usages):
            lines = [{"timestamp": "2026-11-20T10:00:00Z", "message": {"role": "user", "content": brief}}]
            for i, u in enumerate(usages):
                lines.append({"timestamp": f"2026-11-20T10:0{i + 1}:00Z", "message": {"id": f"{name}{i}", "model": "m1", "usage": u}})
            with open(os.path.join(agents, name + ".jsonl"), "w", encoding="utf-8") as fh:
                fh.write("\n".join(json.dumps(x) for x in lines))

        u = {"input_tokens": 10, "output_tokens": 5, "cache_read_input_tokens": 100, "cache_creation_input_tokens": 20}
        transcript("impl", "run: run_1_session_ink-90--p07\nBuild P07", [u, u])
        transcript("rev", [{"type": "text", "text": "run: run_1_session_ink-90--p07\nReview"}], [u])
        transcript("other", "run: run_1_session_ink-90--p070\nNot this one", [u])
        code, out, err = run("run", "run_1_session_ink-90--p07", "ink-90--p07", "ink-booking", "done", "PR open", "Built it.", "implement,review", "ses_x",
                             env={"BOARD_OUT": self.tmp.name, "CLAUDE_PROJECTS": root})
        self.assertEqual(code, 0, err)
        path, ref = out.strip().split("\n")
        self.assertEqual(ref, "collection=runs doc_id=run_1_session_ink-90--p07")
        doc = json.loads(text(path))
        self.assertEqual((doc["kind"], doc["trigger"], doc["status"], doc["task"]), ("session", "session", "done", "ink-90--p07"))
        self.assertEqual(doc["stages"], ["implement", "review"])
        self.assertEqual(doc["group"], {"id": "ses_x"})
        self.assertEqual(doc["startedAt"], "2026-11-20T10:00:00Z")
        self.assertEqual(doc["metrics"]["tokens"], {"input": 30, "output": 15, "cacheRead": 300, "cacheWrite": 60, "turns": 3, "firstContext": 130})
        self.assertEqual(doc["metrics"]["model"], "m1")

    def test_run_without_transcripts_says_why(self):
        code, out, err = run("run", "run_2", "ink-90--p01", "ink-booking", "failed", "x", "y",
                             env={"BOARD_OUT": self.tmp.name, "CLAUDE_PROJECTS": os.path.join(self.tmp.name, "none")})
        self.assertEqual(code, 0, err)
        doc = json.loads(text(out.strip().split("\n")[0]))
        self.assertIsNone(doc["metrics"]["tokens"])
        self.assertIn("run: run_2", doc["metrics"]["note"])
        self.assertNotEqual(run("run", "run_2", "t", "p", "maybe", "x", "y")[0], 0)


if __name__ == "__main__":
    unittest.main()
