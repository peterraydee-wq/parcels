//! parcels-core — the engine behind `parcels`.
//!
//! speaks newline-delimited JSON on stdout so any front end (the Go/Charm TUI, a script,
//! another tool) can drive every package manager through one interface.
//!
//!   parcels-core providers
//!   parcels-core search  <query> [--only winget,scoop]
//!   parcels-core list            [--only ...]
//!   parcels-core outdated        [--only ...]
//!   parcels-core run <install|uninstall|upgrade> <provider> <id> [--version v]
//!   parcels-core export          [--only ...]      one-line manifest of everything installed
//!   parcels-core import <file> [--pin]             install everything in a manifest
mod json;
mod parse;
mod providers;
mod util;

use json::{obj, J};
use providers::{all, remove_path, Action, Launcher, Pkg, Plan, Provider};
use std::env;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Mutex;
use std::thread;
use std::time::Instant;
use util::*;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let code = run(&args);
    std::process::exit(code);
}

struct Flags {
    pos: Vec<String>,
    only: Vec<String>,
    version: Option<String>,
    pin: bool,
}

fn flags(args: &[String]) -> Flags {
    let mut f = Flags { pos: vec![], only: vec![], version: None, pin: false };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--only" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    f.only = v.split(',').map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect();
                }
            }
            "--version" => {
                i += 1;
                f.version = args.get(i).cloned();
            }
            "--pin" => f.pin = true,
            other => f.pos.push(other.to_string()),
        }
        i += 1;
    }
    f
}

fn run(a: &[String]) -> i32 {
    let Some(cmd) = a.first() else {
        usage();
        return 2;
    };
    let f = flags(&a[1..]);
    match cmd.as_str() {
        "providers" => cmd_providers(),
        "doctor" => cmd_doctor(),
        "search" => {
            let q = f.pos.join(" ");
            let q = q.trim().trim_start_matches('-').trim().to_string();
            if q.is_empty() {
                emit(&obj(vec![("type", "done".into()), ("error", "empty query".into())]));
                return 2;
            }
            cmd_fan("search", &q, &f.only)
        }
        "list" => cmd_fan("list", "", &f.only),
        "outdated" => cmd_fan("outdated", "", &f.only),
        "run" => cmd_run(&f),
        "export" => cmd_export(&f.only),
        "import" => cmd_import(&f),
        "fox" | "loomer" => {
            print!("{}", FOX);
            0
        }
        "floppy" => {
            print!("{}", FLOPPY);
            0
        }
        "--version" | "-V" | "version" => {
            println!("parcels-core {}  ·  loomer approves", VERSION);
            0
        }
        _ => {
            usage();
            2
        }
    }
}

fn usage() {
    eprintln!(
        "parcels-core {}\n\nusage:\n  parcels-core providers\n  parcels-core doctor\n  parcels-core search <query> [--only a,b]\n  parcels-core list|outdated [--only a,b]\n  parcels-core run <install|uninstall|upgrade> <provider> <id> [--version v]\n  parcels-core export [--only a,b]\n  parcels-core import <manifest.json> [--pin]\n\noutput is JSON lines. most people want the TUI: `parcels`.",
        VERSION
    );
}

fn select(only: &[String]) -> Vec<Box<dyn Provider>> {
    all().into_iter().filter(|p| only.is_empty() || only.iter().any(|o| o == p.id())).collect()
}

/// "pip 24.0 from /usr/lib/… (python 3.12)" -> "pip 24.0"; "go version go1.24 linux/amd64" -> "go1.24"
fn short_version(s: &str) -> String {
    let mut t = s.trim();
    for pre in ["go version ", "git version "] {
        if let Some(r) = t.strip_prefix(pre) {
            t = r;
        }
    }
    let t = t.split(" from ").next().unwrap_or(t);
    let t = t.split(" (").next().unwrap_or(t);
    let t = if t.starts_with("go1") { t.split(' ').next().unwrap_or(t) } else { t };
    t.chars().take(28).collect::<String>().trim().to_string()
}

fn guard<F: FnOnce()>(id: &str, f: F) {
    if catch_unwind(AssertUnwindSafe(f)).is_err() {
        emit(&obj(vec![("type", "end".into()), ("provider", id.into()), ("count", 0usize.into()), ("error", "internal error".into())]));
    }
}

