#!/usr/bin/env python3
"""The GitHub identity local agent sessions act as: the pbtb-local-agent App.

A Claude Code session on the workstation pushes, opens and merges PRs, files
issues and dispatches workflows as `pbtb-local-agent[bot]`, not as the person
whose `gh` keyring is on the machine. Three pieces make that hold:

- this checkout's `.claude/settings.local.json` `env` (written by `setup` or
  `env`) points `GH_CONFIG_DIR` at the agent's own gh directory, replaces
  git's credential helpers with `agent_identity.py credential`, and makes the
  bot the commit author and committer. Claude Code applies it to every tool
  process and hook of the session; a person's own terminal never sees it.
- `refresh`, run by the PreToolUse hook before every Bash / PowerShell call,
  keeps `gh/hosts.yml` holding an installation token with at least
  REFRESH_MARGIN_S left, or PLACEHOLDER when none can be minted. The file is
  never absent and its token never empty, because gh falls back to the
  keyring, which holds the person's token: `gh auth git-credential` does so
  when hosts.yml is absent, `gh api` when `oauth_token` is empty.
- `.claude/hooks/guard-shell.py` refuses the commands that would reach around
  this, and `gh` / `git push` / `git commit` in a session without the env.

  agent_identity.py app-url                      the URL that creates the App; then the steps it prints
  agent_identity.py setup --app-id N --key PEM   move the key in, record the App, write this checkout's env
  agent_identity.py env [--remove]               write (or remove) this checkout's settings.local.json env
  agent_identity.py status                       every check the identity rests on; exit 1 when one fails
  agent_identity.py refresh                      the hook: mint a token when the one held is near expiry
  agent_identity.py credential get               git's credential helper

The key and the tokens live in DIR, outside every checkout; nothing here
prints either but `credential get`, whose output git reads (the hook refuses
it as a command). This is a guardrail against acting as the person by mistake,
not a boundary: any process of the workstation user can read DIR.
"""
from __future__ import annotations

import argparse
import base64
import json
import os
import re
import shutil
import sys
import time
from datetime import datetime
from pathlib import Path

REPO = "iengai/pbtb-rust"
APP_NAME = "pbtb-local-agent"
ROOT = Path(__file__).resolve().parents[2]
DIR = Path.home() / ".config" / "pbtb-agent"
KEY = DIR / "private-key.pem"
APP = DIR / "app.json"
TOKEN = DIR / "token.json"
LOG = DIR / "refresh.log"
# Touched when a mint fails, so an offline session does not wait on the
# network before every command.
MINT_FAILED = DIR / "mint-failed"
MINT_RETRY_S = 60
GH_DIR = DIR / "gh"
HOSTS = GH_DIR / "hosts.yml"
# Non-empty and never valid: an empty or absent token lets gh fall back to the keyring.
PLACEHOLDER = "pbtb-agent-no-token"
# Installation tokens live an hour; a command started right after a refresh
# (a `gh pr checks --watch` on the gate, a `gh run watch`) gets at least this.
REFRESH_MARGIN_S = 50 * 60
API = "https://api.github.com"
# What the installation must grant, exactly: secrets, variables and
# administration stay with the person.
PERMISSIONS = {
    "actions": "write",
    "checks": "read",
    "contents": "write",
    "issues": "write",
    "metadata": "read",
    "pull_requests": "write",
    "statuses": "read",
    "workflows": "write",
}
# Relative, so it resolves in whichever checkout git runs it from: git runs a
# helper from the top of the worktree. A checkout without this script fails
# to authenticate rather than falling back.
CREDENTIAL_HELPER = "!python scripts/ops/agent_identity.py credential"


# ---------------------------------------------------------------- plumbing

def api(method: str, path: str, *, bearer: str | None, body: dict | None = None):
    import urllib.request

    headers = {"Accept": "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28", "User-Agent": APP_NAME}
    if bearer:
        headers["Authorization"] = f"Bearer {bearer}"
    data = None
    if body is not None:
        data = json.dumps(body).encode()
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(API + path, data=data, method=method, headers=headers)
    with urllib.request.urlopen(req, timeout=15) as r:
        raw = r.read()
    return json.loads(raw) if raw else None


def b64url(raw: bytes) -> str:
    return base64.urlsafe_b64encode(raw).rstrip(b"=").decode()


