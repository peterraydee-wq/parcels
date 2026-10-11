//! every package source den knows. to add one: implement `Provider`, push it in `all()`.
use crate::json::{self, J};
use crate::parse::*;
use crate::util::*;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    Install,
    Uninstall,
    Upgrade,
}

impl Action {
    pub fn parse(s: &str) -> Option<Action> {
        match s {
            "install" => Some(Action::Install),
            "uninstall" => Some(Action::Uninstall),
            "upgrade" => Some(Action::Upgrade),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Pkg {
    pub provider: String,
    pub id: String,
    pub name: String,
    pub version: String,
    pub latest: String,
    pub desc: String,
    pub cmd: String,
    pub cmd2: String,
}

impl Pkg {
    pub fn to_json(&self, kind: &str) -> J {
        let mut v: Vec<(&str, J)> = vec![("type", kind.into()), ("provider", self.provider.clone().into()), ("id", self.id.clone().into())];
        let name = if self.name.is_empty() { &self.id } else { &self.name };
        v.push(("name", name.clone().into()));
        for (k, s) in [
            ("version", &self.version),
            ("latest", &self.latest),
            ("desc", &self.desc),
            ("cmd", &self.cmd),
            ("cmd2", &self.cmd2),
        ] {
            if !s.is_empty() {
                v.push((k, s.clone().into()));
            }
        }
        json::obj(v)
    }
}

fn mk(provider: &str, id: &str, ver: &str, desc: &str) -> Pkg {
    Pkg {
        provider: provider.to_string(),
        id: id.to_string(),
        name: id.to_string(),
        version: ver.to_string(),
        desc: desc.to_string(),
        ..Default::default()
    }
}

pub struct Launcher {
    pub exe: PathBuf,
    pub pre: Vec<String>,
}

impl Launcher {
    pub fn new(exe: PathBuf) -> Self {
        Launcher { exe, pre: Vec::new() }
    }
    fn args(&self, rest: &[&str]) -> Vec<String> {
        self.pre.iter().cloned().chain(rest.iter().map(|s| s.to_string())).collect()
    }
    pub fn cap(&self, rest: &[&str], secs: u64) -> Result<Cap, String> {
        capture(&self.exe, &self.args(rest), secs)
    }
    /// stdout of a command; only an error when it printed nothing and exited non-zero
    /// (npm outdated, for one, exits 1 *with* useful output)
    pub fn out(&self, rest: &[&str], secs: u64) -> Result<String, String> {
        let c = self.cap(rest, secs)?;
        if !c.ok && c.out.trim().is_empty() {
            return Err(first_line(&c.err).unwrap_or_else(|| format!("exit code {}", c.code)));
        }
        Ok(c.out)
    }
    fn plan(&self, rest: &[&str]) -> Plan {
        Plan::Argv(self.exe.clone(), self.args(rest))
    }
}

pub enum Plan {
    Argv(PathBuf, Vec<String>),
    Remove(PathBuf),
}

impl Plan {
    pub fn preview(&self) -> String {
        match self {
            Plan::Argv(exe, args) => {
                let stem = exe.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let mut s = stem;
                for a in args {
                    s.push(' ');
                    if a.contains(' ') {
                        s.push('"');
                        s.push_str(a);
                        s.push('"');
                    } else {
                        s.push_str(a);
                    }
                }
                s
            }
            Plan::Remove(p) => format!("delete {}", p.display()),
        }
    }
}

pub trait Provider: Sync + Send {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn kind(&self) -> &'static str;
    fn blurb(&self) -> &'static str;
    fn exes(&self) -> &'static [&'static str];
    fn version_args(&self) -> &'static [&'static str] {
        &["--version"]
    }
    fn caps(&self) -> &'static [&'static str] {
        &["search", "list", "install", "uninstall", "upgrade"]
    }
    fn probe(&self) -> Option<Launcher> {
        self.exes().iter().find_map(|e| which(e)).map(Launcher::new)
    }
    fn hint(&self) -> &'static str {
        install_hint(self.id())
    }
    fn search(&self, l: &Launcher, q: &str) -> Result<Vec<Pkg>, String>;
    fn list(&self, l: &Launcher) -> Result<Vec<Pkg>, String>;
    fn outdated(&self, _l: &Launcher) -> Result<Vec<Pkg>, String> {
        Err("outdated is not supported".into())
    }
    fn plan(&self, l: &Launcher, a: Action, id: &str, ver: Option<&str>, dry: bool) -> Result<Plan, String>;
}

pub fn install_hint(id: &str) -> &'static str {
    match id {
        "winget" => "ships with Windows 10/11 as App Installer (Microsoft Store)",
        "scoop" => "irm get.scoop.sh | iex",
        "choco" => "see chocolatey.org/install",
        "npm" => "winget install OpenJS.NodeJS.LTS",
        "pnpm" => "npm i -g pnpm",
        "bun" => "irm bun.sh/install.ps1 | iex",
        "pip" => "winget install Python.Python.3.12",
        "uv" => "winget install astral-sh.uv",
        "pipx" => "pip install pipx",
        "cargo" => "winget install Rustlang.Rustup",
        "go" => "winget install GoLang.Go",
        "vcpkg" => "clone microsoft/vcpkg, run bootstrap-vcpkg.bat, set VCPKG_ROOT",
        "dotnet" => "winget install Microsoft.DotNet.SDK.8",
        "git" => "winget install Git.Git",
        _ => "",
    }
}

