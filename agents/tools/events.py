#!/usr/bin/env python3
"""Query the structured event log produced by Relux.

Subcommands answer the post-mortem questions an agent (or human) typically
asks of an `events.json`: what was visible at the failure point, what
shell looked like at a given seq, how spans nest, which effects were
deduplicated, etc.

The schema this script targets is documented in
`references/events-schema.md` and `references/events-failures.md`.
"""

import argparse
import json
import sys
from pathlib import Path

TRANSPARENT_IMPURE_BIFS = {"annotate", "log", "sleep"}


def load(path):
    with open(path) as f:
        return json.load(f)


def emit_json(value):
    json.dump(value, sys.stdout, indent=2)
    sys.stdout.write("\n")


def span_by_id(data, sid):
    if sid is None:
        return None
    return data.get("spans", {}).get(str(sid))


def event_at(data, seq):
    for ev in data.get("events", []):
        if ev["seq"] == seq:
            return ev
    return None


def shell_matches(event_or_buffer, shell_arg):
    if shell_arg is None:
        return True
    return (
        event_or_buffer.get("shell") == shell_arg
        or event_or_buffer.get("shell_marker") == shell_arg
    )


def is_transparent_bif(span):
    if not span or span.get("kind") != "fn-call" or span.get("callee_kind") != "bif":
        return False
    return span.get("is_pure") or span.get("name") in TRANSPARENT_IMPURE_BIFS


def scope_context(data, span_id):
    """Walk ancestors. Return (ambient_scope_id, innermost_fn_id_or_None).

    `effect-cleanup` hops to its `setup_span` so cleanup sees the
    effect's lets/overlay, not the test's. `fn-call` is a hard barrier
    once seen unless it is a transparent BIF (pure, or annotate/log/
    sleep -- see the runtime's same rule).
    """
    innermost_fn = None
    cur = span_by_id(data, span_id)
    while cur:
        kind = cur["kind"]
        if kind in ("test", "effect-setup"):
            return cur["id"], innermost_fn
        if kind == "effect-cleanup":
            return cur["setup_span"], innermost_fn
        if kind == "fn-call" and not is_transparent_bif(cur) and innermost_fn is None:
            innermost_fn = cur["id"]
        parent = cur.get("parent")
        if parent is None:
            break
        cur = span_by_id(data, parent)
    return None, innermost_fn


def vars_visible(data, event_seq, viewer_span_id, viewer_shell_marker):
    """Reconstruct the vars visible at (event_seq, span, shell_marker).

    Mirrors the runtime's three-map model: ambient scope (test or
    effect-setup) at the bottom, shell-local lets on top of that, fn-call
    frames as a hard barrier.
    """
    spans = data.get("spans", {})
    scope_vars, shell_vars, frame_vars = {}, {}, {}

    for span in spans.values():
        if span and span["kind"] == "fn-call" and not is_transparent_bif(span):
            frame_vars[span["id"]] = dict(span.get("args", []))

    for ev in data.get("events", []):
        if ev["seq"] > event_seq:
            break
        ambient, inner_fn = scope_context(data, ev["span"])
        ev_shell = ev.get("shell_marker")

        if ev["kind"] == "var-let":
            name, value = ev["name"], ev["value"]
            if inner_fn is not None:
                frame_vars.setdefault(inner_fn, {})[name] = value
            elif ev_shell is not None:
                shell_vars.setdefault(ev_shell, {})[name] = value
            elif ambient is not None:
                scope_vars.setdefault(ambient, {})[name] = value

        elif ev["kind"] == "var-assign":
            name, value = ev["name"], ev["value"]
            for m in (
                frame_vars.get(inner_fn) if inner_fn is not None else None,
                shell_vars.get(ev_shell) if ev_shell else None,
                scope_vars.get(ambient) if ambient is not None else None,
            ):
                if m and name in m:
                    m[name] = value
                    break

        elif ev["kind"] == "effect-expose-var":
            emitter = span_by_id(data, ev["span"])
            if not emitter or emitter["kind"] != "effect-setup":
                continue
            alias = emitter.get("alias")
            parent = emitter.get("parent")
            if alias is None or parent is None:
                continue
            parent_ambient, _ = scope_context(data, parent)
            if parent_ambient is None:
                continue
            scope_vars.setdefault(parent_ambient, {})[f"{alias}.{ev['name']}"] = ev["value"]

    view_ambient, view_fn = scope_context(data, viewer_span_id)
    if view_fn is not None:
        return dict(frame_vars.get(view_fn, {}))

    out = dict(scope_vars.get(view_ambient, {})) if view_ambient is not None else {}
    if viewer_shell_marker is not None:
        out.update(shell_vars.get(viewer_shell_marker, {}))
    return out


