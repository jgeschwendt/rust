//! `lints` holds every jgeschwendt crate to one canonical `[lints]` table: `check` diffs
//! each crate's table against it, `report` counts what clippy finds under it, and
//! `inventory` diffs it against the pinned toolchain's lint lists.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read as _;
use std::iter::once;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio, id};
use std::time::{Duration, Instant};
use std::{env, fs, thread};

/// The canonical table, verbatim as a crate pastes it into its Cargo.toml.
const CANON: &str = include_str!("../lints.toml");
/// Cargo's lint levels with their rustc flags, weakest first: the index is the strength.
const LEVELS: [(&str, &str); 4] = [
    ("allow", "-A"),
    ("warn", "-W"),
    ("deny", "-D"),
    ("forbid", "-F"),
];
/// Directory names the crate walk never enters.
const SKIP: [&str; 6] = [
    ".archive",
    ".git",
    "_build",
    "deps",
    "node_modules",
    "target",
];

/// A flattened `[lints]` table: `name → value`, tool lints prefixed `<tool>::`.
type Lints = BTreeMap<String, toml::Value>;

/// Flattens a `[lints]` value; `workspace = true` and other non-tables drop out.
fn flat(lints: Option<&toml::Value>) -> Lints {
    lints
        .and_then(toml::Value::as_table)
        .into_iter()
        .flatten()
        .filter_map(|(tool, table)| Some((tool, table.as_table()?)))
        .flat_map(|(tool, table)| {
            table.iter().map(move |(name, value)| {
                let key = if tool == "rust" {
                    name.clone()
                } else {
                    format!("{tool}::{name}")
                };
                (key, value.clone())
            })
        })
        .collect()
}

/// A lint value's strength: its index in [`LEVELS`].
fn rank(value: &toml::Value) -> usize {
    let level = value.get("level").unwrap_or(value).as_str();
    LEVELS
        .iter()
        .position(|&(known, _)| Some(known) == level)
        .unwrap_or_default()
}

/// Reads and parses a TOML file; unreadable or malformed reads as `None`.
fn read(path: &Path) -> Option<(String, toml::Table)> {
    let text = fs::read_to_string(path).ok()?;
    let table = text.parse().ok()?;
    Some((text, table))
}

/// Pushes every `Cargo.toml` with a `[package]` under `dir`, skipping [`SKIP`] and symlinks.
fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            if !SKIP.iter().any(|skip| name == *skip) {
                walk(&path, found);
            }
        } else if name == "Cargo.toml"
            && read(&path).is_some_and(|manifest| manifest.1.contains_key("package"))
        {
            found.push(fs::canonicalize(&path).unwrap_or(path));
        } else {
            // not a package manifest
        }
    }
}