def app_jwt(app_id: int, key_pem: bytes) -> str:
    """The RS256 JWT that authenticates as the App itself (ten minutes at most, clock skew allowed for)."""
    from cryptography.hazmat.primitives import hashes, serialization
    from cryptography.hazmat.primitives.asymmetric import padding

    now = int(time.time())
    head = b64url(json.dumps({"alg": "RS256", "typ": "JWT"}).encode())
    claims = b64url(json.dumps({"iat": now - 60, "exp": now + 540, "iss": int(app_id)}).encode())
    key = serialization.load_pem_private_key(key_pem, password=None)
    sig = key.sign(f"{head}.{claims}".encode(), padding.PKCS1v15(), hashes.SHA256())
    return f"{head}.{claims}.{b64url(sig)}"


def write_atomic(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(f"{path.name}.{os.getpid()}.tmp")
    tmp.write_text(text, encoding="utf-8", newline="\n")
    # Windows refuses the replace while a parallel hook or gh has the file open.
    for attempt in range(20):
        try:
            os.replace(tmp, path)
            return
        except PermissionError:
            if attempt == 19:
                raise
            time.sleep(0.05)


def same_path(a: str | None, b: Path) -> bool:
    return bool(a) and os.path.normcase(os.path.normpath(a)) == os.path.normcase(os.path.normpath(str(b)))


def session_has_identity() -> bool:
    """Whether this process runs under the env `env` writes (a Claude Code session in a set-up checkout)."""
    return same_path(os.environ.get("GH_CONFIG_DIR"), GH_DIR)


def app_record() -> dict:
    return json.loads(APP.read_text(encoding="utf-8"))


def hosts_token() -> str:
    try:
        m = re.search(r"^\s*oauth_token:\s*(\S+)\s*$", HOSTS.read_text(encoding="utf-8"), re.M)
    except OSError:
        return ""
    return m.group(1).strip("'\"") if m else ""


def write_hosts(token: str, login: str) -> None:
    write_atomic(HOSTS, f"github.com:\n    oauth_token: {token}\n    user: {login}\n    git_protocol: https\n")


def note(line: str) -> None:
    try:
        old = LOG.read_text(encoding="utf-8").splitlines()[-200:] if LOG.exists() else []
        write_atomic(LOG, "\n".join(old + [f"{datetime.now().isoformat(timespec='seconds')} {line}"]) + "\n")
    except OSError:
        pass


# ---------------------------------------------------------------- token

def mint(rec: dict) -> dict:
    bearer = app_jwt(rec["app_id"], KEY.read_bytes())
    inst = api("GET", f"/repos/{REPO}/installation", bearer=bearer)
    tok = api("POST", f"/app/installations/{inst['id']}/access_tokens", bearer=bearer,
              body={"repositories": [REPO.split("/", 1)[1]]})
    return {"token": tok["token"], "expires_at": tok["expires_at"], "installation_id": inst["id"]}


def seconds_left(meta: dict) -> float:
    return datetime.fromisoformat(meta["expires_at"].replace("Z", "+00:00")).timestamp() - time.time()


def refresh() -> str:
    """The token hosts.yml holds once it has REFRESH_MARGIN_S left; PLACEHOLDER when none can be had.

    Outside a session with the identity env it does nothing and returns "":
    CI runs and a person's terminal load the same hooks.
    """
    if not session_has_identity():
        return ""
    login = f"{APP_NAME}[bot]"
    try:
        if not KEY.exists():
            raise FileNotFoundError(f"no key at {KEY}")
        rec = app_record()
        login = rec["bot_login"]
        meta = json.loads(TOKEN.read_text(encoding="utf-8")) if TOKEN.exists() else {}
        if not meta or seconds_left(meta) < REFRESH_MARGIN_S:
            # A held token with time left still serves while minting fails.
            held = bool(meta) and seconds_left(meta) > 60
            recent = MINT_FAILED.exists() and time.time() - MINT_FAILED.stat().st_mtime < MINT_RETRY_S
            if recent and not held:
                raise RuntimeError(f"a mint failed under {MINT_RETRY_S}s ago")
            if not recent:
                try:
                    meta = mint(rec)
                    write_atomic(TOKEN, json.dumps(meta))
                    MINT_FAILED.unlink(missing_ok=True)
                except Exception as e:  # noqa: BLE001
                    MINT_FAILED.touch()
                    if not held:
                        raise
                    note(f"mint failed, {int(seconds_left(meta) // 60)} min left on the held token: "
                         f"{type(e).__name__}: {str(e)[:300]}")
        if hosts_token() != meta["token"]:
            write_hosts(meta["token"], login)
        return meta["token"]
    except Exception as e:  # noqa: BLE001 -- any failure leaves the placeholder, never the keyring
        note(f"refresh failed: {type(e).__name__}: {str(e)[:300]}")
        try:
            TOKEN.unlink(missing_ok=True)
            if hosts_token() != PLACEHOLDER:
                write_hosts(PLACEHOLDER, login)
        except OSError as e2:
            note(f"placeholder not written: {e2}")
        return PLACEHOLDER


def credential(action: str) -> int:
    fields = dict(line.split("=", 1) for line in sys.stdin.read().splitlines() if "=" in line)
    if action != "get" or fields.get("host") != "github.com":
        return 0
    token = refresh() or hosts_token() or PLACEHOLDER
    print(f"username=x-access-token\npassword={token}")
    return 0


# ---------------------------------------------------------------- setup

def session_env(rec: dict) -> dict[str, str]:
    login = rec["bot_login"]
    email = f"{rec['bot_id']}+{login}@users.noreply.github.com"
    return {
        "GH_CONFIG_DIR": str(GH_DIR),
        # gh prefers these over hosts.yml; empty is unset to gh.
        "GH_TOKEN": "",
        "GITHUB_TOKEN": "",
        # An empty helper resets the list git has read so far (system GCM,
        # the global `gh auth git-credential`); the env entries are read last.
        "GIT_CONFIG_COUNT": "3",
        "GIT_CONFIG_KEY_0": "credential.helper",
        "GIT_CONFIG_VALUE_0": "",
        "GIT_CONFIG_KEY_1": "credential.https://github.com.helper",
        "GIT_CONFIG_VALUE_1": "",
        "GIT_CONFIG_KEY_2": "credential.https://github.com.helper",
        "GIT_CONFIG_VALUE_2": CREDENTIAL_HELPER,
        "GIT_AUTHOR_NAME": login,
        "GIT_AUTHOR_EMAIL": email,
        "GIT_COMMITTER_NAME": login,
        "GIT_COMMITTER_EMAIL": email,
    }


SETTINGS_LOCAL = ROOT / ".claude" / "settings.local.json"


def write_env(rec: dict | None) -> None:
    """Merge the identity env into (or, with rec None, remove it from) this checkout's settings.local.json."""
    data = json.loads(SETTINGS_LOCAL.read_text(encoding="utf-8")) if SETTINGS_LOCAL.exists() else {}
    env = data.get("env", {})
    keys = session_env({"bot_login": "", "bot_id": 0}).keys()
    for k in keys:
        env.pop(k, None)
    if rec is not None:
        env.update(session_env(rec))
    if env:
        data["env"] = env
    else:
        data.pop("env", None)
    write_atomic(SETTINGS_LOCAL, json.dumps(data, indent=2) + "\n")


def cmd_app_url(_: argparse.Namespace) -> int:
    from urllib.parse import urlencode

    params = {"name": APP_NAME, "description": f"Local agent sessions working on {REPO}",
              "url": f"https://github.com/{REPO}", "public": "false", "webhook_active": "false", **PERMISSIONS}
    print("https://github.com/settings/apps/new?" + urlencode(params))
    print(f"""
1. Open the URL signed in as the repository owner and click "Create GitHub App"
   (if the name is taken, pick another; nothing here depends on the slug).
2. On the App's page: "Generate a private key" (a .pem downloads). Note the App ID.
3. "Install App" -> the owner -> "Only select repositories" -> {REPO.split('/')[1]}.
4. python scripts/ops/agent_identity.py setup --app-id <App ID> --key <the .pem>
5. From your own terminal: gh variable set LOCAL_AGENT_USER_ID --body <the bot id setup prints>""")
    return 0


def cmd_setup(a: argparse.Namespace) -> int:
    DIR.mkdir(parents=True, exist_ok=True)
    src = Path(a.key)
    if not same_path(str(src.resolve()), KEY.resolve()):
        shutil.move(str(src), KEY)
    bearer = app_jwt(a.app_id, KEY.read_bytes())
    app = api("GET", "/app", bearer=bearer)
    inst = api("GET", f"/repos/{REPO}/installation", bearer=bearer)
    login = f"{app['slug']}[bot]"
    user = api("GET", f"/users/{login}", bearer=None)
    rec = {"app_id": app["id"], "slug": app["slug"], "bot_login": login, "bot_id": user["id"],
           "installation_id": inst["id"]}
    write_atomic(APP, json.dumps(rec, indent=2) + "\n")
    TOKEN.unlink(missing_ok=True)
    write_env(rec)
    print(f"App {app['slug']} (id {app['id']}) installed on {REPO} (installation {inst['id']}); "
          f"bot {login}, user id {user['id']}.")
    print(f"Wrote the identity env to {SETTINGS_LOCAL}; Claude Code sessions in this checkout pick it up.")
    print(f"From your own terminal: gh variable set LOCAL_AGENT_USER_ID --body {user['id']}")
    return 0


def cmd_env(a: argparse.Namespace) -> int:
    write_env(None if a.remove else app_record())
    print(f"{'Removed the identity env from' if a.remove else 'Wrote the identity env to'} {SETTINGS_LOCAL}")
    return 0


def cmd_status(_: argparse.Namespace) -> int:
    failed = 0

    def report(name: str, ok: bool | None, detail: str) -> None:
        nonlocal failed
        failed += ok is False
        print(f"{'ok  ' if ok else 'FAIL' if ok is False else 'note'} {name}: {detail}")

    if not (KEY.exists() and APP.exists()):
        report("app", False, f"{KEY} or {APP} missing; run app-url, then setup")
        return 1
    rec = app_record()
    report("app", True, f"{rec['slug']} (id {rec['app_id']}), bot {rec['bot_login']} id {rec['bot_id']}")

    try:
        inst = api("GET", f"/repos/{REPO}/installation", bearer=app_jwt(rec["app_id"], KEY.read_bytes()))
        perms = inst.get("permissions", {})
        diff = {k: (perms.get(k), PERMISSIONS.get(k)) for k in set(perms) | set(PERMISSIONS) if perms.get(k) != PERMISSIONS.get(k)}
        report("installation", not diff, f"on {REPO}, permissions " + ("as designed" if not diff else f"differ (granted, wanted): {diff}"))
    except Exception as e:  # noqa: BLE001
        report("installation", False, f"not readable on {REPO}: {e}")

    wanted = session_env(rec)
    local = json.loads(SETTINGS_LOCAL.read_text(encoding="utf-8")).get("env", {}) if SETTINGS_LOCAL.exists() else {}
    report("checkout env", all(local.get(k) == v for k, v in wanted.items()),
           f"{SETTINGS_LOCAL}" + ("" if all(local.get(k) == v for k, v in wanted.items()) else " lacks the identity env; run env"))
    inherited = [k for k in ("GH_TOKEN", "GITHUB_TOKEN") if os.environ.get(k)]
    if inherited:
        report("inherited tokens", False, f"{', '.join(inherited)} set in this process: gh would use it over hosts.yml")
    # Windows drops an env variable set to "", so absent counts as empty.
    in_session = session_has_identity() and all((os.environ.get(k) or "") == v for k, v in wanted.items())
    report("session env", in_session, "this process runs with the identity env" if in_session
           else "this process lacks the identity env (run status from a Claude Code session in this checkout)")

    token = refresh() if in_session else hosts_token()
    try:
        if not token or token == PLACEHOLDER:
            raise RuntimeError(f"hosts.yml holds {'the placeholder' if token else 'no token'} (see {LOG})")
        repos = api("GET", "/installation/repositories", bearer=token)
        names = [r["full_name"] for r in repos.get("repositories", [])]
        meta = json.loads(TOKEN.read_text(encoding="utf-8"))
        report("token", names == [REPO], f"accepted for {names}, {int(seconds_left(meta) // 60)} min left")
    except Exception as e:  # noqa: BLE001
        report("token", False, str(e)[:300])

    report("LOCAL_AGENT_USER_ID", None, f"the App cannot read repository variables; confirm from your own "
                                        f"terminal that `gh variable get LOCAL_AGENT_USER_ID` prints {rec['bot_id']}")
    return 1 if failed else 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sp = ap.add_subparsers(dest="cmd", required=True)
    sp.add_parser("app-url", help="print the URL that creates the App, and the steps after it")
    s = sp.add_parser("setup", help="move the key in, record the App, write this checkout's env")
    s.add_argument("--app-id", type=int, required=True)
    s.add_argument("--key", required=True, help="the downloaded .pem; it is moved, not copied")
    e = sp.add_parser("env", help="write this checkout's settings.local.json env")
    e.add_argument("--remove", action="store_true", help="remove it instead: sessions act as the person again")
    sp.add_parser("status", help="every check the identity rests on")
    sp.add_parser("refresh", help="the PreToolUse hook")
    c = sp.add_parser("credential", help="git's credential helper")
    c.add_argument("action")
    a = ap.parse_args()
    if a.cmd == "refresh":
        sys.stdin.read()
        refresh()
        return 0
    if a.cmd == "credential":
        return credential(a.action)
    return {"app-url": cmd_app_url, "setup": cmd_setup, "env": cmd_env, "status": cmd_status}[a.cmd](a)


if __name__ == "__main__":
    sys.exit(main())
