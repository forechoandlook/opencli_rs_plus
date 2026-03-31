#!/bin/bash
# Automated test script for opencli-rs and opencli
# Tests all Public and Browser mode commands, records results
# Usage:
#   ./scripts/test-all-commands.sh              # test opencli-rs (default)
#   ./scripts/test-all-commands.sh opencli      # test original opencli
#   ./scripts/test-all-commands.sh both         # test both side by side

set -o pipefail

# Parse argument
TEST_MODE="${1:-opencli-rs}"

case "$TEST_MODE" in
    opencli-rs|rs)
        BINARIES=("./target/release/opencli-rs")
        LABELS=("opencli-rs")
        ;;
    opencli|original)
        BINARIES=("opencli")
        LABELS=("opencli")
        ;;
    both|compare)
        BINARIES=("./target/release/opencli-rs" "opencli")
        LABELS=("opencli-rs" "opencli")
        ;;
    *)
        echo "Usage: $0 [opencli-rs|opencli|both]"
        exit 1
        ;;
esac

PUBLIC_TIMEOUT=30
BROWSER_TIMEOUT=90
LIMIT=3

# Colors for terminal output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
GRAY='\033[0;90m'
BLUE='\033[0;34m'
NC='\033[0m'

# Per-binary counters (use arrays for multi-binary support)
declare -a TOTALS OKS EMPTYS ERROR_COUNTS SKIPPEDS
declare -a TIMING_FILES REPORTS

for i in "${!BINARIES[@]}"; do
    TOTALS[$i]=0
    OKS[$i]=0
    EMPTYS[$i]=0
    ERROR_COUNTS[$i]=0
    SKIPPEDS[$i]=0
    TIMING_FILES[$i]=$(mktemp /tmp/opencli-test-timing-${LABELS[$i]}.XXXXXX)
    REPORTS[$i]="test-results-${LABELS[$i]}.md"

    cat > "${REPORTS[$i]}" << HEADER
# ${LABELS[$i]} Automated Test Report

> Generated at: TIMESTAMP

## Summary

| Status | Count |
|--------|-------|
| OK     | OK_COUNT |
| EMPTY  | EMPTY_COUNT |
| ERROR  | ERROR_COUNT |
| SKIP   | SKIP_COUNT |
| **Total** | **TOTAL_COUNT** |

## Results

HEADER
    sed -i '' "s/TIMESTAMP/$(date '+%Y-%m-%d %H:%M:%S')/" "${REPORTS[$i]}"
    echo "| Status | Site | Command | Mode | Detail |" >> "${REPORTS[$i]}"
    echo "|--------|------|---------|------|--------|" >> "${REPORTS[$i]}"
done

