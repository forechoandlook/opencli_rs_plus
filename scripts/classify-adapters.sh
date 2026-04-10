#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${ROOT_DIR}/docs/generated"
OUT_FILE="${OUT_DIR}/adapter-classification.tsv"

mkdir -p "${OUT_DIR}"

classify_adapter() {
  local file="$1"
  local site name strategy browser category reason

  site="$(sed -n 's/^site: //p' "$file" | head -n 1)"
  name="$(sed -n 's/^name: //p' "$file" | head -n 1)"
  strategy="$(sed -n 's/^strategy: //p' "$file" | head -n 1)"
  browser="$(sed -n 's/^browser: //p' "$file" | head -n 1)"

  if [[ -z "${strategy}" ]]; then
    strategy="unspecified"
  fi
  if [[ -z "${browser}" ]]; then
    browser="false"
  fi

  if rg -q '^strategy: ui$' "$file"; then
    category="ui_automation"
    reason="strategy=ui"
  elif rg -q '^[[:space:]]*- bg_fetch:' "$file"; then
    category="api_bg_fetch"
    reason="bg_fetch"
  elif rg -q 'fetch\(' "$file"; then
    if rg -q 'method:[[:space:]]*(POST|PUT|PATCH|DELETE)' "$file" \
      || [[ "$name" =~ ^(like|unlike|follow|unfollow|save|unsave|comment|reply|send|invite|mark|subscribe|upvote|add-to-cart|batchgreet|greet|exchange|feedback|write|new|ask)$ ]]; then
      category="api_write_or_mutation"
      reason="fetch+mutation"
    elif rg -q '^[[:space:]]*- navigate:' "$file"; then
      category="api_page_fetch"
      reason="navigate+fetch"
    else
      category="api_direct_fetch"
      reason="fetch_without_navigate"
    fi
  elif rg -q '^[[:space:]]*- navigate:' "$file"; then
    category="page_navigation_dom"
    reason="navigate_without_fetch"
  else
    category="other_pipeline"
    reason="no_fetch_no_navigate"
  fi

  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "${site}" "${name}" "${strategy}" "${browser}" "${category}" "${reason}" "${file#${ROOT_DIR}/}"
}

{
  printf 'site\tname\tstrategy\tbrowser\tcategory\treason\tpath\n'
  while IFS= read -r file; do
    classify_adapter "$file"
  done < <(find "${ROOT_DIR}/adapters" -mindepth 2 -maxdepth 2 -name '*.yaml' | sort)
} > "${OUT_FILE}"

echo "Wrote ${OUT_FILE}"
