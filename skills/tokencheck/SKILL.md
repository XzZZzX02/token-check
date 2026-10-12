---
name: tokencheck
description: "Use when the user asks how to install, configure, run, troubleshoot, or integrate the tokencheck/tkc CLI for local Claude Code, Codex, OpenCode, Pi, and Kimi Code token usage reports, snapshots, pricing, terminal charts, or Obsidian dashboards."
---

# Tokencheck

Use this skill to help users operate the `tokencheck` Rust CLI. The crate is published as `ethan-tkc`; installed binaries are `tkc` and `tokencheck`.

## First Checks

1. Prefer `tkc --version` and `tkc --help` to confirm the installed CLI.
2. Use `tkc doctor` when the user has missing data, wrong paths, unreadable snapshots, pricing-file issues, or Obsidian integration problems.
3. Use `tkc config show` before changing persistent settings, and `tkc pricing show` before changing prices.
4. Distinguish report commands from write commands:
   - `tkc fetch` writes or updates the JSON snapshot.
   - `summary`, `days`, `heatmap`, `projects`, `models`, and `tools` read and render reports.
   - `tkc pricing use` and `tkc config` write the config file; `tkc pricing init` writes pricing files.
   - `tkc obsidian` writes or updates the Obsidian dashboard Markdown file.

## Installation

Install or update from crates.io:

```bash
cargo install ethan-tkc
cargo install ethan-tkc --force
```

Install only this Codex Skill without cloning the source repository:

```bash
python3 ~/.codex/skills/.system/skill-installer/scripts/install-skill-from-github.py \
  --repo hanlife02/token-check \
  --path skills/tokencheck
```

If old local binaries conflict:

```bash
cargo uninstall tokencheck
cargo install ethan-tkc
```

From a source checkout:

```bash
cargo run --bin tkc -- summary
cargo install --path .
```

## Core Usage

Fast overview:

```bash
tkc
tkc summary
```

Persist scan results:

```bash
tkc fetch
```

Read only from the saved snapshot:

```bash
tkc summary --from-json
tkc days --from-json --limit 30
```

Source filters:

```bash
tkc summary --source claude
tkc models --source codex
tkc days --source opencode
tkc tools --source pi
tkc summary --source kimi
```

Date filters are inclusive and affect report commands only:

```bash
tkc summary --since 30d
tkc days --since 2026-05-01 --until today
tkc models --from-json --since 2026-01-01 --until 2026-03-31
```

Terminal visualizations:

```bash
tkc days --chant --limit 30
tkc heatmap --months 12
```

## Configuration

Interactive configuration:

```bash
tkc config
```

Show or reset config:

```bash
tkc config show
tkc config reset
```

Blank input keeps and saves the current value. `none`, `null`, or `-` clears optional path fields.

Important config fields:

- `source`: default data source, one of `all`, `claude`, `codex`, `opencode`, `pi`, or `kimi`.
- `data_file`: default JSON snapshot path used by `fetch` and `--from-json`.
- `pricing_profile`: active pricing profile name; `builtin` keeps embedded default prices only.
- `pricing_dir`: directory that holds pricing profiles; defaults to `~/.config/tokencheck/pricing`.
- `pricing_files`: extra pricing files loaded in order on top of the active profile.
- `pricing_file`: legacy single pricing file; still honored as the first extra file.
- `pricing_builtin`: set to `false` to ignore embedded default prices entirely.
- `obsidian_snapshot_file`: JSON snapshot path for Obsidian integration.
- `obsidian_dashboard_file`: Markdown dashboard path for Obsidian integration.
- `limit`: default row/chart count.
- `heatmap_months`: default `heatmap` month count.
- `language`: `en` or `zh`.

## Pricing Profiles

Every price comes from a JSON pricing file; the embedded defaults live in `assets/pricing/*.json`, are compiled into the binary, and are copied and edited with `tkc pricing init`. A freshly copied binary needs no pricing files at all.

Where profiles are read from, in order:

1. `--pricing-dir <DIR>`.
2. `pricing_dir` in the config file.
3. `<config dir>/pricing`, which defaults to `~/.config/tokencheck/pricing`.

The config file itself comes from `$TOKENCHECK_CONFIG`, else `$XDG_CONFIG_HOME/tokencheck/config.json`, else `$HOME/.config/tokencheck/config.json`. When none of those exist (launchd, cron, containers without `HOME`) reports still run on the embedded prices and `tkc pricing show` prints `Pricing directory: none`; using a profile then requires `HOME`, `XDG_CONFIG_HOME`, `TOKENCHECK_CONFIG`, or an explicit `--pricing-dir`.

`--home` only selects the directory that is scanned for agent logs; it never affects config or pricing lookup. Relative paths in the config file resolve against the current working directory, like `--pricing-file` does.

```bash
tkc pricing list                        # profiles found in the pricing directory
tkc pricing show                        # effective layers, order, accounting
tkc pricing use personal                # persist a profile
tkc pricing use builtin                 # go back to embedded defaults
tkc pricing init                        # write editable copies to <pricing_dir>/default/
tkc pricing check ~/proxy/claude.json   # validate a pricing file
tkc pricing match gpt-5.6               # explain which file prices a model
```