pub fn all() -> Vec<Box<dyn Provider>> {
    vec![
        Box::new(Winget),
        Box::new(Scoop),
        Box::new(Choco),
        Box::new(Npm),
        Box::new(Pnpm),
        Box::new(Bun),
        Box::new(Pip),
        Box::new(Uv),
        Box::new(Pipx),
        Box::new(Cargo),
        Box::new(Go),
        Box::new(Vcpkg),
        Box::new(Dotnet),
        Box::new(Git),
    ]
}

// ------------------------------------------------------------------ shared lookups

fn npm_registry(provider: &str, q: &str) -> Result<Vec<Pkg>, String> {
    let url = format!("https://registry.npmjs.org/-/v1/search?text={}&size=25", urlencode(q));
    let v = http_json(&url, &[])?.ok_or("registry: not found")?;
    let mut out = Vec::new();
    for o in v.get("objects").map(|o| o.items()).unwrap_or(&[]) {
        if let Some(p) = o.get("package") {
            let name = p.text("name");
            if !name.is_empty() {
                out.push(mk(provider, &name, &p.text("version"), &p.text("description")));
            }
        }
    }
    Ok(out)
}

/// PyPI has no search API, so: exact-name lookups (plus a couple of common spellings)
fn pypi_lookup(provider: &str, q: &str) -> Result<Vec<Pkg>, String> {
    let first = q.split_whitespace().next().unwrap_or("");
    let mut cands: Vec<String> = vec![first.to_string(), first.replace('_', "-"), format!("python-{}", first)];
    cands.dedup();
    let mut out: Vec<Pkg> = Vec::new();
    let mut last_err = None;
    for c in cands {
        if c.is_empty() || !c.chars().all(|ch| ch.is_ascii_alphanumeric() || "._-".contains(ch)) {
            continue;
        }
        match http_json(&format!("https://pypi.org/pypi/{}/json", c), &[]) {
            Ok(Some(v)) => {
                if let Some(info) = v.get("info") {
                    let name = info.text("name");
                    if !name.is_empty() && !out.iter().any(|p| p.id.eq_ignore_ascii_case(&name)) {
                        out.push(mk(provider, &name, &info.text("version"), &info.text("summary")));
                    }
                }
            }
            Ok(None) => {}
            Err(e) => last_err = Some(e),
        }
    }
    if out.is_empty() {
        if let Some(e) = last_err {
            return Err(e);
        }
    }
    Ok(out)
}

fn version_spec(sep: &str, id: &str, ver: Option<&str>) -> String {
    match ver {
        Some(v) => format!("{}{}{}", id, sep, v),
        None => id.to_string(),
    }
}

// ------------------------------------------------------------------ winget

pub struct Winget;

impl Winget {
    fn table(l: &Launcher, args: &[&str]) -> Result<Option<Table>, String> {
        let c = l.cap(args, 90)?;
        if let Some(t) = parse_table(&c.out, &["Id", "Version"]) {
            return Ok(Some(t));
        }
        if c.ok || c.out.contains("No ") {
            return Ok(None);
        }
        Err(first_line(&c.out).or_else(|| first_line(&c.err)).unwrap_or_else(|| format!("exit code {}", c.code)))
    }
    fn rows(t: &Table, with_latest: bool) -> Vec<Pkg> {
        let (cn, ci, cv, cs, ca) = (t.col(&["Name"]), t.col(&["Id"]), t.col(&["Version"]), t.col(&["Source"]), t.col(&["Available"]));
        let mut out = Vec::new();
        for r in &t.rows {
            let id = t.cell(r, ci);
            // ARP\ and MSIX\ entries are installed apps winget can see but not manage by id
            if id.is_empty() || id.contains(' ') || id.starts_with("ARP\\") || id.starts_with("MSIX\\") {
                continue;
            }
            let mut p = mk("winget", id, t.cell(r, cv).trim_start_matches(['<', '>', ' ']), t.cell(r, cs));
            p.name = t.cell(r, cn).to_string();
            if with_latest {
                p.latest = t.cell(r, ca).to_string();
            }
            out.push(p);
        }
        out
    }
}

impl Provider for Winget {
    fn id(&self) -> &'static str {
        "winget"
    }
    fn name(&self) -> &'static str {
        "WinGet"
    }
    fn kind(&self) -> &'static str {
        "system"
    }
    fn blurb(&self) -> &'static str {
        "Microsoft's package manager (winget + msstore sources)"
    }
    fn exes(&self) -> &'static [&'static str] {
        &["winget"]
    }
    fn caps(&self) -> &'static [&'static str] {
        &["search", "list", "outdated", "install", "uninstall", "upgrade"]
    }
    fn search(&self, l: &Launcher, q: &str) -> Result<Vec<Pkg>, String> {
        let t = Self::table(l, &["search", "--query", q, "--accept-source-agreements", "--disable-interactivity"])?;
        Ok(t.map(|t| Self::rows(&t, false)).unwrap_or_default())
    }
    fn list(&self, l: &Launcher) -> Result<Vec<Pkg>, String> {
        let t = Self::table(l, &["list", "--accept-source-agreements", "--disable-interactivity"])?;
        Ok(t.map(|t| Self::rows(&t, true)).unwrap_or_default())
    }
    fn outdated(&self, l: &Launcher) -> Result<Vec<Pkg>, String> {
        let t = Self::table(l, &["upgrade", "--accept-source-agreements", "--disable-interactivity"])?;
        Ok(t.map(|t| Self::rows(&t, true)).unwrap_or_default().into_iter().filter(|p| !p.latest.is_empty()).collect())
    }
    fn plan(&self, l: &Launcher, a: Action, id: &str, ver: Option<&str>, _dry: bool) -> Result<Plan, String> {
        let mut v: Vec<&str> = match a {
            Action::Install => vec!["install", "--id", id, "-e", "--silent", "--accept-package-agreements", "--accept-source-agreements", "--disable-interactivity"],
            Action::Upgrade => vec!["upgrade", "--id", id, "-e", "--silent", "--accept-package-agreements", "--accept-source-agreements", "--disable-interactivity"],
            Action::Uninstall => vec!["uninstall", "--id", id, "-e", "--silent", "--disable-interactivity"],
        };
        if let (Some(ver), true) = (ver, a == Action::Install) {
            v.push("--version");
            v.push(ver);
        }
        Ok(l.plan(&v))
    }
}

