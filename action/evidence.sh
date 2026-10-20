#!/usr/bin/env bash
# AgentWorth / Evidence, the GitHub Action (docs/specs/evidence-check.md section 4.2).
#
#   1. Work out base, head and the pull request from the event (pull_request or workflow_run).
#   2. Install archie from a release tarball and verify its .sha256 (or use archie-path).
#   3. Make sure base and head are present, then fetch refs/notes/agentworth from the head repo.
#   4. Collect check runs and commit statuses on the head SHA, waiting up to wait-for-ci.
#   5. Collect the GitHub signing keys of the PR author and commit authors.
#   6. Run `archie repo receipt verify` (offline) and post the check run. When the token cannot
#      create check runs (fork pull requests get a read-only token), fall back to the job
#      summary and a notice.
#
# The step always exits 0: the conclusion lives on the check, never in the job status
# (spec section 3.2). Nothing from the pull request is executed. Bash 3.2 compatible (macOS).
#
# Test hooks (apps/cli/tests/action_e2e.rs): AGENTWORTH_GH replaces `gh`,
# AGENTWORTH_RELEASE_BASE_URL replaces the release download URL, AGENTWORTH_GITHUB_URL replaces
# https://github.com for fork note fetches, AGENTWORTH_POLL_SECONDS sets the CI poll interval.

set -uo pipefail

CHECK_NAME="AgentWorth / Evidence"
RELEASE_REPO="unfoundbox-crew/agentworth"
TARGET="x86_64-unknown-linux-gnu"
GH="${AGENTWORTH_GH:-gh}"
GITHUB_URL="${AGENTWORTH_GITHUB_URL:-https://github.com}"
RELEASE_BASE_URL="${AGENTWORTH_RELEASE_BASE_URL:-$GITHUB_URL/$RELEASE_REPO/releases/download}"
POLL="${AGENTWORTH_POLL_SECONDS:-15}"
WORK="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/agentworth-evidence"
SUMMARY="${GITHUB_STEP_SUMMARY:-/dev/null}"
OUTPUT="${GITHUB_OUTPUT:-/dev/null}"
REPO="${GITHUB_REPOSITORY:-}"
EVENT="${GITHUB_EVENT_NAME:-}"
EVENT_PATH="${GITHUB_EVENT_PATH:-}"

# Workflow commands are parsed from stdout, so anything echoed into one is reduced to plain
# printable text first: no "::", no "%", no newlines.
clean() { printf '%s' "$*" | tr -d '\r' | tr '\n' ' ' | sed -e 's/::/: :/g' -e 's/%/%25/g' | cut -c1-400; }
notice() { echo "::notice title=$CHECK_NAME::$(clean "$*")"; }
warn() { echo "::warning title=$CHECK_NAME::$(clean "$*")"; }
# Every early stop goes through here, so the step exits 0 and still says why.
stop() {
  [ "$#" -gt 0 ] && warn "$*"
  echo "posted=false" >>"$OUTPUT"
  exit 0
}

is_sha() { printf '%s' "$1" | grep -Eq '^([0-9a-f]{40}|[0-9a-f]{64})$'; }
is_uint() { printf '%s' "$1" | grep -Eq '^[0-9]+$'; }
is_full_repo() { printf '%s' "$1" | grep -Eq '^[A-Za-z0-9-]+/[A-Za-z0-9._-]+$'; }
is_login() { printf '%s' "$1" | grep -Eq '^[A-Za-z0-9]([A-Za-z0-9]|-[A-Za-z0-9]){0,38}$'; }
now() { date +%s; }

command -v jq >/dev/null 2>&1 || stop "jq is required on the runner (preinstalled on GitHub-hosted runners)"
command -v git >/dev/null 2>&1 || stop "git is required on the runner"
is_full_repo "$REPO" || stop "GITHUB_REPOSITORY is not set; this runs inside GitHub Actions"
[ -n "$EVENT_PATH" ] && [ -f "$EVENT_PATH" ] || stop "no event payload (GITHUB_EVENT_PATH)"
git rev-parse --git-dir >/dev/null 2>&1 || stop "no git checkout here: add actions/checkout with fetch-depth: 0 before this step"
mkdir -p "$WORK" || stop "cannot create $WORK"

# ---------------------------------------------------------------------------------------------
# Inputs
# ---------------------------------------------------------------------------------------------

