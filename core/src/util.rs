//! process plumbing, PATH lookup, input validation, tiny curl-based http, NDJSON emit.
use crate::json::{self, J};
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{mpsc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

pub struct Cap {
    pub ok: bool,
    pub code: i32,
    pub out: String,
    pub err: String,
}

// ---------------------------------------------------------------- emit

static OUT: Mutex<()> = Mutex::new(());

/// one JSON object per line on stdout, flushed. if the TUI went away, so do we.
pub fn emit(v: &J) {
    let _g = OUT.lock().unwrap_or_else(|e| e.into_inner());
    let so = std::io::stdout();
    let mut so = so.lock();
    if writeln!(so, "{}", v.dump()).is_err() || so.flush().is_err() {
        std::process::exit(0);
    }
}

// ---------------------------------------------------------------- PATH

static DIRS: OnceLock<Vec<PathBuf>> = OnceLock::new();

fn norm_key(p: &Path) -> String {
    let s = p.to_string_lossy().trim_end_matches(['\\', '/']).to_string();
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s
    }
}

/// Every directory we look for tools in, in priority order:
///   1. this process's PATH
///   2. (windows) the *registry* PATH, machine + user — so tools installed after your terminal
///      opened (stale PATH) are still found
///   3. well-known install dirs: scoop shims, cargo, bun, go, dotnet, vcpkg, npm, pnpm, python…
/// children get the merged list too, so e.g. `cargo install` can find rustc next to it.
pub fn search_dirs() -> &'static Vec<PathBuf> {
    DIRS.get_or_init(|| {
        let mut out: Vec<PathBuf> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut add = |p: PathBuf| {
            if !p.as_os_str().is_empty() && seen.insert(norm_key(&p)) {
                out.push(p);
            }
        };
        if let Some(path) = env::var_os("PATH") {
            for d in env::split_paths(&path) {
                add(d);
            }
        }
        #[cfg(windows)]
        for d in registry_path_dirs() {
            add(d);
        }
        for d in well_known_dirs() {
            if d.is_dir() {
                add(d);
            }
        }
        out
    })
}

pub fn on_process_path(dir: &Path) -> bool {
    let Some(path) = env::var_os("PATH") else { return false };
    let want = norm_key(dir);
    env::split_paths(&path).any(|d| norm_key(&d) == want)
}

