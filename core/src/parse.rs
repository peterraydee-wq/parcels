//! parsers for the human-oriented output of package managers that have no JSON mode.
//! kept pure (text in, rows out) so they're unit-testable on any OS.
use crate::util::strip_ansi;

/// strip ansi, undo carriage-return redraws ("spinner\rreal line") and CRLF
pub fn clean_lines(text: &str) -> Vec<String> {
    strip_ansi(text)
        .lines()
        .map(|l| {
            let l = l.trim_end_matches('\r');
            l.rsplit('\r').next().unwrap_or("").trim_end().to_string()
        })
        .collect()
}

pub struct Table {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl Table {
    /// column index: exact (case-insensitive) header match first, then "contains"
    pub fn col(&self, keys: &[&str]) -> Option<usize> {
        for k in keys {
            if let Some(i) = self.headers.iter().position(|h| h.eq_ignore_ascii_case(k)) {
                return Some(i);
            }
        }
        for k in keys {
            let k = k.to_lowercase();
            if let Some(i) = self.headers.iter().position(|h| h.to_lowercase().contains(&k)) {
                return Some(i);
            }
        }
        None
    }
    pub fn cell<'a>(&self, row: &'a [String], i: Option<usize>) -> &'a str {
        i.and_then(|i| row.get(i)).map(|s| s.as_str()).unwrap_or("")
    }
}

fn is_dash_line(l: &str) -> bool {
    let t = l.trim();
    t.len() >= 3 && t.contains('-') && t.chars().all(|c| c == '-' || c == ' ')
}

fn dash_runs(l: &[char]) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut prev = ' ';
    for (i, &c) in l.iter().enumerate() {
        if c == '-' && prev != '-' {
            starts.push(i);
        }
        prev = c;
    }
    starts
}

fn word_starts(l: &[char]) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut prev = ' ';
    for (i, &c) in l.iter().enumerate() {
        if c != ' ' && prev == ' ' {
            starts.push(i);
        }
        prev = c;
    }
    starts
}

fn slice(l: &[char], a: usize, b: Option<usize>) -> String {
    if a >= l.len() {
        return String::new();
    }
    let end = b.unwrap_or(l.len()).min(l.len());
    l[a..end.max(a)].iter().collect::<String>().trim().to_string()
}

/// Fixed-width tables as printed by winget, PowerShell (scoop) and dotnet.
/// Column starts come from the dashed rule when it has several runs (PowerShell style),
/// otherwise from the header's word starts (winget style: one long rule).
/// `must_have` are substrings the header line must contain.
pub fn parse_table(text: &str, must_have: &[&str]) -> Option<Table> {
    let lines = clean_lines(text);
    for i in 0..lines.len().saturating_sub(1) {
        let h = &lines[i];
        let d = &lines[i + 1];
        if !is_dash_line(d) || !must_have.iter().all(|m| h.contains(m)) {
            continue;
        }
        let hc: Vec<char> = h.chars().collect();
        let dc: Vec<char> = d.chars().collect();
        let runs = dash_runs(&dc);
        let starts = if runs.len() >= 2 { runs } else { word_starts(&hc) };
        if starts.len() < 2 {
            continue;
        }
        let headers: Vec<String> = (0..starts.len())
            .map(|k| slice(&hc, starts[k], starts.get(k + 1).copied()))
            .collect();
        let mut rows = Vec::new();
        for l in &lines[i + 2..] {
            if l.trim().is_empty() {
                break;
            }
            let lc: Vec<char> = l.chars().collect();
            rows.push(
                (0..starts.len())
                    .map(|k| slice(&lc, starts[k], starts.get(k + 1).copied()))
                    .collect(),
            );
        }
        return Some(Table { headers, rows });
    }
    None
}

/// `id|version` (choco -r); returns (id, rest-of-fields)
pub fn parse_pipes(text: &str) -> Vec<Vec<String>> {
    clean_lines(text)
        .into_iter()
        .filter(|l| l.contains('|'))
        .map(|l| l.split('|').map(|s| s.trim().to_string()).collect::<Vec<_>>())
        .filter(|f| f.len() >= 2 && !f[0].is_empty() && !f[0].contains(' '))
        .collect()
}

