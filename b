#!/usr/bin/env python3

import argparse
import fnmatch
import json
import os
from pathlib import Path
import shutil
import shlex
import subprocess
import sys
import tempfile

# Define application identifiers for normal and debug mode
APP_ID = "io.github.hrniels.Eventix"
APP_ID_DEBUG = APP_ID + "-debug"

PY_VENV = Path("run/venv")


def dev_env():
    """Sets up the development environment by configuring environment variables
    and creating symbolic links for required directories."""
    env = os.environ.copy()
    run_dir = Path("run")
    os.makedirs(run_dir / APP_ID_DEBUG, 0o700, exist_ok=True)

    # (re-)create symlinks to data/static and data/icons
    # we use symlinks here so that `./b watch` sees changes to these files
    dirs = ["static", "icons", "locale"]
    for dirname in dirs:
        dir_in_run = run_dir / APP_ID_DEBUG / dirname
        if dir_in_run.exists():
            if dir_in_run.is_file() or dir_in_run.is_symlink():
                os.unlink(dir_in_run)
            else:
                shutil.rmtree(dir_in_run, ignore_errors=True)
        os.symlink((Path("data") / dirname).absolute(),
                   dir_in_run,
                   target_is_directory=True)

    # Add DavMail binary to PATH for subprocess usage
    davmail_bin = os.path.abspath("contrib/davmail/dist")
    if not os.path.isfile(davmail_bin + "/davmail"):
        sys.exit("Please install davmail first via ./b davmail")
    vdirsyncer_bin = PY_VENV / "bin"
    if not os.path.isfile(vdirsyncer_bin / "vdirsyncer"):
        sys.exit("Please install vdirsyncer first via ./b vdirsyncer")
    eventix_bin = os.path.abspath("target/debug")
    env["PATH"] = os.pathsep.join([davmail_bin, str(vdirsyncer_bin.absolute()), eventix_bin, env.get("PATH", "")])
    # use a project-local directory for data and config
    env["XDG_DATA_HOME"] = str(run_dir.absolute())
    env["XDG_CONFIG_HOME"] = str(run_dir.absolute())
    # for debugging
    env["RUST_LOG"] = "trace"
    env["RUST_BACKTRACE"] = "full"
    return env


def run_cmd(args):
    """Executes a command with the prepared development environment."""
    try:
        subprocess.run(args, env=dev_env())
    except KeyboardInterrupt:
        pass
    except Exception as e:
        print(e)


def cmd_run(args):
    """Runs the Eventix application in development mode."""
    cmd_args = [
        "cargo", "run", "--bin", "eventix", "--",
        "--address", args.address,
        "--port", str(args.port)
    ]
    run_cmd(cmd_args)


def cmd_watch(args):
    """Watches for changes in the source code and reruns Eventix on changes."""
    cmd = shlex.join([
        "run", "--",
        "--address", args.address,
        "--port", str(args.port)
    ])
    cmd_args = [
        "cargo", "watch", "-C", "bin/eventix",
        "-w", "../../bin/eventix",
        "-w", "../../bin/build.rs",
        "-w", "../../libs",
        "-w", "../../data",
        "-w", "../../Cargo.toml",
        "-x", cmd
    ]
    run_cmd(cmd_args)


def cmd_app(args):
    """Runs the Eventix app."""
    cmd_args = [
        "cargo", "run", "--bin", "eventix-app", "--",
        "--address", args.address,
        "--port", str(args.port)
    ]
    run_cmd(cmd_args)


def cmd_import(args):
    """Imports an ICS file into Eventix."""
    path = Path(args.file).resolve().as_uri()
    cmd_args = ["cargo", "run", "--bin", "eventix-import", "--", path]
    run_cmd(cmd_args)


def cmd_davmail(args):
    """Builds Davmail using Maven and Ant."""
    subprocess.run(["mvn", "install"], cwd='contrib/davmail', check=True)
    subprocess.run(["ant", "dist"], cwd='contrib/davmail', check=True)


def cmd_vdirsyncer(args):
    """Builds vdirsyncer using venv and pip."""
    subprocess.run(["python", "-m", "venv", str(PY_VENV)])
    subprocess.run([str(PY_VENV / "bin/pip"), "install", "-e", "contrib/vdirsyncer"])