/// `Path    REG_EXPAND_SZ    C:\a;%USERPROFILE%\b`  ->  the value part
#[cfg_attr(not(windows), allow(dead_code))]
pub fn parse_reg_value(text: &str, name: &str) -> Option<String> {
    for line in text.lines() {
        let t = line.trim();
        let Some(head) = t.get(..name.len()) else { continue };
        if !head.eq_ignore_ascii_case(name) {
            continue;
        }
        let rest = t.get(name.len()..)?.trim_start();
        for ty in ["REG_EXPAND_SZ", "REG_SZ"] {
            if let Some(v) = rest.strip_prefix(ty) {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

/// expand %NAME% from the environment; unknown names are left as written
#[cfg_attr(not(windows), allow(dead_code))]
pub fn expand_env(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.find('%') {
            Some(j) => {
                let name = &after[..j];
                match env::var(name) {
                    Ok(v) if !name.is_empty() => out.push_str(&v),
                    _ => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[j + 1..];
            }
            None => {
                out.push('%');
                out.push_str(after);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(windows)]
fn registry_path_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for key in [r"HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment", r"HKCU\Environment"] {
        let args: Vec<String> = ["query", key, "/v", "Path"].iter().map(|s| s.to_string()).collect();
        // plain: no PATH merge (we are the thing computing it)
        if let Ok(c) = capture_inner(Path::new("reg"), &args, 10, false) {
            if let Some(v) = parse_reg_value(&c.out, "Path") {
                for d in expand_env(&v).split(';') {
                    let d = d.trim().trim_matches('"');
                    if !d.is_empty() {
                        out.push(PathBuf::from(d));
                    }
                }
            }
        }
    }
    out
}

fn well_known_dirs() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = Vec::new();
    let ev = |n: &str| env::var_os(n).filter(|s| !s.is_empty()).map(PathBuf::from);
    let home = home_dir();
    let mut push = |p: Option<PathBuf>| {
        if let Some(p) = p {
            v.push(p);
        }
    };
    let under = |base: &Option<PathBuf>, rel: &str| base.as_ref().map(|b| b.join(rel));

    // cross-platform-ish
    push(ev("CARGO_HOME").map(|p| p.join("bin")));
    push(under(&home, ".cargo/bin"));
    push(ev("BUN_INSTALL").map(|p| p.join("bin")));
    push(under(&home, ".bun/bin"));
    push(ev("GOBIN"));
    push(ev("GOPATH").map(|p| p.join("bin")));
    push(under(&home, "go/bin"));
    push(under(&home, ".local/bin"));
    push(under(&home, ".dotnet/tools"));
    push(ev("PNPM_HOME"));
    push(ev("VCPKG_ROOT"));
    push(ev("VCPKG_INSTALLATION_ROOT"));
    push(under(&home, "vcpkg"));

    if cfg!(windows) {
        let local = ev("LOCALAPPDATA");
        let roam = ev("APPDATA");
        let pf = ev("ProgramFiles").unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
        let pd = ev("ProgramData").unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
        push(ev("SCOOP").map(|p| p.join("shims")));
        push(under(&home, "scoop/shims"));
        push(ev("SCOOP_GLOBAL").map(|p| p.join("shims")));
        push(Some(pd.join("scoop").join("shims")));
        push(ev("ChocolateyInstall").map(|p| p.join("bin")));
        push(Some(pd.join("chocolatey").join("bin")));
        push(under(&local, "Microsoft/WindowsApps"));
        push(Some(pf.join("Go").join("bin")));
        push(Some(pf.join("dotnet")));
        push(Some(pf.join("nodejs")));
        push(Some(pf.join("Git").join("cmd")));
        push(under(&roam, "npm"));
        push(under(&local, "pnpm"));
        push(Some(PathBuf::from(r"C:\vcpkg")));
        push(Some(PathBuf::from(r"C:\src\vcpkg")));
        // python: per-user installs and `pip --user` script dirs
        for (base, rel) in [(&local, "Programs/Python"), (&roam, "Python")] {
            if let Some(dir) = under(base, rel) {
                if let Ok(rd) = fs::read_dir(&dir) {
                    for e in rd.flatten() {
                        if e.file_name().to_string_lossy().to_lowercase().starts_with("python") {
                            let p = e.path();
                            v.push(p.join("Scripts"));
                            v.push(p);
                        }
                    }
                }
            }
        }
    } else {
        v.push(PathBuf::from("/usr/local/go/bin"));
    }
    v
}

/// find an executable in `search_dirs()`, honouring PATHEXT on Windows (so scoop.cmd / npm.cmd
/// resolve). symlink_metadata on purpose: winget.exe is an app-execution alias that a normal
/// stat can't follow.
pub fn which(name: &str) -> Option<PathBuf> {
    let exts: Vec<String> = if cfg!(windows) {
        env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string())
            .split(';')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_lowercase())
            .collect()
    } else {
        vec![String::new()]
    };
    let exists = |p: &Path| fs::symlink_metadata(p).map(|m| !m.is_dir()).unwrap_or(false);
    for dir in search_dirs() {
        for ext in &exts {
            let cand = dir.join(format!("{}{}", name, ext));
            if exists(&cand) {
                return Some(cand);
            }
        }
    }
    None
}

pub fn home_dir() -> Option<PathBuf> {
    env::var_os("USERPROFILE").or_else(|| env::var_os("HOME")).map(PathBuf::from)
}

// ---------------------------------------------------------------- running things

fn base_cmd(exe: &Path, args: &[String], merge_path: bool) -> Command {
    let mut c = Command::new(exe);
    if merge_path {
        if let Ok(p) = env::join_paths(search_dirs().iter()) {
            c.env("PATH", p);
        }
    }
    c.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("NO_COLOR", "1");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    c
}

/// run to completion with a timeout; stdout+stderr captured.
pub fn capture(exe: &Path, args: &[String], secs: u64) -> Result<Cap, String> {
    capture_inner(exe, args, secs, true)
}

pub fn capture_inner(exe: &Path, args: &[String], secs: u64, merge_path: bool) -> Result<Cap, String> {
    let mut child = base_cmd(exe, args, merge_path)
        .spawn()
        .map_err(|e| format!("{}: {}", exe.display(), e))?;
    let mut so = child.stdout.take().ok_or("no stdout")?;
    let mut se = child.stderr.take().ok_or("no stderr")?;
    let t1 = thread::spawn(move || {
        let mut b = Vec::new();
        let _ = so.read_to_end(&mut b);
        b
    });
    let t2 = thread::spawn(move || {
        let mut b = Vec::new();
        let _ = se.read_to_end(&mut b);
        b
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {
                if start.elapsed() > Duration::from_secs(secs) {
                    let _ = child.kill();
                    let _ = child.wait();
                    // don't join readers: a grandchild may still hold the pipes open.
                    return Err(format!("timed out after {}s", secs));
                }
                thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(e.to_string()),
        }
    };
    let out = t1.join().unwrap_or_default();
    let err = t2.join().unwrap_or_default();
    Ok(Cap {
        ok: status.success(),
        code: status.code().unwrap_or(-1),
        out: String::from_utf8_lossy(&out).into_owned(),
        err: String::from_utf8_lossy(&err).into_owned(),
    })
}

fn pump(mut r: impl Read + Send + 'static, is_err: bool, tx: mpsc::Sender<(bool, String)>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut buf: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 4096];
        let flush = |buf: &mut Vec<u8>| {
            if !buf.is_empty() {
                let s = String::from_utf8_lossy(buf).into_owned();
                buf.clear();
                let _ = tx.send((is_err, s));
            }
        };
        loop {
            match r.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    for &b in &chunk[..n] {
                        // installers redraw progress with \r, so split on both
                        if b == b'\n' || b == b'\r' {
                            flush(&mut buf);
                        } else {
                            buf.push(b);
                        }
                    }
                }
            }
        }
        flush(&mut buf);
    })
}