// ------------------------------------------------------------------ providers

fn cmd_providers() -> i32 {
    let ps = all();
    thread::scope(|s| {
        for p in &ps {
            s.spawn(move || {
                guard(p.id(), || {
                    let l = p.probe();
                    let (version, path) = match &l {
                        Some(l) => {
                            let v = l
                                .cap(p.version_args(), 10)
                                .ok()
                                .and_then(|c| first_line(&c.out).or_else(|| first_line(&c.err)))
                                .map(|s| short_version(&s))
                                .unwrap_or_default();
                            (v, l.exe.to_string_lossy().into_owned())
                        }
                        None => (String::new(), String::new()),
                    };
                    let caps: Vec<J> = p.caps().iter().map(|c| (*c).into()).collect();
                    emit(&obj(vec![
                        ("type", "provider".into()),
                        ("id", p.id().into()),
                        ("name", p.name().into()),
                        ("kind", p.kind().into()),
                        ("blurb", p.blurb().into()),
                        ("hint", p.hint().into()),
                        ("available", l.is_some().into()),
                        ("version", version.into()),
                        ("path", path.into()),
                        ("caps", J::Arr(caps)),
                    ]));
                });
            });
        }
    });
    emit(&obj(vec![("type", "done".into())]));
    0
}

// ------------------------------------------------------------------ doctor

/// why was (or wasn't) each manager found? one line per manager plus the directories searched.
fn cmd_doctor() -> i32 {
    let dirs: Vec<J> = search_dirs().iter().map(|d| d.to_string_lossy().into_owned().into()).collect();
    emit(&obj(vec![("type", "dirs".into()), ("dirs", J::Arr(dirs))]));
    for p in all() {
        let tried: Vec<J> = p.exes().iter().map(|e| (*e).into()).collect();
        let l = p.probe();
        let (path, via) = match &l {
            Some(l) => {
                let dir_on_path = l.exe.parent().map(on_process_path).unwrap_or(false);
                let via = if !l.pre.is_empty() {
                    "python -m pip"
                } else if dir_on_path {
                    "PATH"
                } else {
                    "outside PATH"
                };
                (l.exe.to_string_lossy().into_owned(), via)
            }
            None => (String::new(), ""),
        };
        emit(&obj(vec![
            ("type", "doctor".into()),
            ("id", p.id().into()),
            ("name", p.name().into()),
            ("available", l.is_some().into()),
            ("path", path.into()),
            ("via", via.into()),
            ("tried", J::Arr(tried)),
            ("hint", p.hint().into()),
        ]));
    }
    emit(&obj(vec![("type", "done".into())]));
    0
}

// ------------------------------------------------------------------ search / list / outdated

enum Outcome {
    Skipped,
    Done(Result<Vec<Pkg>, String>),
}

fn fan(kind: &str, q: &str, only: &[String], with_cmds: bool, sink: &(dyn Fn(&dyn Provider, Outcome, u128) + Sync)) {
    let ps = select(only);
    thread::scope(|s| {
        for p in &ps {
            s.spawn(move || {
                let t0 = Instant::now();
                guard(p.id(), || {
                    if with_cmds {
                        emit(&obj(vec![("type", "start".into()), ("provider", p.id().into())]));
                    }
                    let Some(l) = p.probe() else {
                        sink(p.as_ref(), Outcome::Skipped, 0);
                        return;
                    };
                    let mut res = match kind {
                        "search" => p.search(&l, q),
                        "list" => p.list(&l),
                        _ => p.outdated(&l),
                    };
                    if let Ok(pkgs) = res.as_mut() {
                        pkgs.truncate(40);
                        if with_cmds {
                            for pk in pkgs.iter_mut() {
                                add_cmds(p.as_ref(), &l, kind, pk);
                            }
                        }
                    }
                    sink(p.as_ref(), Outcome::Done(res), t0.elapsed().as_millis());
                });
            });
        }
    });
}

