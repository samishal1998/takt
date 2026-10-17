import unittest
from artifact_abi import glibc_requirements


class GlibcRequirements(unittest.TestCase):
    def test_optional_new_weak_import_does_not_raise_required_floor(self):
        symbols = "1: 0000 0 FUNC GLOBAL DEFAULT UND read@GLIBC_2.2.5\n2: 0000 0 FUNC WEAK DEFAULT UND pidfd_spawnp@GLIBC_2.39"
        versions = "Name: GLIBC_2.2.5 Flags: none Version: 3\nName: GLIBC_2.39 Flags: WEAK Version: 17"
        result = glibc_requirements(symbols, versions)
        self.assertEqual(result["required_versions"], ["GLIBC_2.2.5"])
        self.assertEqual(result["unsupported_required_versions"], [])
        self.assertEqual(result["weak_imports"], [{"symbol": "pidfd_spawnp", "version": "GLIBC_2.39"}])

    def test_strong_import_fails_even_if_version_is_marked_weak(self):
        result = glibc_requirements("1: 0000 0 FUNC GLOBAL DEFAULT UND new_function@GLIBC_2.39", "Name: GLIBC_2.39 Flags: WEAK Version: 17")
        self.assertEqual(result["unsupported_required_versions"], ["GLIBC_2.39"])

    def test_nonweak_loader_requirement_is_not_hidden_by_weak_symbol(self):
        result = glibc_requirements("1: 0000 0 FUNC WEAK DEFAULT UND new_function@GLIBC_2.39", "Name: GLIBC_2.39 Flags: none Version: 17")
        self.assertEqual(result["unsupported_required_versions"], ["GLIBC_2.39"])

    def test_unknown_glibc_abi_tag_requires_review(self):
        result = glibc_requirements("", "Name: GLIBC_ABI_DT_RELR Flags: none Version: 3")
        self.assertEqual(result["unsupported_required_versions"], ["GLIBC_ABI_DT_RELR"])


if __name__ == "__main__":
    unittest.main()
