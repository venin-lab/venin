//! Permission rules in Claude Code's settings shape: which allow rules are too wide, what each lets run without
//! asking, and which acting MCP tools run with no one asking. Rule shapes follow Claude Code's permission docs: `*`
//! matches any text, `:*` at the end equals ` *`, a `*` straight after letters (`py*`) also matches longer names, `//`
//! is the disk root, `~/` the home folder, `/x` starts at the settings file's folder, `Write(path)` matches nothing,
//! and a glob in the MCP server name approves nothing.

use regex::Regex;
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::sync::LazyLock;

pub type Acts = BTreeSet<&'static str>;

fn acts(list: &[&'static str]) -> Acts {
    list.iter().copied().collect()
}

fn rx(pattern: &str) -> Regex {
    Regex::new(pattern).expect("a valid pattern")
}

const ALL: [&str; 8] = [
    "code", "push", "publish", "send", "delete", "spend", "install", "commit",
];
/// Worst first; a rule that lets everything run comes before all of them.
const RANK: [&str; 9] = [
    "code", "push", "publish", "install", "delete", "discard", "send", "spend", "secrets",
];

fn all() -> Acts {
    acts(&ALL)
}

const LANG: [(&str, &str); 26] = [
    ("python", "Python"),
    ("node", "JavaScript"),
    ("deno", "JavaScript"),
    ("bun", "JavaScript"),
    ("tsx", "TypeScript"),
    ("ts-node", "TypeScript"),
    ("ruby", "Ruby"),
    ("perl", "Perl"),
    ("php", "PHP"),
    ("lua", "Lua"),
    ("osascript", "AppleScript"),
    ("Rscript", "R"),
    ("tclsh", "Tcl"),
    ("swift", "Swift"),
    ("pwsh", "PowerShell"),
    ("powershell", "PowerShell"),
    ("sh", "shell"),
    ("bash", "shell"),
    ("zsh", "shell"),
    ("fish", "shell"),
    ("dash", "shell"),
    ("ksh", "shell"),
    ("awk", "awk"),
    ("gawk", "awk"),
    ("nawk", "awk"),
    ("mawk", "awk"),
];

fn lang(head: &str) -> Option<&'static str> {
    LANG.iter().find(|(h, _)| *h == head).map(|(_, l)| *l)
}

/// Flag letters that take inline code, per language.
fn inline(lang: &str) -> &'static str {
    match lang {
        "Python" | "shell" | "PowerShell" => "c",
        "JavaScript" | "TypeScript" => "ep",
        "Ruby" | "Lua" | "AppleScript" | "R" | "Swift" => "e",
        "Perl" => "eE",
        "PHP" => "r",
        _ => "",
    }
}

/// Flags that take a value, per language.
fn value_flags(lang: &str) -> &'static [&'static str] {
    match lang {
        "Python" => &["-W", "-X"],
        "JavaScript" => &[
            "-r",
            "--require",
            "--import",
            "--loader",
            "--experimental-loader",
            "--input-type",
        ],
        "TypeScript" => &["-r", "--require", "--import"],
        "Perl" => &["-I", "-M", "-m"],
        "Ruby" => &["-I", "-r"],
        "awk" => &["-F", "-v"],
        "PHP" => &["-c", "-d"],
        _ => &[],
    }
}

fn wrap_value(head: &str) -> &'static [&'static str] {
    match head {
        "xargs" => &["-n", "-I", "-L", "-P", "-s", "-E", "-d", "-a"],
        "env" => &["-u", "-C", "-S"],
        "timeout" => &["-s", "-k"],
        "caffeinate" => &["-t", "-w"],
        "nice" => &["-n"],
        "stdbuf" => &["-i", "-o", "-e"],
        "uv" => &[
            "--with",
            "--python",
            "-p",
            "--project",
            "--directory",
            "--env-file",
            "--from",
        ],
        "poetry" => &["-C", "--directory"],
        "conda" => &["-n", "--name", "-p", "--prefix"],
        "devbox" => &["-c", "--config"],
        _ => &[],
    }
}

const RISKY_VERBS: [&str; 37] = [
    "run",
    "exec",
    "delete",
    "rm",
    "remove",
    "deploy",
    "apply",
    "destroy",
    "create",
    "merge",
    "close",
    "push",
    "publish",
    "release",
    "send",
    "api",
    "compose",
    "ssh",
    "cp",
    "scp",
    "mv",
    "kill",
    "stop",
    "start",
    "restart",
    "scale",
    "edit",
    "patch",
    "set",
    "update",
    "upgrade",
    "install",
    "uninstall",
    "wallet",
    "rpc",
    "script",
    "transfer",
];
const NET: [&str; 16] = [
    "curl", "wget", "nc", "ncat", "netcat", "socat", "telnet", "ftp", "ssh", "scp", "sftp",
    "rsync", "http", "https", "aria2c", "xh",
];
const CLOUD: [&str; 23] = [
    "gh",
    "aws",
    "gcloud",
    "az",
    "kubectl",
    "helm",
    "terraform",
    "pulumi",
    "wrangler",
    "vercel",
    "netlify",
    "firebase",
    "fly",
    "flyctl",
    "heroku",
    "supabase",
    "doctl",
    "docker",
    "podman",
    "cast",
    "forge",
    "solana",
    "spl-token",
];
const CHAIN: [&str; 4] = ["cast", "forge", "solana", "spl-token"];
const CHAIN_VERBS: [&str; 14] = [
    "send", "publish", "wallet", "rpc", "create", "script", "transfer", "deploy", "program",
    "stake", "withdraw", "mint", "burn", "approve",
];
const READ_VERBS: [&str; 36] = [
    "list",
    "ls",
    "get",
    "describe",
    "show",
    "view",
    "status",
    "whoami",
    "version",
    "help",
    "logs",
    "log",
    "info",
    "search",
    "inspect",
    "ps",
    "images",
    "diff",
    "plan",
    "validate",
    "call",
    "balance",
    "block",
    "tx",
    "receipt",
    "nonce",
    "code",
    "storage",
    "chain-id",
    "sig",
    "build",
    "test",
    "fmt",
    "tail",
    "--version",
    "--help",
];
const RUNNERS: [(&str, &str); 14] = [
    ("uv", "run"),
    ("poetry", "run"),
    ("pipenv", "run"),
    ("conda", "run"),
    ("direnv", "exec"),
    ("devbox", "run"),
    ("mise", "exec"),
    ("mise", "x"),
    ("pixi", "run"),
    ("hatch", "run"),
    ("pdm", "run"),
    ("rye", "run"),
    ("nix", "run"),
    ("bundle", "exec"),
];
const WRAPPERS: [&str; 10] = [
    "timeout",
    "nice",
    "nohup",
    "time",
    "stdbuf",
    "builtin",
    "noglob",
    "caffeinate",
    "env",
    "xargs",
];
const DELETERS: [&str; 6] = ["rm", "rmdir", "unlink", "shred", "srm", "trash"];
const WRITERS: [&str; 8] = [
    "cp", "mv", "ln", "chmod", "chown", "chflags", "dd", "truncate",
];
const VAULTS: [&str; 7] = ["op", "pass", "bw", "lpass", "gopass", "keyring", "vault"];
const GIT_LOSE: [&str; 7] = [
    "reset", "clean", "checkout", "restore", "switch", "stash", "rebase",
];
const GIT_DELETE: [&str; 9] = [
    "rm",
    "branch",
    "worktree",
    "gc",
    "prune",
    "reflog",
    "update-ref",
    "filter-branch",
    "filter-repo",
];
const GIT_RUN: [&str; 3] = ["submodule", "bisect", "config"];
const GIT_MORE: [&str; 12] = [
    "push",
    "commit",
    "add",
    "log",
    "status",
    "diff",
    "show",
    "fetch",
    "pull",
    "merge",
    "tag",
    "send-email",
];
const NPM_SUBS: [&str; 13] = [
    "run",
    "run-script",
    "exec",
    "x",
    "dlx",
    "install",
    "i",
    "add",
    "ci",
    "publish",
    "test",
    "update",
    "uninstall",
];
/// Names Claude Code drops before matching a rule.
const STRIPPED: [&str; 9] = [
    "timeout", "time", "nice", "nohup", "stdbuf", "command", "builtin", "noglob", "xargs",
];

