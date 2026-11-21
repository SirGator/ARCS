"""External Rules peer for the revision-aware repair process test."""
import json
import pathlib
import sqlite3
import sys

PLAN = "arcs.plan_candidate.v2"
EDIT = "arcs.workspace_edit_candidate.v1"
CHECK = "arcs.workspace_revision_check_candidate.v1"
EDIT_RECEIPT = "arcs.execution_result.workspace_edit.v1"
CHECK_RECEIPT = "arcs.execution_result.workspace_revision_check.v2"
CONTEXT = "arcs.plan_replanning.v2"
ID = "workspace.replanner"

profile_path = pathlib.Path(sys.argv[1])
profile = json.loads(profile_path.read_text())
db = sqlite3.connect(profile_path.with_suffix(".sqlite"))
db.execute("CREATE TABLE IF NOT EXISTS calls (id TEXT PRIMARY KEY, input TEXT, response TEXT)")


def context(inv, version, schema):
    return next(item for item in inv["context"]
                if item["version_id"] == version and item["schema_id"] == schema)


def dependency(inv):
    binding, = inv["constraints"]["task_dependencies"]
    selection, = binding["context"]
    return context(inv, selection["version_id"], EDIT_RECEIPT)


def propose(inv):
    target = inv["target_schema_id"]
    required = []
    if target == PLAN:
        replan = inv["constraints"].get("task_replanning")
        if not replan:
            payload = {"steps": [
                {"id": "edit", "template": "edit"},
                {"id": "check", "template": "check",
                 "dependencies": [{"step_id": "edit", "payload_fields": ["result"]}]},
            ]}
        else:
            digest = next(item for item in inv["context"]
                          if item["version_id"] == replan["context_version_id"])
            if digest["schema_id"] != CONTEXT:
                return None, required
            check = next(step for step in digest["payload"]["steps"] if step["id"] == "check")
            edit = next(step for step in digest["payload"]["steps"] if step["id"] == "edit")
            assert check["resolution"] == "blocked" and check["output_version_ids"] == []
            failure = context(inv, check["failure_receipt_version_id"], CHECK_RECEIPT)["payload"]
            previous, = edit["output_version_ids"]
            applied = context(inv, previous, EDIT_RECEIPT)["payload"]["result"]
            if "diagnostics" not in failure:
                assert set(failure) == {"result"}
                return None, required
            assert set(failure) == {"result", "diagnostics"}
            assert "AssertionError: 6 != 5" in failure["diagnostics"]["stderr"]["text"]
            result = failure["result"]
            assert result["status"] == "failed" and result["exit_code"] != 0
            assert result["revision"] == applied["commit"]
            payload = {"steps": [
                {"id": "edit", "template": "retry-edit", "dependencies": [
                    {"step_id": "edit", "previous_revision": True, "payload_fields": ["result"]}
                ]},
                {"id": "check", "template": "retry-check",
                 "dependencies": [{"step_id": "edit", "payload_fields": ["result"]}]},
            ]}
    else:
        required = inv["allowed_capabilities"]
        result = dependency(inv)["payload"]["result"]
        assert result["status"] == "applied"
        task_id = inv["constraints"]["goal_runtime"]["task_version_id"]
        task = context(inv, task_id, "arcs.task.workspace_repair.v1")["payload"]
        if target == EDIT:
            payload = {"path": result["path"], "base_commit": result["commit"],
                       "replacement": profile["replacement"]}
        elif target == CHECK:
            payload = {"check": task["check"], "revision": result["commit"]}
        else:
            raise AssertionError(target)
    return payload, required


def reply_to(inv):
    key = inv["invocation_id"]
    raw = json.dumps(inv, sort_keys=True)
    saved = db.execute("SELECT input,response FROM calls WHERE id=?", (key,)).fetchone()
    if saved:
        assert saved[0] == raw
        return json.loads(saved[1])
    payload, required = propose(inv)
    candidates = [] if payload is None else [{
        "schema_id": inv["target_schema_id"],
        "required_capabilities": required,
        "referenced_versions": [item["version_id"] for item in inv["context"]],
        "payload": payload,
    }]
    reply = {"type": "reasoning_result_v2", "response": {
        "invocation_id": key, "request_id": inv["request_id"],
        "status": "solved" if candidates else "no_solution", "candidates": candidates,
        "trace": {"method": inv["method"], "implementation": "workspace-retry-test",
                  "version": "1", "iterations": 1, "details": {}},
    }}
    with db:
        db.execute("INSERT INTO calls VALUES(?,?,?)", (key, raw, json.dumps(reply)))
    return reply


for line in sys.stdin:
    envelope = json.loads(line)
    message = envelope["message"]
    if message["type"] == "hello":
        response = {"type": "manifest", "manifest": {
            "protocol_version": 1, "adapter_id": ID, "adapter_version": "1",
            "capabilities": [{"id": "solution.propose",
                              "contract": {"kind": "reason", "emits": [PLAN, EDIT, CHECK]},
                              "required_permissions": ["reason.compute"]}],
        }}
    elif message["type"] == "reason_v2":
        response = reply_to(message["invocation"])
    elif message["type"] == "shutdown":
        response = {"type": "bye"}
    else:
        raise AssertionError(message["type"])
    print(json.dumps({"protocol_version": 1, "request_id": envelope["request_id"],
                      "message": response}), flush=True)
    if response["type"] == "bye":
        break
