#!/usr/bin/env python3
"""Fetch recent posts/videos for all followees on zhihu / xiaohongshu / bilibili.

Prefer the built-in command when using a rebuilt opencli:

  opencli batch zhihu following --each user --id url_token --limit 100 --out ./following-archive/zhihu --all --resume
  opencli batch xiaohongshu following --each user --id id --limit 100 --out ./following-archive/xiaohongshu --all --resume
  opencli batch bilibili following --each user-videos --id mid --limit 100 --out ./following-archive/bilibili --all --resume
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import time
import traceback
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = Path(os.environ.get("FOLLOWING_ARCHIVE_DIR", ROOT / "following-archive"))
LIMIT = int(os.environ.get("POST_LIMIT", "100"))
OPENCLI = os.environ.get("OPENCLI", "opencli")
PLATFORMS = [p for p in os.environ.get("PLATFORMS", "zhihu,xiaohongshu,bilibili").split(",") if p]
SLEEP = float(os.environ.get("REQUEST_SLEEP", "0.4"))
MAX_RETRIES = int(os.environ.get("MAX_RETRIES", "3"))


def log(msg: str) -> None:
    ts = datetime.now().strftime("%H:%M:%S")
    line = f"[{ts}] {msg}"
    print(line, flush=True)
    OUT.mkdir(parents=True, exist_ok=True)
    with (OUT / "run.log").open("a", encoding="utf-8") as f:
        f.write(line + "\n")


def safe_name(s: str, fallback: str = "unknown") -> str:
    s = (s or "").strip() or fallback
    s = re.sub(r"[\\/:*?\"<>|\s]+", "_", s)
    s = re.sub(r"_+", "_", s).strip("._")
    return (s[:80] or fallback)


def run_opencli(args: list[str], timeout: int = 300) -> list | dict:
    cmd = [OPENCLI, *args, "-f", "json"]
    last_err = None
    for attempt in range(1, MAX_RETRIES + 1):
        try:
            proc = subprocess.run(
                cmd,
                capture_output=True,
                text=True,
                timeout=timeout,
            )
            out = (proc.stdout or "").strip()
            err = (proc.stderr or "").strip()
            if proc.returncode != 0:
                raise RuntimeError(f"exit={proc.returncode} stderr={err[:500]} stdout={out[:300]}")
            # opencli may print "Elapsed: ..." after JSON; isolate JSON payload
            text = out
            # Prefer last complete JSON array/object
            for start_char, end_char in (("[", "]"), ("{", "}")):
                start = text.find(start_char)
                end = text.rfind(end_char)
                if start != -1 and end != -1 and end > start:
                    try:
                        return json.loads(text[start : end + 1])
                    except json.JSONDecodeError:
                        pass
            # fallback: first non-empty line that looks like JSON
            for line in text.splitlines():
                line = line.strip()
                if line.startswith(("[", "{")):
                    return json.loads(line)
            raise RuntimeError(f"no JSON in output: {out[:400]}")
        except Exception as e:
            last_err = e
            log(f"  retry {attempt}/{MAX_RETRIES} failed: {e}")
            time.sleep(1.5 * attempt)
    raise RuntimeError(str(last_err))


def write_json(path: Path, data) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def load_progress() -> dict:
    p = OUT / "progress.json"
    if p.exists():
        try:
            return json.loads(p.read_text(encoding="utf-8"))
        except Exception:
            pass
    return {"done": {}, "failed": {}, "started_at": datetime.now(timezone.utc).isoformat()}


def save_progress(prog: dict) -> None:
    prog["updated_at"] = datetime.now(timezone.utc).isoformat()
    write_json(OUT / "progress.json", prog)


def done_key(platform: str, uid: str) -> str:
    return f"{platform}:{uid}"


def fetch_following_zhihu() -> list[dict]:
    data = run_opencli(["zhihu", "following", "--all", "true"], timeout=180)
    if not isinstance(data, list):
        raise RuntimeError(f"unexpected following payload: {type(data)}")
    return data


def fetch_following_xhs() -> list[dict]:
    data = run_opencli(["xiaohongshu", "following", "--all", "true"], timeout=120)
    if not isinstance(data, list):
        raise RuntimeError(f"unexpected following payload: {type(data)}")
    return data


def fetch_following_bili() -> list[dict]:
    data = run_opencli(["bilibili", "following", "--all", "true"], timeout=300)
    if not isinstance(data, list):
        raise RuntimeError(f"unexpected following payload: {type(data)}")
    # filter placeholder rows
    return [r for r in data if str(r.get("mid", "")).isdigit()]


def posts_zhihu(token: str) -> list[dict]:
    # each kind up to LIMIT; merge by date and keep LIMIT most recent
    data = run_opencli(
        ["zhihu", "user", token, "--limit", str(LIMIT), "--type", "all"],
        timeout=240,
    )
    rows = data if isinstance(data, list) else []
    rows = sorted(rows, key=lambda r: str(r.get("date") or ""), reverse=True)
    return rows[:LIMIT]


def posts_xhs(uid: str) -> list[dict]:
    data = run_opencli(
        ["xiaohongshu", "user", uid, "--limit", str(LIMIT)],
        timeout=240,
    )
    return data if isinstance(data, list) else []


def posts_bili(mid: str) -> list[dict]:
    data = run_opencli(
        ["bilibili", "user-videos", mid, "--limit", str(LIMIT)],
        timeout=180,
    )
    return data if isinstance(data, list) else []


def process_platform(platform: str, prog: dict) -> dict:
    stats = {"following": 0, "ok": 0, "skip": 0, "fail": 0, "posts": 0}
    base = OUT / platform
    users_dir = base / "users"

    log(f"=== {platform}: fetch following ===")
    if platform == "zhihu":
        following = fetch_following_zhihu()
    elif platform == "xiaohongshu":
        following = fetch_following_xhs()
    elif platform == "bilibili":
        following = fetch_following_bili()
    else:
        raise ValueError(platform)

    write_json(base / "following.json", following)
    stats["following"] = len(following)
    log(f"{platform}: following count = {len(following)}")

    for i, person in enumerate(following, 1):
        if platform == "zhihu":
            uid = str(person.get("url_token") or "").strip()
            name = str(person.get("name") or uid)
            if not uid:
                continue
            folder = users_dir / safe_name(f"{uid}_{name}", uid)
            outfile = "posts.json"
            fetch_fn = posts_zhihu
        elif platform == "xiaohongshu":
            uid = str(person.get("id") or "").strip()
            name = str(person.get("name") or uid)
            if not uid:
                continue
            folder = users_dir / safe_name(f"{uid}_{name}", uid)
            outfile = "posts.json"
            fetch_fn = posts_xhs
        else:
            uid = str(person.get("mid") or "").strip()
            name = str(person.get("name") or uid)
            if not uid.isdigit():
                continue
            folder = users_dir / safe_name(f"{uid}_{name}", uid)
            outfile = "videos.json"
            fetch_fn = posts_bili

        key = done_key(platform, uid)
        posts_path = folder / outfile
        if key in prog.get("done", {}) and posts_path.exists():
            stats["skip"] += 1
            log(f"[{platform} {i}/{len(following)}] SKIP {name} ({uid})")
            continue

        log(f"[{platform} {i}/{len(following)}] FETCH {name} ({uid})")
        try:
            posts = fetch_fn(uid)
            write_json(posts_path, posts)
            write_json(
                folder / "meta.json",
                {
                    "platform": platform,
                    "id": uid,
                    "name": name,
                    "profile": person,
                    "post_count": len(posts),
                    "fetched_at": datetime.now(timezone.utc).isoformat(),
                    "limit": LIMIT,
                },
            )
            prog.setdefault("done", {})[key] = {
                "name": name,
                "count": len(posts),
                "path": str(posts_path.relative_to(OUT)),
                "at": datetime.now(timezone.utc).isoformat(),
            }
            if key in prog.get("failed", {}):
                del prog["failed"][key]
            save_progress(prog)
            stats["ok"] += 1
            stats["posts"] += len(posts)
            log(f"  -> {len(posts)} items -> {posts_path.relative_to(OUT)}")
        except Exception as e:
            stats["fail"] += 1
            prog.setdefault("failed", {})[key] = {
                "name": name,
                "error": str(e)[:500],
                "at": datetime.now(timezone.utc).isoformat(),
            }
            save_progress(prog)
            log(f"  FAIL {name}: {e}")
            # keep going
        time.sleep(SLEEP)

    write_json(base / "summary.json", stats)
    return stats


def main() -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    prog = load_progress()
    if "started_at" not in prog:
        prog["started_at"] = datetime.now(timezone.utc).isoformat()
    save_progress(prog)

    log(f"archive dir: {OUT}")
    log(f"platforms: {PLATFORMS}, post_limit={LIMIT}")
    all_stats = {}
    for platform in PLATFORMS:
        try:
            all_stats[platform] = process_platform(platform, prog)
        except Exception as e:
            log(f"PLATFORM FATAL {platform}: {e}")
            traceback.print_exc()
            all_stats[platform] = {"error": str(e)}

    write_json(OUT / "summary.json", {
        "finished_at": datetime.now(timezone.utc).isoformat(),
        "limit": LIMIT,
        "platforms": all_stats,
        "done_count": len(prog.get("done", {})),
        "failed_count": len(prog.get("failed", {})),
    })
    log(f"DONE summary={json.dumps(all_stats, ensure_ascii=False)}")
    return 0 if not prog.get("failed") else 1


if __name__ == "__main__":
    sys.exit(main())