// ------------------------------------------------------------------ scoop

pub struct Scoop;

fn scoop_id(name: &str, bucket: &str) -> String {
    if bucket.is_empty() || bucket == "main" {
        name.to_string()
    } else {
        format!("{}/{}", bucket, name)
    }
}

impl Provider for Scoop {
    fn id(&self) -> &'static str {
        "scoop"
    }
    fn name(&self) -> &'static str {
        "Scoop"
    }
    fn kind(&self) -> &'static str {
        "system"
    }
    fn blurb(&self) -> &'static str {
        "portable, per-user installs from buckets"
    }
    fn exes(&self) -> &'static [&'static str] {
        &["scoop"]
    }
    fn caps(&self) -> &'static [&'static str] {
        &["search", "list", "outdated", "install", "uninstall", "upgrade"]
    }
    fn search(&self, l: &Launcher, q: &str) -> Result<Vec<Pkg>, String> {
        let out = l.out(&["search", q], 120)?;
        if let Some(t) = parse_table(&out, &["Name", "Version"]) {
            let (cn, cv, cs, cb) = (t.col(&["Name"]), t.col(&["Version"]), t.col(&["Source"]), t.col(&["Binaries", "Binary"]));
            return Ok(t
                .rows
                .iter()
                .filter(|r| !t.cell(r, cn).is_empty())
                .map(|r| {
                    let mut p = mk("scoop", &scoop_id(t.cell(r, cn), t.cell(r, cs)), t.cell(r, cv), t.cell(r, cb));
                    p.name = t.cell(r, cn).to_string();
                    p
                })
                .collect());
        }
        Ok(parse_scoop_old(&out)
            .into_iter()
            .map(|(n, v, b)| {
                let mut p = mk("scoop", &scoop_id(&n, &b), &v, "");
                p.name = n;
                p
            })
            .collect())
    }
    fn list(&self, l: &Launcher) -> Result<Vec<Pkg>, String> {
        if let Ok(out) = l.out(&["export"], 60) {
            if let Ok(v) = json::parse(&out) {
                if let Some(apps) = v.get("apps") {
                    return Ok(apps
                        .items()
                        .iter()
                        .map(|a| {
                            let mut p = mk("scoop", &scoop_id(&a.text("Name"), &a.text("Source")), &a.text("Version"), &a.text("Info"));
                            p.name = a.text("Name");
                            p
                        })
                        .collect());
                }
            }
        }
        let out = l.out(&["list"], 60)?;
        let Some(t) = parse_table(&out, &["Name", "Version"]) else { return Ok(vec![]) };
        let (cn, cv, cs) = (t.col(&["Name"]), t.col(&["Version"]), t.col(&["Source"]));
        Ok(t.rows.iter().map(|r| mk("scoop", &scoop_id(t.cell(r, cn), t.cell(r, cs)), t.cell(r, cv), "")).collect())
    }
    fn outdated(&self, l: &Launcher) -> Result<Vec<Pkg>, String> {
        let out = l.out(&["status"], 120)?;
        let Some(t) = parse_table(&out, &["Name", "Latest"]) else { return Ok(vec![]) };
        let (cn, ci, cl) = (t.col(&["Name"]), t.col(&["Installed Version"]), t.col(&["Latest Version"]));
        Ok(t
            .rows
            .iter()
            .filter(|r| !t.cell(r, cn).is_empty() && !t.cell(r, cl).is_empty())
            .map(|r| {
                let mut p = mk("scoop", t.cell(r, cn), t.cell(r, ci), "");
                p.latest = t.cell(r, cl).to_string();
                p
            })
            .collect())
    }
    fn plan(&self, l: &Launcher, a: Action, id: &str, ver: Option<&str>, _dry: bool) -> Result<Plan, String> {
        let spec = version_spec("@", id, if a == Action::Install { ver } else { None });
        Ok(match a {
            Action::Install => l.plan(&["install", &spec]),
            Action::Uninstall => l.plan(&["uninstall", &spec]),
            Action::Upgrade => l.plan(&["update", &spec]),
        })
    }
}

// ------------------------------------------------------------------ chocolatey

pub struct Choco;