WAIT="${INPUT_WAIT_FOR_CI:-600}"
if ! is_uint "$WAIT" || [ "$WAIT" -gt 3600 ]; then
  warn "wait-for-ci must be 0 to 3600 seconds; using 600"
  WAIT=600
fi
is_uint "$POLL" && [ "$POLL" -ge 1 ] || POLL=15
TRANSPORT="${INPUT_TRANSPORT:-auto}"
case "$TRANSPORT" in
  auto | notes | file) ;;
  *)
    warn "transport must be auto, notes or file; using auto"
    TRANSPORT=auto
    ;;
esac
FAIL_ON="${INPUT_FAIL_ON:-}"

# ---------------------------------------------------------------------------------------------
# 1. The pull request
# ---------------------------------------------------------------------------------------------

ev() { jq -r "$1 // empty" "$EVENT_PATH"; }

BASE="" HEAD="" PR="" HEAD_REPO="" PR_AUTHOR=""
case "$EVENT" in
  pull_request | pull_request_review | pull_request_review_comment)
    BASE="$(ev .pull_request.base.sha)"
    HEAD="$(ev .pull_request.head.sha)"
    PR="$(ev .pull_request.number)"
    HEAD_REPO="$(ev .pull_request.head.repo.full_name)"
    PR_AUTHOR="$(ev .pull_request.user.login)"
    ;;
  workflow_run)
    # Runs in the base repository's context after the named workflows complete, so a fork PR
    # gets a token that can write checks. Only git data and API results are read.
    HEAD="$(ev .workflow_run.head_sha)"
    HEAD_REPO="$(ev .workflow_run.head_repository.full_name)"
    PR="$(ev '.workflow_run.pull_requests[0].number')"
    if [ -z "$PR" ]; then
      # GitHub leaves pull_requests empty for fork PRs; look the PR up by its head branch.
      owner="$(ev .workflow_run.head_repository.owner.login)"
      branch="$(ev .workflow_run.head_branch)"
      if is_login "$owner" && [ -n "$branch" ]; then
        q_branch="$(jq -rn --arg b "$branch" '$b|@uri')"
        PR="$("$GH" api "repos/$REPO/pulls?state=open&per_page=100&head=$owner:$q_branch" 2>/dev/null |
          jq -r --arg h "$HEAD" '[.[] | select(.head.sha == $h)][0].number // empty' 2>/dev/null)"
      fi
    fi
    if is_uint "$PR"; then
      pr_json="$("$GH" api "repos/$REPO/pulls/$PR" 2>/dev/null)"
      BASE="$(printf '%s' "$pr_json" | jq -r '.base.sha // empty' 2>/dev/null)"
      PR_AUTHOR="$(printf '%s' "$pr_json" | jq -r '.user.login // empty' 2>/dev/null)"
    fi
    if [ -z "$PR" ]; then
      notice "this workflow_run is not for an open pull request; nothing to check"
      stop
    fi
    ;;
  pull_request_target)
    stop "pull_request_target is not supported (it would run with a write token next to PR code); use pull_request, or workflow_run for fork PRs"
    ;;
  *)
    notice "runs on pull_request or workflow_run events; this is $EVENT, nothing to check"
    stop
    ;;
esac

is_sha "$HEAD" || stop "could not read the head commit from the $EVENT event"
is_sha "$BASE" || stop "could not read the base commit of the pull request"
is_uint "$PR" || stop "could not read the pull request number"
is_full_repo "$HEAD_REPO" || HEAD_REPO="$REPO"
is_login "$PR_AUTHOR" || PR_AUTHOR=""

# ---------------------------------------------------------------------------------------------
# 2. archie
# ---------------------------------------------------------------------------------------------

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