fn sorted(items: impl IntoIterator<Item = &'static str>) -> Vec<&'static str> {
    items
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

static GIT_SUBS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    sorted(
        GIT_LOSE
            .into_iter()
            .chain(GIT_DELETE)
            .chain(GIT_RUN)
            .chain(GIT_MORE),
    )
});

static HEADS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    sorted(
        LANG.iter()
            .map(|(h, _)| *h)
            .chain(["python3", "python2"])
            .chain(NET)
            .chain(CLOUD)
            .chain(DELETERS)
            .chain(WRITERS)
            .chain(VAULTS)
            .chain(RUNNERS.iter().map(|(h, _)| *h))
            .chain(WRAPPERS)
            .chain([
                "git", "npm", "npx", "pnpm", "pnpx", "yarn", "bunx", "uvx", "pipx", "pip", "pip3",
                "brew", "gem", "cargo", "go", "make", "just", "sudo", "su", "doas", "eval", "exec",
                "command", "security", "sqlite3",
            ]),
    )
});

fn subs(head: &str) -> Vec<&'static str> {
    match head {
        "git" => GIT_SUBS.clone(),
        "npm" | "pnpm" | "yarn" => NPM_SUBS.to_vec(),
        "pip" | "pip3" => vec!["install", "show", "list"],
        "uv" => vec!["run", "add", "pip"],
        "bun" => vec!["run", "x"],
        "deno" => vec!["run", "eval"],
        "docker" => vec!["run", "exec", "compose", "ps", "images"],
        "gh" => vec!["api", "pr", "repo", "release", "workflow"],
        "cast" => sorted(CHAIN_VERBS),
        "forge" => vec!["create", "script", "build", "test"],
        _ => Vec::new(),
    }
}

static SAFE_SUB: LazyLock<BTreeSet<&'static str>> = LazyLock::new(|| {
    GIT_SUBS
        .iter()
        .copied()
        .chain(READ_VERBS)
        .chain(CHAIN_VERBS)
        .chain([
            "run",
            "run-script",
            "exec",
            "x",
            "dlx",
            "init",
            "task",
            "install",
            "i",
            "add",
            "publish",
            "deploy",
            "release",
            "pages",
            "apply",
            "destroy",
            "delete",
            "api",
            "pr",
            "repo",
            "s3",
            "compose",
            "find-generic-password",
            "find-internet-password",
            "dump-keychain",
            "export",
            "cms",
            "pip",
            "-m",
            "-c",
            "-e",
            "-I",
        ])
        .collect()
});

static CLOUD_WORDS: LazyLock<BTreeSet<&'static str>> = LazyLock::new(|| {
    SAFE_SUB
        .iter()
        .copied()
        .chain(RISKY_VERBS)
        .chain([
            "s3",
            "ec2",
            "iam",
            "lambda",
            "app",
            "functions",
            "compute",
            "issue",
            "gist",
            "secret",
            "r2",
            "kv",
            "d1",
            "workers",
            "storage",
            "sql",
            "container",
            "image",
            "volume",
            "network",
            "service",
            "pods",
            "deployment",
        ])
        .collect()
});

static ASSIGN: LazyLock<Regex> = LazyLock::new(|| rx(r"^[A-Za-z_]\w*="));
static DURATION: LazyLock<Regex> = LazyLock::new(|| rx(r"^\d+(\.\d+)?[smhd]?$"));
static RULE: LazyLock<Regex> = LazyLock::new(|| rx(r"(?s)^\s*([\w\-*]+)\s*(?:\((.*)\))?\s*$"));

/// A rule's tool and its bracketed argument.
pub fn parse(rule: &str) -> (String, Option<String>) {
    match RULE.captures(rule) {
        Some(c) => (c[1].to_string(), c.get(2).map(|m| m.as_str().to_string())),
        None => (rule.to_string(), None),
    }
}

/// POSIX shell words; on an unclosed quote, words split at spaces with their quotes trimmed.
pub fn split_words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut chars = s.chars();
    let mut ok = true;
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(x) => word.push(x),
                        None => {
                            ok = false;
                            break;
                        }
                    }
                }
            }
            '"' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(x) if "$`\"\\\n".contains(x) => word.push(x),
                            Some(x) => {
                                word.push('\\');
                                word.push(x);
                            }
                            None => {
                                ok = false;
                                break;
                            }
                        },
                        Some(x) => word.push(x),
                        None => {
                            ok = false;
                            break;
                        }
                    }
                }
            }
            '\\' => match chars.next() {
                Some(x) => {
                    started = true;
                    word.push(x);
                }
                None => ok = false,
            },
            c if c.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            c => {
                started = true;
                word.push(c);
            }
        }
        if !ok {
            break;
        }
    }
    if !ok {
        return s
            .split_whitespace()
            .map(|w| w.trim_matches(['"', '\'']).to_string())
            .collect();
    }
    if started {
        out.push(word);
    }
    out
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn severity(a: &Acts) -> i32 {
    if ALL.iter().all(|x| a.contains(x)) {
        return -1;
    }
    a.iter()
        .filter_map(|x| RANK.iter().position(|r| r == x))
        .min()
        .map_or(RANK.len() as i32, |i| i as i32)
}

/// What one exact (or narrow) command does.
fn acts_of(head: &str, sub: &str) -> Acts {
    if head == "git" {
        return match sub {
            "push" => acts(&["push"]),
            "commit" => acts(&["commit"]),
            "send-email" => acts(&["send"]),
            s if GIT_LOSE.contains(&s) => acts(&["discard"]),
            s if GIT_DELETE.contains(&s) => acts(&["delete"]),
            _ => Acts::new(),
        };
    }
    if DELETERS.contains(&head) {
        return acts(&["delete"]);
    }
    if CHAIN.contains(&head) && CHAIN_VERBS.contains(&sub) {
        return acts(&["spend"]);
    }
    if ["publish", "deploy", "release"].contains(&sub)
        || (CLOUD.contains(&head) && ["pages", "apply", "destroy"].contains(&sub))
    {
        return acts(&["publish"]);
    }
    if ["install", "i", "add"].contains(&sub)
        && [
            "npm", "pnpm", "yarn", "pip", "pip3", "brew", "gem", "cargo", "uv", "bun",
        ]
        .contains(&head)
    {
        return acts(&["install"]);
    }
    Acts::new()
}

/// Is an interpreter rule's code left open: an open quote before the wildcard, an inline-code flag with no code fixed
/// after it, code from standard input, or no script named at all?
fn interp(lang: &str, rest: &[String], fixed: &str) -> bool {
    if fixed.matches('"').count() % 2 == 1 || fixed.matches('\'').count() % 2 == 1 {
        return true;
    }
    let letters = inline(lang);
    for (k, w) in rest.iter().enumerate() {
        let short = w.len() > 1
            && w.starts_with('-')
            && w[1..].chars().all(|c| c.is_ascii_alphabetic())
            && w[1..].chars().any(|c| letters.contains(c));
        if [
            "--eval",
            "--print",
            "--command",
            "-Command",
            "-EncodedCommand",
        ]
        .contains(&w.as_str())
            || short
        {
            return !rest[k + 1..]
                .iter()
                .any(|x| !x.trim_matches(['"', '\'', '<', '-', ' ']).is_empty());
        }
        if w == "-" || w.starts_with('<') {
            return true;
        }
    }
    let mut args = 0;
    let mut skip = false;
    for w in rest {
        if skip {
            skip = false;
        } else if value_flags(lang).contains(&w.as_str()) {
            skip = true;
        } else if !w.starts_with('-') {
            args += 1;
        }
    }
    args == 0
}