impl Provider for Choco {
    fn id(&self) -> &'static str {
        "choco"
    }
    fn name(&self) -> &'static str {
        "Chocolatey"
    }
    fn kind(&self) -> &'static str {
        "system"
    }
    fn blurb(&self) -> &'static str {
        "the big community repo (admin shell needed to install)"
    }
    fn exes(&self) -> &'static [&'static str] {
        &["choco"]
    }
    fn caps(&self) -> &'static [&'static str] {
        &["search", "list", "outdated", "install", "uninstall", "upgrade"]
    }
    fn search(&self, l: &Launcher, q: &str) -> Result<Vec<Pkg>, String> {
        let out = l.out(&["search", q, "-r"], 90)?;
        Ok(parse_pipes(&out).into_iter().map(|f| mk("choco", &f[0], &f[1], "")).collect())
    }
    fn list(&self, l: &Launcher) -> Result<Vec<Pkg>, String> {
        // choco 1.x: `list` hits the server unless -l; choco 2.x: `list` is local already
        let v1 = l.out(&["--version"], 15).map(|v| v.trim().starts_with("1.")).unwrap_or(false);
        let out = if v1 { l.out(&["list", "-l", "-r"], 60)? } else { l.out(&["list", "-r"], 60)? };
        Ok(parse_pipes(&out).into_iter().map(|f| mk("choco", &f[0], &f[1], "")).collect())
    }
    fn outdated(&self, l: &Launcher) -> Result<Vec<Pkg>, String> {
        let out = l.out(&["outdated", "-r"], 120)?;
        Ok(parse_pipes(&out)
            .into_iter()
            .filter(|f| f.len() >= 3)
            .map(|f| {
                let mut p = mk("choco", &f[0], &f[1], "");
                p.latest = f[2].clone();
                p
            })
            .collect())
    }
    fn plan(&self, l: &Launcher, a: Action, id: &str, ver: Option<&str>, _dry: bool) -> Result<Plan, String> {
        let mut v: Vec<&str> = match a {
            Action::Install => vec!["install", id, "-y"],
            Action::Uninstall => vec!["uninstall", id, "-y"],
            Action::Upgrade => vec!["upgrade", id, "-y"],
        };
        if let (Some(ver), true) = (ver, a == Action::Install) {
            v.push("--version");
            v.push(ver);
        }
        Ok(l.plan(&v))
    }
}

// ------------------------------------------------------------------ npm / pnpm / bun

pub struct Npm;

impl Provider for Npm {
    fn id(&self) -> &'static str {
        "npm"
    }
    fn name(&self) -> &'static str {
        "npm"
    }
    fn kind(&self) -> &'static str {
        "dev"
    }
    fn blurb(&self) -> &'static str {
        "node packages, installed globally (-g)"
    }
    fn exes(&self) -> &'static [&'static str] {
        &["npm"]
    }
    fn caps(&self) -> &'static [&'static str] {
        &["search", "list", "outdated", "install", "uninstall", "upgrade"]
    }
    fn search(&self, _l: &Launcher, q: &str) -> Result<Vec<Pkg>, String> {
        npm_registry("npm", q)
    }
    fn list(&self, l: &Launcher) -> Result<Vec<Pkg>, String> {
        let out = l.out(&["ls", "-g", "--depth=0", "--json"], 60)?;
        let v = json::parse(&out)?;
        Ok(v.get("dependencies")
            .map(|d| d.fields().iter().map(|(k, x)| mk("npm", k, &x.text("version"), "")).collect())
            .unwrap_or_default())
    }
    fn outdated(&self, l: &Launcher) -> Result<Vec<Pkg>, String> {
        let out = l.out(&["outdated", "-g", "--json"], 120)?;
        if out.trim().is_empty() {
            return Ok(vec![]);
        }
        let v = json::parse(&out)?;
        Ok(v.fields()
            .iter()
            .map(|(k, x)| {
                let mut p = mk("npm", k, &x.text("current"), "");
                p.latest = x.text("latest");
                p
            })
            .collect())
    }
    fn plan(&self, l: &Launcher, a: Action, id: &str, ver: Option<&str>, _dry: bool) -> Result<Plan, String> {
        Ok(match a {
            Action::Install => l.plan(&["install", "-g", &version_spec("@", id, ver)]),
            Action::Uninstall => l.plan(&["uninstall", "-g", id]),
            Action::Upgrade => l.plan(&["install", "-g", &format!("{}@latest", id)]),
        })
    }
}

pub struct Pnpm;

impl Provider for Pnpm {
    fn id(&self) -> &'static str {
        "pnpm"
    }
    fn name(&self) -> &'static str {
        "pnpm"
    }
    fn kind(&self) -> &'static str {
        "dev"
    }
    fn blurb(&self) -> &'static str {
        "node packages via pnpm's global store"
    }
    fn exes(&self) -> &'static [&'static str] {
        &["pnpm"]
    }
    fn search(&self, _l: &Launcher, q: &str) -> Result<Vec<Pkg>, String> {
        npm_registry("pnpm", q)
    }
    fn list(&self, l: &Launcher) -> Result<Vec<Pkg>, String> {
        let out = l.out(&["ls", "-g", "--depth=0", "--json"], 60)?;
        let v = json::parse(&out)?;
        let mut res = Vec::new();
        let roots: Vec<&J> = if let J::Arr(a) = &v { a.iter().collect() } else { vec![&v] };
        for r in roots {
            if let Some(d) = r.get("dependencies") {
                for (k, x) in d.fields() {
                    res.push(mk("pnpm", k, &x.text("version"), ""));
                }
            }
        }
        Ok(res)
    }
    fn plan(&self, l: &Launcher, a: Action, id: &str, ver: Option<&str>, _dry: bool) -> Result<Plan, String> {
        Ok(match a {
            Action::Install => l.plan(&["add", "-g", &version_spec("@", id, ver)]),
            Action::Uninstall => l.plan(&["remove", "-g", id]),
            Action::Upgrade => l.plan(&["add", "-g", &format!("{}@latest", id)]),
        })
    }
}

pub struct Bun;

