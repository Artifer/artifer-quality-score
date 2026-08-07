"""Config loading. A typo in a config file must fail loudly, never silently."""

from __future__ import annotations

import json

import pytest

from aqs import profile as prof
from aqs.profile import (
    DEFAULT_PROFILE,
    ProfileError,
    find_config,
    load_profile,
)


def write_config(directory, data) -> None:
    (directory / ".aqs.json").write_text(json.dumps(data))


class TestDiscovery:
    def test_no_config_uses_defaults(self, tmp_path):
        assert load_profile(tmp_path) is DEFAULT_PROFILE

    def test_finds_config_in_parent_directory(self, tmp_path):
        write_config(tmp_path, {"name": "custom"})
        nested = tmp_path / "a" / "b"
        nested.mkdir(parents=True)
        assert find_config(nested) == tmp_path / ".aqs.json"
        assert load_profile(nested).name == "custom"

    def test_explicit_path_wins(self, tmp_path):
        write_config(tmp_path, {"name": "discovered"})
        explicit = tmp_path / "other.json"
        explicit.write_text(json.dumps({"name": "explicit"}))
        assert load_profile(tmp_path, explicit).name == "explicit"


class TestOverrides:
    def test_threshold_override_applies(self, tmp_path, score):
        write_config(
            tmp_path, {"thresholds": {"function_cyclomatic_complexity_warning": 3}}
        )
        profile = load_profile(tmp_path)
        assert profile.threshold("function_cyclomatic_complexity_warning") == 3

        source = "def f(x):\n    if x:\n        return 1\n    if x > 1:\n        return 2\n    if x > 2:\n        return 3\n    return 0\n"
        assert prof.COMPLEX_METHOD not in {
            f.category for f in score(source).all_findings
        }
        relaxed = score(source, custom_profile=profile)
        assert prof.COMPLEX_METHOD in {f.category for f in relaxed.all_findings}

    def test_rule_shorthand_sets_weight(self, tmp_path):
        write_config(tmp_path, {"rules": {prof.BUMPY_ROAD: 0.0}})
        profile = load_profile(tmp_path)
        assert profile.rule(prof.BUMPY_ROAD).weight == 0.0
        # Everything else is untouched.
        assert profile.rule(prof.COMPLEX_METHOD).weight == 1.0

    def test_rule_object_sets_individual_fields(self, tmp_path):
        write_config(tmp_path, {"rules": {prof.LARGE_METHOD: {"base": 2.0, "cap": 3.0}}})
        rule = load_profile(tmp_path).rule(prof.LARGE_METHOD)
        assert (rule.base, rule.cap) == (2.0, 3.0)
        assert rule.weight == 1.0

    def test_defaults_are_not_mutated_by_a_load(self, tmp_path):
        write_config(tmp_path, {"rules": {prof.LARGE_METHOD: 0.0}})
        load_profile(tmp_path)
        assert DEFAULT_PROFILE.rule(prof.LARGE_METHOD).weight == 1.0


class TestValidation:
    def test_unknown_rule_is_rejected(self, tmp_path):
        write_config(tmp_path, {"rules": {"Made Up Rule": 0.5}})
        with pytest.raises(ProfileError, match="unknown rule"):
            load_profile(tmp_path)

    def test_unknown_threshold_is_rejected(self, tmp_path):
        write_config(tmp_path, {"thresholds": {"nope": 1}})
        with pytest.raises(ProfileError, match="unknown threshold"):
            load_profile(tmp_path)

    def test_unknown_rule_field_is_rejected(self, tmp_path):
        write_config(tmp_path, {"rules": {prof.LARGE_METHOD: {"wieght": 0.5}}})
        with pytest.raises(ProfileError, match="unknown field"):
            load_profile(tmp_path)

    def test_invalid_json_is_reported_with_the_path(self, tmp_path):
        (tmp_path / ".aqs.json").write_text("{not json")
        with pytest.raises(ProfileError, match="invalid JSON"):
            load_profile(tmp_path)

    def test_non_object_config_is_rejected(self, tmp_path):
        (tmp_path / ".aqs.json").write_text("[1, 2, 3]")
        with pytest.raises(ProfileError, match="JSON object"):
            load_profile(tmp_path)


class TestPathClassification:
    @pytest.mark.parametrize(
        "path",
        [
            "test_thing.py",
            "apps/core/tests/test_models.py",
            "src/thing.spec.ts",
            "src/components/Button.test.tsx",
            "conftest.py",
        ],
    )
    def test_recognises_test_paths(self, path):
        assert DEFAULT_PROFILE.is_test_path(path)

    @pytest.mark.parametrize("path", ["apps/core/models.py", "src/lib/utils.ts"])
    def test_production_paths_are_not_tests(self, path):
        assert not DEFAULT_PROFILE.is_test_path(path)

    @pytest.mark.parametrize(
        "path",
        [
            "node_modules/react/index.js",
            "src/components/ui/button.tsx",
            "apps/core/migrations/0001_initial.py",
            ".venv/lib/site.py",
            "src/types/api.d.ts",
        ],
    )
    def test_excludes_vendored_and_generated_code(self, path):
        assert DEFAULT_PROFILE.is_excluded(path)

    def test_own_components_are_not_excluded(self):
        assert not DEFAULT_PROFILE.is_excluded("src/components/TenderTable.tsx")