/// `a` without its first `n` space-separated tokens; unchanged when it has fewer.
fn drop_tokens(a: &str, n: usize) -> String {
    let mut rest = a.trim_start();
    for _ in 0..n {
        let token = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let after = &rest[token..];
        let gap = after.len() - after.trim_start().len();
        if token == 0 || gap == 0 {
            return a.to_string();
        }
        rest = &after[gap..];
    }
    rest.to_string()
}

type Why = (Option<String>, Acts);

fn wide(why: &str, a: Acts) -> Why {
    (Some(why.to_string()), a)
}

fn narrow() -> Why {
    (None, Acts::new())
}

/// Why a Bash allow rule is too wide (or `None`), and the actions it lets run without asking.
fn bash(arg: Option<&str>) -> Why {
    let trimmed = arg.unwrap_or_default().trim();
    if ["", "*", ":*"].contains(&trimmed) {
        return wide("any shell command", all());
    }
    let a = match trimmed.strip_suffix(":*") {
        Some(head) => format!("{head} *"),
        None => trimmed.to_string(),
    };
    if a.starts_with('*') {
        return wide("any program at all", all());
    }
    let fixed = a.split('*').next().unwrap_or_default();
    let mut words = split_words(fixed);
    let mut n_set = 0;
    while words.first().is_some_and(|w| ASSIGN.is_match(w)) {
        words.remove(0);
        n_set += 1;
    }
    if words.is_empty() {
        return wide("any shell command", all());
    }
    let wild = a.contains('*');
    if wild && !fixed.is_empty() && !fixed.ends_with(char::is_whitespace) {
        let last = words.last().cloned().unwrap_or_default();
        let lead = &words[..words.len() - 1];
        let known: Vec<&str> = match words.len() {
            2 => subs(&words[0]),
            1 => HEADS
                .iter()
                .copied()
                .filter(|h| !STRIPPED.contains(h))
                .collect(),
            _ => Vec::new(),
        };
        let mut best: Option<(&str, Why)> = None;
        for w in known.into_iter().filter(|w| w.starts_with(&last)) {
            let mut cand = lead.to_vec();
            cand.push(w.to_string());
            let found = bash(Some(&format!("{} *", cand.join(" "))));
            if found.1.is_empty() {
                continue;
            }
            let key = |name: &str, a: &Acts| (severity(a), -(a.len() as i64), name != last);
            if best
                .as_ref()
                .is_none_or(|(bw, b)| key(w, &found.1) < key(bw, &b.1))
            {
                best = Some((w, found));
            }
        }
        if let Some((w, (why, found))) = best {
            if w == last {
                return (why, found);
            }
            let mut shown = lead.to_vec();
            shown.push(last);
            return (
                why.map(|why| format!("{why} ({}* covers {w})", shown.join(" "))),
                found,
            );
        }
    }
    let mut head = basename(&words[0]).to_string();
    let mut rest: Vec<String> = words[1..].to_vec();
    if head == "uv" && rest.first().is_some_and(|r| r == "tool") {
        if rest.get(1).is_some_and(|r| r == "run") {
            head = "uvx".into();
            rest = rest[2..].to_vec();
        } else {
            head = "pip".into();
            rest = rest[1..].to_vec();
        }
    }
    let first = rest.first().map(String::as_str).unwrap_or_default();
    let runner = RUNNERS.contains(&(head.as_str(), first));
    let h = head.as_str();
    if WRAPPERS.contains(&h)
        || runner
        || (h == "command" && !rest.iter().any(|r| r == "-v" || r == "-V"))
    {
        let mut k = if runner { 2 } else { 1 };
        let skip = wrap_value(h);
        while k < words.len() {
            let w = words[k].as_str();
            if ASSIGN.is_match(w) || w.starts_with('-') {
                k += if skip.contains(&w) { 2 } else { 1 };
            } else if h == "timeout" && DURATION.is_match(w) {
                k += 1;
            } else {
                break;
            }
        }
        if k >= words.len() {
            return if wild {
                wide("any command", all())
            } else {
                narrow()
            };
        }
        if !wild {
            let sub = words.get(k + 1).map(String::as_str).unwrap_or_default();
            return (None, acts_of(basename(&words[k]), sub));
        }
        return bash(Some(&drop_tokens(&a, n_set + k)));
    }
    let mut config = false;
    let mut after: &[String] = &[];
    let sub: String = if h == "git" {
        let mut k = 0;
        while k < rest.len() && rest[k].starts_with('-') {
            config |= rest[k] == "-c";
            k += if rest[k] == "-C" || rest[k] == "-c" {
                2
            } else {
                1
            };
        }
        after = rest.get(k + 1..).unwrap_or_default();
        rest.get(k).cloned().unwrap_or_default()
    } else {
        rest.iter()
            .find(|w| !w.starts_with('-'))
            .cloned()
            .unwrap_or_default()
    };
    let sub = sub.as_str();
    if !wild {
        return (None, acts_of(h, sub));
    }
    let args: Vec<&str> = rest
        .iter()
        .map(String::as_str)
        .filter(|w| !w.starts_with('-'))
        .collect();
    let language = lang(h.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.'));
    if language == Some("Python")
        && let Some(m) = rest[..rest.len().saturating_sub(1)]
            .iter()
            .position(|w| w == "-m")
    {
        let module = &rest[m + 1..];
        let pip = module.first().is_some_and(|x| x == "pip")
            && module.get(1).is_none_or(|x| x == "install");
        return if pip {
            wide(
                "installs any Python package (its setup code runs)",
                acts(&["install", "code"]),
            )
        } else {
            narrow()
        };
    }
    if let Some(language) = language {
        let open = if h == "deno" || h == "bun" {
            args.is_empty()
                || ["x", "eval", "exec", "repl", "dlx", "create"].contains(&args[0])
                || (["run", "task"].contains(&args[0]) && args.len() < 2)
        } else {
            interp(language, &rest, fixed)
        };
        return if open {
            wide(&format!("any {language} code"), acts(&["code"]))
        } else {
            narrow()
        };
    }
    if ["npm", "pnpm", "yarn"].contains(&h) {
        return match sub {
            "" => wide(
                &format!("every {h} command, publish included"),
                acts(&["install", "publish", "code"]),
            ),
            "exec" | "x" | "dlx" | "init" | "create" => {
                wide("downloads and runs any package", acts(&["install", "code"]))
            }
            "run" | "run-script" if args.len() < 2 => {
                wide("any script in package.json", acts(&["code"]))
            }
            "install" | "i" | "add" => wide(
                "installs any package (its install scripts run)",
                acts(&["install", "code"]),
            ),
            "publish" => wide("publishes packages", acts(&["publish"])),
            _ => narrow(),
        };
    }
    if ["pip", "pip3"].contains(&h)
        || (h == "uv" && ["pip", "add"].contains(&sub))
        || (h == "pipx" && sub == "install")
    {
        let sub = if h == "uv" && sub == "pip" {
            rest.iter()
                .skip(1)
                .find(|w| !w.starts_with('-'))
                .map(String::as_str)
                .unwrap_or_default()
        } else {
            sub
        };
        return if ["", "install", "add"].contains(&sub) {
            wide(
                "installs any Python package (its setup code runs)",
                acts(&["install", "code"]),
            )
        } else {
            narrow()
        };
    }
    if ["npx", "bunx", "pnpx", "uvx", "pipx"].contains(&h) {
        if args.is_empty() || (h == "pipx" && args[0] == "run" && args.len() < 2) {
            return wide("downloads and runs any package", acts(&["install", "code"]));
        }
        return if lang(args[0]).is_some() {
            bash(Some(&format!("{} *", args.join(" "))))
        } else {
            narrow()
        };
    }
    if ["brew", "gem", "cargo", "go"].contains(&h) {
        return if ["", "install", "run"].contains(&sub) {
            wide(
                &format!("installs or runs anything through {h}"),
                acts(&["install", "code"]),
            )
        } else {
            narrow()
        };
    }
    if h == "make" || h == "just" {
        return if sub.is_empty() {
            wide("any target in the build file", acts(&["code"]))
        } else {
            narrow()
        };
    }
    if ["sudo", "su", "doas"].contains(&h) {
        return wide("any command as the administrator", all());
    }
    if ["eval", "exec", "sqlite3"].contains(&h) {
        return wide("any command", all());
    }
    if NET.contains(&h) {
        return wide("any address on the internet", acts(&["send"]));
    }
    if CLOUD.contains(&h) {
        let first3: Vec<&str> = args.iter().take(3).copied().collect();
        if first3.iter().any(|w| READ_VERBS.contains(w))
            && !first3.iter().any(|w| RISKY_VERBS.contains(w))
        {
            return narrow();
        }
        let mut label = vec![h];
        for w in args.iter().copied() {
            if !CLOUD_WORDS.contains(w) || label.len() == 3 {
                break;
            }
            label.push(w);
        }
        let label = label.join(" ");
        if CHAIN.contains(&h) {
            return if args.is_empty() || args.iter().any(|w| CHAIN_VERBS.contains(w)) {
                wide(&format!("any {label} command"), acts(&["spend"]))
            } else {
                narrow()
            };
        }
        let what = if ["docker", "podman", "kubectl", "helm"].contains(&h) {
            acts(&["code"])
        } else {
            acts(&["publish", "delete"])
        };
        return wide(&format!("any {label} command"), what);
    }
    if DELETERS.contains(&h) {
        return wide("deletes any file", acts(&["delete"]));
    }
    if WRITERS.contains(&h) {
        return wide(&format!("{h} on any file"), acts(&["delete"]));
    }
    if h == "security" {
        let keys = [
            "",
            "find-generic-password",
            "find-internet-password",
            "dump-keychain",
            "export",
        ];
        return if keys.contains(&sub) {
            wide("reads passwords from the keychain", acts(&["secrets"]))
        } else {
            narrow()
        };
    }
    if VAULTS.contains(&h) {
        return wide(
            "reads passwords from a password manager",
            acts(&["secrets"]),
        );
    }
    if h == "git" {
        if config || GIT_RUN.contains(&sub) {
            return wide("can make git run any program", acts(&["code"]));
        }
        if sub.is_empty() {
            return wide(
                "every git command, force push and hard reset included",
                acts(&["push", "commit", "discard", "delete", "code"]),
            );
        }
        if ["stash", "worktree", "reflog"].contains(&sub)
            && after.first().is_some_and(|x| x == "list" || x == "show")
        {
            return narrow();
        }
        if sub == "push" {
            return wide("pushes to any remote, force push included", acts(&["push"]));
        }
        if GIT_LOSE.contains(&sub) {
            return wide(
                &format!("git {sub} can throw away work that is not committed"),
                acts(&["discard"]),
            );
        }
        if GIT_DELETE.contains(&sub) {
            return wide(
                &format!("git {sub} can delete files, branches or history"),
                acts(&["delete"]),
            );
        }
        if sub == "send-email" {
            return wide("sends email", acts(&["send"]));
        }
    }
    (None, acts_of(h, sub))
}

/// What an MCP tool's name says it does.
pub fn name_acts(name: &str) -> Acts {
    let mut snake = String::new();
    let mut prev: Option<char> = None;
    for c in name.chars() {
        if prev.is_some_and(|p| p.is_ascii_lowercase()) && c.is_ascii_uppercase() {
            snake.push('_');
        }
        snake.push(c);
        prev = Some(c);
    }
    let snake = snake.to_lowercase();
    let words: Vec<&str> = snake
        .split(|c: char| !c.is_ascii_alphanumeric())
        .flat_map(split_digits)
        .filter(|w| !w.is_empty())
        .collect();
    let (Some(first), Some(last)) = (words.first(), words.last()) else {
        return Acts::new();
    };
    let read_first = [
        "get", "list", "show", "search", "read", "fetch", "find", "describe", "check", "view",
        "query", "lookup", "preview", "inspect", "estimate",
    ];
    let read_last = [
        "status", "list", "preview", "info", "quote", "estimate", "history", "logs", "state",
        "config", "options", "presets",
    ];
    if read_first.contains(first) || read_last.contains(last) {
        return Acts::new();
    }
    let joined = words.join("_");
    let names: [(&'static str, &[&str]); 5] = [
        (
            "send",
            &[
                "send", "email", "mail", "reply", "forward", "tweet", "post", "upload", "share",
                "dm",
            ],
        ),
        (
            "delete",
            &[
                "delete", "trash", "wipe", "erase", "destroy", "purge", "drop", "unlink", "remove",
            ],
        ),
        ("publish", &["deploy", "publish", "release", "unpublish"]),
        (
            "spend",
            &[
                "buy", "sell", "pay", "purchase", "order", "withdraw", "deposit", "mint", "stake",
                "rent", "checkout", "transfer", "trade", "swap", "approve",
            ],
        ),
        (
            "code",
            &[
                "exec",
                "execute",
                "eval",
                "evaluate",
                "shell",
                "run_python",
                "run_code",
                "run_script",
                "sandbox",
            ],
        ),
    ];
    let mut out: Acts = names
        .iter()
        .filter(|(_, words)| words.iter().any(|w| named(&joined, w)))
        .map(|(k, _)| *k)
        .collect();
    if ["replay", "paper", "backtest", "sim", "simulate"]
        .iter()
        .any(|w| bounded(&joined, w).next().is_some())
    {
        out.remove("spend");
    }
    out
}

/// Splits `abc12` into `abc` and `12`, as `[a-z]+|\d+` would.
fn split_digits(w: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let b = w.as_bytes();
    for i in 1..=b.len() {
        if i == b.len() || b[i].is_ascii_digit() != b[i - 1].is_ascii_digit() {
            out.push(&w[start..i]);
            start = i;
        }
    }
    out
}

/// Ends of each place `word` stands in `joined` with no letter on either side.
fn bounded<'a>(joined: &'a str, word: &'a str) -> impl Iterator<Item = usize> + 'a {
    joined.match_indices(word).filter_map(move |(at, _)| {
        let end = at + word.len();
        let letter = |c: Option<&u8>| c.is_some_and(u8::is_ascii_lowercase);
        let b = joined.as_bytes();
        (!letter(at.checked_sub(1).and_then(|p| b.get(p))) && !letter(b.get(end))).then_some(end)
    })
}

fn named(joined: &str, word: &str) -> bool {
    bounded(joined, word).any(|end| {
        let rest = &joined[end..];
        match word {
            "remove" => !["_background", "_bg", "_watermark", "_noise"]
                .iter()
                .any(|x| rest.starts_with(x)),
            "swap" => {
                rest.starts_with('_')
                    && ["token", "coin", "eth", "sol", "usd"]
                        .iter()
                        .any(|x| rest[1..].contains(x))
            }
            "approve" => {
                rest.starts_with('_')
                    && ["token", "allowance", "spend"]
                        .iter()
                        .any(|x| rest[1..].contains(x))
            }
            _ => true,
        }
    })
}

const SENSITIVE: [&str; 16] = [
    ".ssh",
    ".aws",
    ".gnupg",
    ".config",
    ".docker",
    ".kube",
    ".claude.json",
    ".npmrc",
    ".netrc",
    ".git-credentials",
    ".pypirc",
    ".zsh_history",
    ".bash_history",
    "Library/Keychains",
    "Library/Application Support",
    "Library/Cookies",
];

static SECRET_FILE: LazyLock<Regex> = LazyLock::new(|| {
    rx(concat!(
        r"(^|/)(\.env(\.[\w.-]+)?|id_[a-z0-9]+|[\w.-]*\.(pem|key|p12|pfx)|credentials(\.json)?|\.npmrc|\.netrc|",
        r"\.pypirc|\.git-credentials|\.claude\.json)$"
    ))
});
static KEY_EXT: LazyLock<Regex> = LazyLock::new(|| rx(r"\.(pem|key|p12|pfx)$"));
static EXT_TAIL: LazyLock<Regex> = LazyLock::new(|| rx(r"^(?:\*\*/)?\*\.(\w+)$"));
static HOME_DIR: LazyLock<Regex> = LazyLock::new(|| rx(r"^/(?:Users|home)/[^/]+"));

/// `os.path.normpath` for absolute paths.
fn normpath(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for p in path.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    format!("/{}", parts.join("/"))
}

/// The home folder `real` lies in: the given one, or any `/Users/<name>` or `/home/<name>`.
fn home_of<'a>(real: &'a str, home: &'a str) -> Option<&'a str> {
    if real == home || real.starts_with(&format!("{home}/")) {
        return Some(home);
    }
    HOME_DIR.find(real).map(|m| m.as_str())
}

fn path_rule(tool: &str, arg: Option<&str>, home: &str, base: &str) -> Why {
    let (verb, a) = if tool == "Read" {
        ("reads", acts(&["secrets"]))
    } else {
        ("changes", acts(&["delete"]))
    };
    if tool == "Write" && !matches!(arg, None | Some("" | "*")) {
        return narrow();
    }
    let arg = match arg {
        Some(x) if !["*", "**"].contains(&x.trim()) && !x.is_empty() => x,
        _ => return wide(&format!("{verb} any file"), a),
    };
    let trimmed = arg.trim_end_matches('/');
    if SECRET_FILE.is_match(trimmed) {
        let name = basename(trimmed);
        let kind = KEY_EXT
            .captures(name)
            .map_or_else(|| name.to_string(), |c| format!(".{}", &c[1]));
        return wide(&format!("{verb} {kind} files"), acts(&["secrets"]));
    }
    let fixed = arg.split(['*', '?', '[']).next().unwrap_or_default();
    let tail = arg[fixed.len()..].trim_matches('/');
    let what = if ["", "**", "**/*", "*"].contains(&tail) {
        "every file".to_string()
    } else if let Some(c) = EXT_TAIL.captures(tail) {
        format!("every .{} file", &c[1])
    } else {
        "matching files".to_string()
    };
    let raw = if arg.starts_with("//") {
        format!("/{}", fixed.trim_start_matches('/'))
    } else if arg.starts_with('~') {
        format!("{home}{}", &fixed[1..])
    } else {
        format!("{base}/{}", fixed.trim_start_matches('/'))
    };
    let real = normpath(&raw);
    let parent = normpath(&format!("{home}/.."));
    let top = real.trim_start_matches('/');
    if ["", "Users", "home"].contains(&top) || real == parent {
        return wide(&format!("{verb} {what} on the disk"), a);
    }
    let Some(home) = home_of(&real, home) else {
        return narrow();
    };
    if real == home {
        return wide(&format!("{verb} {what} in your home folder"), a);
    }
    for s in SENSITIVE {
        let p = format!("{home}/{s}");
        if real == p || real.starts_with(&format!("{p}/")) || p.starts_with(&format!("{real}/")) {
            return wide(
                &format!("{verb} {}", s.replace("Library/", "")),
                acts(&["secrets"]),
            );
        }
    }
    narrow()
}

/// Why an allow rule is too wide (or `None`), and the actions it lets run. `base` is where `/x` and `x` start.
pub fn breadth(rule: &str, home: &str, base: &str) -> Why {
    let (tool, arg) = parse(rule);
    let arg = arg.as_deref();
    match tool.as_str() {
        "Bash" => bash(arg),
        "Read" | "Edit" | "Write" => path_rule(&tool, arg, home, base),
        "WebFetch" if ["", "domain:*", "*"].contains(&arg.unwrap_or_default().trim()) => {
            wide("any website", acts(&["send"]))
        }
        t if t.starts_with("mcp__") => {
            let parts: Vec<&str> = t.split("__").collect();
            if arg.is_some() || parts.len() < 2 || parts[1].is_empty() || parts[1].contains('*') {
                return narrow();
            }
            let name = parts[2..].join("__");
            if name.is_empty() || name == "*" {
                return wide(&format!("every tool of {}", parts[1]), acts(&["*"]));
            }
            if name.contains('*') {
                return narrow();
            }
            let a = name_acts(&name);
            let why = a
                .contains("code")
                .then(|| format!("any code, through {name}"));
            (why, a)
        }
        _ => narrow(),
    }
}

/// A rule as the page may show it: the program and a known subcommand, never arguments, paths or keys.
pub fn shown_rule(rule: &str, home: &str, base: &str) -> String {
    let (tool, arg) = parse(rule);
    match tool.as_str() {
        "Bash" => {
            let Some(arg) = arg else {
                return "Bash".into();
            };
            let mut a = arg.trim().to_string();
            if ["*", ":*", ""].contains(&a.as_str()) {
                return "Bash(*)".into();
            }
            if let Some(head) = a.strip_suffix(":*") {
                a = format!("{head} *");
            }
            let fixed = a.split('*').next().unwrap_or_default();
            let attached =
                a.contains('*') && !fixed.is_empty() && !fixed.ends_with(char::is_whitespace);
            let words: Vec<String> = split_words(fixed)
                .into_iter()
                .filter(|w| !ASSIGN.is_match(w))
                .collect();
            let head = words.first().map_or("", |w| basename(w));
            let plain = |h: &str| {
                let mut c = h.chars();
                c.next().is_some_and(|f| f.is_ascii_lowercase())
                    && h.len() <= 16
                    && c.all(|x| x.is_ascii_lowercase() || x.is_ascii_digit() || ".+-".contains(x))
                    && !h
                        .as_bytes()
                        .windows(3)
                        .any(|w| w.iter().all(u8::is_ascii_digit))
            };
            let safe = HEADS.contains(&head) || plain(head);
            let mut rest: &[String] = words.get(1..).unwrap_or_default();
            if head == "git" {
                let mut k = 0;
                while k < rest.len() && rest[k].starts_with('-') {
                    k += if rest[k] == "-C" || rest[k] == "-c" {
                        2
                    } else {
                        1
                    };
                }
                rest = rest.get(k..).unwrap_or_default();
            }
            let sub = match rest.first() {
                Some(r)
                    if safe
                        && head != "echo"
                        && head != "printf"
                        && (SAFE_SUB.contains(r.as_str())
                            || (attached
                                && rest.len() == 1
                                && subs(head).iter().any(|x| x.starts_with(r.as_str())))) =>
                {
                    r.as_str()
                }
                _ => "",
            };
            let body = [if safe { head } else { "a program" }, sub]
                .into_iter()
                .filter(|x| !x.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            let star = if !a.contains('*') {
                ""
            } else if attached
                && safe
                && (words.len() == 1 || (!sub.is_empty() && words.last().is_some_and(|w| w == sub)))
            {
                "*"
            } else {
                " *"
            };
            format!("Bash({body}{star})")
        }
        "Read" | "Edit" | "Write" => {
            let Some(arg) = arg.filter(|a| !a.is_empty()) else {
                return tool;
            };
            let why = path_rule(&tool, Some(&arg), home, base)
                .0
                .unwrap_or_default();
            let place = if why.ends_with("on the disk") {
                "whole disk".to_string()
            } else if why.ends_with("home folder") {
                "home folder".to_string()
            } else if why.is_empty() {
                "one folder".to_string()
            } else {
                why.trim_start_matches("reads ")
                    .trim_start_matches("changes ")
                    .to_string()
            };
            format!("{tool}({place})")
        }
        "WebFetch" => match arg.as_deref().map(str::trim) {
            None | Some("") => "WebFetch".into(),
            Some("domain:*" | "*") => "WebFetch(domain:*)".into(),
            Some(_) => "WebFetch(one site)".into(),
        },
        _ => tool,
    }
}

fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Number(n)) => n.as_f64() != Some(0.0),
        Some(_) => true,
    }
}