def cmd_test(args):
    """Runs cargo tests with the prepared development environment."""
    cmd = ["cargo", "test"]
    cmd.extend(args.cargo_args)
    if args.nocapture:
        cmd.extend(["--", "--nocapture"])
    subprocess.run(cmd, env=dev_env(), check=True)


def cmd_coverage(args):
    """Generates code coverage information for the workspace."""
    cmd = [
        "cargo", "llvm-cov",
        "--workspace",
        "--exclude", "eventix-import",
        "--exclude", "eventix-app",
        "--exclude", "evlist"
    ]

    if not args.file:
        subprocess.run(cmd, check=True)
        return

    with tempfile.TemporaryDirectory() as tmpdir:
        report_path = Path(tmpdir) / "coverage.json"
        subprocess.run(cmd + ["--json", "--output-path", str(report_path)], check=True)
        report = json.loads(report_path.read_text())
    covered_files = find_covered_files(report, args.file)
    for idx, covered_file in enumerate(covered_files):
        if idx > 0:
            print()
        print_file_coverage(covered_file)


def find_covered_files(report, requested_pattern):
    """Returns coverage entries for all files whose paths contain the requested pattern."""
    requested = requested_pattern.replace("\\", "/")
    is_glob = any(ch in requested for ch in "*?[]")
    matches = []
    for data in report.get("data", []):
        for file_data in data.get("files", []):
            filename = file_data["filename"].replace("\\", "/")
            if fnmatch.fnmatch(filename, requested) or (
                not is_glob and requested in filename
            ):
                matches.append(file_data)

    if not matches:
        sys.exit(f"No coverage data found for pattern '{requested_pattern}'.")

    return sorted(matches, key=lambda file_data: file_data["filename"])


def build_line_coverage(file_data, line_count):
    """Builds a line-to-execution-count map from LLVM segment coverage data."""
    coverage = {}
    segments = file_data.get("segments", [])
    for idx, segment in enumerate(segments):
        line, _col, count, has_count, _is_region_entry, is_gap_region = segment
        if not has_count or is_gap_region or line > line_count:
            continue

        next_line = line_count + 1
        if idx + 1 < len(segments):
            next_line = segments[idx + 1][0]

        last_line = line if next_line == line else min(next_line - 1, line_count)
        for current in range(line, last_line + 1):
            prev = coverage.get(current)
            coverage[current] = count if prev is None else max(prev, count)

    return coverage


def print_file_coverage(file_data):
    """Prints line-by-line coverage for a single source file."""
    path = Path(file_data["filename"])
    lines = path.read_text().splitlines()
    coverage = build_line_coverage(file_data, len(lines))

    print(f"\033[1m{path}\033[0m")
    for idx, line in enumerate(lines, start=1):
        count = coverage.get(idx)
        if count is None:
            marker = " " * 7
        elif count == 0:
            marker = "#####  "
        else:
            marker = f"{count:>7}"
        print(f"{marker} {idx:>5} | {line}")


NPM_PREFIX = Path("target")
PRETTIER = ["npx", "--prefix", str(NPM_PREFIX), "prettier"]


def _ensure_npm_deps():
    """Installs npm dependencies into target/node_modules if not already present.

    Uses ``npm install --prefix target`` so that node_modules stays out of the
    repository root. A symlink from target/package.json to the root package.json
    is created first so that npm can locate the dependency list.
    """
    NPM_PREFIX.mkdir(exist_ok=True)
    pkg_link = NPM_PREFIX / "package.json"
    if not pkg_link.exists():
        pkg_link.symlink_to("../package.json")
    if not (NPM_PREFIX / "node_modules").exists():
        subprocess.run(["npm", "install", "--prefix", str(NPM_PREFIX)], check=True)


def cmd_format(args):
    """Formats Rust, JS, CSS, and HTML template files."""
    _ensure_npm_deps()
    subprocess.run(["cargo", "fmt"])
    subprocess.run(["yamlfmt", "-conf", ".yamlfmt.yaml", ".github"])
    subprocess.run(PRETTIER + ["--write",
                               "data/static/**/*.js",
                               "data/static/style.css",
                               "bin/eventix/templates/**/*.htm"], check=True)


