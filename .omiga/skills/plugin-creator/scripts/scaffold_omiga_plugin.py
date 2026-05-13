#!/usr/bin/env python3
"""Scaffold an Omiga-native bundled plugin.

This helper intentionally creates small placeholders only. The agent must still fill
real Operator/Template implementations before claiming product readiness.
"""
from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Iterable


def slug(value: str) -> str:
    text = value.strip().lower()
    text = re.sub(r"[^a-z0-9]+", "-", text)
    text = re.sub(r"-+", "-", text).strip("-")
    return text or "unit"


def unit_id(value: str) -> str:
    return slug(value).replace("-", "_")


def title(value: str) -> str:
    known = {"bwa": "BWA", "star": "STAR", "ngs": "NGS", "pca": "PCA", "r": "R"}
    return " ".join(known.get(part.lower(), part.capitalize()) for part in re.split(r"[-_\s]+", value) if part)


def write_text(path: Path, content: str, *, force: bool) -> None:
    if path.exists() and not force:
        raise SystemExit(f"Refusing to overwrite existing file: {path}")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def json_dump(path: Path, data: object, *, force: bool) -> None:
    write_text(path, json.dumps(data, indent=2, ensure_ascii=False) + "\n", force=force)


def split_items(values: Iterable[str]) -> list[str]:
    items: list[str] = []
    for value in values:
        for item in value.split(","):
            item = item.strip()
            if item:
                items.append(item)
    return items


def plugin_manifest(args: argparse.Namespace) -> dict:
    caps = [args.category]
    if args.subcategory:
        caps.append(args.subcategory)
    caps.extend(split_items(args.capability))
    if args.kind in {"operators", "mixed"}:
        caps.append("Operator")
    if args.kind in {"templates", "mixed"}:
        caps.append("Template")
    if args.kind in {"skills", "mixed"}:
        caps.append("Skill")
    # Preserve insertion order while deduplicating.
    capabilities = list(dict.fromkeys(cap for cap in caps if cap))

    manifest: dict = {
        "name": args.plugin_id,
        "version": args.version,
        "description": args.description or args.short_description,
        "interface": {
            "displayName": args.display_name,
            "shortDescription": args.short_description,
            "longDescription": args.long_description or args.short_description,
            "developerName": args.developer_name,
            "category": args.category,
            "capabilities": capabilities,
            "brandColor": args.brand_color,
            "defaultPrompt": [
                args.default_prompt
                or f"Use {args.display_name} for {args.short_description.rstrip('.').lower()}."
            ],
        },
    }
    if args.kind in {"operators", "mixed"} or args.operator:
        manifest["operators"] = "./operators"
    if args.kind in {"templates", "mixed"} or args.template:
        manifest["templates"] = "./templates"
    if args.kind in {"skills", "mixed"} or args.skill:
        manifest["skills"] = "./skills"
    if args.with_environments:
        manifest["environments"] = "./environments"
    return manifest


def operator_yaml(name: str, env_ref: str = "") -> str:
    sid = unit_id(name)
    human = title(name)
    env_ref_line = f"  envRef: {env_ref}\n" if env_ref else ""
    return f"""apiVersion: omiga.ai/operator/v1alpha1
kind: Operator
metadata:
  id: {sid}
  version: 0.1.0
  name: {human}
  description: TODO: Replace with a precise one-sentence description.
  tags: [operator]
interface:
  inputs:
    input:
      kind: file
      required: true
      description: TODO: Input file consumed by this operator.
  params:
    threads:
      kind: integer
      default: 1
      minimum: 1
      maximum: 128
    extra:
      kind: string
      default: ""
      description: Optional additional CLI arguments; document tool-specific restrictions before exposing broadly.
  outputs:
    result:
      kind: file
      glob: result.*
      required: true
    summary:
      kind: json
      required: false
runtime:
{env_ref_line}  placement:
    supported: [local, ssh]
  container:
    supported: [none]
resources:
  cpu:
    default: 1
    exposed: true
  walltime:
    default: 600s
    exposed: true
execution:
  argv:
    - python3
    - ./scripts/{slug(name)}.py
    - ${{inputs.input}}
    - ${{outdir}}
    - ${{params.threads}}
    - ${{params.extra}}
"""


