//! End to end: the built binary over temp fixture groves, with sh stubs for `mise`,
//! `clippy-driver` and `rustc` wherever the real tool would touch the machine.

use serde_json as _;

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::{Path, PathBuf};
    use std::process::{Command, id};
    use std::{env, fs};

    /// The canonical table under test.
    const CANON: &str = include_str!("../lints.toml");
    /// A test's fallible outcome.
    type Outcome = Result<(), Box<dyn Error>>;

    /// A fresh temp `$HOME` named for the test.
    fn home(name: &str) -> Result<PathBuf, Box<dyn Error>> {
        let dir = env::temp_dir().join(format!("lints-{name}-{}", id()));
        drop(fs::remove_dir_all(&dir));
        fs::create_dir_all(&dir)?;
        Ok(fs::canonicalize(dir)?)
    }

    /// Writes `text` at `home/path`, creating parents; `mode` 0o755 makes a stub.
    fn put(home: &Path, path: &str, text: &str, mode: u32) -> Outcome {
        let file = home.join(path);
        fs::create_dir_all(file.parent().ok_or("no parent")?)?;
        fs::write(&file, text)?;
        Ok(fs::set_permissions(file, fs::Permissions::from_mode(mode))?)
    }

    /// `lints args` with `$HOME` = `home` and `home/bin` first on `$PATH`: its exit code, a
    /// newline, its stdout.
    fn lints(home: &Path, args: &[&str], envs: &[(&str, &str)]) -> Result<String, Box<dyn Error>> {
        let path = format!("{}:{}", home.join("bin").display(), env::var("PATH")?);
        let out = Command::new(env!("CARGO_BIN_EXE_lints"))
            .args(args)
            .env("HOME", home)
            .env("PATH", path)
            .envs(envs.iter().copied())
            .current_dir(home)
            .output()?;
        Ok(format!(
            "{}\n{}",
            out.status.code().unwrap_or_default(),
            String::from_utf8(out.stdout)?
        ))
    }

    /// `Err` showing both sides unless `got == want`.
    fn same(got: &str, want: &str) -> Outcome {
        if got == want {
            Ok(())
        } else {
            Err(format!("got:\n{got}\nwant:\n{want}").into())
        }
    }

    /// `check` sorts every crate the walk finds into clean, drift and opt-outs.
    #[test]
    fn check() -> Outcome {
        let home = home("check")?;
        let grove = ".grove/code/jgeschwendt";
        let pkg = "[package]\nname = \"x\"\n\n";
        let inherit = format!("{pkg}[lints]\nworkspace = true\n");
        let tuned = CANON
            .replace("unsafe_code = \"forbid\"", "unsafe_code = \"warn\"")
            .replace(
                "[lints.clippy]\n",
                "[lints.clippy]\nbare_allow = \"allow\"\n# a reason\nreasoned = \"allow\"\n",
            );
        for (path, text) in [
            ("bare/main/Cargo.toml", pkg.to_owned()),
            ("bare/main/broken/Cargo.toml", "[package".to_owned()),
            ("bare/main/orphan/Cargo.toml", inherit.clone()),
            ("clean/main/Cargo.toml", format!("{pkg}{CANON}")),
            ("clean/main/target/Cargo.toml", pkg.to_owned()),
            ("drift/main/Cargo.toml", format!("{pkg}{tuned}")),
            (
                "few/main/Cargo.toml",
                format!("{pkg}{}", CANON.replace("unused_results = \"warn\"\n", "")),
            ),
            (
                "ws/main/Cargo.toml",
                format!(
                    "[workspace]\n\n{}",
                    CANON.replace("[lints.", "[workspace.lints.")
                ),
            ),
            ("ws/main/member/Cargo.toml", inherit),
        ] {
            put(&home, &format!("{grove}/{path}"), &text, 0o644)?;
        }
        same(
            &lints(&home, &["check"], &[])?,
            "1
| crate | status | detail |
| --- | --- | --- |
| bare/main | drift | no lints table |
| bare/main/orphan | drift | no lints table |
| clean/main | clean |  |
| drift/main | drift | unsafe_code warn < forbid; clippy::bare_allow allowed without a reason; opt-outs: clippy::reasoned |
| few/main | drift | missing 1: unused_results |
| ws/main/member | clean |  |
",
        )?;
        put(
            &home,
            "elsewhere/Cargo.toml",
            &format!("{pkg}[lints.clippy]\nall = \"warn\"\n"),
            0o644,
        )?;
        same(
            &lints(&home, &["check", "elsewhere"], &[])?
                .starts_with("1\n")
                .to_string(),
            "true",
        )?;
        same(
            &lints(&home, &["check", &format!("{grove}/clean")], &[])?,
            "0\n| crate | status | detail |\n| --- | --- | --- |\n| clean/main | clean |  |\n",
        )
    }

    /// The repo's own Cargo.toml carries lints.toml verbatim.
    #[test]
    fn dogfood() -> Outcome {
        let own: toml::Table =
            fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))?.parse()?;
        same(
            &(own.get("lints") == CANON.parse::<toml::Table>()?.get("lints")).to_string(),
            "true",
        )
    }

    /// `inventory` is clean on the pinned toolchain and lists both diffs against a stub one.
    #[test]
    fn inventory() -> Outcome {
        let home = home("inventory")?;
        same(
            &lints(&home, &["inventory"], &[])?,
            "0\nmissing from lints.toml (0):\nunknown to the toolchain (0):\n",
        )?;
        put(
            &home,
            "bin/clippy-driver",
            "#!/bin/sh\nprintf 'Lint checks provided by rustc:\\n  new-lint  allow  m\\n  unsafe-code  allow  m\\n  gated  allow  m\\nLint groups loaded by this crate:\\n  clippy::all  clippy::a, clippy::b\\nLint checks loaded by this crate:\\n  clippy::a  warn  m\\n  clippy::c  allow  m\\n'\n",
            0o755,
        )?;
        put(
            &home,
            "bin/rustc",
            "#!/bin/sh\necho 'warning: unknown lint: `gated`' >&2\n",
            0o755,
        )?;
        let out = lints(&home, &["inventory"], &[])?;
        same(
            &out.starts_with("1\nmissing from lints.toml (2):\n  clippy::c\n  new_lint\nunknown to the toolchain (").to_string(),
            "true",
        )
    }

    /// `report` imposes the table through trailing clippy args and tabulates per-lint counts;
    /// a failed, killed, timed-out or unspawnable run is a row saying so.
    #[test]
    fn report() -> Outcome {
        let home = home("report")?;
        for name in ["a", "b", "fail", "killed", "slow"] {
            put(
                &home,
                &format!(".grove/code/jgeschwendt/{name}/main/Cargo.toml"),
                "[package]\n",
                0o644,
            )?;
        }
        let message = |code: &str, at: &str| {
            format!(
                r#"{{"reason":"compiler-message","message":{{"code":{{"code":"{code}"}},"rendered":"{at}"}}}}"#
            )
        };
        let canned = [
            message("clippy::x", "1"),
            message("clippy::x", "1"),
            message("clippy::x", "2"),
            message("clippy::y", "1"),
        ];
        put(
            &home,
            "canned",
            &format!("{}\nCompiling\n", canned.join("\n")),
            0o644,
        )?;
        put(
            &home,
            "bin/mise",
            &format!(
                "#!/bin/sh\necho \"$@\" > {0}/args\ncase $PWD in\n  */fail/*) exit 101;;\n  */killed/*) kill -9 $$;;\n  */slow/*) sleep 5;;\n  */b/*) grep -v clippy::y {0}/canned;;\n  *) cat {0}/canned;;\nesac\n",
                home.display()
            ),
            0o755,
        )?;
        same(
            &lints(&home, &["report"], &[("LINTS_TIMEOUT_SECS", "1")])?,
            "0
| lint | total | a/main | b/main | fail/main | killed/main | slow/main |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| clippy::x | 4 | 2 | 2 |  |  |  |
| clippy::y | 1 | 1 |  |  |  |  |

| crate | run |
| --- | --- |
| a/main | ok |
| b/main | ok |
| fail/main | failed (exit 101) |
| killed/main | failed (signal) |
| slow/main | timed out after 1 s |
",
        )?;
        let args = fs::read_to_string(home.join("args"))?;
        same(
            &args.contains("--message-format=json -- -W clippy::all -W clippy::cargo -W clippy::nursery -W clippy::pedantic -W absolute_paths_not_starting_with_crate").to_string(),
            "true",
        )?;
        let out = lints(
            &home,
            &["report", ".grove/code/jgeschwendt/a"],
            &[("PATH", "/nonexistent")],
        )?;
        same(
            out.lines().last().unwrap_or_default(),
            "| a/main | failed (mise not found) |",
        )
    }

    /// An unknown verb prints usage and exits 2.
    #[test]
    fn usage() -> Outcome {
        same(&lints(&home("usage")?, &["nope"], &[])?, "2\n")
    }
}