/// run and call `on_line(is_stderr, line)` as output arrives. returns the exit code.
pub fn stream<F: FnMut(bool, &str)>(exe: &Path, args: &[String], mut on_line: F) -> Result<i32, String> {
    let mut child = base_cmd(exe, args, true)
        .spawn()
        .map_err(|e| format!("{}: {}", exe.display(), e))?;
    let so = child.stdout.take().ok_or("no stdout")?;
    let se = child.stderr.take().ok_or("no stderr")?;
    let (tx, rx) = mpsc::channel();
    let h1 = pump(so, false, tx.clone());
    let h2 = pump(se, true, tx);
    for (is_err, raw) in rx {
        let line = strip_ansi(&raw);
        if line.trim().is_empty() {
            continue;
        }
        on_line(is_err, &line);
    }
    let _ = h1.join();
    let _ = h2.join();
    let st = child.wait().map_err(|e| e.to_string())?;
    Ok(st.code().unwrap_or(-1))
}

// ---------------------------------------------------------------- text helpers

pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\u{1b}' {
            match it.peek() {
                Some('[') => {
                    it.next();
                    for d in it.by_ref() {
                        if ('@'..='~').contains(&d) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    // OSC … BEL or ST
                    it.next();
                    while let Some(d) = it.next() {
                        if d == '\u{7}' {
                            break;
                        }
                        if d == '\u{1b}' {
                            it.next();
                            break;
                        }
                    }
                }
                _ => {}
            }
        } else if c != '\u{8}' {
            out.push(c);
        }
    }
    out
}

pub fn first_line(s: &str) -> Option<String> {
    s.lines().map(|l| l.trim()).find(|l| !l.is_empty()).map(|l| l.to_string())
}

/// is this a progress-bar style line (so a UI may overwrite instead of append)?
pub fn is_progress(line: &str) -> bool {
    let t = line.trim();
    t.chars().any(|c| matches!(c, '█' | '▒' | '░' | '▓'))
        || (t.ends_with('%') && t.chars().filter(|c| c.is_ascii_digit()).count() > 0 && t.len() < 80)
}

/// spinner frames (- \ | /) that winget & friends print when not on a tty
pub fn is_spinner(line: &str) -> bool {
    let t = line.trim();
    t.is_empty() || t.chars().all(|c| matches!(c, '-' | '\\' | '|' | '/' | ' '))
}

pub fn urlencode(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{:02X}", b));
        }
    }
    o
}

pub fn fmt_count(n: f64) -> String {
    if n >= 1_000_000.0 {
        format!("{:.1}M", n / 1_000_000.0)
    } else if n >= 1000.0 {
        format!("{:.1}k", n / 1000.0)
    } else {
        format!("{}", n as i64)
    }
}

// ---------------------------------------------------------------- validation
// ids/versions end up as argv entries (never a shell string), but we still refuse anything
// that could be read as a flag or a path trick.

pub fn valid_token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 200
        && !s.starts_with('-')
        && !s.contains("..")
        && s.chars().all(|c| c.is_ascii_alphanumeric() || "._@/+:~-".contains(c))
}