Precedence, lowest to highest: embedded defaults → active profile files → `pricing_file` / `pricing_files` / `--pricing-file`. Within a layer, exact model keys beat wildcard keys, source-scoped entries beat generic ones, and `"unpriced": true` blocks fallback to lower layers.

A profile is either `<pricing_dir>/<name>.json` or a `<pricing_dir>/<name>/` directory whose `*.json` files are loaded in file-name order.

## Pricing Files

Use `--pricing-file`, `pricing_files`, or a profile when unknown proxy models need prices, built-in prices need overrides, or the same model costs differently per source.

Pricing file format, dollars per 1M tokens:

```json
{
  "schema": 2,
  "name": "personal",
  "sources": ["claude"],
  "accounting": {
    "default": "cached_input_separate",
    "by_source": { "codex": "cached_input_subset" },
    "legacy_codex_event_subset": true
  },
  "models": {
    "my-proxy-model": {
      "input": 2.0,
      "cached_input": 0.5,
      "cache_creation_5m": 3.0,
      "cache_creation_1h": 4.0,
      "output": 8.0
    },
    "tiered-model": {
      "input": 2.0,
      "cached_input": 0.2,
      "output": 12.0,
      "tiers": [
        { "prompt_tokens_over": 200000, "input": 4.0, "cached_input": 0.4, "output": 18.0 }
      ]
    },
    "shared-model": [
      { "source": "claude", "input": 10.0, "output": 20.0 },
      { "source": "codex", "input": 1.0, "output": 2.0 }
    ],
    "blocked-model": { "unpriced": true }
  }
}
```

Rules:

- `input` and `output` are required unless `"unpriced": true`.
- `cached_input` defaults to `input`; `cache_creation_5m` and `cache_creation_1h` default to `0`.
- Model keys may end with `*` for prefix matching; `-2026...` style snapshot suffixes fall back to the base key.
- `sources` / `source` scope a file or a single entry to `claude`, `codex`, `opencode`, `pi`, or `kimi`, so one model can be priced per source and split across several files.
- `tiers[].prompt_tokens_over` switches to a higher price band; unset tier fields fall back to the base price.
- `note` is free-form text shown by `tkc pricing match`.
- Model names are normalized like built-in prices: case-insensitive, `_` becomes `-`, and `provider/model` matches by the last segment.
- Higher layers override lower layers; `unknown` and `<synthetic>` are never priced.

## Obsidian Workflow

Configure both Obsidian paths via `tkc config`:

```text
Obsidian snapshot file: /path/to/Vault/data/tokencheck.json
Obsidian dashboard file: /path/to/Vault/2 - Docs/token-check/Token Usage Dashboard.md
```

Generate or update the dashboard:

```bash
tkc obsidian
```

Refresh the snapshot first, then update the dashboard:

```bash
tkc obsidian --fetch
```

The generated dashboard is one Markdown file with five DataviewJS sections: Summary, Recent Days, Top Models, Top Projects, and Top Tools. Summary uses one primary total-token metric plus a compact supporting grid. Recent Days retains the 365-day usage heatmap. The three Top sections render pie charts, show categories with at least 5% share separately, combine smaller categories into `Other`, and reveal the slice label, value, and percentage on hover. The dashboard reads the snapshot with a vault-relative path when possible, usually `data/tokencheck.json`, adapts to Obsidian light and dark themes, and allows the annual heatmap to scroll horizontally in narrow panes.

`tkc obsidian` overwrites the configured dashboard file. It does not delete adjacent section notes created by older versions because users may have edited them.

Obsidian requirements:

- Install and enable the Dataview community plugin.
- Enable Dataview JavaScript queries.
- Open the configured dashboard Markdown file.

## Troubleshooting

Start with:

```bash
tkc doctor
```

Common fixes:

- Missing Claude data: confirm `$HOME/.claude/projects` exists or pass `--home`.
- Missing Codex data: confirm `$HOME/.codex/sessions` exists or pass `--home`.
- Missing OpenCode data: confirm `$HOME/.local/share/opencode` exists or pass `--home`.
- Missing Pi data: confirm `$HOME/.pi/agent/sessions` exists or pass `--home`.
- Missing Kimi Code data: confirm `$HOME/.kimi-code/sessions` exists or pass `--home`.
- No saved history: run `tkc fetch`.
- Obsidian dashboard missing data: run `tkc obsidian --fetch`, then reopen or refresh the note.
- JSON file is hidden in Obsidian: enable `Files and links -> Detect all file extensions`, or use the Dashboard Markdown instead of opening JSON directly.
- Pricing warnings: run `tkc pricing match <model>` to see which layer matched, then add or fix an entry in a pricing file. `tkc pricing check` validates hand-written files, and `tkc pricing show` confirms which files are actually active.
- Unexpected costs after switching profiles: run `tkc pricing show` and check the layer order; the last layer wins, and `tkc pricing use builtin` returns to embedded defaults.

## Development And Release Checks

When modifying the project, run:

```bash
cargo fmt --all -- --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo publish --dry-run --allow-dirty
```

For releases, bump `Cargo.toml`, ensure `Cargo.lock` follows, commit, and push to `main` to trigger the Cargo publish workflow.