def operator_script(name: str) -> str:
    human = title(name)
    return f'''#!/usr/bin/env python3
"""Placeholder implementation for {human}. Replace before production use."""
from __future__ import annotations

import json
import sys
from pathlib import Path


def main() -> int:
    if len(sys.argv) < 3:
        print("usage: script <input> <outdir> [threads] [extra]", file=sys.stderr)
        return 2
    input_path = Path(sys.argv[1])
    outdir = Path(sys.argv[2])
    outdir.mkdir(parents=True, exist_ok=True)
    result = outdir / "result.txt"
    result.write_text(f"TODO: implement {human} for {{input_path}}\\n", encoding="utf-8")
    (outdir / "outputs.json").write_text(
        json.dumps({{"summary": {{"tool": "{slug(name)}", "status": "placeholder"}}}}, indent=2) + "\\n",
        encoding="utf-8",
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
'''


def template_yaml(name: str) -> str:
    sid = unit_id(name)
    human = title(name)
    return f"""apiVersion: omiga.ai/unit/v1alpha1
kind: Template
metadata:
  id: {sid}
  version: 0.1.0
  name: {human}
  description: TODO: Replace with a precise template description.
  tags: [template]
classification:
  category: TODO/category
  tags: [template]
  stageInput: []
  stageOutput: []
exposure:
  exposeToAgent: true
interface:
  inputs: {{}}
  params: {{}}
  outputs: {{}}
template:
  engine: jinja2
  entry: ./template.txt.j2
execution:
  interpreter: text
  argv: []
"""


def skill_md(name: str) -> str:
    sid = slug(name)
    human = title(name)
    return f"""---
name: {sid}
description: TODO: Describe when Omiga agents should use the {human} skill.
---

# {human}

TODO: Add concise workflow rules and verification steps.

$ARGUMENTS
"""


def environment_manifest(env_ref: str, runtime: str) -> str:
    human = title(env_ref)
    if runtime == "docker":
        return f"""apiVersion: omiga.ai/environment/v1alpha1
kind: Environment
metadata:
  id: {env_ref}
  version: 0.1.0
  name: {human}
runtime:
  type: docker
  dockerfile: ./Dockerfile
requirements:
  system:
    - docker CLI and running Docker daemon
diagnostics:
  checkCommand: [docker, version]
  installHint: Install Docker Desktop/Engine, ensure `docker` is on PATH, and start the Docker daemon. Omiga can build this local image from Dockerfile.
"""
    if runtime == "singularity":
        return f"""apiVersion: omiga.ai/environment/v1alpha1
kind: Environment
metadata:
  id: {env_ref}
  version: 0.1.0
  name: {human}
runtime:
  type: singularity
  definitionFile: ./singularity.def
requirements:
  system:
    - singularity or apptainer
diagnostics:
  checkCommand: [singularity, --version]
  installHint: Install SingularityCE or Apptainer and ensure `singularity` or `apptainer` is on PATH. Omiga can build this local image from singularity.def.
"""
    return f"""apiVersion: omiga.ai/environment/v1alpha1
kind: Environment
metadata:
  id: {env_ref}
  version: 0.1.0
  name: {human}
runtime:
  type: conda
  condaEnvFile: ./conda.yaml
requirements:
  system:
    - micromamba or mamba or conda
diagnostics:
  checkCommand: [true]
  installHint: Install the official micromamba binary at $HOME/.omiga/bin/micromamba or set OMIGA_MICROMAMBA; Omiga creates this env from conda.yaml/conda.yml.
"""


def environment_runtime_file_name(runtime: str) -> str:
    if runtime == "docker":
        return "Dockerfile"
    if runtime == "singularity":
        return "singularity.def"
    return "conda.yaml"


def environment_runtime_file(runtime: str) -> str:
    if runtime == "docker":
        return "FROM ubuntu:22.04\n\n# TODO: install runtime tools for this plugin.\n"
    if runtime == "singularity":
        return """Bootstrap: docker
From: ubuntu:22.04

%post
    # TODO: install runtime tools for this plugin.
    true
"""
    return "channels:\n  - conda-forge\n  - bioconda\n  - nodefaults\ndependencies: []\n"


