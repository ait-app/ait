"""Regression checks for component metadata extraction used by audit scripts."""

import tempfile
import unittest
from pathlib import Path

from rust_method_specs import read_method_specs


class ComponentMethodSpecsTest(unittest.TestCase):
    def write_source(self, root, relative, source):
        path = root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(source)

    def test_reads_directions_and_ignores_test_declarations(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.write_source(root, "crates/owner/src/connection.rs", '''
pub const METHODS: &[MethodSpec] = &[
    MethodSpec::request("connection.ping"),
    MethodSpec::event("terminal.input"),
    MethodSpec::response(
        "browser.automation.execute.response",
    ),
];
''')
            self.write_source(root, "crates/owner/src/tests/fixture.rs", '''
const METHODS: &[MethodSpec] = &[MethodSpec::request("connection.ping")];
''')
            self.assertEqual(read_method_specs(root), {
                "connection.ping": "request",
                "terminal.input": "event",
                "browser.automation.execute.response": "response",
            })

    def test_rejects_duplicate_names_across_components(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = 'const METHODS: &[MethodSpec] = &[MethodSpec::request("same.request")];'
            self.write_source(root, "crates/first/src/lib.rs", source)
            self.write_source(root, "crates/second/src/lib.rs", source)
            with self.assertRaisesRegex(ValueError, "Duplicate component method"):
                read_method_specs(root)

    def test_rejects_unsupported_or_missing_declarations(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with self.assertRaisesRegex(ValueError, "No component-owned"):
                read_method_specs(root)
            self.write_source(root, "crates/owner/src/lib.rs", '''
const METHODS: &[MethodSpec] = &[MethodSpec::unknown("name")];
''')
            with self.assertRaisesRegex(ValueError, "Unrecognized method metadata"):
                read_method_specs(root)


if __name__ == "__main__":
    unittest.main()