impl Provider for Bun {
    fn id(&self) -> &'static str {
        "bun"
    }
    fn name(&self) -> &'static str {
        "Bun"
    }
    fn kind(&self) -> &'static str {
        "dev"
    }
    fn blurb(&self) -> &'static str {
        "npm-registry packages through bun's global install"
    }
    fn exes(&self) -> &'static [&'static str] {
        &["bun"]
    }
    fn search(&self, _l: &Launcher, q: &str) -> Result<Vec<Pkg>, String> {
        npm_registry("bun", q)
    }
    fn list(&self, l: &Launcher) -> Result<Vec<Pkg>, String> {
        let out = l.out(&["pm", "ls", "-g"], 60)?;
        Ok(parse_bun_list(&out).into_iter().map(|(n, v)| mk("bun", &n, &v, "")).collect())
    }
    fn plan(&self, l: &Launcher, a: Action, id: &str, ver: Option<&str>, _dry: bool) -> Result<Plan, String> {
        Ok(match a {
            Action::Install => l.plan(&["add", "-g", &version_spec("@", id, ver)]),
            Action::Uninstall => l.plan(&["remove", "-g", id]),
            Action::Upgrade => l.plan(&["add", "-g", &format!("{}@latest", id)]),
        })
    }
}

// ------------------------------------------------------------------ python: pip / uv / pipx

pub struct Pip;

impl Provider for Pip {
    fn id(&self) -> &'static str {
        "pip"
    }
    fn name(&self) -> &'static str {
        "pip"
    }
    fn kind(&self) -> &'static str {
        "dev"
    }
    fn blurb(&self) -> &'static str {
        "PyPI packages into your default python (name lookup, not fuzzy search)"
    }
    fn exes(&self) -> &'static [&'static str] {
        &["pip", "pip3"]
    }
    fn caps(&self) -> &'static [&'static str] {
        &["search", "list", "outdated", "install", "uninstall", "upgrade"]
    }
    fn probe(&self) -> Option<Launcher> {
        if let Some(p) = self.exes().iter().find_map(|e| which(e)) {
            return Some(Launcher::new(p));
        }
        // no pip on PATH: try `python -m pip`, but verify it. on Windows a bare `python.exe` can be
        // the Microsoft Store stub, which exists without python being installed.
        for e in ["python", "python3", "py"] {
            if let Some(p) = which(e) {
                let mut l = Launcher::new(p);
                l.pre = vec!["-m".into(), "pip".into()];
                if matches!(l.cap(&["--version"], 15), Ok(c) if c.ok) {
                    return Some(l);
                }
            }
        }
        None
    }
    fn search(&self, _l: &Launcher, q: &str) -> Result<Vec<Pkg>, String> {
        pypi_lookup("pip", q)
    }
    fn list(&self, l: &Launcher) -> Result<Vec<Pkg>, String> {
        let out = l.out(&["list", "--format=json", "--disable-pip-version-check"], 60)?;
        Ok(json::parse(&out)?.items().iter().map(|x| mk("pip", &x.text("name"), &x.text("version"), "")).collect())
    }
    fn outdated(&self, l: &Launcher) -> Result<Vec<Pkg>, String> {
        let out = l.out(&["list", "--outdated", "--format=json", "--disable-pip-version-check"], 180)?;
        Ok(json::parse(&out)?
            .items()
            .iter()
            .map(|x| {
                let mut p = mk("pip", &x.text("name"), &x.text("version"), "");
                p.latest = x.text("latest_version");
                p
            })
            .collect())
    }
    fn plan(&self, l: &Launcher, a: Action, id: &str, ver: Option<&str>, _dry: bool) -> Result<Plan, String> {
        Ok(match a {
            Action::Install => l.plan(&["install", &version_spec("==", id, ver)]),
            Action::Uninstall => l.plan(&["uninstall", "-y", id]),
            Action::Upgrade => l.plan(&["install", "--upgrade", id]),
        })
    }
}

pub struct Uv;

impl Provider for Uv {
    fn id(&self) -> &'static str {
        "uv"
    }
    fn name(&self) -> &'static str {
        "uv tool"
    }
    fn kind(&self) -> &'static str {
        "dev"
    }
    fn blurb(&self) -> &'static str {
        "python CLI tools in isolated envs, the fast way"
    }
    fn exes(&self) -> &'static [&'static str] {
        &["uv"]
    }
    fn search(&self, _l: &Launcher, q: &str) -> Result<Vec<Pkg>, String> {
        pypi_lookup("uv", q)
    }
    fn list(&self, l: &Launcher) -> Result<Vec<Pkg>, String> {
        let out = l.out(&["tool", "list"], 60)?;
        Ok(parse_uv_list(&out).into_iter().map(|(n, v)| mk("uv", &n, &v, "")).collect())
    }
    fn plan(&self, l: &Launcher, a: Action, id: &str, ver: Option<&str>, _dry: bool) -> Result<Plan, String> {
        Ok(match a {
            Action::Install => l.plan(&["tool", "install", &version_spec("==", id, ver)]),
            Action::Uninstall => l.plan(&["tool", "uninstall", id]),
            Action::Upgrade => l.plan(&["tool", "upgrade", id]),
        })
    }
}

pub struct Pipx;