fn add_cmds(p: &dyn Provider, l: &Launcher, kind: &str, pk: &mut Pkg) {
    if !valid_token(&pk.id) {
        return;
    }
    let prev = |a: Action| p.plan(l, a, &pk.id, None, true).map(|pl| pl.preview()).unwrap_or_default();
    match kind {
        "search" => pk.cmd = prev(Action::Install),
        "list" => {
            pk.cmd = prev(Action::Uninstall);
            pk.cmd2 = prev(Action::Upgrade);
        }
        _ => {
            pk.cmd = prev(Action::Upgrade);
            pk.cmd2 = prev(Action::Uninstall);
        }
    }
}

fn cmd_fan(kind: &str, q: &str, only: &[String]) -> i32 {
    let ev = match kind {
        "search" => "result",
        "list" => "installed",
        _ => "outdated",
    };
    fan(kind, q, only, true, &|p, out, ms| match out {
        Outcome::Skipped => emit(&obj(vec![
            ("type", "end".into()),
            ("provider", p.id().into()),
            ("count", 0usize.into()),
            ("skipped", true.into()),
            ("error", "not installed".into()),
        ])),
        Outcome::Done(Ok(pkgs)) => {
            for pk in &pkgs {
                emit(&pk.to_json(ev));
            }
            emit(&obj(vec![("type", "end".into()), ("provider", p.id().into()), ("count", pkgs.len().into()), ("ms", (ms as usize).into())]));
        }
        Outcome::Done(Err(e)) => emit(&obj(vec![
            ("type", "end".into()),
            ("provider", p.id().into()),
            ("count", 0usize.into()),
            ("ms", (ms as usize).into()),
            ("error", e.into()),
        ])),
    });
    emit(&obj(vec![("type", "done".into())]));
    0
}

// ------------------------------------------------------------------ run

/// returns (ok, exit code). streams `line` events; emits `plan` first and `exit` last.
fn do_action(action: Action, provider: &str, id: &str, ver: Option<&str>) -> (bool, i32) {
    let fail = |msg: &str| -> (bool, i32) {
        emit(&obj(vec![("type", "line".into()), ("stream", "err".into()), ("text", msg.into())]));
        emit(&obj(vec![("type", "exit".into()), ("ok", false.into()), ("code", (-1i64).into())]));
        (false, -1)
    };
    let ps = all();
    let Some(p) = ps.iter().find(|p| p.id() == provider) else {
        return fail(&format!("unknown provider '{}'", provider));
    };
    let id_ok = if provider == "git" { valid_repo(id).is_some() } else { valid_token(id) };
    if !id_ok {
        return fail(&format!("refusing id '{}' (unexpected characters)", id));
    }
    if let Some(v) = ver {
        if !valid_token(v) {
            return fail(&format!("refusing version '{}'", v));
        }
    }
    let Some(l) = p.probe() else {
        return fail(&format!("{} is not installed on this machine", p.name()));
    };
    let plan = match p.plan(&l, action, id, ver, false) {
        Ok(pl) => pl,
        Err(e) => return fail(&e),
    };
    emit(&obj(vec![("type", "plan".into()), ("cmd", plan.preview().into())]));
    match plan {
        Plan::Argv(exe, args) => {
            let mut last = String::new();
            let r = stream(&exe, &args, |is_err, line| {
                if is_spinner(line) || line == last {
                    return;
                }
                last = line.to_string();
                emit(&obj(vec![
                    ("type", "line".into()),
                    ("stream", if is_err { "err" } else { "out" }.into()),
                    ("text", line.into()),
                    ("progress", is_progress(line).into()),
                ]));
            });
            match r {
                Ok(code) => {
                    emit(&obj(vec![("type", "exit".into()), ("ok", (code == 0).into()), ("code", (code as i64).into())]));
                    (code == 0, code)
                }
                Err(e) => fail(&e),
            }
        }
        Plan::Remove(path) => match remove_path(&path) {
            Ok(()) => {
                emit(&obj(vec![("type", "line".into()), ("stream", "out".into()), ("text", format!("removed {}", path.display()).into())]));
                emit(&obj(vec![("type", "exit".into()), ("ok", true.into()), ("code", 0i64.into())]));
                (true, 0)
            }
            Err(e) => fail(&e),
        },
    }
}