install_archie() {
  local version="${INPUT_ARCHIE_VERSION:-}" ref="${AGENTWORTH_ACTION_REF:-}"
  if [ -z "$version" ] && printf '%s' "$ref" | grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+$'; then
    version="$ref"
  fi
  if [ -z "$version" ]; then
    version="$("$GH" api "repos/$RELEASE_REPO/releases/latest" 2>/dev/null | jq -r '.tag_name // empty' 2>/dev/null)"
  fi
  case "$version" in v*) ;; *) version="v$version" ;; esac
  printf '%s' "$version" | grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+$' ||
    stop "could not resolve an archie release (set archie-version, such as v0.1.30)"
  if [ "${RUNNER_OS:-Linux}" != "Linux" ] || [ "${RUNNER_ARCH:-X64}" != "X64" ]; then
    stop "release binaries are installed for Linux X64 runners only; use runs-on: ubuntu-latest or set archie-path"
  fi

  local dir="${RUNNER_TOOL_CACHE:-$WORK/tools}/agentworth/${version#v}/x64"
  if [ -x "$dir/archie" ]; then
    ARCHIE="$dir/archie"
    return
  fi
  local name="agentworth-$version-$TARGET.tar.gz" tmp="$WORK/download"
  rm -rf "$tmp"
  mkdir -p "$tmp" "$dir" || stop "cannot create $dir"
  curl -fsSL --retry 3 -o "$tmp/$name" "$RELEASE_BASE_URL/$version/$name" ||
    stop "could not download $name"
  curl -fsSL --retry 3 -o "$tmp/$name.sha256" "$RELEASE_BASE_URL/$version/$name.sha256" ||
    stop "could not download $name.sha256"
  local want got
  want="$(awk '{print $1; exit}' "$tmp/$name.sha256" | tr 'A-F' 'a-f')"
  got="$(sha256_of "$tmp/$name")"
  printf '%s' "$want" | grep -Eq '^[0-9a-f]{64}$' || stop "$name.sha256 does not hold a SHA-256"
  # Never run a binary whose checksum does not match the release.
  [ "$want" = "$got" ] || stop "checksum mismatch for $name (expected $want, got $got); not running it"
  tar -xzf "$tmp/$name" -C "$dir" archie || stop "could not unpack archie from $name"
  chmod +x "$dir/archie"
  ARCHIE="$dir/archie"
}

ARCHIE="${INPUT_ARCHIE_PATH:-}"
if [ -n "$ARCHIE" ]; then
  [ -x "$ARCHIE" ] || stop "archie-path $ARCHIE is not an executable file"
else
  install_archie
fi
"$ARCHIE" repo receipt verify --help >/dev/null 2>&1 ||
  stop "this archie has no 'repo receipt verify' (it needs v0.1.30 or later)"

# ---------------------------------------------------------------------------------------------
# 3. Commits and notes
# ---------------------------------------------------------------------------------------------

has_commit() { git cat-file -e "$1^{commit}" 2>/dev/null; }
if [ "$(git rev-parse --is-shallow-repository 2>/dev/null)" = "true" ]; then
  git fetch -q --no-tags --unshallow origin 2>/dev/null ||
    warn "the checkout is shallow and could not be deepened; use actions/checkout with fetch-depth: 0"
fi
# workflow_run checks out the default branch; the PR head lives under refs/pull/N/head of the
# base repository, fork or not.
has_commit "$HEAD" || git fetch -q --no-tags origin "+refs/pull/$PR/head:refs/agentworth/pr-head" 2>/dev/null
has_commit "$BASE" || git fetch -q --no-tags origin "$BASE" 2>/dev/null
has_commit "$HEAD" || stop "head commit $HEAD is not in this checkout; use actions/checkout with fetch-depth: 0"
has_commit "$BASE" || stop "base commit $BASE is not in this checkout; use actions/checkout with fetch-depth: 0"

if [ "$TRANSPORT" != "file" ]; then
  # Receipts are pushed by the author to the repository they push to: the head repository.
  if [ "$HEAD_REPO" = "$REPO" ]; then
    notes_src="origin"
  else
    notes_src="$GITHUB_URL/$HEAD_REPO.git"
  fi
  if ! git fetch -q --no-tags "$notes_src" "+refs/notes/agentworth:refs/notes/agentworth" 2>/dev/null; then
    echo "No refs/notes/agentworth on $HEAD_REPO (no receipts pushed as notes)."
  fi
fi

# ---------------------------------------------------------------------------------------------
# 4. Head CI
# ---------------------------------------------------------------------------------------------

# This job's own check suite is left out, or the check would wait on itself.
SELF_SUITE="null"
if is_uint "${GITHUB_RUN_ID:-}"; then
  s="$("$GH" api "repos/$REPO/actions/runs/$GITHUB_RUN_ID" 2>/dev/null | jq -r '.check_suite_id // empty' 2>/dev/null)"
  is_uint "$s" && SELF_SUITE="$s"
fi