def default_failure_vantage(data):
    """When the test failed/cancelled, prefer its failure point as the
    vantage. Otherwise the last event."""
    outcome = data.get("outcome", {})
    if outcome.get("kind") in ("fail", "cancelled"):
        seq = outcome.get("event_seq")
        span = outcome.get("span")
        if seq is not None and span is not None:
            ev = event_at(data, seq)
            shell = ev.get("shell_marker") if ev else None
            return seq, span, shell
    events = data.get("events", [])
    if not events:
        return None, None, None
    last = events[-1]
    return last["seq"], last["span"], last.get("shell_marker")


def cmd_vars(args):
    data = load(args.events)
    seq, span, shell = default_failure_vantage(data)
    if args.at_seq is not None:
        seq = args.at_seq
        ev = event_at(data, seq)
        if ev:
            if args.span is None:
                span = ev["span"]
            if args.shell is None:
                shell = ev.get("shell_marker")
    if args.span is not None:
        span = args.span
    if args.shell is not None:
        shell = args.shell

    if seq is None or span is None:
        sys.stderr.write("no vantage available (empty event stream and no failure)\n")
        sys.exit(2)

    result = vars_visible(data, seq, span, shell)
    if args.with_env:
        env = {e["key"]: e["value"] for e in data.get("env", {}).get("bootstrap", [])}
        env.update(result)
        result = env
    emit_json({
        "vantage": {"event_seq": seq, "span": span, "shell_marker": shell},
        "vars": result,
    })


def cmd_stack(args):
    data = load(args.events)
    span_id = args.span
    if span_id is None:
        seq = args.at_seq
        if seq is None:
            outcome = data.get("outcome", {})
            seq = outcome.get("event_seq")
        if seq is None:
            sys.stderr.write("need --span or --at-seq, and no outcome vantage available\n")
            sys.exit(2)
        ev = event_at(data, seq)
        if ev is None:
            sys.stderr.write(f"no event at seq {seq}\n")
            sys.exit(2)
        span_id = ev["span"]

    out = []
    cur = span_by_id(data, span_id)
    while cur is not None:
        frame = {"id": cur["id"], "kind": cur["kind"]}
        for k in ("name", "effect", "alias", "shell"):
            if k in cur and cur[k] is not None:
                frame[k] = cur[k]
        out.append(frame)
        parent = cur.get("parent")
        cur = span_by_id(data, parent) if parent is not None else None
    emit_json(out)


def cmd_buffer(args):
    data = load(args.events)
    cap = args.at_seq if args.at_seq is not None else None
    out = []
    for b in data.get("buffer_events", []):
        if cap is not None and b["seq"] > cap:
            break
        if b.get("kind") != "grew":
            continue
        if not shell_matches(b, args.shell):
            continue
        out.append(b.get("data", ""))
    sys.stdout.write("".join(out))
    if out and not out[-1].endswith("\n"):
        sys.stdout.write("\n")


def cmd_timeline(args):
    data = load(args.events)
    merged = []
    for ev in data.get("events", []):
        if shell_matches(ev, args.shell):
            merged.append({"seq": ev["seq"], "kind": ev["kind"], "src": "event"})
    for b in data.get("buffer_events", []):
        if shell_matches(b, args.shell):
            merged.append({"seq": b["seq"], "kind": b["kind"], "src": "buffer"})
    merged.sort(key=lambda r: r["seq"])
    emit_json(merged)


def cmd_dedup(args):
    data = load(args.events)
    groups = {}
    for sp in data.get("spans", {}).values():
        if sp.get("kind") != "effect-setup":
            continue
        marker = sp.get("marker")
        groups.setdefault(marker, []).append({
            "span": sp["id"],
            "effect": sp.get("effect"),
            "alias": sp.get("alias"),
            "is_reuse": sp.get("is_reuse"),
            "overlay": sp.get("overlay"),
        })
    out = [
        {"marker": k, "acquires": sorted(v, key=lambda r: r["span"])}
        for k, v in groups.items()
    ]
    out.sort(key=lambda g: min(a["span"] for a in g["acquires"]))
    emit_json(out)