/// `cargo search` lines:  name = "1.2.3"    # description
pub fn parse_cargo_search(text: &str) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for l in clean_lines(text) {
        let Some((name, rest)) = l.split_once(" = \"") else { continue };
        if name.contains(' ') {
            continue;
        }
        let Some((ver, tail)) = rest.split_once('"') else { continue };
        let desc = tail.trim().trim_start_matches('#').trim().to_string();
        out.push((name.to_string(), ver.to_string(), desc));
    }
    out
}

/// `cargo install --list`:  name v1.2.3:   (followed by indented binary names)
pub fn parse_cargo_list(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for l in clean_lines(text) {
        if l.starts_with(char::is_whitespace) || l.is_empty() {
            continue;
        }
        let mut it = l.split_whitespace();
        let (Some(name), Some(ver)) = (it.next(), it.next()) else { continue };
        if let Some(v) = ver.strip_prefix('v') {
            out.push((name.to_string(), v.trim_end_matches(':').to_string()));
        }
    }
    out
}

/// vcpkg search / list: `name  version  description…`
pub fn parse_vcpkg(text: &str) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for l in clean_lines(text) {
        if l.starts_with(char::is_whitespace) {
            continue;
        }
        let mut it = l.split_whitespace();
        let (Some(name), Some(ver)) = (it.next(), it.next()) else { continue };
        if !ver.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) || name.contains('[') {
            continue;
        }
        let desc = it.collect::<Vec<_>>().join(" ");
        out.push((name.to_string(), ver.to_string(), desc));
    }
    out
}

/// `uv tool list`:  name v1.2.3  (followed by "- exe" lines)
pub fn parse_uv_list(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for l in clean_lines(text) {
        if l.starts_with('-') || l.starts_with(char::is_whitespace) || l.is_empty() {
            continue;
        }
        let mut it = l.split_whitespace();
        let (Some(name), Some(ver)) = (it.next(), it.next()) else { continue };
        if let Some(v) = ver.strip_prefix('v') {
            out.push((name.to_string(), v.to_string()));
        }
    }
    out
}

/// `bun pm ls -g`:  ├── name@1.2.3
pub fn parse_bun_list(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for l in clean_lines(text) {
        let Some((_, rest)) = l.split_once("── ") else { continue };
        let rest = rest.trim();
        if let Some(i) = rest.rfind('@') {
            if i > 0 {
                out.push((rest[..i].to_string(), rest[i + 1..].to_string()));
            }
        }
    }
    out
}