/// Settings that loosen every rule at once.
pub fn wide_settings(
    mode: Option<&str>,
    top: &Map<String, Value>,
    editor: &Map<String, Value>,
) -> Vec<String> {
    let mut out = Vec::new();
    if mode == Some("bypassPermissions") {
        out.push("bypass mode: every tool runs with no one asking".to_string());
    }
    if truthy(top.get("skipDangerousModePermissionPrompt")) {
        out.push("the warning before bypass mode is switched off".to_string());
    }
    if truthy(top.get("enableAllProjectMcpServers")) {
        out.push("any project's MCP servers start without asking".to_string());
    }
    if truthy(editor.get("claudeCode.allowDangerouslySkipPermissions")) {
        out.push("the editor offers bypass mode".to_string());
    }
    if editor
        .get("claudeCode.initialPermissionMode")
        .and_then(Value::as_str)
        == Some("bypassPermissions")
    {
        out.push("the editor starts every session in bypass mode".to_string());
    }
    out
}

static FRONT: LazyLock<Regex> = LazyLock::new(|| rx(r"(?s)\A---\n(.*?)\n---"));
static GRANTS: LazyLock<Regex> =
    LazyLock::new(|| rx(r"(?m)^allowed-tools:[ \t]*(.*(?:\n[ \t]+-.*)*)"));