# If both mode, also create a comparison report
if [ ${#BINARIES[@]} -gt 1 ]; then
    COMPARE_REPORT="test-results-compare.md"
    cat > "$COMPARE_REPORT" << 'HEADER'
# opencli-rs vs opencli Comparison Report

> Generated at: TIMESTAMP

## Results

| Site | Command | Mode | opencli-rs | opencli | Match? |
|------|---------|------|------------|---------|--------|
HEADER
    sed -i '' "s/TIMESTAMP/$(date '+%Y-%m-%d %H:%M:%S')/" "$COMPARE_REPORT"
fi

# ── Helper functions ──

# Run a single test for one binary, return status via global _LAST_STATUS and _LAST_DETAIL
_run_single() {
    local binary="$1" site="$2" cmd="$3" extra_args="$4" timeout="$5"
    local full_cmd="$binary $site $cmd $extra_args --format json"

    local output exit_code start_ts end_ts
    start_ts=$(perl -e 'use Time::HiRes qw(time); print time')
    output=$(perl -e 'alarm shift; exec @ARGV' "$timeout" bash -c "$full_cmd" 2>&1)
    exit_code=$?
    end_ts=$(perl -e 'use Time::HiRes qw(time); print time')
    _LAST_ELAPSED=$(perl -e "printf '%.1f', $end_ts - $start_ts")
    if [ $exit_code -eq 142 ]; then exit_code=124; fi

    if [ $exit_code -eq 124 ]; then
        _LAST_STATUS="TIMEOUT"
        _LAST_DETAIL="Timed out after ${timeout}s"
    elif [ $exit_code -ne 0 ]; then
        _LAST_STATUS="ERROR"
        _LAST_DETAIL=$(echo "$output" | grep -v "^$" | grep -v "^[[:space:]]*$" | grep -v "^\[2m" | tail -3 | tr '\n' ' ' | head -c 120)
    else
        local json_check
        json_check=$(echo "$output" | grep -v "^$" | grep -v "^(" | grep -v "^Elapsed" | head -1)
        if [ -z "$json_check" ] || [ "$json_check" = "null" ] || [ "$json_check" = "[]" ] || [ "$json_check" = "{}" ]; then
            _LAST_STATUS="EMPTY"
            _LAST_DETAIL="Empty (${_LAST_ELAPSED}s)"
        else
            local count
            count=$(echo "$output" | python3 -c "import sys,json; d=json.load(sys.stdin); print(len(d) if isinstance(d,list) else 1)" 2>/dev/null || echo "?")
            _LAST_STATUS="OK"
            _LAST_DETAIL="$count item(s) in ${_LAST_ELAPSED}s"
        fi
    fi
}

_status_icon() {
    case "$1" in
        OK)      echo "✅" ;;
        EMPTY)   echo "📭" ;;
        ERROR)   echo "❌" ;;
        TIMEOUT) echo "⏱️" ;;
        SKIP)    echo "⏭️" ;;
    esac
}

_status_color() {
    case "$1" in
        OK)      echo "${GREEN}" ;;
        EMPTY)   echo "${YELLOW}" ;;
        ERROR)   echo "${RED}" ;;
        TIMEOUT) echo "${RED}" ;;
        *)       echo "${GRAY}" ;;
    esac
}