def update_marketplace(repo: Path, args: argparse.Namespace) -> None:
    marketplace = repo / "src-tauri" / "bundled_plugins" / "marketplace.json"
    if not marketplace.exists():
        raise SystemExit(f"Marketplace not found: {marketplace}")
    data = json.loads(marketplace.read_text(encoding="utf-8"))
    plugins = data.setdefault("plugins", [])
    existing = next((entry for entry in plugins if entry.get("name") == args.plugin_id), None)
    entry = {
        "name": args.plugin_id,
        "source": {"source": "local", "path": f"./plugins/{args.plugin_id}"},
        "policy": {"installation": "AVAILABLE", "authentication": "ON_USE"},
        "category": args.category,
    }
    if existing is None:
        plugins.append(entry)
    else:
        existing.update(entry)
    marketplace.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description="Scaffold an Omiga bundled plugin")
    parser.add_argument("--repo", default=".", help="Repository root containing src-tauri/bundled_plugins")
    parser.add_argument("--plugin-id", required=True, help="Stable plugin id, e.g. ngs-alignment")
    parser.add_argument("--display-name", required=True, help="Human display name, e.g. Alignment")
    parser.add_argument("--category", required=True, help="Top-level product category")
    parser.add_argument("--subcategory", default="", help="Product subcategory, stored as a capability")
    parser.add_argument("--kind", choices=["plugin", "operators", "templates", "skills", "mixed"], default="plugin")
    parser.add_argument("--operator", action="append", default=[], help="Operator unit slug/name; repeat or comma-separate")
    parser.add_argument("--template", action="append", default=[], help="Template unit slug/name; repeat or comma-separate")
    parser.add_argument("--skill", action="append", default=[], help="Plugin-carried Skill name; repeat or comma-separate")
    parser.add_argument("--short-description", required=True)
    parser.add_argument("--long-description", default="")
    parser.add_argument("--description", default="")
    parser.add_argument("--developer-name", default="Omiga")
    parser.add_argument("--brand-color", default="#2563EB")
    parser.add_argument("--version", default="0.1.0")
    parser.add_argument("--capability", action="append", default=[], help="Additional capability; repeat or comma-separate")
    parser.add_argument("--default-prompt", default="")
    parser.add_argument("--with-environments", action="store_true")
    parser.add_argument("--env-ref", default="", help="Optional envRef to add to generated operators and environment profile")
    parser.add_argument("--env-runtime", choices=["conda", "docker", "singularity"], default="conda", help="Runtime profile type for --with-environments")
    parser.add_argument("--marketplace", action="store_true", help="Add/update bundled marketplace entry")
    parser.add_argument("--force", action="store_true", help="Overwrite generated placeholder files")
    args = parser.parse_args()

    args.plugin_id = slug(args.plugin_id)
    repo = Path(args.repo).resolve()
    plugins_root = repo / "src-tauri" / "bundled_plugins" / "plugins"
    if not plugins_root.is_dir():
        raise SystemExit(f"Bundled plugin root not found: {plugins_root}")
    plugin_root = plugins_root / args.plugin_id
    plugin_root.mkdir(parents=True, exist_ok=True)

    json_dump(plugin_root / "plugin.json", plugin_manifest(args), force=args.force or not (plugin_root / "plugin.json").exists())
    operator_env_ref = slug(args.env_ref or f"{args.plugin_id}-env") if args.with_environments else slug(args.env_ref)

    for op in split_items(args.operator):
        op_slug = slug(op)
        write_text(plugin_root / "operators" / op_slug / "operator.yaml", operator_yaml(op, operator_env_ref), force=args.force)
        script = plugin_root / "scripts" / f"{op_slug}.py"
        write_text(script, operator_script(op), force=args.force)
        script.chmod(0o755)

    for tmpl in split_items(args.template):
        tmpl_slug = slug(tmpl)
        write_text(plugin_root / "templates" / tmpl_slug / "template.yaml", template_yaml(tmpl), force=args.force)
        write_text(plugin_root / "templates" / tmpl_slug / "template.txt.j2", "TODO: render template body here.\n", force=args.force)

    for skill in split_items(args.skill):
        skill_slug = slug(skill)
        write_text(plugin_root / "skills" / skill_slug / "SKILL.md", skill_md(skill), force=args.force)

    if args.with_environments:
        env_ref = slug(args.env_ref or f"{args.plugin_id}-env")
        env_dir = plugin_root / "environments" / env_ref
        write_text(env_dir / "environment.yaml", environment_manifest(env_ref, args.env_runtime), force=args.force)
        write_text(env_dir / environment_runtime_file_name(args.env_runtime), environment_runtime_file(args.env_runtime), force=args.force)

    if args.marketplace:
        update_marketplace(repo, args)

    print(f"Scaffolded {args.plugin_id} at {plugin_root}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