def cmd_format_check(args):
    """Checks Rust, JS, CSS, and HTML template files (exits non-zero on diff)."""
    _ensure_npm_deps()
    subprocess.run(["cargo", "fmt", "--", "--check"])
    subprocess.run(PRETTIER + ["--check",
                               "data/static/**/*.js",
                               "data/static/style.css",
                               "bin/eventix/templates/**/*.htm"], check=True)


def cmd_flatpak(args):
    """Builds a Flatpak package for Eventix, including dependencies."""
    state_dir = Path("flatpak/state")
    repo_dir = Path("flatpak/repo")
    srclist_dir = Path("flatpak/srclists")
    javadeps_dir = Path("flatpak/java-deps")
    sdk_id = "org.gnome.Sdk//50"
    srclist_dir.mkdir(exist_ok=True)

    # Ensure cargo-sources.json is up to date
    venv_bin = PY_VENV / "bin"
    subprocess.run(["python", "-m", "venv", str(PY_VENV)])
    subprocess.run([
        venv_bin / "pip",
        "install", "aiohttp", "tomlkit", "requirements-parser", "packaging"
    ], check=True)
    subprocess.run([
        str(venv_bin / "python"), "contrib/flatpak-cargo-generator.py",
        "Cargo.lock", "-o", str(srclist_dir / "cargo-sources.json")
    ], check=True)

    # download vdirsyncer dependencies and generate source list
    subprocess.run([
        str(venv_bin / "python"), "contrib/flatpak-pip-generator.py",
        "--output", str(srclist_dir / "python-sources"),
        "--pyproject-file", "contrib/vdirsyncer/pyproject.toml"
    ], check=True)

    # prevent question of whether to install it into system or user
    user_flag = ["--user"] if (Path.home() / ".local/share/flatpak/runtime/org.gnome.Sdk/x86_64/50").exists() else []
    # build DavMail and generate source list
    with tempfile.TemporaryDirectory(dir="flatpak") as tmp_javadeps:
        # Use relative path for Maven repo local to avoid flatpak-in-flatpak issues
        rel_tmp_javadeps = os.path.relpath(tmp_javadeps, "contrib/davmail")
        log_file = Path(tmp_javadeps) / "maven-log.txt"
        try:
            with open(log_file, "w") as f:
                subprocess.run([
                    "flatpak",
                    "run",
                    *user_flag,
                    "--command=sh",
                    "--share=network",
                    "--filesystem=" + str(Path.cwd()),
                    sdk_id,
                    "-c",
                    "export PATH=/usr/lib/sdk/openjdk/bin:$PATH && "
                    "export JAVA_HOME=/usr/lib/sdk/openjdk && "
                    "cd contrib/davmail && "
                    "mvn install -Dmaven.repo.local=" + rel_tmp_javadeps,
                ], check=True, stdout=f)
            subprocess.run([
                str(venv_bin / "python"), "contrib/flatpak-gradle-generator.py",
                "--destdir", "flatpak/java-deps",
                str(log_file), str(srclist_dir / "java-sources.json")
            ], check=True)
        finally:
            if log_file.exists():
                log_file.unlink()

    # generate archive for flatpak JSON

    subprocess.run([
        "tar", "czf", "flatpak/source.tar.gz",
        "--exclude=contrib/davmail/dist",
        # put everything into a subdirectory
        "--transform=s#^#eventix/#",
        # include .git for GIT_HASH and submodule version metadata
        ".git", "bin", "contrib", "data", "libs", "Cargo.toml", "Cargo.lock", "package.json", "LICENSE",
        # include the files for flatpak building
        "flatpak/" + APP_ID + "-Import.desktop",
        "flatpak/" + APP_ID + ".desktop",
        "flatpak/" + APP_ID + ".metainfo.xml",
    ], check=True)

    # build everything (without network access)
    add_args = ["--disable-cache"] if not args.no_rebuild else []
    subprocess.run([
        "flatpak", "run", "--command=flathub-build", "org.flatpak.Builder",
        "--state-dir=" + str(state_dir),
        "--repo=" + str(repo_dir),
        "--delete-build-dirs",
        *add_args,
        "flatpak/" + APP_ID + ".json",
    ], check=True)

    # create flatpak package
    subprocess.run([
        "flatpak", "build-bundle", str(repo_dir), "flatpak/Eventix.flatpak", APP_ID
    ], check=True)

    # remove builddir; apparently we cannot control where that's stored
    shutil.rmtree("builddir", ignore_errors=True)

    print()
    print("Flatpak ready. You can install it via:")
    print("$ flatpak install --user flatpak/Eventix.flatpak")


