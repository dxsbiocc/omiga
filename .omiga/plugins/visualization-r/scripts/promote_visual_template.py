#!/usr/bin/env python3
"""Promote a visualization-r Rscript source artifact into a user/project preference Template.

This helper intentionally performs only safe, explicit replacements. It does not try to infer
arbitrary R code intent.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path


def safe_id(value: str) -> str:
    out = ''.join(ch if ch.isalnum() or ch in '._-' else '_' for ch in value.strip())
    if not out:
        raise SystemExit('template id must not be empty')
    return out


def marketplace_paths(scope: str, project_root: Path) -> tuple[str, Path, str]:
    if scope == 'user':
        root = Path.home() / '.omiga' / 'plugins'
        return 'omiga-user', root, 'user-visualization-r'
    root = project_root / '.omiga' / 'plugins'
    return 'omiga-project', root, 'project-visualization-r'


def load_json(path: Path, default: dict) -> dict:
    if not path.exists():
        return default
    return json.loads(path.read_text())


def write_json(path: Path, value: dict, dry_run: bool) -> None:
    raw = json.dumps(value, indent=2) + '\n'
    if dry_run:
        print(f'[dry-run] write {path}\n{raw}')
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(raw)


def upsert_marketplace(marketplace_path: Path, marketplace_name: str, plugin_name: str, dry_run: bool) -> None:
    data = load_json(marketplace_path, {
        'name': marketplace_name,
        'interface': {'displayName': 'Omiga User' if marketplace_name == 'omiga-user' else 'Omiga Project'},
        'plugins': [],
    })
    data.setdefault('name', marketplace_name)
    data.setdefault('interface', {'displayName': marketplace_name})
    plugins = data.setdefault('plugins', [])
    entry = {
        'name': plugin_name,
        'source': {'source': 'local', 'path': f'./{plugin_name}'},
        'policy': {'installation': 'AVAILABLE', 'authentication': 'ON_USE'},
        'category': 'Visualization',
    }
    for i, existing in enumerate(plugins):
        if existing.get('name') == plugin_name:
            plugins[i] = entry
            break
    else:
        plugins.append(entry)
    write_json(marketplace_path, data, dry_run)


def plugin_manifest(plugin_name: str, display: str) -> dict:
    return {
        'name': plugin_name,
        'version': '0.1.0',
        'description': 'Preference R visualization templates promoted from editable Rscript source artifacts.',
        'templates': './templates',
        'interface': {
            'displayName': display,
            'category': 'Visualization',
            'capabilities': ['Template', 'Rscript', 'Visualization Preference'],
        },
    }


def template_yaml(template_id: str, name: str, base_template: str) -> str:
    return f"""apiVersion: omiga.ai/unit/v1alpha1
kind: Template
metadata:
  id: {template_id}
  version: 0.1.0
  name: {name}
  description: User/project preference visualization template promoted from an editable Rscript source artifact.
  tags:
    - visualization
    - r
    - preference-template
classification:
  category: visualization/preference
  tags:
    - visualization
    - r
    - preference-template
  stageInput:
    - table
  stageOutput:
    - static_figure
exposure:
  exposeToAgent: true
interface:
  inputs:
    table:
      kind: file
      required: true
      description: Input CSV/TSV table.
  params:
    delimiter:
      kind: enum
      enum: [auto, tab, comma]
      default: auto
  outputs:
    figure_png:
      kind: file
      glob: figure.png
      required: true
    figure_pdf:
      kind: file
      glob: figure.pdf
      required: true
    plot_script:
      kind: file
      glob: plot-script.R
      required: true
    rerun_script:
      kind: file
      glob: rerun.sh
      required: true
runtime:
  envRef: r-base
template:
  engine: jinja2
  entry: ./template.R.j2
aliases:
  - {template_id}
preference:
  baseTemplate: {base_template}
execution:
  interpreter: Rscript
  argv:
    - ${{inputs.table}}
    - ${{outdir}}
    - ${{params.delimiter}}
"""


def promote_script(raw: str, input_path: str | None, outdir_path: str | None) -> str:
    text = raw
    # Keep the rendered helper path by default. Preference templates are intentionally minimal
    # and do not copy the bundled helper library unless a future explicit freeze workflow asks
    # for that. This preserves the source artifact's known-good execution context.
    if input_path:
        text = text.replace(input_path, '{{ inputs.table }}')
    if outdir_path:
        text = text.replace(outdir_path, '{{ outdir }}')
    return text


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--script', required=True, help='Path to an editable Rscript source artifact, for example plot-script.R')
    parser.add_argument('--scope', choices=['user', 'project'], default='project')
    parser.add_argument('--project-root', default=os.getcwd())
    parser.add_argument('--id', required=True, help='New template id')
    parser.add_argument('--name', default=None, help='Display name')
    parser.add_argument('--base-template', default='unknown')
    parser.add_argument('--input-path', default=None, help='Known input path to replace with {{ inputs.table }}')
    parser.add_argument('--outdir-path', default=None, help='Known output dir to replace with {{ outdir }}')
    parser.add_argument('--dry-run', action='store_true')
    args = parser.parse_args()

    script_path = Path(args.script).expanduser().resolve()
    if not script_path.is_file():
        raise SystemExit(f'plot script not found: {script_path}')
    template_id = safe_id(args.id)
    name = args.name or template_id.replace('_', ' ').replace('-', ' ').title()
    project_root = Path(args.project_root).resolve()
    marketplace_name, marketplace_root, plugin_name = marketplace_paths(args.scope, project_root)
    plugin_root = marketplace_root / plugin_name
    template_root = plugin_root / 'templates' / template_id

    promoted = promote_script(script_path.read_text(), args.input_path, args.outdir_path)
    yaml = template_yaml(template_id, name, args.base_template)
    manifest = plugin_manifest(plugin_name, 'User R Visualization Preferences' if args.scope == 'user' else 'Project R Visualization Preferences')

    files = {
        plugin_root / 'plugin.json': json.dumps(manifest, indent=2) + '\n',
        template_root / 'template.yaml': yaml,
        template_root / 'template.R.j2': promoted,
    }
    for path, content in files.items():
        if args.dry_run:
            print(f'[dry-run] write {path}\n{content[:1200]}{"..." if len(content) > 1200 else ""}\n')
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content)
    upsert_marketplace(marketplace_root / 'marketplace.json', marketplace_name, plugin_name, args.dry_run)
    print(f'Promoted {script_path} to {template_root}')
    print('Install/enable the plugin through Omiga plugin settings if it is not already active.')


if __name__ == '__main__':
    main()
