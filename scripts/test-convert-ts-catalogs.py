"""Focused tests for Qt-to-gettext plural identity and placeholder conversion."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location(
    "catalogs", Path(__file__).with_name("convert-ts-catalogs.py")
)
catalogs = importlib.util.module_from_spec(spec)
spec.loader.exec_module(catalogs)


class PluralMapping(unittest.TestCase):
    def test_favorite_aliases(self):
        for source, singular, plural in [
            ("%n favorite(s)", "{n} favorite", "{n} favorites"),
            ("%n system(s) with favorites", "{n} system with favorites", "{n} systems with favorites"),
        ]:
            self.assertEqual(catalogs.map_plural_ids(source, source), (singular, plural))

    def test_unrelated_sources_are_unchanged(self):
        self.assertEqual(catalogs.map_plural_ids("%1 systems", None), ("{} systems", None))
        self.assertEqual(catalogs.map_plural_ids("%n item", "%n items"), ("{n} item", "{n} items"))

    def test_all_translated_forms_retain_count_placeholder(self):
        forms = ["%n элемент", "%n элемента", "%n элементов"]
        self.assertEqual(
            [catalogs.map_msgstr(form) for form in forms],
            ["{n} элемент", "{n} элемента", "{n} элементов"],
        )


if __name__ == "__main__":
    unittest.main()
