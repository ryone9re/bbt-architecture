#!/usr/bin/env python3
"""Reject forbidden Cargo edges, including dev/build/optional/target-specific deps."""
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
ALLOWED = {
    "apps": {"business", "entrypoint", "integration", "infrastructure"},
    "business": {"business"},
    "entrypoint": {"business", "infrastructure"},
    "integration": {"business", "infrastructure"},
    "infrastructure": {"infrastructure"},
}
BUSINESS_LIBRARIES = {"async-trait", "chrono", "thiserror", "uuid"}


def area(package):
    path = Path(package["manifest_path"]).relative_to(ROOT)
    if path.parts[0] == "apps":
        return "apps"
    assert path.parts[0] == "src" and path.parts[1] in ALLOWED, f"unclassified crate: {path}"
    return path.parts[1]


def violations(packages):
    by_path = {str(Path(p["manifest_path"]).parent.resolve()): p for p in packages}
    errors = []
    for package in packages:
        source = area(package)
        if source == "apps":
            assert all(t["kind"] == ["bin"] for t in package["targets"]), "Apps must be bin-only"
        for dep in package["dependencies"]:
            if dep.get("path"):
                target = by_path.get(str(Path(dep["path"]).resolve()))
                if target is None or area(target) not in ALLOWED[source]:
                    errors.append(f'{package["name"]} -> {dep["name"]}: forbidden workspace dependency')
            elif source == "business" and dep["name"] not in BUSINESS_LIBRARIES:
                errors.append(f'{package["name"]} -> {dep["name"]}: not an approved general-purpose library')
    return errors


def main():
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--format-version=1", "--no-deps", "--locked"], cwd=ROOT))
    packages = metadata["packages"]
    errors = violations(packages)
    assert not errors, "\n".join(errors)
    # A small mutation check proves that the validator rejects an illegal edge.
    import copy
    mutated = copy.deepcopy(packages)
    business = next(p for p in mutated if area(p) == "business")
    target = next(p for p in mutated if area(p) == "integration")
    business["dependencies"].append({"name": target["name"], "path": str(Path(target["manifest_path"]).parent)})
    assert violations(mutated), "forbidden dependency escaped validation"
    print(f"BBT dependency directions verified ({len(packages)} crates)")


if __name__ == "__main__":
    main()