impl Provider for Pipx {
    fn id(&self) -> &'static str {
        "pipx"
    }
    fn name(&self) -> &'static str {
        "pipx"
    }
    fn kind(&self) -> &'static str {
        "dev"
    }
    fn blurb(&self) -> &'static str {
        "python CLI apps, one venv each"
    }
    fn exes(&self) -> &'static [&'static str] {
        &["pipx"]
    }
    fn search(&self, _l: &Launcher, q: &str) -> Result<Vec<Pkg>, String> {
        pypi_lookup("pipx", q)
    }
    fn list(&self, l: &Launcher) -> Result<Vec<Pkg>, String> {
        let out = l.out(&["list", "--json"], 60)?;
        let v = json::parse(&out)?;
        Ok(v.get("venvs")
            .map(|vs| {
                vs.fields()
                    .iter()
                    .map(|(k, x)| {
                        let ver = x.get("metadata").and_then(|m| m.get("main_package")).map(|m| m.text("package_version")).unwrap_or_default();
                        mk("pipx", k, &ver, "")
                    })
                    .collect()
            })
            .unwrap_or_default())
    }
    fn plan(&self, l: &Launcher, a: Action, id: &str, ver: Option<&str>, _dry: bool) -> Result<Plan, String> {
        Ok(match a {
            Action::Install => l.plan(&["install", &version_spec("==", id, ver)]),
            Action::Uninstall => l.plan(&["uninstall", id]),
            Action::Upgrade => l.plan(&["upgrade", id]),
        })
    }
}

// ------------------------------------------------------------------ cargo / crates.io

pub struct Cargo;

impl Provider for Cargo {
    fn id(&self) -> &'static str {
        "cargo"
    }
    fn name(&self) -> &'static str {
        "cargo install"
    }
    fn kind(&self) -> &'static str {
        "dev"
    }
    fn blurb(&self) -> &'static str {
        "crates.io binaries, built from source (uses cargo-binstall if present)"
    }
    fn exes(&self) -> &'static [&'static str] {
        &["cargo"]
    }
    fn search(&self, l: &Launcher, q: &str) -> Result<Vec<Pkg>, String> {
        let out = l.out(&["search", q, "--limit", "25"], 60)?;
        Ok(parse_cargo_search(&out).into_iter().map(|(n, v, d)| mk("cargo", &n, &v, &d)).collect())
    }
    fn list(&self, l: &Launcher) -> Result<Vec<Pkg>, String> {
        let out = l.out(&["install", "--list"], 30)?;
        Ok(parse_cargo_list(&out).into_iter().map(|(n, v)| mk("cargo", &n, &v, "")).collect())
    }
    fn plan(&self, l: &Launcher, a: Action, id: &str, ver: Option<&str>, _dry: bool) -> Result<Plan, String> {
        let binstall = which("cargo-binstall").is_some();
        Ok(match a {
            Action::Install | Action::Upgrade => {
                let mut v: Vec<&str> = if binstall { vec!["binstall", "-y", id] } else { vec!["install", id] };
                if a == Action::Upgrade && !binstall {
                    v.push("--force");
                }
                if let (Some(ver), true) = (ver, a == Action::Install) {
                    v.push(if binstall { "--version" } else { "--version" });
                    v.push(ver);
                }
                l.plan(&v)
            }
            Action::Uninstall => l.plan(&["uninstall", id]),
        })
    }
}

// ------------------------------------------------------------------ go install

pub struct Go;

fn go_bin_dir(l: &Launcher) -> Result<PathBuf, String> {
    let gobin = l.out(&["env", "GOBIN"], 15)?.trim().to_string();
    if !gobin.is_empty() {
        return Ok(PathBuf::from(gobin));
    }
    let gopath = l.out(&["env", "GOPATH"], 15)?;
    let first = gopath.trim().split(if cfg!(windows) { ';' } else { ':' }).next().unwrap_or("").to_string();
    if first.is_empty() {
        return Err("GOPATH is empty".into());
    }
    Ok(PathBuf::from(first).join("bin"))
}

/// module path -> binary name (last element, skipping a trailing /vN)
fn go_bin_name(id: &str) -> String {
    let mut parts: Vec<&str> = id.split('/').collect();
    if parts.len() > 1 {
        let last = parts[parts.len() - 1];
        if last.len() > 1 && last.starts_with('v') && last[1..].chars().all(|c| c.is_ascii_digit()) {
            parts.pop();
        }
    }
    let n = parts.last().copied().unwrap_or(id).to_string();
    if cfg!(windows) {
        format!("{}.exe", n)
    } else {
        n
    }
}

fn go_escape(path: &str) -> String {
    path.chars().flat_map(|c| if c.is_ascii_uppercase() { vec!['!', c.to_ascii_lowercase()] } else { vec![c] }).collect()
}