/// owner/repo with sane characters only
pub fn valid_repo(s: &str) -> Option<(&str, &str)> {
    let (o, r) = s.split_once('/')?;
    let ok = |p: &str| {
        !p.is_empty()
            && p != "."
            && p != ".."
            && p.len() <= 100
            && p.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    };
    if ok(o) && ok(r) {
        Some((o, r))
    } else {
        None
    }
}

// ---------------------------------------------------------------- http (via curl, which ships with Windows 10+)

pub fn http_json(url: &str, headers: &[(&str, String)]) -> Result<Option<J>, String> {
    let curl = which("curl").ok_or_else(|| "curl not found (needed for web lookups)".to_string())?;
    let mut args: Vec<String> = ["-sSL", "--fail", "--max-time", "20", "-A", "parcels/0.1"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    for (k, v) in headers {
        args.push("-H".into());
        args.push(format!("{}: {}", k, v));
    }
    args.push(url.to_string());
    let c = capture(&curl, &args, 30)?;
    if c.code == 22 {
        return Ok(None); // 404 & friends
    }
    if !c.ok {
        return Err(first_line(&c.err).unwrap_or_else(|| format!("curl exit {}", c.code)));
    }
    json::parse(&c.out).map(Some)
}

/// GitHub repository search (optionally authenticated through GITHUB_TOKEN to lift rate limits)
pub fn github_search(q: &str) -> Result<Vec<J>, String> {
    let url = format!(
        "https://api.github.com/search/repositories?q={}&per_page=25",
        urlencode(q)
    );
    let mut h = vec![("Accept", "application/vnd.github+json".to_string())];
    if let Ok(t) = env::var("GITHUB_TOKEN") {
        if !t.is_empty() {
            h.push(("Authorization", format!("Bearer {}", t)));
        }
    }
    let v = http_json(&url, &h)?.ok_or("github: not found")?;
    Ok(v.get("items").map(|i| i.items().to_vec()).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi() {
        assert_eq!(strip_ansi("\u{1b}[32mgreen\u{1b}[0m ok"), "green ok");
        assert_eq!(strip_ansi("a\u{1b}]0;title\u{7}b"), "ab");
    }

    #[test]
    fn tokens() {
        assert!(valid_token("Microsoft.PowerToys"));
        assert!(valid_token("@scope/pkg"));
        assert!(valid_token("zlib:x64-windows"));
        assert!(!valid_token("--force"));
        assert!(!valid_token("a b"));
        assert!(!valid_token("a;rm"));
        assert!(!valid_token("../x"));
        assert!(!valid_token(""));
    }

    #[test]
    fn repos() {
        assert_eq!(valid_repo("charmbracelet/bubbletea"), Some(("charmbracelet", "bubbletea")));
        assert!(valid_repo("a/../b").is_none());
        assert!(valid_repo("../x").is_none());
        assert!(valid_repo("justone").is_none());
    }

    #[test]
    fn reg_value() {
        let out = "\nHKEY_CURRENT_USER\\Environment\n    Path    REG_EXPAND_SZ    C:\\a;%USERPROFILE%\\b\n\n";
        assert_eq!(parse_reg_value(out, "Path").unwrap(), "C:\\a;%USERPROFILE%\\b");
        let sz = "    PATH    REG_SZ    C:\\x\n";
        assert_eq!(parse_reg_value(sz, "Path").unwrap(), "C:\\x");
        assert!(parse_reg_value("nothing here", "Path").is_none());
    }

    #[test]
    fn expand() {
        env::set_var("PARCELS_TEST_VAR", "HOME_X");
        assert_eq!(expand_env("%PARCELS_TEST_VAR%\\bin;%NOPE_NOT_SET%\\x"), "HOME_X\\bin;%NOPE_NOT_SET%\\x");
        assert_eq!(expand_env("100%"), "100%");
        assert_eq!(expand_env("plain"), "plain");
    }

    #[test]
    fn dirs_are_deduped_and_searchable() {
        let d = search_dirs();
        let mut keys: Vec<String> = d.iter().map(|p| norm_key(p)).collect();
        let n = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), n);
        assert!(which("sh").is_some() || cfg!(windows));
    }

    #[test]
    fn enc() {
        assert_eq!(urlencode("a b&c"), "a%20b%26c");
        assert_eq!(urlencode("@x/y"), "%40x%2Fy");
    }

    #[test]
    fn progress() {
        assert!(is_progress("  ██████░░░░  45%"));
        assert!(!is_progress("Successfully installed"));
        assert!(is_spinner("  \\ "));
        assert!(!is_spinner("-- done --"));
    }
}