/// old scoop search format:   "    name (1.2.3) --> includes 'x.exe'"
pub fn parse_scoop_old(text: &str) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let mut bucket = String::new();
    for l in clean_lines(text) {
        if l.starts_with('\'') && l.contains("bucket") {
            bucket = l.trim_start_matches('\'').split('\'').next().unwrap_or("").to_string();
            continue;
        }
        if !l.starts_with("    ") {
            continue;
        }
        let t = l.trim();
        if let Some((name, rest)) = t.split_once(" (") {
            let ver = rest.split(')').next().unwrap_or("").to_string();
            out.push((name.to_string(), ver, bucket.clone()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINGET_SEARCH: &str = "\
Name                Id                        Version Match               Source
--------------------------------------------------------------------------------
PowerToys (Preview) Microsoft.PowerToys       0.90.1  Moniker: powertoys  winget
Windows Terminal    Microsoft.WindowsTerminal 1.22.1  Tag: terminal       winget
";

    #[test]
    fn winget_table() {
        let t = parse_table(WINGET_SEARCH, &["Id", "Version"]).unwrap();
        let id = t.col(&["Id"]);
        let ver = t.col(&["Version"]);
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.cell(&t.rows[0], id), "Microsoft.PowerToys");
        assert_eq!(t.cell(&t.rows[1], ver), "1.22.1");
        assert_eq!(t.cell(&t.rows[0], t.col(&["Name"])), "PowerToys (Preview)");
        assert_eq!(t.cell(&t.rows[1], t.col(&["Source"])), "winget");
    }

    #[test]
    fn winget_with_spinner_junk() {
        let junk = format!("   - \r   \\ \r   | \r{}", WINGET_SEARCH.replace('\n', "\r\n"));
        let t = parse_table(&junk, &["Id", "Version"]);
        assert!(t.is_some());
        assert_eq!(t.unwrap().rows.len(), 2);
    }

    #[test]
    fn winget_upgrade_footer() {
        let s = "\
Name  Id         Version Available Source
-----------------------------------------
Git   Git.Git    2.44.0  2.45.1    winget
";
        let t = parse_table(s, &["Id", "Available"]).unwrap();
        assert_eq!(t.cell(&t.rows[0], t.col(&["Available"])), "2.45.1");
    }

    #[test]
    fn powershell_table_multiword_headers() {
        let s = "\
Name      Installed Version Latest Version Missing Dependencies Info
----      ----------------- -------------- -------------------- ----
git       2.44.0            2.45.1
ripgrep   14.0.0            14.1.0
";
        let t = parse_table(s, &["Name", "Latest"]).unwrap();
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.cell(&t.rows[1], t.col(&["Installed Version"])), "14.0.0");
        assert_eq!(t.cell(&t.rows[1], t.col(&["Latest Version"])), "14.1.0");
    }

    #[test]
    fn scoop_search() {
        let s = "\
Results from local buckets...

Name        Version Source Binaries
----        ------- ------ --------
ripgrep     14.1.0  main
ripgrep-bin 14.1.0  extras rg.exe
";
        let t = parse_table(s, &["Name", "Version"]).unwrap();
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.cell(&t.rows[1], t.col(&["Source"])), "extras");
    }

    #[test]
    fn no_table() {
        assert!(parse_table("No package found matching input criteria.", &["Id"]).is_none());
    }

    #[test]
    fn choco() {
        let r = parse_pipes("Chocolatey v2.2.2\ngit|2.44.0\nnodejs|22.1.0\n");
        assert_eq!(r.len(), 2);
        assert_eq!(r[1][0], "nodejs");
        let o = parse_pipes("git|2.44.0|2.45.1|false\n");
        assert_eq!(o[0][2], "2.45.1");
    }

    #[test]
    fn cargo() {
        let s = "ripgrep = \"14.1.0\"    # Fast line-oriented search tool\nfd-find = \"10.1.0\"  # A simple, fast alternative to find\n... and 1234 crates more (use --limit N to see more)\n";
        let r = parse_cargo_search(s);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].0, "ripgrep");
        assert_eq!(r[0].1, "14.1.0");
        assert!(r[1].2.starts_with("A simple"));
        let l = parse_cargo_list("ripgrep v14.1.0:\n    rg\nbat v0.24.0 (https://github.com/x/y#abc):\n    bat\n");
        assert_eq!(l, vec![("ripgrep".to_string(), "14.1.0".to_string()), ("bat".to_string(), "0.24.0".to_string())]);
    }

    #[test]
    fn vcpkg() {
        let s = "zlib                 1.3.1            A compression library\ncurl[brotli]         Support HTTP compression\nboost-algorithm  1.84.0#1  Boost algorithm module\n";
        let r = parse_vcpkg(s);
        assert_eq!(r.len(), 2);
        assert_eq!(r[1].1, "1.84.0#1");
        let l = parse_vcpkg("zlib:x64-windows    1.3.1    A compression library\n");
        assert_eq!(l[0].0, "zlib:x64-windows");
    }

    #[test]
    fn uv_and_bun() {
        let r = parse_uv_list("black v24.2.0\n- black\n- blackd\nruff v0.4.1\n- ruff\n");
        assert_eq!(r.len(), 2);
        let b = parse_bun_list("/home/x/.bun/install/global node_modules (2)\n├── @scope/pkg@1.0.0\n└── typescript@5.4.5\n");
        assert_eq!(b[0], ("@scope/pkg".to_string(), "1.0.0".to_string()));
        assert_eq!(b[1].0, "typescript");
    }

    #[test]
    fn scoop_old() {
        let s = "'main' bucket:\n    git (2.44.0) --> includes 'git.exe'\n    gitui (0.26.1)\n";
        let r = parse_scoop_old(s);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0], ("git".to_string(), "2.44.0".to_string(), "main".to_string()));
    }
}