fn cmd_run(f: &Flags) -> i32 {
    if f.pos.len() < 3 {
        usage();
        return 2;
    }
    let Some(action) = Action::parse(&f.pos[0]) else {
        eprintln!("unknown action '{}'", f.pos[0]);
        return 2;
    };
    let (ok, _) = do_action(action, &f.pos[1], &f.pos[2], f.version.as_deref());
    if ok {
        0
    } else {
        1
    }
}

// ------------------------------------------------------------------ export / import

fn cmd_export(only: &[String]) -> i32 {
    let found: Mutex<Vec<Pkg>> = Mutex::new(Vec::new());
    fan("list", "", only, false, &|_, out, _| {
        if let Outcome::Done(Ok(pkgs)) = out {
            found.lock().unwrap_or_else(|e| e.into_inner()).extend(pkgs);
        }
    });
    let mut pkgs = found.into_inner().unwrap_or_default();
    pkgs.sort_by(|a, b| (a.provider.as_str(), a.id.to_lowercase()).cmp(&(b.provider.as_str(), b.id.to_lowercase())));
    let created = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let list: Vec<J> = pkgs
        .iter()
        .map(|p| obj(vec![("provider", p.provider.clone().into()), ("id", p.id.clone().into()), ("version", p.version.clone().into())]))
        .collect();
    emit(&obj(vec![
        ("type", "manifest".into()),
        ("parcels", 1usize.into()),
        ("created", (created as usize).into()),
        ("note", "a floppy's worth of your machine".into()),
        ("packages", J::Arr(list)),
    ]));
    0
}

fn cmd_import(f: &Flags) -> i32 {
    let Some(path) = f.pos.first() else {
        usage();
        return 2;
    };
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            emit(&obj(vec![("type", "done".into()), ("error", format!("{}: {}", path, e).into())]));
            return 1;
        }
    };
    let doc = match json::parse(&text) {
        Ok(d) => d,
        Err(e) => {
            emit(&obj(vec![("type", "done".into()), ("error", format!("bad manifest: {}", e).into())]));
            return 1;
        }
    };
    let items = doc.get("packages").map(|p| p.items().to_vec()).unwrap_or_default();
    let total = items.len();
    let mut failed = 0usize;
    for (i, it) in items.iter().enumerate() {
        let (prov, id, ver) = (it.text("provider"), it.text("id"), it.text("version"));
        emit(&obj(vec![("type", "item".into()), ("index", (i + 1).into()), ("total", total.into()), ("provider", prov.clone().into()), ("id", id.clone().into())]));
        let pin = if f.pin && !ver.is_empty() { Some(ver.as_str()) } else { None };
        let (ok, _) = do_action(Action::Install, &prov, &id, pin);
        if !ok {
            failed += 1;
        }
    }
    emit(&obj(vec![("type", "done".into()), ("total", total.into()), ("failed", failed.into())]));
    if failed == 0 {
        0
    } else {
        1
    }
}

// ------------------------------------------------------------------ easter eggs

const FOX: &str = "\x1b[38;2;230;152;117m
     |\\     /|
     | \\___/ |
    /  \x1b[38;2;167;192;128mo\x1b[38;2;230;152;117m   \x1b[38;2;167;192;128mo\x1b[38;2;230;152;117m  \\
   (     v     )
    \\   ---   /
     `-.___.-'
\x1b[38;2;167;192;128m      loomer says: sudo apt-get a life\x1b[0m
";

const FLOPPY: &str = "\x1b[38;2;167;192;128m
   ______________
  |[]  ______  []|
  |   |      |   |
  |   | parc |   |     1.44 MB
  |   | els  |   |     of packages
  |   |______|   |
  |  ____________|
  | |  __   ____ |
  |_|_|__|_|____|_|
\x1b[0m";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_short() {
        assert_eq!(short_version("pip 24.0 from /usr/lib/python3/dist-packages/pip (python 3.12)"), "pip 24.0");
        assert_eq!(short_version("go version go1.24.13 linux/amd64"), "go1.24.13");
        assert_eq!(short_version("git version 2.43.0"), "2.43.0");
        assert_eq!(short_version("uv 0.11.7 (x86_64-unknown-linux-gnu)"), "uv 0.11.7");
        assert_eq!(short_version("10.9.7"), "10.9.7");
    }
}