/// Runs `program args` in `dir` in its own process group, killed after `timeout`:
/// `(None, _)` when it timed out, else its outcome and stdout then stderr.
fn run(program: &str, args: &[String], dir: &Path, timeout: Duration) -> (Option<String>, String) {
    let spawned = Command::new(program)
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0i32)
        .spawn();
    let Ok(mut child) = spawned else {
        return (Some(format!("failed ({program} not found)")), String::new());
    };
    let pipes = (child.stdout.take(), child.stderr.take());
    let reader = thread::spawn(move || {
        let mut text = String::new();
        let read_out = pipes.0.map(|mut pipe| pipe.read_to_string(&mut text));
        let read_err = pipes.1.map(|mut pipe| pipe.read_to_string(&mut text));
        drop((read_out, read_err));
        text
    });
    let started = Instant::now();
    let outcome = loop {
        if let Ok(Some(status)) = child.try_wait() {
            break Some(match status.code() {
                Some(0i32) => "ok".to_owned(),
                Some(code) => format!("failed (exit {code})"),
                None => "failed (signal)".to_owned(),
            });
        }
        if started.elapsed() > timeout {
            drop(
                Command::new("kill")
                    .args(["-KILL", &format!("-{}", child.id())])
                    .status(),
            );
            break None;
        }
        thread::sleep(Duration::from_millis(50));
    };
    (outcome, reader.join().unwrap_or_default())
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let verb = args.next().unwrap_or_default();
    let canon = flat(
        CANON
            .parse::<toml::Table>()
            .unwrap_or_default()
            .get("lints"),
    );
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join(".grove/code/jgeschwendt");
    let grove = fs::canonicalize(&home).unwrap_or(home);
    let mut dirs: Vec<PathBuf> = args.map(PathBuf::from).collect();
    if dirs.is_empty() {
        dirs = fs::read_dir(&grove)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path().join("main"))
            .collect();
    }
    let mut manifests = Vec::new();
    for dir in &dirs {
        walk(dir, &mut manifests);
    }
    manifests.sort();
    manifests.dedup();
    let labels: Vec<String> = manifests
        .iter()
        .map(|manifest| {
            let dir = manifest.parent().unwrap_or(manifest);
            dir.strip_prefix(&grove)
                .unwrap_or(dir)
                .display()
                .to_string()
        })
        .collect();
    match verb.as_str() {
        "check" => {
            let mut drift = false;
            println!("| crate | status | detail |\n| --- | --- | --- |");
            for (manifest, label) in manifests.iter().zip(&labels) {
                let (own_text, table) = read(manifest).unwrap_or_default();
                let lints = table.get("lints");
                let inherits = lints
                    .and_then(|value| value.get("workspace"))
                    .and_then(toml::Value::as_bool)
                    == Some(true);
                let (text, prefix, own) = if inherits {
                    manifest
                        .ancestors()
                        .skip(1)
                        .find_map(|dir| {
                            read(&dir.join("Cargo.toml"))
                                .filter(|root| root.1.contains_key("workspace"))
                        })
                        .map_or_else(Default::default, |(root_text, root)| {
                            (
                                root_text,
                                "workspace.",
                                flat(
                                    root.get("workspace")
                                        .and_then(|workspace| workspace.get("lints")),
                                ),
                            )
                        })
                } else {
                    (own_text, "", flat(lints))
                };
                let mut missing = Vec::new();
                let mut problems = Vec::new();
                let mut opt_outs = Vec::new();
                for name in canon
                    .keys()
                    .chain(own.keys().filter(|name| !canon.contains_key(*name)))
                {
                    let want = canon.get(name).map_or(0, rank);
                    match own.get(name).map(rank) {
                        None if want > 0 => missing.push(name.as_str()),
                        Some(0) if want > 0 || !canon.contains_key(name) => {
                            let (tool, key) = name.split_once("::").unwrap_or(("rust", name));
                            let header = format!("[{prefix}lints.{tool}]");
                            let body: Vec<&str> = text
                                .lines()
                                .skip_while(|line| line.trim() != header)
                                .skip(1)
                                .take_while(|line| !line.starts_with('['))
                                .collect();
                            if body.windows(2).any(|pair| matches!(pair, [above, line] if above.trim_start().starts_with('#') && line.split('=').next().map(str::trim) == Some(key))) {
                                opt_outs.push(name.as_str());
                            } else {
                                problems.push(format!("{name} allowed without a reason"));
                            }
                        }
                        Some(have) if have < want => {
                            let level =
                                |index: usize| LEVELS.get(index).map_or("", |&(level, _)| level);
                            problems.push(format!("{name} {} < {}", level(have), level(want)));
                        }
                        _ => {}
                    }
                }
                let shown: Vec<&str> = missing.iter().copied().take(3).collect();
                let more = if missing.len() > shown.len() {
                    ", \u{2026}"
                } else {
                    ""
                };
                let head = if own.is_empty() {
                    "no lints table".to_owned()
                } else {
                    format!("missing {}: {}{more}", missing.len(), shown.join(", "))
                };
                let mut detail: Vec<String> = (own.is_empty() || !missing.is_empty())
                    .then_some(head)
                    .into_iter()
                    .chain(problems)
                    .collect();
                let status = if detail.is_empty() { "clean" } else { "drift" };
                drift |= !detail.is_empty();
                detail.extend(
                    (!opt_outs.is_empty()).then(|| format!("opt-outs: {}", opt_outs.join(", "))),
                );
                println!("| {label} | {status} | {} |", detail.join("; "));
            }
            ExitCode::from(u8::from(drift))
        }
        "report" => {
            let timeout: u64 = env::var("LINTS_TIMEOUT_SECS")
                .ok()
                .and_then(|secs| secs.parse().ok())
                .unwrap_or(900);
            let mut flags: Vec<(i64, &String, &str)> = canon
                .iter()
                .map(|(name, value)| {
                    let priority = value
                        .get("priority")
                        .and_then(toml::Value::as_integer)
                        .unwrap_or_default();
                    (
                        priority,
                        name,
                        LEVELS.get(rank(value)).map_or("", |&(_, flag)| flag),
                    )
                })
                .collect();
            flags.sort_unstable();
            let command: Vec<String> = [
                "x",
                "-C",
                ".",
                "--",
                "cargo",
                "clippy",
                "--all-targets",
                "--no-deps",
                "--message-format=json",
                "--",
            ]
            .into_iter()
            .map(str::to_owned)
            .chain(
                flags
                    .into_iter()
                    .flat_map(|(_, name, flag)| [flag.to_owned(), name.clone()]),
            )
            .collect();
            let mut hits = Vec::new();
            let mut runs = Vec::new();
            for (index, manifest) in manifests.iter().enumerate() {
                let (outcome, out) = run(
                    "mise",
                    &command,
                    manifest.parent().unwrap_or(manifest),
                    Duration::from_secs(timeout),
                );
                for message in out
                    .lines()
                    .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                {
                    if let Some(code) = message
                        .pointer("/message/code/code")
                        .and_then(serde_json::Value::as_str)
                    {
                        hits.push((
                            code.to_owned(),
                            index,
                            message
                                .pointer("/message/rendered")
                                .map(ToString::to_string),
                        ));
                    }
                }
                runs.push(outcome.unwrap_or_else(|| format!("timed out after {timeout} s")));
            }
            hits.sort_unstable();
            hits.dedup();
            println!(
                "| lint | total | {} |\n| --- | ---: |{}",
                labels.join(" | "),
                " ---: |".repeat(labels.len())
            );
            let lints: BTreeSet<&String> = hits.iter().map(|hit| &hit.0).collect();
            let mut rows: Vec<(usize, &String, Vec<usize>)> = lints
                .into_iter()
                .map(|lint| {
                    let per: Vec<usize> = (0..labels.len())
                        .map(|index| {
                            hits.iter()
                                .filter(|hit| hit.0 == *lint && hit.1 == index)
                                .count()
                        })
                        .collect();
                    (hits.iter().filter(|hit| hit.0 == *lint).count(), lint, per)
                })
                .collect();
            rows.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(right.1)));
            for (total, lint, per) in rows {
                let cells: Vec<String> = per
                    .iter()
                    .map(|&count| {
                        if count > 0 {
                            count.to_string()
                        } else {
                            String::new()
                        }
                    })
                    .collect();
                println!("| {lint} | {total} | {} |", cells.join(" | "));
            }
            let table: Vec<String> = labels
                .iter()
                .zip(runs)
                .map(|(label, outcome)| format!("| {label} | {outcome} |"))
                .collect();
            println!("\n| crate | run |\n| --- | --- |\n{}", table.join("\n"));
            ExitCode::SUCCESS
        }
        "inventory" => {
            let minute = Duration::from_secs(60);
            let (_, help) = run(
                "clippy-driver",
                &["-W".to_owned(), "help".to_owned()],
                Path::new("."),
                minute,
            );
            let mut section = "";
            let rows: Vec<(&str, String, Vec<String>)> = help
                .lines()
                .filter_map(|line| {
                    section = if line.starts_with("Lint ") {
                        line
                    } else {
                        section
                    };
                    let mut words = line
                        .split_whitespace()
                        .map(|word| word.trim_end_matches(',').replace('-', "_"));
                    Some((section, words.next()?, words.collect()))
                })
                .collect();
            let defaults: BTreeMap<&String, &String> = rows
                .iter()
                .filter_map(|row| {
                    Some((&row.1, row.2.first()?)).filter(|_| row.0.contains("checks"))
                })
                .collect();
            let groups: BTreeMap<&String, &Vec<String>> = rows
                .iter()
                .filter(|row| row.0.contains("groups"))
                .map(|row| (&row.1, &row.2))
                .collect();
            let tmp = env::temp_dir();
            let rustc: Vec<String> = ["-", "--crate-type=lib", "--emit=metadata", "-o"]
                .into_iter()
                .map(str::to_owned)
                .chain(once(
                    tmp.join(format!("lints-probe-{}.rmeta", id()))
                        .display()
                        .to_string(),
                ))
                .chain(
                    defaults
                        .iter()
                        .filter(|&(name, default)| *default == "allow" && !name.contains("::"))
                        .flat_map(|(name, _)| ["-W".to_owned(), (*name).clone()]),
                )
                .collect();
            let (_, probed) = run("rustc", &rustc, &tmp, minute);
            let unstable: BTreeSet<&str> = probed
                .lines()
                .filter(|line| {
                    line.starts_with("warning: unknown lint") || line.contains("has been renamed")
                })
                .filter_map(|line| line.split('`').nth(1))
                .collect();
            let covered: BTreeSet<&String> = canon
                .keys()
                .filter_map(|name| groups.get(name))
                .copied()
                .flatten()
                .collect();
            let missing: Vec<&String> = defaults
                .iter()
                .filter(|&(name, default)| {
                    !canon.contains_key(*name)
                        && if name.contains("::") {
                            !covered.contains(*name)
                        } else {
                            *default == "allow" && !unstable.contains(name.as_str())
                        }
                })
                .map(|(&name, _)| name)
                .collect();
            let unknown: Vec<&String> = canon
                .keys()
                .filter(|&name| {
                    unstable.contains(name.as_str())
                        || !(defaults.contains_key(name) || groups.contains_key(name))
                })
                .collect();
            for (title, names) in [
                ("missing from lints.toml", &missing),
                ("unknown to the toolchain", &unknown),
            ] {
                let list: Vec<String> = once(format!("{title} ({}):", names.len()))
                    .chain(names.iter().map(|name| format!("  {name}")))
                    .collect();
                println!("{}", list.join("\n"));
            }
            ExitCode::from(u8::from(!(missing.is_empty() && unknown.is_empty())))
        }
        _ => {
            eprintln!("usage: lints check [<dir>...] | lints report [<dir>...] | lints inventory");
            ExitCode::from(2)
        }
    }
}