static GRANT: LazyLock<Regex> = LazyLock::new(|| rx(r"[\w*-]+(?:\([^)]*\))?"));

/// Wide tools a skill pre-approves for itself through `allowed-tools`: they run without asking while it is in use.
/// Plain Read, Edit and Write are left out: most skills need them and protected files still ask.
pub fn skill_grants(skill_md: &str, home: &str, base: &str) -> Vec<(String, String)> {
    let Some(front) = FRONT.captures(skill_md) else {
        return Vec::new();
    };
    let Some(list) = GRANTS.captures(&front[1]) else {
        return Vec::new();
    };
    GRANT
        .find_iter(&list[1])
        .map(|m| m.as_str().to_string())
        .filter(|r| !["Read", "Edit", "Write"].contains(&parse(r).0.as_str()))
        .filter_map(|r| breadth(&r, home, base).0.map(|why| (r, why)))
        .collect()
}

/// `fnmatch.fnmatchcase`: `*`, `?` and `[...]` (with `!` for not).
pub fn glob(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    fn class(p: &[char], at: usize, c: char) -> Option<(bool, usize)> {
        let mut i = at + 1;
        let negate = p.get(i) == Some(&'!');
        if negate {
            i += 1;
        }
        let start = i;
        let mut hit = false;
        while i < p.len() && (p[i] != ']' || i == start) {
            if p.get(i + 1) == Some(&'-') && p.get(i + 2).is_some_and(|e| *e != ']') {
                hit |= (p[i]..=p[i + 2]).contains(&c);
                i += 3;
            } else {
                hit |= p[i] == c;
                i += 1;
            }
        }
        (i < p.len()).then_some((hit != negate, i + 1))
    }
    fn go(p: &[char], n: &[char]) -> bool {
        match p.first() {
            None => n.is_empty(),
            Some('*') => (0..=n.len()).any(|k| go(&p[1..], &n[k..])),
            Some('?') => !n.is_empty() && go(&p[1..], &n[1..]),
            Some('[') => match (n.first(), class(p, 0, n.first().copied().unwrap_or('\0'))) {
                (Some(_), Some((true, next))) => go(&p[next..], &n[1..]),
                (_, None) => n.first() == Some(&'[') && go(&p[1..], &n[1..]),
                _ => false,
            },
            Some(c) => n.first() == Some(c) && go(&p[1..], &n[1..]),
        }
    }
    go(&p, &n)
}