run_test() {
    local site="$1" cmd="$2" mode="$3" extra_args="$4" timeout="$5"

    # Track test number (use first binary's counter)
    TOTALS[0]=$((${TOTALS[0]} + 1))
    local test_num=${TOTALS[0]}

    # Store results per binary for comparison
    declare -a statuses elapsed_times details

    for i in "${!BINARIES[@]}"; do
        _run_single "${BINARIES[$i]}" "$site" "$cmd" "$extra_args" "$timeout"
        statuses[$i]="$_LAST_STATUS"
        elapsed_times[$i]="$_LAST_ELAPSED"
        details[$i]="$_LAST_DETAIL"

        echo "${_LAST_ELAPSED}" >> "${TIMING_FILES[$i]}"

        case "$_LAST_STATUS" in
            OK)      OKS[$i]=$((${OKS[$i]} + 1)) ;;
            EMPTY)   EMPTYS[$i]=$((${EMPTYS[$i]} + 1)) ;;
            ERROR|TIMEOUT) ERROR_COUNTS[$i]=$((${ERROR_COUNTS[$i]} + 1)) ;;
        esac

        local icon=$(_status_icon "$_LAST_STATUS")
        echo "| $icon $_LAST_STATUS | \`$site\` | \`$cmd\` | $mode | ${_LAST_DETAIL:0:120} |" >> "${REPORTS[$i]}"
    done

    # Print to terminal
    if [ ${#BINARIES[@]} -eq 1 ]; then
        local clr=$(_status_color "${statuses[0]}")
        printf "${CYAN}[%3d] %-20s %-25s${NC} ${clr}%-7s${NC} (%ss)\n" "$test_num" "$site" "$cmd" "${statuses[0]}" "${elapsed_times[0]}"
    else
        local clr0=$(_status_color "${statuses[0]}")
        local clr1=$(_status_color "${statuses[1]}")
        local match=""
        if [ "${statuses[0]}" = "${statuses[1]}" ]; then
            match="${GREEN}✓${NC}"
        else
            match="${RED}✗${NC}"
        fi
        printf "${CYAN}[%3d] %-18s %-20s${NC} ${clr0}%-7s${NC}(%5ss) ${clr1}%-7s${NC}(%5ss) %b\n" \
            "$test_num" "$site" "$cmd" "${statuses[0]}" "${elapsed_times[0]}" "${statuses[1]}" "${elapsed_times[1]}" "$match"
    fi

    # Write comparison report
    if [ ${#BINARIES[@]} -gt 1 ]; then
        local match_mark
        if [ "${statuses[0]}" = "${statuses[1]}" ]; then
            match_mark="✅"
        else
            match_mark="❌"
        fi
        echo "| \`$site\` | \`$cmd\` | $mode | ${statuses[0]} (${elapsed_times[0]}s) | ${statuses[1]} (${elapsed_times[1]}s) | $match_mark |" >> "$COMPARE_REPORT"
    fi
}

skip_test() {
    local site="$1" cmd="$2" mode="$3" reason="$4"

    TOTALS[0]=$((${TOTALS[0]} + 1))
    local test_num=${TOTALS[0]}

    for i in "${!BINARIES[@]}"; do
        SKIPPEDS[$i]=$((${SKIPPEDS[$i]} + 1))
        echo "| ⏭️ SKIP | \`$site\` | \`$cmd\` | $mode | $reason |" >> "${REPORTS[$i]}"
    done

    printf "${GRAY}[%3d] %-20s %-25s SKIP (%s)${NC}\n" "$test_num" "$site" "$cmd" "$reason"

    if [ ${#BINARIES[@]} -gt 1 ]; then
        echo "| \`$site\` | \`$cmd\` | $mode | SKIP | SKIP | ⏭️ |" >> "$COMPARE_REPORT"
    fi
}

# ═══════════════════════════════════════════════════
# PUBLIC MODE COMMANDS (no browser needed)
# ═══════════════════════════════════════════════════
echo ""
echo "════════════════════════════════════════════"
echo "  Testing: ${LABELS[*]}"
if [ ${#BINARIES[@]} -gt 1 ]; then
    printf "  %-45s ${BLUE}%-12s %-12s${NC} Match\n" "" "${LABELS[0]}" "${LABELS[1]}"
fi
echo "════════════════════════════════════════════"
echo ""
echo "── PUBLIC MODE ──"
echo ""

# hackernews
run_test hackernews top     Public "--limit $LIMIT" $PUBLIC_TIMEOUT
run_test hackernews new     Public "--limit $LIMIT" $PUBLIC_TIMEOUT
run_test hackernews best    Public "--limit $LIMIT" $PUBLIC_TIMEOUT
run_test hackernews ask     Public "--limit $LIMIT" $PUBLIC_TIMEOUT
run_test hackernews show    Public "--limit $LIMIT" $PUBLIC_TIMEOUT
run_test hackernews jobs    Public "--limit $LIMIT" $PUBLIC_TIMEOUT
run_test hackernews search  Public "rust --limit $LIMIT" $PUBLIC_TIMEOUT
run_test hackernews user    Public "pg" $PUBLIC_TIMEOUT

# devto
run_test devto top  Public "--limit $LIMIT" $PUBLIC_TIMEOUT
run_test devto tag  Public "rust --limit $LIMIT" $PUBLIC_TIMEOUT
run_test devto user Public "ben --limit $LIMIT" $PUBLIC_TIMEOUT

# lobsters
run_test lobsters hot     Public "--limit $LIMIT" $PUBLIC_TIMEOUT
run_test lobsters newest  Public "--limit $LIMIT" $PUBLIC_TIMEOUT
run_test lobsters active  Public "--limit $LIMIT" $PUBLIC_TIMEOUT
run_test lobsters tag     Public "rust --limit $LIMIT" $PUBLIC_TIMEOUT

# stackoverflow
run_test stackoverflow hot        Public "--limit $LIMIT" $PUBLIC_TIMEOUT
run_test stackoverflow search     Public "rust --limit $LIMIT" $PUBLIC_TIMEOUT
run_test stackoverflow bounties   Public "--limit $LIMIT" $PUBLIC_TIMEOUT
run_test stackoverflow unanswered Public "--limit $LIMIT" $PUBLIC_TIMEOUT

# steam
run_test steam top-sellers Public "--limit $LIMIT" $PUBLIC_TIMEOUT

# linux-do
run_test linux-do hot        Public "--limit $LIMIT" $PUBLIC_TIMEOUT
run_test linux-do latest     Public "--limit $LIMIT" $PUBLIC_TIMEOUT
run_test linux-do search     Public "linux --limit $LIMIT" $PUBLIC_TIMEOUT
run_test linux-do categories Public "--limit $LIMIT" $PUBLIC_TIMEOUT
run_test linux-do topic      Public "1" $PUBLIC_TIMEOUT

# arxiv
run_test arxiv search Public "machine-learning --limit $LIMIT" $PUBLIC_TIMEOUT
run_test arxiv paper  Public "2301.00001" $PUBLIC_TIMEOUT

# wikipedia
run_test wikipedia search   Public "Rust --limit $LIMIT" $PUBLIC_TIMEOUT
run_test wikipedia summary  Public "Rust" $PUBLIC_TIMEOUT
run_test wikipedia random   Public "" $PUBLIC_TIMEOUT
run_test wikipedia trending Public "--limit $LIMIT" $PUBLIC_TIMEOUT

# apple-podcasts
run_test apple-podcasts search   Public "tech --limit $LIMIT" $PUBLIC_TIMEOUT
run_test apple-podcasts top      Public "--limit $LIMIT" $PUBLIC_TIMEOUT
run_test apple-podcasts episodes Public "1535809341 --limit $LIMIT" $PUBLIC_TIMEOUT

# xiaoyuzhou
run_test xiaoyuzhou podcast          Public "日谈公园" $PUBLIC_TIMEOUT
run_test xiaoyuzhou podcast-episodes Public "61791bb1ee1bb7e2a4591cc2 --limit $LIMIT" $PUBLIC_TIMEOUT
run_test xiaoyuzhou episode          Public "61791bb1ee1bb7e2a4591cc2" $PUBLIC_TIMEOUT

# bbc
run_test bbc news Public "--limit $LIMIT" $PUBLIC_TIMEOUT

# hf
run_test hf top Public "--limit $LIMIT" $PUBLIC_TIMEOUT

# sinafinance
run_test sinafinance news Public "--limit $LIMIT" $PUBLIC_TIMEOUT

# google (public ones)
run_test google suggest Public "rust" $PUBLIC_TIMEOUT
run_test google news    Public "technology --limit $LIMIT" $PUBLIC_TIMEOUT
run_test google trends  Public "--limit $LIMIT" $PUBLIC_TIMEOUT

# v2ex (public)
run_test v2ex hot     Public  "--limit $LIMIT" $PUBLIC_TIMEOUT
run_test v2ex latest  Public  "--limit $LIMIT" $PUBLIC_TIMEOUT
run_test v2ex topic   Public  "1200650" $PUBLIC_TIMEOUT
run_test v2ex nodes   Public  "--limit $LIMIT" $PUBLIC_TIMEOUT

# ═══════════════════════════════════════════════════
# BROWSER MODE COMMANDS (need Chrome + extension)
# ═══════════════════════════════════════════════════
echo ""
echo "── BROWSER MODE ──"
echo ""

# Check if daemon is running (try opencli-rs first, then opencli)
if ! curl -s http://127.0.0.1:19825/health > /dev/null 2>&1; then
    echo "⚠️  Daemon not running. Starting daemon..."
    if [ -f "./target/release/opencli-rs" ]; then
        ./target/release/opencli-rs --daemon &
    else
        opencli --daemon 2>/dev/null &
    fi
    sleep 3
fi

# Check extension
EXT_STATUS=$(curl -s http://127.0.0.1:19825/status 2>/dev/null | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('extension') or d.get('extensionConnected', False))" 2>/dev/null)
if [ "$EXT_STATUS" != "True" ]; then
    echo "⚠️  Chrome extension not connected. Browser tests may fail."
    echo ""
fi

# bilibili
run_test bilibili hot          Browser "--limit $LIMIT" $BROWSER_TIMEOUT
run_test bilibili ranking      Browser "--limit $LIMIT" $BROWSER_TIMEOUT
run_test bilibili search       Browser "rust --limit $LIMIT" $BROWSER_TIMEOUT
run_test bilibili me           Browser "" $BROWSER_TIMEOUT
run_test bilibili feed         Browser "--limit $LIMIT" $BROWSER_TIMEOUT
run_test bilibili dynamic      Browser "--limit $LIMIT" $BROWSER_TIMEOUT
run_test bilibili history      Browser "--limit $LIMIT" $BROWSER_TIMEOUT
run_test bilibili favorite     Browser "--limit $LIMIT" $BROWSER_TIMEOUT
run_test bilibili following    Browser "--limit $LIMIT" $BROWSER_TIMEOUT
run_test bilibili subtitle     Browser "BV1GPPxznEAb" $BROWSER_TIMEOUT
run_test bilibili user-videos  Browser "946974 --limit $LIMIT" $BROWSER_TIMEOUT

# twitter
run_test twitter profile       Browser "nash_su" $BROWSER_TIMEOUT
run_test twitter timeline      Browser "--limit $LIMIT" $BROWSER_TIMEOUT
run_test twitter trending      Browser "--limit $LIMIT" $BROWSER_TIMEOUT
run_test twitter bookmarks     Browser "--limit $LIMIT" $BROWSER_TIMEOUT
run_test twitter followers     Browser "nash_su --limit $LIMIT" $BROWSER_TIMEOUT
run_test twitter following     Browser "nash_su --limit $LIMIT" $BROWSER_TIMEOUT
run_test twitter notifications Browser "--limit $LIMIT" $BROWSER_TIMEOUT
run_test twitter thread        Browser "2035034608639914186" $BROWSER_TIMEOUT
run_test twitter article       Browser "2036098438908293349" $BROWSER_TIMEOUT
run_test twitter search        Browser "rust --limit $LIMIT" $BROWSER_TIMEOUT
run_test twitter download      Browser "2036080526302531590" $BROWSER_TIMEOUT

# reddit
run_test reddit hot           Browser "--limit $LIMIT" $BROWSER_TIMEOUT
run_test reddit frontpage     Browser "--limit $LIMIT" $BROWSER_TIMEOUT
run_test reddit popular       Browser "--limit $LIMIT" $BROWSER_TIMEOUT
run_test reddit search        Browser "rust --limit $LIMIT" $BROWSER_TIMEOUT
run_test reddit subreddit     Browser "programming --limit $LIMIT" $BROWSER_TIMEOUT
run_test reddit user          Browser "spez" $BROWSER_TIMEOUT
run_test reddit user-posts    Browser "spez --limit $LIMIT" $BROWSER_TIMEOUT
run_test reddit user-comments Browser "spez --limit $LIMIT" $BROWSER_TIMEOUT

# zhihu
run_test zhihu hot      Browser "--limit $LIMIT" $BROWSER_TIMEOUT
run_test zhihu search   Browser "rust --limit $LIMIT" $BROWSER_TIMEOUT
run_test zhihu question Browser "1951716962645288920 --limit $LIMIT" $BROWSER_TIMEOUT
run_test zhihu download Browser "'https://www.zhihu.com/question/1951716962645288920/answer/2002355716682424913'" $BROWSER_TIMEOUT

# weixin
run_test weixin download Browser "'https://mp.weixin.qq.com/s/kkPLw1Cl45-8a3aU8M-zKA'" $BROWSER_TIMEOUT

# douban
run_test douban movie-hot Browser "--limit $LIMIT" $BROWSER_TIMEOUT
run_test douban book-hot  Browser "--limit $LIMIT" $BROWSER_TIMEOUT
run_test douban subject   Browser "35010610" $BROWSER_TIMEOUT
run_test douban marks     Browser "--limit $LIMIT" $BROWSER_TIMEOUT
run_test douban search    Browser "肖申克的救赎 --limit $LIMIT" $BROWSER_TIMEOUT
run_test douban top250    Browser "--limit $LIMIT" $BROWSER_TIMEOUT

# xiaohongshu
run_test xiaohongshu user     Browser "694d2065000000003702bdd6" $BROWSER_TIMEOUT
run_test xiaohongshu download Browser "69c0b9150000000023005ccd" $BROWSER_TIMEOUT

# xueqiu
run_test xueqiu hot        Browser "--limit $LIMIT" $BROWSER_TIMEOUT
run_test xueqiu hot-stock  Browser "--limit $LIMIT" $BROWSER_TIMEOUT
run_test xueqiu search     Browser "茅台 --limit $LIMIT" $BROWSER_TIMEOUT
run_test xueqiu stock      Browser "SH600519" $BROWSER_TIMEOUT
run_test xueqiu feed       Browser "--limit $LIMIT" $BROWSER_TIMEOUT

# weibo
run_test weibo hot    Browser "--limit $LIMIT" $BROWSER_TIMEOUT
run_test weibo search Browser "科技 --limit $LIMIT" $BROWSER_TIMEOUT

# weread
run_test weread shelf   Browser "--limit $LIMIT" $BROWSER_TIMEOUT
run_test weread search  Browser "编程 --limit $LIMIT" $BROWSER_TIMEOUT
run_test weread ranking Browser "--limit $LIMIT" $BROWSER_TIMEOUT

# youtube
run_test youtube search    Browser "rust --limit $LIMIT" $BROWSER_TIMEOUT
run_test youtube video     Browser "Hu0ukHlge-4" $BROWSER_TIMEOUT
run_test youtube transcript Browser "Hu0ukHlge-4" $BROWSER_TIMEOUT

# linkedin / reuters / ctrip / smzdm
run_test linkedin search Browser "openclaw --limit $LIMIT" $BROWSER_TIMEOUT
run_test reuters search  Browser "openclaw --limit $LIMIT" $BROWSER_TIMEOUT
run_test ctrip search    Browser "西安" $BROWSER_TIMEOUT
run_test smzdm search    Browser "openclaw --limit $LIMIT" $BROWSER_TIMEOUT

# grok
run_test grok ask Browser "'what is openclaw, tell me in 10 words'" $BROWSER_TIMEOUT

# yahoo-finance / barchart
run_test yahoo-finance quote Browser "AAPL" $BROWSER_TIMEOUT
run_test barchart quote      Browser "AAPL" $BROWSER_TIMEOUT
run_test barchart flow       Browser "--limit $LIMIT" $BROWSER_TIMEOUT

# v2ex (browser)
run_test v2ex daily Browser "" $BROWSER_TIMEOUT
run_test v2ex me    Browser "" $BROWSER_TIMEOUT

# medium / substack / sinablog / bloomberg
run_test medium search    Browser "rust --limit $LIMIT" $BROWSER_TIMEOUT
run_test medium feed      Browser "--limit $LIMIT" $BROWSER_TIMEOUT
run_test substack search  Browser "technology --limit $LIMIT" $BROWSER_TIMEOUT
run_test substack feed    Browser "--limit $LIMIT" $BROWSER_TIMEOUT
run_test sinablog hot     Browser "--limit $LIMIT" $BROWSER_TIMEOUT
run_test sinablog search  Browser "科技 --limit $LIMIT" $BROWSER_TIMEOUT
run_test bloomberg main   Browser "--limit $LIMIT" $BROWSER_TIMEOUT
run_test bloomberg markets Browser "--limit $LIMIT" $BROWSER_TIMEOUT
run_test bloomberg tech   Browser "--limit $LIMIT" $BROWSER_TIMEOUT

# ── SKIP: Side-effect commands (write operations) ──
skip_test twitter    post         Browser "Side-effect: posts a tweet"
skip_test twitter    reply        Browser "Side-effect: replies to tweet"
skip_test twitter    delete       Browser "Side-effect: deletes a tweet"
skip_test twitter    like         Browser "Side-effect: likes a tweet"
skip_test twitter    follow       Browser "Side-effect: follows user"
skip_test twitter    unfollow     Browser "Side-effect: unfollows user"
skip_test twitter    bookmark     Browser "Side-effect: bookmarks tweet"
skip_test twitter    unbookmark   Browser "Side-effect: removes bookmark"
skip_test twitter    block        Browser "Side-effect: blocks user"
skip_test twitter    unblock      Browser "Side-effect: unblocks user"
skip_test twitter    hide-reply   Browser "Side-effect: hides reply"
skip_test twitter    accept       Browser "Side-effect: accepts DM"
skip_test twitter    reply-dm     Browser "Side-effect: sends DM"
skip_test reddit     upvote       Browser "Side-effect: upvotes post"
skip_test reddit     save         Browser "Side-effect: saves post"
skip_test reddit     comment      Browser "Side-effect: posts comment"
skip_test reddit     subscribe    Browser "Side-effect: subscribes"
skip_test jike       create       Browser "Side-effect: creates post"
skip_test jike       like         Browser "Side-effect: likes post"
skip_test jike       comment      Browser "Side-effect: posts comment"
skip_test jike       repost       Browser "Side-effect: reposts"
skip_test facebook   add-friend   Browser "Side-effect: sends friend request"
skip_test facebook   join-group   Browser "Side-effect: joins group"
skip_test instagram  follow       Browser "Side-effect: follows user"
skip_test instagram  unfollow     Browser "Side-effect: unfollows user"
skip_test instagram  like         Browser "Side-effect: likes post"
skip_test instagram  unlike       Browser "Side-effect: unlikes post"
skip_test instagram  comment      Browser "Side-effect: posts comment"
skip_test instagram  save         Browser "Side-effect: saves post"
skip_test instagram  unsave       Browser "Side-effect: unsaves post"
skip_test tiktok     follow       Browser "Side-effect: follows user"
skip_test tiktok     unfollow     Browser "Side-effect: unfollows user"
skip_test tiktok     like         Browser "Side-effect: likes video"
skip_test tiktok     unlike       Browser "Side-effect: unlikes video"
skip_test tiktok     comment      Browser "Side-effect: posts comment"
skip_test tiktok     save         Browser "Side-effect: saves video"
skip_test tiktok     unsave       Browser "Side-effect: unsaves video"
skip_test xiaohongshu publish     Browser "Side-effect: publishes note"
skip_test boss       greet        Browser "Side-effect: sends greeting"
skip_test boss       batchgreet   Browser "Side-effect: batch greets"
skip_test boss       send         Browser "Side-effect: sends message"
skip_test boss       invite       Browser "Side-effect: sends invite"
skip_test boss       mark         Browser "Side-effect: marks candidate"
skip_test coupang    add-to-cart  Browser "Side-effect: adds to cart"
skip_test bilibili   download     Browser "Side-effect: downloads video (needs yt-dlp)"

# ── SKIP: Desktop mode commands ──
for site in cursor codex chatwise chatgpt doubao-app antigravity notion discord-app; do
    skip_test "$site" "(all)" Desktop "Requires desktop app"
done

# ── SKIP: Remaining commands needing special setup ──
skip_test xiaohongshu creator-notes         Browser "Needs creator account"
skip_test xiaohongshu creator-note-detail   Browser "Needs specific note ID"
skip_test xiaohongshu creator-notes-summary Browser "Needs creator account"
skip_test xiaohongshu creator-profile       Browser "Needs creator account"
skip_test xiaohongshu creator-stats         Browser "Needs creator account"
skip_test douban      reviews               Browser "Needs login state"
skip_test jimeng      generate              Browser "Needs prompt input"
skip_test jimeng      history               Browser "Needs account"
skip_test chaoxing    assignments           Browser "Needs student account"
skip_test chaoxing    exams                 Browser "Needs student account"
skip_test youtube     transcript-group      Browser "Needs specific video ID"

# ═══════════════════════════════════════════════════
# FINALIZE REPORT
# ═══════════════════════════════════════════════════

# Update summary counts
sed -i '' "s/OK_COUNT/$OK/" "$REPORT"
sed -i '' "s/EMPTY_COUNT/$EMPTY/" "$REPORT"
sed -i '' "s/ERROR_COUNT/$ERRORS/" "$REPORT"
sed -i '' "s/SKIP_COUNT/$SKIPPED/" "$REPORT"
sed -i '' "s/TOTAL_COUNT/$TOTAL/" "$REPORT"

# Calculate timing stats per binary
calc_timing() {
    local timing_file="$1"
    python3 << PYEOF
times = []
with open("$timing_file") as f:
    for line in f:
        line = line.strip()
        if line:
            try: times.append(float(line))
            except ValueError: pass
if not times:
    print("N/A|N/A|N/A|N/A|N/A|0")
else:
    times.sort()
    n = len(times)
    total = sum(times)
    avg = total / n
    median = times[n // 2] if n % 2 == 1 else (times[n // 2 - 1] + times[n // 2]) / 2
    print(f"{times[0]:.1f}|{times[-1]:.1f}|{avg:.1f}|{median:.1f}|{total:.1f}|{n}")
PYEOF
}

echo ""
echo "════════════════════════════════════════════"
echo "  TEST COMPLETE"
echo "════════════════════════════════════════════"

for i in "${!BINARIES[@]}"; do
    TIMING_STATS=$(calc_timing "${TIMING_FILES[$i]}")
    T_FASTEST=$(echo "$TIMING_STATS" | cut -d'|' -f1)
    T_SLOWEST=$(echo "$TIMING_STATS" | cut -d'|' -f2)
    T_AVG=$(echo "$TIMING_STATS" | cut -d'|' -f3)
    T_MEDIAN=$(echo "$TIMING_STATS" | cut -d'|' -f4)
    T_TOTAL=$(echo "$TIMING_STATS" | cut -d'|' -f5)
    T_COUNT=$(echo "$TIMING_STATS" | cut -d'|' -f6)

    # Update report summary
    sed -i '' "s/OK_COUNT/${OKS[$i]}/" "${REPORTS[$i]}"
    sed -i '' "s/EMPTY_COUNT/${EMPTYS[$i]}/" "${REPORTS[$i]}"
    sed -i '' "s/ERROR_COUNT/${ERROR_COUNTS[$i]}/" "${REPORTS[$i]}"
    sed -i '' "s/SKIP_COUNT/${SKIPPEDS[$i]}/" "${REPORTS[$i]}"
    sed -i '' "s/TOTAL_COUNT/$((${OKS[$i]} + ${EMPTYS[$i]} + ${ERROR_COUNTS[$i]} + ${SKIPPEDS[$i]}))/" "${REPORTS[$i]}"

    cat >> "${REPORTS[$i]}" << TIMING_EOF

## Timing Statistics

| Metric | Value |
|--------|-------|
| Tests executed | $T_COUNT |
| Fastest | ${T_FASTEST}s |
| Slowest | ${T_SLOWEST}s |
| Average | ${T_AVG}s |
| Median | ${T_MEDIAN}s |
| Total time | ${T_TOTAL}s |
TIMING_EOF

    echo ""
    printf "  ${BLUE}── ${LABELS[$i]} ──${NC}\n"
    printf "  ${GREEN}OK:${NC}      %d\n" "${OKS[$i]}"
    printf "  ${YELLOW}EMPTY:${NC}   %d\n" "${EMPTYS[$i]}"
    printf "  ${RED}ERROR:${NC}   %d\n" "${ERROR_COUNTS[$i]}"
    printf "  ${GRAY}SKIP:${NC}    %d\n" "${SKIPPEDS[$i]}"
    echo ""
    echo "  Timing (${T_COUNT} executed):"
    printf "    Fastest:  %ss\n" "$T_FASTEST"
    printf "    Slowest:  %ss\n" "$T_SLOWEST"
    printf "    Average:  %ss\n" "$T_AVG"
    printf "    Median:   %ss\n" "$T_MEDIAN"
    printf "    Total:    %ss\n" "$T_TOTAL"
    echo ""
    echo "  Report: ${REPORTS[$i]}"

    rm -f "${TIMING_FILES[$i]}"
done

if [ ${#BINARIES[@]} -gt 1 ]; then
    echo ""
    echo "  Comparison report: $COMPARE_REPORT"
fi