impl Provider for Go {
    fn id(&self) -> &'static str {
        "go"
    }
    fn name(&self) -> &'static str {
        "go install"
    }
    fn kind(&self) -> &'static str {
        "dev"
    }
    fn blurb(&self) -> &'static str {
        "Go programs by module path (search = GitHub repos tagged Go)"
    }
    fn exes(&self) -> &'static [&'static str] {
        &["go"]
    }
    fn version_args(&self) -> &'static [&'static str] {
        &["version"]
    }
    fn search(&self, _l: &Launcher, q: &str) -> Result<Vec<Pkg>, String> {
        let mut out = Vec::new();
        // looks like a module path? ask the proxy directly
        if let Some((host, _)) = q.split_once('/') {
            if host.contains('.') && valid_token(q) {
                if let Ok(Some(v)) = http_json(&format!("https://proxy.golang.org/{}/@latest", go_escape(q)), &[]) {
                    out.push(mk("go", q, &v.text("Version"), "module found on proxy.golang.org"));
                }
            }
        }
        for r in github_search(&format!("{} language:go", q))? {
            let full = r.text("full_name");
            if full.is_empty() {
                continue;
            }
            let desc = format!("★{}  {}", fmt_count(r.get("stargazers_count").map(|n| if let J::Num(n) = n { *n } else { 0.0 }).unwrap_or(0.0)), r.text("description"));
            out.push(mk("go", &format!("github.com/{}", full), "", &desc));
        }
        Ok(out)
    }
    fn list(&self, l: &Launcher) -> Result<Vec<Pkg>, String> {
        let dir = go_bin_dir(l)?;
        let mut out = Vec::new();
        let Ok(rd) = fs::read_dir(&dir) else { return Ok(out) };
        for e in rd.flatten().take(300) {
            let path = e.path();
            if !path.is_file() {
                continue;
            }
            let p = path.to_string_lossy().into_owned();
            let Ok(c) = l.cap(&["version", "-m", &p], 10) else { continue };
            let (mut pkgpath, mut modver) = (String::new(), String::new());
            for line in c.out.lines() {
                let f: Vec<&str> = line.trim().split('\t').collect();
                if f.len() >= 2 && f[0] == "path" {
                    pkgpath = f[1].to_string();
                }
                if f.len() >= 3 && f[0] == "mod" && modver.is_empty() {
                    modver = f[2].to_string();
                }
            }
            if !pkgpath.is_empty() {
                out.push(mk("go", &pkgpath, &modver, ""));
            }
        }
        Ok(out)
    }
    fn plan(&self, l: &Launcher, a: Action, id: &str, ver: Option<&str>, _dry: bool) -> Result<Plan, String> {
        Ok(match a {
            Action::Install | Action::Upgrade => {
                let v = if a == Action::Upgrade { "latest" } else { ver.unwrap_or("latest") };
                l.plan(&["install", &format!("{}@{}", id, v)])
            }
            Action::Uninstall => {
                let f = go_bin_dir(l)?.join(go_bin_name(id));
                if !f.is_file() {
                    return Err(format!("{} not found", f.display()));
                }
                Plan::Remove(f)
            }
        })
    }
}

// ------------------------------------------------------------------ vcpkg

pub struct Vcpkg;

impl Provider for Vcpkg {
    fn id(&self) -> &'static str {
        "vcpkg"
    }
    fn name(&self) -> &'static str {
        "vcpkg"
    }
    fn kind(&self) -> &'static str {
        "dev"
    }
    fn blurb(&self) -> &'static str {
        "C/C++ libraries (needs vcpkg on PATH)"
    }
    fn exes(&self) -> &'static [&'static str] {
        &["vcpkg"]
    }
    fn version_args(&self) -> &'static [&'static str] {
        &["version"]
    }
    fn search(&self, l: &Launcher, q: &str) -> Result<Vec<Pkg>, String> {
        let out = l.out(&["search", q], 120)?;
        Ok(parse_vcpkg(&out).into_iter().map(|(n, v, d)| mk("vcpkg", &n, &v, &d)).collect())
    }
    fn list(&self, l: &Launcher) -> Result<Vec<Pkg>, String> {
        let out = l.out(&["list"], 60)?;
        Ok(parse_vcpkg(&out).into_iter().map(|(n, v, d)| mk("vcpkg", &n, &v, &d)).collect())
    }
    fn plan(&self, l: &Launcher, a: Action, id: &str, _ver: Option<&str>, _dry: bool) -> Result<Plan, String> {
        Ok(match a {
            Action::Install => l.plan(&["install", id]),
            Action::Uninstall => l.plan(&["remove", id]),
            Action::Upgrade => l.plan(&["upgrade", id, "--no-dry-run"]),
        })
    }
}

// ------------------------------------------------------------------ dotnet tools

pub struct Dotnet;

impl Provider for Dotnet {
    fn id(&self) -> &'static str {
        "dotnet"
    }
    fn name(&self) -> &'static str {
        "dotnet tool"
    }
    fn kind(&self) -> &'static str {
        "dev"
    }
    fn blurb(&self) -> &'static str {
        ".NET global tools from NuGet"
    }
    fn exes(&self) -> &'static [&'static str] {
        &["dotnet"]
    }
    fn search(&self, l: &Launcher, q: &str) -> Result<Vec<Pkg>, String> {
        let out = l.out(&["tool", "search", q, "--take", "25"], 60)?;
        let Some(t) = parse_table(&out, &["Package", "Version"]) else { return Ok(vec![]) };
        let (ci, cv, ca) = (t.col(&["Package ID", "Package"]), t.col(&["Latest Version", "Version"]), t.col(&["Authors"]));
        Ok(t.rows.iter().filter(|r| !t.cell(r, ci).is_empty()).map(|r| mk("dotnet", t.cell(r, ci), t.cell(r, cv), t.cell(r, ca))).collect())
    }
    fn list(&self, l: &Launcher) -> Result<Vec<Pkg>, String> {
        let out = l.out(&["tool", "list", "-g"], 60)?;
        let Some(t) = parse_table(&out, &["Package", "Version"]) else { return Ok(vec![]) };
        let (ci, cv) = (t.col(&["Package Id", "Package"]), t.col(&["Version"]));
        Ok(t.rows.iter().filter(|r| !t.cell(r, ci).is_empty()).map(|r| mk("dotnet", t.cell(r, ci), t.cell(r, cv), "")).collect())
    }
    fn plan(&self, l: &Launcher, a: Action, id: &str, ver: Option<&str>, _dry: bool) -> Result<Plan, String> {
        Ok(match a {
            Action::Install => {
                let mut v = vec!["tool", "install", "-g", id];
                if let Some(ver) = ver {
                    v.push("--version");
                    v.push(ver);
                }
                l.plan(&v)
            }
            Action::Uninstall => l.plan(&["tool", "uninstall", "-g", id]),
            Action::Upgrade => l.plan(&["tool", "update", "-g", id]),
        })
    }
}

