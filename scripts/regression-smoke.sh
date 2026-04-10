#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"

CONFIG_FILE="${1:-docs/generated/regression-p1.tsv}"
OUT_DIR="${ROOT_DIR}/docs/generated"
TS="$(date +%Y%m%d-%H%M%S)"
OUT_TSV="${OUT_DIR}/regression-smoke-${TS}.tsv"
OUT_MD="${OUT_DIR}/regression-smoke-${TS}.md"

mkdir -p "${OUT_DIR}"

run_with_timeout() {
  local timeout_secs="$1"
  shift
  if command -v gtimeout >/dev/null 2>&1; then
    gtimeout "${timeout_secs}" "$@"
  elif command -v timeout >/dev/null 2>&1; then
    timeout "${timeout_secs}" "$@"
  else
    python3 - "$timeout_secs" "$@" <<'PY'
import subprocess, sys
timeout = int(sys.argv[1])
cmd = sys.argv[2:]
try:
    result = subprocess.run(cmd, timeout=timeout)
    raise SystemExit(result.returncode)
except subprocess.TimeoutExpired:
    raise SystemExit(124)
PY
  fi
}

extract_json_payload() {
  local source_file="$1"
  local json_file="$2"
  python3 - "$source_file" "$json_file" <<'PY'
import sys
src_path, out_path = sys.argv[1], sys.argv[2]
lines = open(src_path, 'r', encoding='utf-8', errors='ignore').read().splitlines()
start = None
for i, line in enumerate(lines):
    if line.startswith('[') or line.startswith('{'):
        start = i
        break
if start is None:
    raise SystemExit(1)
end = len(lines)
for i in range(start + 1, len(lines)):
    if lines[i].startswith('Elapsed:'):
        end = i
        break
payload = '\n'.join(lines[start:end]).strip()
if not payload:
    raise SystemExit(1)
open(out_path, 'w', encoding='utf-8').write(payload)
PY
}

if [[ ! -f "${CONFIG_FILE}" ]]; then
  echo "config not found: ${CONFIG_FILE}" >&2
  exit 1
fi

run_case() {
  local adapter="$1"
  local args="$2"
  local expect_min_rows="$3"
  local expect_fields="$4"
  local timeout_secs="$5"
  local enable_api_dump="$6"

  local status="ok"
  local rows="0"
  local elapsed_ms="0"
  local note=""
  local stdout_file stderr_file json_file dump_dir format_cmd

  stdout_file="$(mktemp)"
  stderr_file="$(mktemp)"
  json_file="$(mktemp)"
  dump_dir="${ROOT_DIR}/tmp/regression-api-dumps/${TS}/$(echo "${adapter}" | tr ' /' '__')"
  mkdir -p "${dump_dir}"

  local cmd=(target/debug/opencli --format json)
  read -r -a adapter_parts <<< "${adapter}"
  cmd+=("${adapter_parts[@]}")
  if [[ -n "${args}" ]]; then
    read -r -a arg_parts <<< "${args}"
    cmd+=("${arg_parts[@]}")
  fi

  local start_ms end_ms
  start_ms="$(python3 - <<'PY'
import time
print(int(time.time()*1000))
PY
)"

  if [[ "${enable_api_dump}" == "1" ]]; then
    OPENCLI_API_DUMP=1 OPENCLI_API_DUMP_DIR="${dump_dir}" run_with_timeout "${timeout_secs}" "${cmd[@]}" >"${stdout_file}" 2>"${stderr_file}" || status="failed"
  else
    run_with_timeout "${timeout_secs}" "${cmd[@]}" >"${stdout_file}" 2>"${stderr_file}" || status="failed"
  fi

  end_ms="$(python3 - <<'PY'
import time
print(int(time.time()*1000))
PY
)"
  elapsed_ms="$((end_ms-start_ms))"

  if [[ "${status}" == "ok" ]]; then
    if ! extract_json_payload "${stdout_file}" "${json_file}"; then
      status="failed"
      note="failed to extract json payload from stdout"
    fi
  fi

  if [[ "${status}" == "ok" ]]; then
    rows="$(python3 - "${json_file}" <<'PY'
import json,sys
path=sys.argv[1]
try:
    data=json.load(open(path))
    print(len(data) if isinstance(data,list) else 0)
except Exception:
    print(0)
PY
)"
    if (( rows < expect_min_rows )); then
      status="failed"
      note="rows ${rows} < expected ${expect_min_rows}"
    fi
    if [[ "${status}" == "ok" && -n "${expect_fields}" ]]; then
      if ! python3 - "${json_file}" "${expect_fields}" <<'PY'
import json,sys
path=sys.argv[1]
fields=[f for f in sys.argv[2].split(',') if f]
data=json.load(open(path))
if not isinstance(data,list) or not data:
    raise SystemExit(1)
for field in fields:
    if field not in data[0]:
        raise SystemExit(1)
raise SystemExit(0)
PY
      then
        status="failed"
        note="missing expected fields: ${expect_fields}"
      fi
    fi
  fi

  if [[ "${status}" == "failed" && -z "${note}" ]]; then
    note="$(tr '\n' ' ' < "${stderr_file}" | sed 's/[[:space:]]\+/ /g' | cut -c1-240)"
  fi

  local dump_count="0"
  dump_count="$(find "${dump_dir}" -type f 2>/dev/null | wc -l | tr -d ' ')"

  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "${adapter}" "${args}" "${status}" "${rows}" "${elapsed_ms}" "${dump_count}" "${expect_fields}" "${note}"

  rm -f "${stdout_file}" "${stderr_file}" "${json_file}"
}

{
  printf 'adapter\targs\tstatus\trows\telapsed_ms\tdump_files\texpect_fields\tnote\n'
  tail -n +2 "${CONFIG_FILE}" | while IFS=$'\t' read -r adapter args expect_min_rows expect_fields timeout_secs enable_api_dump; do
    run_case "${adapter}" "${args}" "${expect_min_rows}" "${expect_fields}" "${timeout_secs}" "${enable_api_dump}"
  done
} > "${OUT_TSV}"

python3 - "${OUT_TSV}" "${OUT_MD}" <<'PY'
import csv,sys
tsv_path, md_path = sys.argv[1], sys.argv[2]
rows=list(csv.DictReader(open(tsv_path), delimiter='\t'))
ok=sum(1 for r in rows if r["status"]=="ok")
failed=len(rows)-ok
with open(md_path, "w") as f:
    f.write("# Regression Smoke\n\n")
    f.write(f"- total: {len(rows)}\n")
    f.write(f"- ok: {ok}\n")
    f.write(f"- failed: {failed}\n\n")
    f.write("| adapter | status | rows | elapsed_ms | dump_files | note |\n")
    f.write("|---|---:|---:|---:|---:|---|\n")
    for r in rows:
        f.write(f"| {r['adapter']} {r['args']} | {r['status']} | {r['rows']} | {r['elapsed_ms']} | {r['dump_files']} | {r['note']} |\n")
PY

echo "Wrote ${OUT_TSV}"
echo "Wrote ${OUT_MD}"