def main():
    parent_parser = argparse.ArgumentParser(add_help=False)
    parent_parser.add_argument(
        "--address", default="127.0.0.1", help="Server address")
    parent_parser.add_argument(
        "--port", type=int, default=8083, help="Server port")

    parser = argparse.ArgumentParser(description="Eventix builder and runner")
    subparsers = parser.add_subparsers(
        dest="command", help="Available commands")
    subparsers.required = True

    run_parser = subparsers.add_parser(
        "run", parents=[parent_parser], help="Run eventix in development mode")
    run_parser.set_defaults(func=cmd_run)

    watch_parser = subparsers.add_parser(
        "watch", parents=[parent_parser],
        help="Watch and rerun eventix on changes")
    watch_parser.set_defaults(func=cmd_watch)

    app_parser = subparsers.add_parser(
        "app", parents=[parent_parser],
        help="Run the eventix app with tray icon")
    app_parser.set_defaults(func=cmd_app)

    import_parser = subparsers.add_parser(
        "import", parents=[parent_parser], help="Import an ICS file")
    import_parser.add_argument("file", help="Path to the ICS file to import")
    import_parser.set_defaults(func=cmd_import)

    davmail_parser = subparsers.add_parser(
        "davmail", parents=[parent_parser], help="Build davmail")
    davmail_parser.set_defaults(func=cmd_davmail)

    vdirsyncer_parser = subparsers.add_parser(
        "vdirsyncer", parents=[parent_parser], help="Build vdirsyncer")
    vdirsyncer_parser.set_defaults(func=cmd_vdirsyncer)

    test_parser = subparsers.add_parser(
        "test", parents=[parent_parser],
        help="Run cargo tests with bundled dev tools")
    test_parser.add_argument(
        "--nocapture", action="store_true",
        help="Show output from passing tests")
    test_parser.set_defaults(cargo_args=[])
    test_parser.set_defaults(func=cmd_test)

    coverage_parser = subparsers.add_parser(
        "coverage", parents=[parent_parser], help="Generate code coverage information")
    coverage_parser.add_argument(
        "file", nargs="?", help="Show line-by-line coverage for a single file"
    )
    coverage_parser.set_defaults(func=cmd_coverage)

    flatpak_parser = subparsers.add_parser(
        "flatpak", parents=[parent_parser], help="Build flatpak package")
    flatpak_parser.add_argument("--no-rebuild", help="Skip build step, just repackage",
                                action="store_true")
    flatpak_parser.set_defaults(func=cmd_flatpak)

    format_parser = subparsers.add_parser(
        "format", parents=[parent_parser],
        help="Format JS, CSS, and HTML templates with Prettier")
    format_parser.set_defaults(func=cmd_format)

    format_check_parser = subparsers.add_parser(
        "format-check", parents=[parent_parser],
        help="Check JS, CSS, and HTML template formatting with Prettier")
    format_check_parser.set_defaults(func=cmd_format_check)

    args, unknown = parser.parse_known_args()
    if args.command == "test":
        args.cargo_args = unknown
    elif unknown:
        parser.error("unrecognized arguments: {}".format(" ".join(unknown)))
    args.func(args)


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as e:
        # Print a concise message for subprocess failures without a Python
        # backtrace. Use shlex.join when the command is a sequence for nicer
        # formatting.
        cmd = shlex.join(e.cmd) if isinstance(e.cmd, (list, tuple)) else e.cmd
        print(f"Command '{cmd}' failed with exit code {e.returncode}", file=sys.stderr)
        # Preserve the subprocess exit code if possible
        try:
            code = int(e.returncode)
        except Exception:
            code = 1
        raise SystemExit(code)
    except KeyboardInterrupt:
        # Respect Ctrl-C with a normal exit code
        raise SystemExit(130)
    except Exception as e:
        # Generic fallback: print the error message only (no traceback)
        print(e, file=sys.stderr)
        raise SystemExit(1)
