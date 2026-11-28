"""Tests of tools/security/scan_suspicious.py: each pattern it looks for, planted in a throwaway
git repository, is found; the forms that are fine are not; MDM's own tree is clean.

    python -I -m unittest discover -s tools/security/tests

The planted texts are put together at run time, so that this file itself holds none of them
(the scanner reads it like every other file).
"""

import contextlib
import io
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", ".."))
sys.path.insert(0, os.path.dirname(HERE))
import scan_suspicious as scan  # noqa: E402

BLANKS = " " * 300
EVAL = "ev" + "al"
ATOB = "at" + "ob"
B64 = "base" + "64"


def git(cwd, *args):
    return subprocess.run(["git", "-c", "user.name=test", "-c", "user.email=test@example.invalid",
                           "-c", "commit.gpgsign=false", *args], cwd=cwd, check=True,
                          capture_output=True).stdout.decode().strip()


class Repo:
    def __init__(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.path = self.tmp.name
        git(self.path, "init", "-q")
        git(self.path, "config", "core.autocrlf", "false")
        self.commit({"README.txt": "a test repository\n"})

    def commit(self, files):
        """Writes (bytes or text) or deletes (None) files and commits; returns the commit id."""
        for name, content in files.items():
            full = os.path.join(self.path, *name.split("/"))
            if content is None:
                os.remove(full)
                continue
            os.makedirs(os.path.dirname(full), exist_ok=True)
            with open(full, "wb") as f:
                f.write(content.encode("utf-8") if isinstance(content, str) else content)
        git(self.path, "add", "-A")
        git(self.path, "commit", "-q", "--allow-empty", "-m", "change")
        return git(self.path, "rev-parse", "HEAD")

    def scan(self, *args):
        out = io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(out):
            code = scan.main([*args, "--repo", self.path])
        return code, out.getvalue()

    def close(self):
        self.tmp.cleanup()


class Base(unittest.TestCase):
    def setUp(self):
        self.repo = Repo()
        self.addCleanup(self.repo.close)
        os.environ.pop("GITHUB_ACTIONS", None)

    def tree(self, files):
        self.repo.commit(files)
        return self.repo.scan("--tree")

    def assertFound(self, files, rule, *texts):
        code, out = self.tree(files)
        self.assertEqual(code, 1, out)
        self.assertIn("[%s]" % rule, out)
        for text in texts:
            self.assertIn(text, out)
        return out

    def assertClean(self, files):
        code, out = self.tree(files)
        self.assertEqual(code, 0, out)
        self.assertIn("Nothing found.", out)


class Indicators(Base):
    def test_every_indicator_in_a_file(self):
        indicators = scan.load_indicators()
        self.assertGreater(len(indicators), 30)
        for text in indicators:
            with self.subTest(text=text):
                self.repo.commit({"src/config.js": "module.exports = {};\nconst x = '%s';\n"
                                  % text.upper()})
                code, out = self.repo.scan("--tree")
                self.assertEqual(code, 1, out)
                self.assertIn("[indicator] src/config.js:2", out)

    def test_indicator_in_a_file_name(self):
        name = next(t for t in scan.load_indicators() if t.endswith(".json"))
        self.assertFound({name: "{}\n"}, "indicator", "a path holding")

    def test_indicator_in_a_binary_file(self):
        text = scan.load_indicators()[0].encode()
        self.assertFound({"public/fonts/x.woff2": b"wOF2\0\0\0" + text + b"\0\1"}, "indicator")

    def test_an_address_inside_a_longer_number_is_not_one(self):
        ip = next(t for t in scan.load_indicators() if scan.IP.match(t))
        self.assertClean({"notes.txt": "version 1%s5 and %s.9\n" % (ip, ip)})
        self.assertFound({"notes.txt": "connect to %s:443\n" % ip}, "indicator")


class HiddenCode(Base):
    def test_code_after_a_run_of_blanks(self):
        cfg = "module.exports = { preset: 'x' };" + BLANKS + "require('child_process').exec(1)\n"
        out = self.assertFound({"jest.config.js": cfg}, "hidden-code", "[300 blanks]")
        self.assertIn("child_process", out)

    def test_tabs_count_too(self):
        self.assertFound({"postcss.config.js": "export default {};" + "\t" * 250 + "x()\n"},
                         "hidden-code")

    def test_trailing_blanks_alone_are_fine(self):
        self.assertClean({"astro.config.mjs": "export default {};" + BLANKS + "\n"})


class LongLines(Base):
    def test_long_lines_in_code(self):
        for name in ("tools/build.py", "vite.config.ts", "web/app.mjs", "lib/x.cjs"):
            with self.subTest(name=name):
                self.assertFound({name: "x = '%s'\n" % ("a" * 1200)}, "long-line", "1206")

    def test_long_lines_in_configuration_json(self):
        for name in ("package.json", "web/tsconfig.app.json", ".eslintrc.json", "extension/manifest.json"):
            with self.subTest(name=name):
                self.assertFound({name: '{"a": "%s"}\n' % ("b" * 1200)}, "long-line")

    def test_long_lines_in_data_json_are_fine(self):
        self.assertClean({"i18n/site/de.json": '{"a": "%s"}\n' % ("b" * 5000)})

    def test_a_thousand_is_fine(self):
        self.assertClean({"web/app.js": "x" * 1000 + "\n"})


class EncodedEval(Base):
    def test_typescript_eval_of_atob(self):
        self.assertFound({"src/init.ts": "%s('x', %s('aGVsbG8='));\n" % (EVAL, ATOB)}, "encoded-eval")

    def test_new_function_of_a_buffer(self):
        line = "new Function(Buffer.from(s, '%s').toString())();\n" % B64
        self.assertFound({"lib/a.cjs": line}, "encoded-eval")

    def test_python_exec_of_base64(self):
        self.assertFound({"tools/run.py": "import base64\nexec(%s.b64decode('cHJpbnQ='))\n" % B64},
                         "encoded-eval")

    def test_far_apart_on_a_minified_line_is_fine(self):
        line = "%s(a);" % EVAL + "x=1;" * 200 + "y=%s(b);\n" % ATOB
        self.assertLess(len(line), 1000)
        self.assertClean({"vendor/lib.min.js": line})
        self.assertClean({"tools/b.py": "%s(t)\n" % EVAL})


class LoaderPatterns(Base):
    def test_obfuscator_table(self):
        self.assertFound({"a.js": "var _$_" + "9b39=(function(o,u){return o})('x');\n"},
                         "loader-pattern", "string table")

    def test_one_letter_globals(self):
        self.assertFound({"a.js": "glob" + "al.o='3a';\n"}, "loader-pattern", "one-letter")
        self.assertFound({"a.js": "glob" + "al.i=\"A6b\";\n"}, "loader-pattern")
        self.assertFound({"a.js": "glob" + "al['r']=\"x\";\n"}, "loader-pattern")

    def test_create_require(self):
        self.assertFound({"eslint.config.mjs": "const r = create" + "Require(import.meta.url);\n"},
                         "loader-pattern", "createRequire")

    def test_ordinary_globals_are_fine(self):
        self.assertClean({"a.js": "global.fetch = fakeFetch;\nglobalThis.MDM = {};\n"})


class SuspiciousFiles(Base):
    def test_planted_file_names(self):
        for name in ("conftest.py", "tests/conftest.py", "api.js", "utils/session.py",
                     "src/utils/http_session.py", "utils/net.py", "utils/clients.py",
                     "utils/transport.py"):
            with self.subTest(name=name):
                self.assertFound({name: "pass\n"}, "suspicious-file")

    def test_other_utils_modules_are_fine(self):
        self.assertClean({"utils/strings.py": "pass\n", "lib/session.py": "pass\n"})

    def test_imports_of_planted_modules(self):
        for line in ("import utils." + "transport", "from utils import " + "net",
                     "from utils." + "session import go", "  import utils." + "clients as c"):
            with self.subTest(line=line):
                self.assertFound({"tools/x.py": line + "\n"}, "suspicious-import")
        self.assertClean({"tools/x.py": "from utils import strings\n"})


class Packages(Base):
    def test_install_scripts_in_a_tree(self):
        for name in ("postinstall", "preinstall", "install", "prepare"):
            with self.subTest(name=name):
                self.repo.commit({"package.json": '{"scripts": {"test": "x", "%s": "node a.js"}}\n'
                                  % name})
                code, out = self.repo.scan("--tree")
                self.assertEqual(code, 1, out)
                self.assertIn("[package-scripts]", out)

    def test_api_js_in_a_script(self):
        self.assertFound({"package.json": '{"scripts": {"build": "node api.js && vite build"}}\n'},
                         "package-scripts", "api.js")

    def test_ordinary_scripts_in_a_tree_are_fine(self):
        self.assertClean({"web/package.json": '{"scripts": {"test": "node --test"}}\n'})

    def test_any_script_change_in_a_diff(self):
        base = self.repo.commit({"web/package.json": '{"scripts": {"test": "node --test"}}\n'})
        head = self.repo.commit({"web/package.json": '{"scripts": {"test": "node --test", '
                                 '"build": "node tools/x.js"}}\n'})
        code, out = self.repo.scan("--diff", "%s..%s" % (base, head))
        self.assertEqual(code, 1, out)
        self.assertIn("a script added: 'build'", out)

    def test_a_new_package_brings_its_scripts(self):
        base = self.repo.commit({})
        head = self.repo.commit({"a/package.json": '{"scripts": {"dev": "vite"}}\n'})
        code, out = self.repo.scan("--diff", "%s..%s" % (base, head))
        self.assertEqual(code, 1, out)
        self.assertIn("'dev'", out)

    def test_other_changes_to_a_package_are_fine(self):
        base = self.repo.commit({"package.json": '{"version": "1", "scripts": {"t": "x", "u": "y"}}\n'})
        head = self.repo.commit({"package.json": '{"version": "2", "scripts": {"t": "x"}}\n'})
        self.assertEqual(self.repo.scan("--diff", "%s..%s" % (base, head))[0], 0)

    def test_a_package_that_does_not_parse(self):
        self.assertFound({"package.json": '{"scripts": '}, "package-scripts", "doesn't parse")


class Assets(Base):
    def test_a_font_that_is_a_script(self):
        self.assertFound({"public/fonts/fa-solid-400.woff2": "(function(){var a=1;})();\n" * 20},
                         "fake-asset", "is text")

    def test_a_picture_that_is_a_script(self):
        self.assertFound({"img/logo.png": "const x = require('fs');\n" * 20}, "fake-asset")

    def test_real_fonts_and_pictures_are_fine(self):
        self.assertClean({
            "a.woff2": b"wOF2\0\1\0\0" + bytes(range(256)),
            "a.ttf": b"\0\1\0\0\0\x10" + bytes(range(256)),
            "a.png": b"\x89PNG\r\n\x1a\n" + bytes(range(256)),
            "a.jpg": b"\xff\xd8\xff\xe0" + bytes(range(256)),
            "a.ico": b"\0\0\1\0\1\0" + bytes(range(256)),
            "a.webp": b"RIFF\0\0\0\0WEBPVP8 " + bytes(range(256)),
            "a.svg": "<svg xmlns='http://www.w3.org/2000/svg'/>\n",
        })

    def test_a_font_of_no_known_format(self):
        self.assertFound({"a.otf": b"\1\2\3\4" + bytes(range(256))}, "fake-asset", "doesn't start")


class Editor(Base):
    def test_a_task_run_when_the_folder_opens(self):
        tasks = '{"tasks": [{"label": "x", "command": "node x",\n "runOptions": {"runOn": "folderOpen"}}]}\n'
        self.assertFound({".vscode/tasks.json": tasks}, "editor-autorun")

    def test_settings_allowing_automatic_tasks(self):
        self.assertFound({".vscode/settings.json": '{"task.allowAutomaticTasks": "on"}\n'},
                         "editor-autorun")
        self.assertFound({"website/.vscode/settings.json": '{"task.allowAutomaticTasks": "on"}\n'},
                         "editor-autorun")

    def test_ordinary_settings_are_fine(self):
        self.assertClean({".vscode/settings.json": '{"editor.tabSize": 4}\n'})


WORKFLOW = """name: t
on:
  pull_request:
jobs:
  a:
    runs-on: ubuntu-latest
    steps:
      - run: make test
"""


class Workflows(Base):
    def diff(self, before, after, name=".github/workflows/t.yml"):
        base = self.repo.commit({name: before})
        head = self.repo.commit({name: after})
        return self.repo.scan("--diff", "%s..%s" % (base, head))

    def test_a_trigger_that_runs_with_secrets(self):
        code, out = self.diff(WORKFLOW, WORKFLOW.replace("pull_request:", "pull_request_target:"))
        self.assertEqual(code, 1, out)
        self.assertIn("[workflow-trigger]", out)
        code, out = self.diff(WORKFLOW, WORKFLOW + "  workflow_run:\n", name=".github/workflows/u.yml")
        self.assertIn("[workflow-trigger]", out)

    def test_a_secret_given_to_pull_request_code(self):
        after = WORKFLOW + "        env:\n          TOKEN: ${{ secrets.DEPLOY_TOKEN }}\n"
        code, out = self.diff(WORKFLOW, after)
        self.assertEqual(code, 1, out)
        self.assertIn("[workflow-secrets]", out)

    def test_a_secret_in_a_workflow_pull_requests_do_not_start_is_fine(self):
        push = WORKFLOW.replace("pull_request:", "push:")
        code, out = self.diff(push, push + "        env:\n          T: ${{ secrets.X }}\n")
        self.assertEqual(code, 0, out)

    def test_the_trigger_in_a_tree(self):
        self.assertFound({".github/workflows/t.yml": WORKFLOW.replace("pull_request:",
                                                                       "pull_request_target:")},
                         "workflow-trigger")


class Diffs(Base):
    def test_only_what_the_change_brought(self):
        base = self.repo.commit({"web/app.js": "a();\n" + "x" * 1500 + "\n"})
        head = self.repo.commit({"web/app.js": "b();\n" + "x" * 1500 + "\n"})
        self.assertEqual(self.repo.scan("--diff", "%s..%s" % (base, head))[0], 0)
        self.assertEqual(self.repo.scan("--tree")[0], 1)

    def test_code_appended_to_an_existing_line(self):
        base = self.repo.commit({"jest.config.js": "module.exports = {};\n"})
        head = self.repo.commit({"jest.config.js": "module.exports = {};" + BLANKS + "x()\n"})
        code, out = self.repo.scan("--diff", "%s..%s" % (base, head))
        self.assertEqual(code, 1, out)
        self.assertIn("[hidden-code] jest.config.js:1", out)

    def test_unchanged_files_are_not_read(self):
        base = self.repo.commit({"conftest.py": "pass\n"})
        head = self.repo.commit({"other.txt": "x\n"})
        self.assertEqual(self.repo.scan("--diff", "%s..%s" % (base, head))[0], 0)

    def test_three_dots_start_at_the_merge_base(self):
        start = self.repo.commit({})
        git(self.repo.path, "checkout", "-q", "-b", "side")
        side = self.repo.commit({"conftest.py": "pass\n"})
        git(self.repo.path, "checkout", "-q", start)
        other = self.repo.commit({"api.js": "x\n"})
        code, out = self.repo.scan("--diff", "%s...%s" % (other, side))
        self.assertEqual(code, 1, out)
        self.assertIn("conftest.py", out)
        self.assertNotIn("api.js", out)


class Allowlist(Base):
    def test_a_reviewed_finding(self):
        base = self.repo.commit({})
        self.repo.commit({"web/app.js": "y" * 1500 + "\n"})
        code, out = self.repo.scan("--tree")
        fp = out.split("(fingerprint ")[1][:16]
        head = self.repo.commit({scan.ALLOWLIST: "long-line web/app.js %s a test string\n" % fp})
        code, out = self.repo.scan("--diff", "%s..%s" % (base, head))
        self.assertEqual(code, 0, out)
        self.assertIn("The change adds to tools/security/allowlist.txt", out)
        self.assertIn("a test string", out)
        # The entry covers that line only.
        self.repo.commit({"web/app.js": "z" * 1500 + "\n"})
        self.assertEqual(self.repo.scan("--tree")[0], 1)

    def test_a_whole_file_for_long_lines_only(self):
        self.repo.commit({scan.ALLOWLIST: "long-line vendor/x.min.js * vendored\n"})
        self.assertClean({"vendor/x.min.js": "a" * 3000 + "\n"})
        self.assertFound({"vendor/x.min.js": "a" * 3000 + "\n" + "b;" + BLANKS + "c()\n"},
                         "hidden-code")
        code, out = self.tree({scan.ALLOWLIST: "hidden-code vendor/x.min.js * no\n"})
        self.assertEqual(code, 2, out)


class History(Base):
    def test_an_indicator_removed_later_is_still_found(self):
        text = scan.load_indicators()[0]
        self.repo.commit({"a.js": "x = '%s'\n" % text})
        self.repo.commit({"a.js": "x = 1\n"})
        self.assertEqual(self.repo.scan("--tree")[0], 0)
        code, out = self.repo.scan("--history")
        self.assertEqual(code, 1, out)
        self.assertIn("[indicator] a.js@", out)
        self.assertIn("in commit(s): ", out)

    def test_a_loader_on_another_branch(self):
        git(self.repo.path, "checkout", "-q", "-b", "other")
        self.repo.commit({"b.js": "ok();\nvar _$_" + "1f2e=(function(){})();\n"})
        git(self.repo.path, "checkout", "-q", "-")
        code, out = self.repo.scan("--history")
        self.assertEqual(code, 1, out)
        self.assertIn("[loader-pattern] b.js@", out)
        self.assertIn(":2", out)

    def test_a_clean_history(self):
        self.repo.commit({"a.js": "global.fetch = f;\n"})
        self.assertEqual(self.repo.scan("--history")[0], 0)


class Output(Base):
    def test_nothing_scanned_acts_as_a_workflow_command(self):
        os.environ["GITHUB_ACTIONS"] = "true"
        self.addCleanup(os.environ.pop, "GITHUB_ACTIONS", None)
        out = self.assertFound({"api.js": "::add-mask::x\n"}, "suspicious-file")
        lines = out.splitlines()
        stop = next(i for i, l in enumerate(lines) if l.startswith("::stop-commands::"))
        token = lines[stop].split("::")[2]
        resume = lines.index("::%s::" % token)
        self.assertLess(stop, resume)
        self.assertTrue(any(l.startswith("::error file=api.js") for l in lines[resume:]))


class ThisRepository(unittest.TestCase):
    def test_mdm_is_clean(self):
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            code = scan.main(["--tree", "HEAD", "--repo", ROOT])
        self.assertEqual(code, 0, out.getvalue())


if __name__ == "__main__":
    unittest.main()