/// Does an allow rule cover this MCP tool? Brackets, and globs before `mcp__<server>__`, approve nothing.
pub fn mcp_match(rule: &str, name: &str) -> bool {
    let parts: Vec<&str> = rule.split("__").collect();
    if rule.contains('(')
        || parts[0] != "mcp"
        || parts.len() < 2
        || parts[1].is_empty()
        || parts[1].contains('*')
    {
        return false;
    }
    if parts.len() == 2 {
        return name.starts_with(&format!("{rule}__"));
    }
    rule == name || (rule.contains('*') && glob(rule, name))
}

/// Does a deny or ask rule cover this tool? Those accept any tool-name glob, `*` and `mcp__*` included.
pub fn guard_match(rule: &str, name: &str) -> bool {
    !rule.contains('(')
        && (rule == name
            || (rule.matches("__").count() == 1 && name.starts_with(&format!("{rule}__")))
            || (rule.contains('*') && glob(rule, name)))
}

/// Tools that act and run with no one asking: an allow rule names them or their server, or bypass mode is on, and no
/// deny or ask rule stops them. Returns (full name, what frees it).
pub fn unguarded(
    tools: &[(String, bool)],
    allow: &[String],
    mode: Option<&str>,
    guards: &[String],
) -> Vec<(String, String)> {
    let bypass = mode == Some("bypassPermissions");
    tools
        .iter()
        .filter(|(n, acting)| *acting && !guards.iter().any(|g| guard_match(g, n)))
        .filter_map(|(n, _)| {
            let by = if bypass {
                Some("bypass mode".to_string())
            } else {
                allow.iter().find(|r| mcp_match(r, n)).cloned()
            };
            by.map(|b| (n.clone(), b))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: &str = "/Users/test";
    const BASE: &str = "/Users/test/Documents/app";

    fn why(rule: &str) -> Option<String> {
        breadth(rule, H, BASE).0
    }

    fn acts_for(rule: &str) -> Acts {
        breadth(rule, H, BASE).1
    }

    #[test]
    fn wide_rules_say_what_they_let_run() {
        let wide = [
            ("Bash", "any shell command"),
            ("Bash(*)", "any shell command"),
            ("Bash(* --version)", "any program at all"),
            ("Bash(python3:*)", "any Python code"),
            ("Bash(python3 -I -c \":*)", "any Python code"),
            ("Bash(python3 - <<:*)", "any Python code"),
            ("Bash(python3 -c \"import json*)", "any Python code"),
            ("Bash(node -e \"console.log*)", "any JavaScript code"),
            ("Bash(bash -lc \":*)", "any shell code"),
            ("Bash(python3 <<:*)", "any Python code"),
            ("Bash(python3 -W ignore *)", "any Python code"),
            ("Bash(node -e *)", "any JavaScript code"),
            ("Bash(osascript:*)", "any AppleScript code"),
            ("Bash(FOO=1 python3:*)", "any Python code"),
            ("Bash(php -r *)", "any PHP code"),
            ("Bash(awk *)", "any awk code"),
            ("Bash(tsx *)", "any TypeScript code"),
            ("Bash(deno run *)", "any JavaScript code"),
            ("Bash(bun x *)", "any JavaScript code"),
            ("Bash(bun run *)", "any JavaScript code"),
            (
                "Bash(git:*)",
                "every git command, force push and hard reset included",
            ),
            (
                "Bash(git push:*)",
                "pushes to any remote, force push included",
            ),
            (
                "Bash(git -C . push *)",
                "pushes to any remote, force push included",
            ),
            (
                "Bash(git -c core.pager=x log *)",
                "can make git run any program",
            ),
            (
                "Bash(git checkout:*)",
                "git checkout can throw away work that is not committed",
            ),
            (
                "Bash(git switch *)",
                "git switch can throw away work that is not committed",
            ),
            (
                "Bash(git rm *)",
                "git rm can delete files, branches or history",
            ),
            (
                "Bash(git worktree *)",
                "git worktree can delete files, branches or history",
            ),
            ("Bash(git submodule *)", "can make git run any program"),
            ("Bash(git send-email *)", "sends email"),
            (
                "Bash(git p*)",
                "pushes to any remote, force push included (git p* covers push)",
            ),
            ("Bash(npm run *)", "any script in package.json"),
            ("Bash(npm:*)", "every npm command, publish included"),
            (
                "Bash(npm r*)",
                "any script in package.json (npm r* covers run)",
            ),
            ("Bash(npx:*)", "downloads and runs any package"),
            ("Bash(npx tsx *)", "any TypeScript code"),
            ("Bash(uvx *)", "downloads and runs any package"),
            (
                "Bash(pip install:*)",
                "installs any Python package (its setup code runs)",
            ),
            (
                "Bash(python3 -m pip install *)",
                "installs any Python package (its setup code runs)",
            ),
            (
                "Bash(uv add *)",
                "installs any Python package (its setup code runs)",
            ),
            ("Bash(uv run *)", "any command"),
            ("Bash(poetry run *)", "any command"),
            ("Bash(direnv exec *)", "any command"),
            ("Bash(devbox run *)", "any command"),
            ("Bash(uv run python -c:*)", "any Python code"),
            ("Bash(env FOO=1 *)", "any command"),
            ("Bash(env *)", "any command"),
            ("Bash(sqlite3 *)", "any command"),
            ("Bash(timeout 30 node:*)", "any JavaScript code"),
            ("Bash(sudo:*)", "any command as the administrator"),
            ("Bash(py*)", "any Python code (py* covers python)"),
            (
                "Bash(n*)",
                "every npm command, publish included (n* covers npm)",
            ),
            ("Bash(c*)", "any command (c* covers caffeinate)"),
            ("Bash(curl:*)", "any address on the internet"),
            (
                "Bash(curl https://api.io/v1 *)",
                "any address on the internet",
            ),
            ("Bash(rm *)", "deletes any file"),
            ("Bash(rm -rf /tmp/build *)", "deletes any file"),
            (
                "Bash(security find-generic-password:*)",
                "reads passwords from the keychain",
            ),
            ("Bash(gh:*)", "any gh command"),
            ("Bash(gh repo *)", "any gh repo command"),
            ("Bash(gh pr *)", "any gh pr command"),
            ("Bash(aws s3 *)", "any aws s3 command"),
            ("Bash(gcloud run deploy *)", "any gcloud run deploy command"),
            ("Bash(wrangler deploy *)", "any wrangler deploy command"),
            ("Bash(kubectl exec *)", "any kubectl exec command"),
            ("Bash(docker compose *)", "any docker compose command"),
            ("Bash(cast send:*)", "any cast send command"),
            ("Bash(cast wallet *)", "any cast wallet command"),
            ("Bash(forge create *)", "any forge create command"),
            (
                "Bash(cast send --private-key 0xFAKEKEY1111 *)",
                "any cast send command",
            ),
            (
                "Bash(docker run -e API_KEY=sk-FAKE999 *)",
                "any docker run command",
            ),
            (
                "Bash(gh api repos/alice/secret-proj/issues *)",
                "any gh api command",
            ),
            ("Bash(uv run python -c \"import json*)", "any Python code"),
            ("Bash(env python3 -c \"print*)", "any Python code"),
            ("Bash(env -i *)", "any command"),
            ("Bash(caffeinate -i *)", "any command"),
            ("Bash(xargs -n 1 *)", "any command"),
            ("Bash(xargs -I {} *)", "any command"),
            ("Bash(sh*)", "any shell code"),
            ("Bash(go*)", "installs or runs anything through go"),
            ("Bash(uv tool run *)", "downloads and runs any package"),
            (
                "Bash(pipx install *)",
                "installs any Python package (its setup code runs)",
            ),
            ("Bash(npm init *)", "downloads and runs any package"),
            ("Bash(deno task *)", "any JavaScript code"),
            ("Read(//**)", "reads every file on the disk"),
            ("Read(~/**)", "reads every file in your home folder"),
            ("Edit(~/**)", "changes every file in your home folder"),
            ("Read(//**/*.md)", "reads every .md file on the disk"),
            ("Read(~/.ssh/**)", "reads .ssh"),
            ("Read(//Users/test/Library/**)", "reads Keychains"),
            ("Read(~/.claude.json)", "reads .claude.json files"),
            ("Read(.env)", "reads .env files"),
            ("Read(~/.npmrc)", "reads .npmrc files"),
            ("Edit", "changes any file"),
            ("Write", "changes any file"),
            ("WebFetch", "any website"),
            ("WebFetch(domain:*)", "any website"),
            ("mcp__gmail", "every tool of gmail"),
            ("mcp__gmail__*", "every tool of gmail"),
            (
                "mcp__blender__execute_blender_code",
                "any code, through execute_blender_code",
            ),
        ];
        for (rule, want) in wide {
            let got = why(rule);
            assert_eq!(got.as_deref(), Some(want), "{rule}");
            let label = got.unwrap_or_default();
            assert!(
                !["FAKE", "alice", "0x", "sk-"]
                    .iter()
                    .any(|x| label.contains(x)),
                "{rule}"
            );
        }
    }

    #[test]
    fn narrow_rules_pass() {
        let narrow = [
            "Bash(npm run build)",
            "Bash(npm run build:*)",
            "Bash(npm run test *)",
            "Bash(python3 scan.py:*)",
            "Bash(python3 -m http.server *)",
            "Bash(python3 -c \"print(1)\" *)",
            "Bash(git log:*)",
            "Bash(git log -c *)",
            "Bash(git status)",
            "Bash(curl https://api.example.com/v1/status)",
            "Bash(npx tsc:*)",
            "Bash(pip show:*)",
            "Bash(security cms:*)",
            "Bash(git commit:*)",
            "Bash(command -v:*)",
            "Bash(gh pr view *)",
            "Bash(aws s3 ls *)",
            "Bash(kubectl get *)",
            "Bash(docker ps *)",
            "Bash(cast call *)",
            "Bash(forge build *)",
            "Bash(uv run pytest *)",
            "Bash(bun run build *)",
            "Read(//tmp/**)",
            "Read(src/**)",
            "Edit(./docs/**)",
            "Read(/notes/**)",
            "Write(~/**)",
            "Write(//**)",
            "Bash(python3 -I scan.py:*)",
            "Bash(python3 -x tool.py *)",
            "Bash(python3 -v x.py *)",
            "Bash(bash -x build.sh *)",
            "Bash(gh pr view 123 *)",
            "Bash(gh issue list --repo alice/private-repo *)",
            "Bash(kubectl get pods -n prod *)",
            "Bash(git stash list *)",
            "Bash(git worktree list *)",
            "WebFetch(domain:docs.python.org)",
            "mcp__gmail__search_threads",
            "mcp__blender__get_*",
            "mcp__*",
            "mcp__*__send",
            "mcp__gmail(send)",
            "Skill(foo)",
            "Agent(Explore)",
            "WebSearch",
        ];
        for rule in narrow {
            assert_eq!(why(rule), None, "{rule}");
        }
    }

    #[test]
    fn a_narrow_rule_still_says_what_it_lets_run() {
        assert_eq!(acts_for("Bash(git push origin main)"), acts(&["push"]));
        assert_eq!(acts_for("Bash(git commit:*)"), acts(&["commit"]));
        assert_eq!(acts_for("Bash(rm -rf build)"), acts(&["delete"]));
        assert_eq!(acts_for("Bash(git checkout:*)"), acts(&["discard"]));
        assert_eq!(acts_for("Bash(npm publish)"), acts(&["publish"]));
        assert_eq!(acts_for("Bash(cast send 0x1 1)"), acts(&["spend"]));
        assert_eq!(acts_for("mcp__gmail__send_message"), acts(&["send"]));
        assert!(acts_for("mcp__blender__execute_blender_code").contains("code"));
        for name in [
            "mcp__tv__tv_launch",
            "mcp__tv__replay_trade",
            "mcp__x__swap_face",
            "mcp__x__remove_background",
            "mcp__x__tiktok_publish_status",
            "mcp__x__get_order",
        ] {
            assert_eq!(acts_for(name), Acts::new(), "{name}");
        }
        for (name, want) in [
            ("mcp__x__place_order", "spend"),
            ("mcp__x__withdraw", "spend"),
            ("mcp__x__approve_token", "spend"),
            ("mcp__x__mint_nft", "spend"),
            ("mcp__x__media_upload", "send"),
            ("mcp__x__delete_project", "delete"),
            ("mcp__x__gpu_rent", "spend"),
        ] {
            assert!(acts_for(name).contains(want), "{name}");
        }
        assert_eq!(name_acts("sendMessage2"), acts(&["send"]));
    }

    #[test]
    fn the_page_never_shows_a_rules_arguments() {
        let cases = [
            (
                "Bash(curl -H 'Authorization: Bearer sk-abc123' https://x.io:*)",
                "Bash(curl *)",
            ),
            ("Bash(git push:*)", "Bash(git push *)"),
            ("Bash(FOO=bar python3 x.py)", "Bash(python3)"),
            ("Bash(echo ghp_FAKEtoken123)", "Bash(echo)"),
            ("Bash(mytool sk_live_FAKE51H *)", "Bash(mytool *)"),
            ("Bash(FOO=\"a b\" python3)", "Bash(python3)"),
            ("Bash(ghp_FAKEtoken12345 run)", "Bash(a program)"),
            ("Bash", "Bash"),
            ("mcp__gmail", "mcp__gmail"),
            ("Read(//**)", "Read(whole disk)"),
            ("Read(//Users/alice/secret-project/**)", "Read(one folder)"),
            ("Read(~/.ssh/**)", "Read(.ssh)"),
            ("Read(~/.ssh/id_rsa)", "Read(id_rsa files)"),
            ("WebFetch(domain:internal.corp.io)", "WebFetch(one site)"),
            ("Read(//Users/test/**)", "Read(home folder)"),
            ("Bash(git -C /Users/alice/x push *)", "Bash(git push *)"),
            ("Bash(git c*)", "Bash(git c*)"),
            ("Bash(sh*)", "Bash(sh*)"),
            ("Read(~/.ssh/alice-prod.pem)", "Read(.pem files)"),
            ("Bash(python3 -c \"import json*)", "Bash(python3 -c *)"),
        ];
        for (rule, want) in cases {
            let got = shown_rule(rule, H, ".");
            assert_eq!(got, want, "{rule}");
            assert!(
                !["sk-abc", "FAKE", "alice"].iter().any(|x| got.contains(x)),
                "{rule}"
            );
        }
    }

    #[test]
    fn settings_that_loosen_everything_are_named() {
        let none = Map::new();
        let map = |v: Value| v.as_object().cloned().unwrap_or_default();
        assert_eq!(
            wide_settings(Some("bypassPermissions"), &none, &none),
            ["bypass mode: every tool runs with no one asking"]
        );
        assert_eq!(
            wide_settings(
                None,
                &map(serde_json::json!({"enableAllProjectMcpServers": true})),
                &none
            ),
            ["any project's MCP servers start without asking"]
        );
        assert_eq!(
            wide_settings(
                None,
                &none,
                &map(serde_json::json!({"claudeCode.initialPermissionMode": "bypassPermissions"}))
            ),
            ["the editor starts every session in bypass mode"]
        );
        assert!(wide_settings(Some("acceptEdits"), &none, &none).is_empty());
    }

    #[test]
    fn a_skill_can_grant_itself_wide_tools() {
        let md = "---\nname: s\nallowed-tools: Read, Write, Bash, WebFetch, Bash(npm:*), Bash(git log:*), mcp__gmail__*\n---\nbody";
        let rules: Vec<String> = skill_grants(md, H, BASE)
            .into_iter()
            .map(|(r, _)| r)
            .collect();
        assert_eq!(rules, ["Bash", "WebFetch", "Bash(npm:*)", "mcp__gmail__*"]);
        let listed = "---\nname: s\nallowed-tools:\n  - Bash(python3:*)\n  - Read\n---\n";
        let rules: Vec<String> = skill_grants(listed, H, BASE)
            .into_iter()
            .map(|(r, _)| r)
            .collect();
        assert_eq!(rules, ["Bash(python3:*)"]);
        assert!(skill_grants("# no frontmatter\nallowed-tools: Bash", H, BASE).is_empty());
    }

    #[test]
    fn acting_tools_are_matched_like_claude_code() {
        for (rule, name) in [
            ("mcp__gmail", "mcp__gmail__send"),
            ("mcp__gmail__*", "mcp__gmail__send"),
            ("mcp__gmail__send_*", "mcp__gmail__send_message"),
            ("mcp__gmail__send_message", "mcp__gmail__send_message"),
            ("mcp__claude_ai_Gmail", "mcp__claude_ai_Gmail__reply"),
        ] {
            assert!(mcp_match(rule, name), "{rule} {name}");
        }
        for (rule, name) in [
            ("mcp__gmail__send_message", "mcp__gmail__send_draft"),
            ("mcp__gmail(send)", "mcp__gmail__send"),
            ("mcp__*", "mcp__gmail__send"),
            ("mcp__gm", "mcp__gmail__send"),
            ("Bash", "mcp__gmail__send"),
            ("*", "mcp__gmail__send"),
            ("mcp__*__send", "mcp__gmail__send"),
        ] {
            assert!(!mcp_match(rule, name), "{rule} {name}");
        }
        for rule in [
            "*",
            "mcp__*",
            "mcp__gmail",
            "mcp__gmail__*",
            "mcp__gmail__send",
        ] {
            assert!(guard_match(rule, "mcp__gmail__send"), "{rule}");
        }
        assert!(
            !guard_match("mcp__gmail(send)", "mcp__gmail__send")
                && !guard_match("mcp__docs", "mcp__gmail__send")
        );
        let tools: Vec<(String, bool)> = [
            ("mcp__gmail__send_message", true),
            ("mcp__gmail__search_threads", false),
            ("mcp__blender__execute_blender_code", true),
            ("mcp__x__delete", true),
        ]
        .map(|(n, a)| (n.to_string(), a))
        .to_vec();
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(
            unguarded(
                &tools,
                &s(&[
                    "mcp__gmail",
                    "mcp__blender__execute_blender_code",
                    "Bash(git:*)"
                ]),
                None,
                &[]
            ),
            [
                (
                    "mcp__gmail__send_message".to_string(),
                    "mcp__gmail".to_string()
                ),
                (
                    "mcp__blender__execute_blender_code".to_string(),
                    "mcp__blender__execute_blender_code".to_string()
                )
            ]
        );
        assert!(unguarded(&tools, &[], None, &[]).is_empty());
        assert!(unguarded(&tools, &s(&["mcp__gmail__search_threads"]), None, &[]).is_empty());
        assert_eq!(
            unguarded(&tools, &[], Some("bypassPermissions"), &[]).len(),
            3
        );
        assert!(unguarded(&tools, &[], Some("auto"), &[]).is_empty());
        assert!(
            unguarded(
                &tools,
                &s(&["mcp__gmail"]),
                None,
                &s(&["mcp__gmail__send_message"])
            )
            .is_empty()
        );
        assert!(unguarded(&tools, &[], Some("bypassPermissions"), &s(&["mcp__*"])).is_empty());
        assert!(
            unguarded(
                &tools,
                &s(&["mcp__*", "mcp__gmail(send_message)"]),
                None,
                &[]
            )
            .is_empty()
        );
    }

    #[test]
    fn helpers_behave_like_their_python_namesakes() {
        assert_eq!(split_words("a 'b c' \"d\\\"e\""), ["a", "b c", "d\"e"]);
        assert_eq!(split_words("a \"b c"), ["a", "b", "c"]);
        assert_eq!(drop_tokens("uv run python -c x", 2), "python -c x");
        assert_eq!(drop_tokens("uv", 2), "uv");
        assert_eq!(normpath("/a/./b/../c/"), "/a/c");
        assert!(
            glob("mcp__gmail__send_*", "mcp__gmail__send_x")
                && glob("a[!b]c", "axc")
                && !glob("a?c", "ac")
        );
        assert_eq!(
            parse("Bash(git push:*)"),
            ("Bash".into(), Some("git push:*".into()))
        );
        assert_eq!(parse("mcp__x"), ("mcp__x".into(), None));
    }
}