// ------------------------------------------------------------------ git (GitHub repos)

pub struct Git;

pub fn repos_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("PARCELS_REPOS") {
        return PathBuf::from(d);
    }
    home_dir().unwrap_or_else(|| PathBuf::from(".")).join("parcels").join("repos")
}

fn repo_dir(id: &str) -> Result<PathBuf, String> {
    let (o, r) = valid_repo(id).ok_or("git ids look like owner/repo")?;
    Ok(repos_dir().join(o).join(r))
}

impl Provider for Git {
    fn id(&self) -> &'static str {
        "git"
    }
    fn name(&self) -> &'static str {
        "git clone"
    }
    fn kind(&self) -> &'static str {
        "source"
    }
    fn blurb(&self) -> &'static str {
        "GitHub repos cloned into ~/parcels/repos (set PARCELS_REPOS to move)"
    }
    fn exes(&self) -> &'static [&'static str] {
        &["git"]
    }
    fn search(&self, _l: &Launcher, q: &str) -> Result<Vec<Pkg>, String> {
        let mut out = Vec::new();
        for r in github_search(q)? {
            let full = r.text("full_name");
            if valid_repo(&full).is_none() {
                continue;
            }
            let stars = match r.get("stargazers_count") {
                Some(J::Num(n)) => *n,
                _ => 0.0,
            };
            let pushed: String = r.text("pushed_at").chars().take(10).collect();
            let mut desc = r.text("description");
            if !pushed.is_empty() {
                desc = format!("{}  (pushed {})", desc, pushed);
            }
            out.push(mk("git", &full, &format!("★{}", fmt_count(stars)), &desc));
        }
        Ok(out)
    }
    fn list(&self, l: &Launcher) -> Result<Vec<Pkg>, String> {
        let root = repos_dir();
        let mut out = Vec::new();
        let Ok(owners) = fs::read_dir(&root) else { return Ok(out) };
        for o in owners.flatten() {
            let Ok(repos) = fs::read_dir(o.path()) else { continue };
            for r in repos.flatten() {
                let dir = r.path();
                if !dir.join(".git").exists() {
                    continue;
                }
                let id = format!("{}/{}", o.file_name().to_string_lossy(), r.file_name().to_string_lossy());
                let d = dir.to_string_lossy().into_owned();
                let rev = l.out(&["-C", &d, "rev-parse", "--short", "HEAD"], 10).map(|s| s.trim().to_string()).unwrap_or_default();
                out.push(mk("git", &id, &rev, &d));
            }
        }
        Ok(out)
    }
    fn plan(&self, l: &Launcher, a: Action, id: &str, ver: Option<&str>, dry: bool) -> Result<Plan, String> {
        let dir = repo_dir(id)?;
        let d = dir.to_string_lossy().into_owned();
        Ok(match a {
            Action::Install => {
                if dir.exists() {
                    return Err(format!("{} already exists", d));
                }
                if !dry {
                    if let Some(parent) = dir.parent() {
                        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                    }
                }
                let url = format!("https://github.com/{}.git", id);
                let mut v = vec!["clone", "--depth", "1"];
                if let Some(b) = ver {
                    v.push("--branch");
                    v.push(b);
                }
                v.push(&url);
                v.push(&d);
                l.plan(&v)
            }
            Action::Upgrade => l.plan(&["-C", &d, "pull", "--ff-only"]),
            Action::Uninstall => {
                // refuse to delete anything that isn't inside the repos dir
                let root = fs::canonicalize(repos_dir()).map_err(|e| e.to_string())?;
                let target = fs::canonicalize(&dir).map_err(|e| e.to_string())?;
                if !target.starts_with(&root) || target == root {
                    return Err("refusing to delete outside the repos dir".into());
                }
                Plan::Remove(target)
            }
        })
    }
}

pub fn remove_path(p: &Path) -> Result<(), String> {
    if p.is_dir() {
        fs::remove_dir_all(p).map_err(|e| e.to_string())
    } else {
        fs::remove_file(p).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn go_names() {
        let n = |s: &str| go_bin_name(s).trim_end_matches(".exe").to_string();
        assert_eq!(n("github.com/junegunn/fzf"), "fzf");
        assert_eq!(n("github.com/x/tool/v2"), "tool");
        assert_eq!(n("golang.org/x/tools/cmd/goimports"), "goimports");
    }

    #[test]
    fn go_escape_caps() {
        assert_eq!(go_escape("github.com/BurntSushi/toml"), "github.com/!burnt!sushi/toml");
    }

    #[test]
    fn registry_is_complete_and_unique() {
        let ps = all();
        let mut ids: Vec<&str> = ps.iter().map(|p| p.id()).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n);
        assert!(n >= 14);
    }

    #[test]
    fn plans_preview() {
        let l = Launcher::new(PathBuf::from("/usr/bin/winget"));
        let w = Winget;
        let p = w.plan(&l, Action::Install, "Microsoft.PowerToys", Some("0.90.1"), true).unwrap();
        let s = p.preview();
        assert!(s.starts_with("winget install --id Microsoft.PowerToys -e"));
        assert!(s.ends_with("--version 0.90.1"));
    }

    #[test]
    fn pkg_json_skips_empty() {
        let p = mk("npm", "left-pad", "1.0.0", "");
        let j = p.to_json("result").dump();
        assert!(j.contains("\"version\":\"1.0.0\""));
        assert!(!j.contains("desc"));
    }

    #[test]
    fn git_ids_validated() {
        assert!(repo_dir("../../etc").is_err());
        assert!(repo_dir("a/b").is_ok());
    }
}