def cmd_pure_trace(args):
    data = load(args.events)
    root = args.span
    spans = data.get("spans", {})

    descendants = {root}
    changed = True
    while changed:
        changed = False
        for sp in spans.values():
            if sp.get("parent") in descendants and sp["id"] not in descendants:
                descendants.add(sp["id"])
                changed = True

    kinds = {"interpolation", "var-let", "var-read", "var-assign", "pure-match", "string-eval"}
    out = []
    for ev in data.get("events", []):
        if ev["span"] not in descendants:
            continue
        if ev["kind"] not in kinds:
            continue
        out.append({
            "seq": ev["seq"],
            "span": ev["span"],
            "kind": ev["kind"],
            **{k: v for k, v in ev.items() if k not in ("seq", "span", "kind", "ts")},
        })
    emit_json(out)


def cmd_diff(args):
    a = load(args.events)
    b = load(args.other)

    # `seq` is unstable across runs (different PTY chunking shifts the shared
    # event/buffer counter) and `buffer_events` are the chunking layer itself,
    # so comparing on either tells you about kernel timing, not test
    # behaviour. The runtime is deterministic at (kind, shell_marker, source)
    # for a given test, so we align events by index on that key.
    def key(e):
        loc = e.get("source") or {}
        return (e["kind"], e.get("shell_marker"), loc.get("file"), loc.get("line"))

    def summary(e):
        out = {"seq": e["seq"], "kind": e["kind"], "shell_marker": e.get("shell_marker")}
        loc = e.get("source")
        if loc:
            out["source"] = {"file": loc.get("file"), "line": loc.get("line")}
        return out

    ea = a.get("events", [])
    eb = b.get("events", [])
    n = min(len(ea), len(eb))
    for i in range(n):
        if key(ea[i]) != key(eb[i]):
            emit_json({
                "first_divergence_index": i,
                "left": summary(ea[i]),
                "right": summary(eb[i]),
            })
            return
    if len(ea) != len(eb):
        emit_json({
            "first_divergence_index": n,
            "left_length": len(ea),
            "right_length": len(eb),
            "note": "event streams agree on (kind, shell_marker, source) for the entire overlap; one continues longer",
        })
        return
    emit_json({"identical": True, "length": n})


def build_parser():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument(
        "--events",
        default="events.json",
        type=Path,
        help="Path to events.json (default: events.json in cwd).",
    )
    sub = p.add_subparsers(dest="cmd", required=True)

    sp = sub.add_parser("vars", help="Variables visible at a vantage.")
    sp.add_argument("--at-seq", type=int, help="Event seq to view from. Default: failure event, else last event.")
    sp.add_argument("--span", type=int, help="Span id of the viewer. Default: span at --at-seq.")
    sp.add_argument("--shell", help="Shell marker of the viewer. Default: shell_marker at --at-seq.")
    sp.add_argument("--with-env", action="store_true", help="Layer env.bootstrap underneath the user vars.")
    sp.set_defaults(func=cmd_vars)

    sp = sub.add_parser("stack", help="Call stack from a span to root.")
    sp.add_argument("--span", type=int, help="Span id. Default: span at --at-seq or failure event.")
    sp.add_argument("--at-seq", type=int, help="Event seq. Used when --span is omitted.")
    sp.set_defaults(func=cmd_stack)

    sp = sub.add_parser(
        "buffer",
        help="Cumulative buffer for a shell. Omit --at-seq to get the final state at end of run.",
    )
    sp.add_argument("shell", help="Shell display name or shell_marker (list them with: jq '.shells | to_entries | map({marker: .key, name: .value.name})' events.json).")
    sp.add_argument(
        "--at-seq",
        type=int,
        help="Cap the concatenation at this seq (exclusive of later grew events). Omit for the cumulative buffer at end of run.",
    )
    sp.set_defaults(func=cmd_buffer)

    sp = sub.add_parser("timeline", help="Per-shell chronological merge of events + buffer events.")
    sp.add_argument("shell", help="Shell display name or shell_marker.")
    sp.set_defaults(func=cmd_timeline)

    sp = sub.add_parser("dedup", help="Effect-setup spans grouped by identity (`marker`).")
    sp.set_defaults(func=cmd_dedup)

    sp = sub.add_parser("pure-trace", help="Pure-eval / value events under a span subtree.")
    sp.add_argument("span", type=int, help="Root span id of the subtree.")
    sp.set_defaults(func=cmd_pure_trace)

    sp = sub.add_parser("diff", help="First event where two runs disagree on (kind, shell_marker, source); ignores seq and buffer_events.")
    sp.add_argument("other", type=Path, help="Path to a second events.json.")
    sp.set_defaults(func=cmd_diff)

    return p


def main():
    args = build_parser().parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