CI_JSON="$WORK/ci.json"
collect_ci() {
  local runs statuses
  runs="$("$GH" api --paginate "repos/$REPO/commits/$HEAD/check-runs?per_page=100" 2>/dev/null |
    jq -s --arg n "$CHECK_NAME" --argjson suite "$SELF_SUITE" \
      '[.[].check_runs[]? | select(.name != $n)
        | select($suite == null or (.check_suite.id // null) != $suite)
        | {name, status, conclusion}]' 2>/dev/null)" || return 1
  statuses="$("$GH" api "repos/$REPO/commits/$HEAD/status?per_page=100" 2>/dev/null |
    jq '[.statuses[]? | {context, state}]' 2>/dev/null)" || return 1
  [ -n "$runs" ] && [ -n "$statuses" ] || return 1
  jq -n --arg h "$HEAD" --argjson c "$runs" --argjson s "$statuses" \
    '{head_sha: $h, check_runs: $c, statuses: $s}' >"$CI_JSON"
}
ci_pending() {
  jq '[.check_runs[] | select(.status != "completed")] + [.statuses[] | select(.state == "pending")] | length' "$CI_JSON"
}

HAVE_CI=0
deadline=$(($(now) + WAIT))
while :; do
  if collect_ci; then
    HAVE_CI=1
    [ "$(ci_pending)" = "0" ] && break
  fi
  [ "$(now)" -lt "$deadline" ] || break
  sleep "$POLL"
done
[ "$HAVE_CI" = 1 ] || warn "could not read check runs on $HEAD; reporting CI as unknown"

# ---------------------------------------------------------------------------------------------
# 5. Signing keys
# ---------------------------------------------------------------------------------------------

KEYS_JSON="$WORK/keys.json"
echo '{}' >"$KEYS_JSON"
logins="$PR_AUTHOR"
commit_logins="$("$GH" api --paginate "repos/$REPO/pulls/$PR/commits?per_page=100" 2>/dev/null |
  jq -r '.[]? | .author.login // empty' 2>/dev/null)"
logins="$(printf '%s\n%s\n' "$logins" "$commit_logins" | awk 'NF && !seen[$0]++' | head -n 10)"
for login in $logins; do
  is_login "$login" || continue
  if "$GH" api "users/$login/ssh_signing_keys?per_page=100" >"$WORK/key-page.json" 2>/dev/null &&
    jq -e 'type == "array"' "$WORK/key-page.json" >/dev/null 2>&1; then
    jq --arg l "$login" --slurpfile k "$WORK/key-page.json" '. + {($l): $k[0]}' "$KEYS_JSON" >"$KEYS_JSON.tmp" &&
      mv "$KEYS_JSON.tmp" "$KEYS_JSON"
  fi
done

# ---------------------------------------------------------------------------------------------
# 6. Verify and post
# ---------------------------------------------------------------------------------------------

CHECK_JSON="$WORK/check.json"
set -- repo receipt verify --repo . --base "$BASE" --head "$HEAD" --keys "$KEYS_JSON" --format check-json
[ "$HAVE_CI" = 1 ] && set -- "$@" --ci "$CI_JSON"
[ -n "$FAIL_ON" ] && set -- "$@" --fail-on "$FAIL_ON"
if ! "$ARCHIE" "$@" >"$CHECK_JSON" 2>"$WORK/verify.err"; then
  stop "archie repo receipt verify failed: $(head -c 400 "$WORK/verify.err")"
fi
jq -e '.name and .head_sha and .conclusion and .output.title and .output.summary' "$CHECK_JSON" >/dev/null 2>&1 ||
  stop "archie repo receipt verify printed no check"

CONCLUSION="$(jq -r .conclusion "$CHECK_JSON")"
TITLE="$(jq -r .output.title "$CHECK_JSON")"
{
  echo "conclusion=$CONCLUSION"
  echo "title=$(clean "$TITLE")"
} >>"$OUTPUT"
jq -r .output.summary "$CHECK_JSON" >>"$SUMMARY"

if "$GH" api -X POST "repos/$REPO/check-runs" --input "$CHECK_JSON" >"$WORK/post.out" 2>"$WORK/post.err"; then
  echo "posted=true" >>"$OUTPUT"
  echo "$CHECK_NAME: $CONCLUSION, $TITLE"
elif grep -Eq '403|Resource not accessible' "$WORK/post.err"; then
  echo "posted=false" >>"$OUTPUT"
  notice "this token cannot create check runs (a fork pull request, or the workflow lacks checks: write). Result: $CONCLUSION, $TITLE. The table is in the job summary; run the Action from a workflow_run workflow to get the check on fork PRs."
else
  echo "posted=false" >>"$OUTPUT"
  warn "could not create the check run: $(head -c 300 "$WORK/post.err"). Result: $CONCLUSION, $TITLE (see the job summary)"
fi
exit 0
