# tokencheck

`tokencheck` 是一个本地优先的 Rust 命令行工具，用来统计本机 Claude Code、Codex、OpenCode、Pi 和 Kimi Code 的 token 使用情况、工具调用、项目分布、模型分布和估算成本。

- crates.io 包名：`ethan-tkc`
- 安装后命令：`tkc`、`tokencheck`
- 当前版本：`1.3.0`
- License：MIT

当前版本优先读取本地日志和会话文件，只统计结构化元数据、token usage 和工具名；默认不会展示 prompt、回复正文、shell 命令参数、工具参数或文件快照内容。

## 目录

- [功能概览](#功能概览)
- [安装](#安装)
- [快速开始](#快速开始)
- [配置](#配置)
- [Codex Skill](#codex-skill)
- [Obsidian 集成](#obsidian-集成)
- [命令参考](#命令参考)
- [数据与快照](#数据与快照)
- [计费](#计费)
- [计费配置](#计费配置)
- [隐私边界](#隐私边界)
- [已知限制](#已知限制)
- [开发](#开发)
- [发布](#发布)
- [License](#license)

## 功能概览

- 统计 Claude Code、Codex、OpenCode、Pi 和 Kimi Code 的总 token 使用量。
- 按日期、项目、模型和工具名聚合使用情况。
- 用 `tkc days --chant` 输出 Tokscale 风格终端柱状图。
- 用 `tkc heatmap` 输出终端周历热力图。
- 按模型价格估算每日、项目、模型和总美元成本。
- 计费完全由配置文件驱动：内置价格本身就是 `assets/pricing/*.json` 数据文件，Rust 代码里没有硬编码的模型名或价格表。
- 支持计费配置（pricing profile）切换，一个配置可以包含多个价格文件，按文件名顺序叠加。
- 支持把价格限定到指定数据来源，同一个模型在不同来源可以命中不同价格文件、使用不同价格。
- 支持自定义模型价格文件，覆盖或补充内置价格。
- 支持 JSON 快照，避免本机日志清理后丢失历史统计。
- 支持 `all`、`claude`、`codex`、`opencode`、`pi`、`kimi` 六种数据来源过滤。
- 支持 `--since`、`--until` 按日期过滤报表。
- 支持 `tkc config` 交互式配置默认行为。
- 支持 `tkc doctor` 诊断配置、快照和本机数据目录。
- 支持 `tkc obsidian` 生成 Obsidian DataviewJS Dashboard。
- 支持英文和中文输出。

## 安装

### 从 crates.io 安装

需要本机已经安装 Rust 和 Cargo。

```bash
cargo install ethan-tkc
```

安装后可以使用两个等价入口：

```bash
tkc --help
tokencheck --help
tkc --version
```

### 更新

```bash
cargo install ethan-tkc --force
```

如果你之前用旧包名或本地路径安装过，可能会看到 `binary already exists`。先移除旧安装，再安装当前发布包：

```bash
cargo uninstall tokencheck
cargo install ethan-tkc
```

### 从源码运行

开发目录内临时运行：

```bash
cargo run --bin tkc -- summary
cargo run --bin tokencheck -- summary
```

开发目录内安装当前源码版本：

```bash
cargo install --path .
```

## 快速开始

查看总览。不传子命令时，默认等价于 `summary`：

```bash
tkc
tkc summary
```

保存或更新 JSON 快照：

```bash
tkc fetch
```

交互式配置默认行为：

```bash
tkc config
```

诊断当前配置、快照和本机数据目录：

```bash
tkc doctor
```

查看最近 10 天的表格统计：

```bash
tkc days --limit 10
```

查看最近日期的终端柱状图：

```bash
tkc days --chant --limit 30
```

查看最近 12 个月的终端热力图：

```bash
tkc heatmap
```

只看 Codex 数据：

```bash
tkc summary --source codex
tkc models --source codex
```

只从快照文件读取，不实时扫描 `$HOME`：

```bash
tkc summary --from-json
tkc days --from-json --limit 30
```

使用自定义快照文件：

```bash
tkc fetch --data-file data/workstation.json
tkc summary --from-json --data-file data/workstation.json
```

查看当前生效的计费配置：

```bash
tkc pricing
tkc pricing list
tkc pricing match gpt-5.6
```

把内置价格落盘成可编辑的计费配置，然后切换过去：

```bash
tkc pricing init
tkc pricing use default
```

临时用另一套价格跑一次报表：

```bash
tkc pricing use work
tkc summary --pricing-profile work
tkc models --pricing-profile personal --pricing-file ~/proxy/claude.json
```

## 配置

`tkc config` 会在终端中一步一步询问配置项，并保存为 JSON 配置文件。每一步直接按 Enter 会保留当前显示的值；如果之前没有配置过，则使用内置默认值并保存。

```bash
tkc config
tkc config show
tkc config reset
```

默认配置文件位置：

```text
~/.config/tokencheck/config.json
```

如果设置了 `XDG_CONFIG_HOME`，配置会写入：

```text
$XDG_CONFIG_HOME/tokencheck/config.json
```

可配置项：

| 配置项 | 默认值 | 说明 |
| --- | --- | --- |
| `language` | `en` | 输出语言。支持 `en` 和 `zh`。 |
| `source` | `all` | 默认数据来源。支持 `all`、`claude`、`codex`、`opencode`、`pi`、`kimi`。 |
| `data_file` | `data/tokencheck.json` | 默认快照文件路径，也就是 `fetch` 保存和 `--from-json` 读取的位置。 |
| `pricing_profile` | 无 | 默认计费配置名。`builtin` 或留空表示只用内置默认价格。 |
| `pricing_dir` | 无 | 计费配置目录。未配置时使用 `~/.config/tokencheck/pricing`（受 `XDG_CONFIG_HOME`、`TOKENCHECK_CONFIG` 影响）。 |
| `pricing_files` | 无 | 额外价格文件列表，按顺序叠加在计费配置之上，后面的文件优先。 |
| `pricing_file` | 无 | 旧版单一价格文件字段，仍然生效，等价于 `pricing_files` 的第一项。 |
| `pricing_builtin` | `true` | 是否继续加载内置默认价格。设为 `false` 时只使用计费配置和额外价格文件。 |
| `obsidian_snapshot_file` | 无 | Obsidian 使用的 JSON 快照路径。`tkc obsidian --fetch` 会写入这个文件；未配置时回退到 `data_file`。 |
| `obsidian_dashboard_file` | 无 | Obsidian Dashboard Markdown 文件路径。`tkc obsidian` 会创建或覆盖这个文件。 |
| `limit` | `20` | 默认表格或排行行数。 |
| `heatmap_months` | `12` | `heatmap` 默认展示月份数。 |

示例流程：

```text
tokencheck config
Config file: /Users/you/.config/tokencheck/config.json
Press Enter on a blank input to keep and save the shown value.

Language [en/zh] (current: en):
Default source [all/claude/codex/opencode/pi/kimi] (current: all):
Snapshot data file (current: data/tokencheck.json): ~/.tokencheck/usage.json
Pricing profile [builtin/default/personal] (current: builtin):
Pricing profile directory [path/none] (current: none):
Extra pricing files [path,path/none] (current: none):
Legacy single pricing file [path/none] (current: none):
Obsidian snapshot file [path/none] (current: none): /Users/you/Vault/data/tokencheck.json
Obsidian dashboard file [path/none] (current: none): /Users/you/Vault/2 - Docs/token-check/Token Usage Dashboard.md
Default row limit (current: 20):
Default heatmap months (current: 12):

config saved: /Users/you/.config/tokencheck/config.json
```

`Pricing profile` 只会列出 `pricing_dir` 里实际存在的配置名；也可以输入 `builtin` 回到内置价格。想切换配置而不进交互界面，用 `tkc pricing use <name>`。

命令行参数会覆盖 `tkc config` 保存的默认值：

```bash
tkc summary --source codex
tkc fetch --data-file data/one-off.json
tkc heatmap --months 6
```

## Codex Skill

本仓库附带一个 Codex Skill，用来让 Codex 更稳定地回答和操作 `tkc`。

```text
skills/tokencheck/
├── SKILL.md
└── agents/openai.yaml
```

如果你只是使用工具，不需要 clone 本仓库。先安装 CLI：

```bash
cargo install ethan-tkc
```

然后通过 Codex 自带的 skill installer 只安装 Codex Skill：

```bash
python3 ~/.codex/skills/.system/skill-installer/scripts/install-skill-from-github.py \
  --repo hanlife02/token-check \
  --path skills/tokencheck
```

如果本机已经存在旧版 skill，installer 会拒绝覆盖。可以先备份旧目录，再重新安装：

```bash
mv ~/.codex/skills/tokencheck ~/.codex/skills/tokencheck.bak
python3 ~/.codex/skills/.system/skill-installer/scripts/install-skill-from-github.py \
  --repo hanlife02/token-check \
  --path skills/tokencheck
```

安装后重启 Codex，并用下面的提示触发：

```text
Use $tokencheck to configure tkc and troubleshoot local token usage reports.
```

验证本仓库内的 skill 文件：

```bash
python3 ~/.codex/skills/.system/skill-creator/scripts/quick_validate.py skills/tokencheck
```

验证已经安装到本机 Codex 的 skill：

```bash
python3 ~/.codex/skills/.system/skill-creator/scripts/quick_validate.py ~/.codex/skills/tokencheck
```

如果你正在开发或审查本项目，也可以从仓库根目录复制 skill 到 Codex skill 目录：

```bash
mkdir -p ~/.codex/skills
cp -R skills/tokencheck ~/.codex/skills/
```

## Obsidian 集成

`tkc obsidian` 会根据配置生成 Obsidian DataviewJS 报表笔记。它使用两个配置项：

| 配置项 | 用途 |
| --- | --- |
| `obsidian_snapshot_file` | DataviewJS 读取的 JSON 数据源。 |
| `obsidian_dashboard_file` | 要创建或更新的 Markdown 报表笔记。 |

示例配置：

```text
Obsidian snapshot file [path/none] (current: none): /Users/you/Vault/data/tokencheck.json
Obsidian dashboard file [path/none] (current: none): /Users/you/Vault/2 - Docs/token-check/Token Usage Dashboard.md
```

生成 Dashboard：

```bash
tkc obsidian
```

生成 Dashboard 并先更新 Obsidian 快照：

```bash
tkc obsidian --fetch
```

临时覆盖路径：

```bash
tkc obsidian \
  --snapshot-file "/Users/you/Vault/data/tokencheck.json" \
  --dashboard-file "/Users/you/Vault/2 - Docs/token-check/Token Usage Dashboard.md"
```

生成结果是单个 Dashboard Markdown 文件，按顺序包含 Summary、Recent Days、Top Models、Top Projects 和 Top Tools 五个 DataviewJS 区块。Summary 使用总 token 主指标和紧凑统计网格；Recent Days 保持最近 365 天的用量热力图；三个 Top 区块使用饼图展示分布，只单独列出占比不低于 5% 的模型、项目或工具，其余合并为 `Other`。鼠标移到饼图扇区时会显示对应名称、数值和占比。Dashboard 会适配 Obsidian 明暗主题，年度热力图在窄窗口中可横向滚动。

每次运行 `tkc obsidian` 都会覆盖配置指定的 Dashboard 文件，但不会删除旧版本生成的相邻分块文件，避免移除用户可能手动修改过的笔记。Obsidian 侧需要安装并启用 Dataview 插件，同时打开 Dataview 的 JavaScript 查询功能。

JSON 通常作为数据源使用，不需要直接在 Obsidian 中阅读。如果想让 Obsidian 文件列表显示 JSON，可以启用 `Files and links -> Detect all file extensions`。

## 命令参考

### 命令一览

| 命令 | 作用 | 常用场景 |
| --- | --- | --- |
| `tkc` | 默认执行 `summary` | 快速查看总览 |
| `tkc config` | 交互式配置默认行为 | 设置语言、快照路径、默认来源和默认显示数量 |
| `tkc config show` | 查看当前生效配置 | 检查配置文件路径和默认值 |
| `tkc config reset` | 删除配置文件并恢复默认值 | 清空本机自定义配置 |
| `tkc pricing` | 等价于 `tkc pricing show` | 查看当前生效的价格层和计费口径 |
| `tkc pricing list` | 列出可用的计费配置 | 看本机有哪些价格文件组合 |
| `tkc pricing show` | 展示生效的价格文件、优先级和计费口径 | 确认某个模型会命中哪个文件 |
| `tkc pricing use <name>` | 把某个计费配置写进配置文件 | 一条命令切换计费方案 |
| `tkc pricing init` | 把内置价格写成可编辑的价格文件 | 在本地改价格、加新模型 |
| `tkc pricing check [FILE...]` | 校验价格文件 | 检查手写 JSON 是否合法 |
| `tkc pricing match <MODEL>` | 解释某个模型在各来源命中的价格 | 排查“为什么这个模型是这个价格” |
| `tkc doctor` | 诊断配置、快照和本机数据目录 | 排查安装后没有数据、路径不对或快照不可读 |
| `tkc obsidian` | 生成 Obsidian DataviewJS Dashboard | 在 Obsidian 中查看 token 用量报表 |
| `tkc fetch` | 扫描本机数据并合并写入 JSON 快照 | 保存历史、换机器前备份、日志清理前归档 |
| `tkc summary` | 输出整体统计和分来源统计 | 查看总 token、session、工具调用和估算成本 |
| `tkc days` | 按日期聚合 token 和成本 | 看最近哪些天用量最高 |
| `tkc days --chant` | 用 Tokscale 风格柱状图展示每日 token | 在终端中快速看趋势 |
| `tkc heatmap` | 用周历热力图展示每日 token 强度 | 看长期使用节奏 |
| `tkc projects` | 按项目路径聚合 token 和成本 | 找出最耗 token 的项目 |
| `tkc models` | 按模型聚合 token 和成本 | 分析模型使用和成本结构 |
| `tkc tools` | 按工具名统计调用次数 | 查看 Read、Edit、Bash 等工具的使用频率 |

### 全局参数

这些参数可以放在子命令前后：

```bash
tkc --source codex summary
tkc summary --source codex
```

| 参数 | 默认值 | 说明 |
| --- | --- | --- |
| `--source all\|claude\|codex\|opencode\|pi\|kimi` | 配置值，初始为 `all` | 选择数据来源。`all` 同时统计 Claude Code、Codex、OpenCode、Pi 和 Kimi Code。 |
| `--home <PATH>` | 当前 `$HOME` | 指定要扫描的 home 目录。适合测试、读取备份目录或统计另一份用户数据。 |
| `--limit <N>` | 配置值，初始为 `20` | 控制排行或日期输出数量。主要影响 `days`、`projects`、`models`、`tools`。 |
| `--from-json` | 关闭 | 只读取 JSON 快照，不扫描实时本机日志。对 `fetch` 无效。 |
| `--data-file <PATH>` | 配置值，初始为 `data/tokencheck.json` | 指定 `fetch` 写入或 `--from-json` 读取的快照文件。 |
| `--pricing-profile <NAME>` | 配置值 | 本次运行使用的计费配置名；`builtin` 表示只用内置价格。 |
| `--pricing-dir <DIR>` | 配置值，初始为 `~/.config/tokencheck/pricing` | 指定计费配置目录。 |
| `--pricing-file <PATH>` | 配置值，初始为无 | 额外加载一个价格 JSON 文件，可以重复传入，后面的文件覆盖前面的。 |
| `--no-builtin-pricing` | 关闭 | 不加载内置默认价格，只使用计费配置和额外价格文件。 |
| `--since <DATE>` | 无 | 只统计该日期及之后的数据。支持 `YYYY-MM-DD`、`today`、`7d`、`30d`。 |
| `--until <DATE>` | 无 | 只统计该日期及之前的数据。支持 `YYYY-MM-DD`、`today`、`7d`、`30d`。 |
| `-h`, `--help` | - | 打印命令帮助。 |

`--home` 只影响实时扫描。使用 `--from-json` 时，命令只读取 `--data-file`，不会访问 `$HOME`。

`--since` 和 `--until` 只影响报表命令，不影响 `fetch` 写入快照，也不影响 `doctor` 诊断；这样快照会保持完整，报表可以按需切片查看。过滤区间是闭区间，包含起止日期。

日期过滤示例：

```bash
tkc summary --since 2026-05-01
tkc days --since 30d
tkc models --since 7d --until today
tkc projects --from-json --since 2026-01-01 --until 2026-03-31
```

相对日期按当前日期向前计算，例如 `7d` 表示 7 天前的日期。

### `tkc` / `tkc summary`

`tkc` 不带子命令时等价于：

```bash
tkc summary
```

`summary` 输出整体统计和按来源拆分的表格。它适合回答这些问题：

- 一共扫描到了多少 session？
- 多少 session 有 token usage？
- 涉及多少项目和模型？
- 总 token 是多少？
- 估算成本是多少？
- Claude Code、Codex、OpenCode、Pi 和 Kimi Code 分别贡献了多少输入、缓存、输出和工具调用？

示例：

```bash
tkc summary
tkc summary --source claude
tkc summary --source codex
tkc summary --source opencode
tkc summary --source pi
tkc summary --source kimi
tkc summary --from-json
tkc summary --since 30d
```

输出顶部字段：

| 字段 | 含义 |
| --- | --- |
| `sessions scanned` | 扫描到并去重后的 session 数量。 |
| `sessions with usage` | 至少包含一次 token usage 的 session 数量。 |
| `projects seen` | 出现过的项目路径数量，不含 `unknown`。 |
| `models seen` | 出现过的模型数量，不含 `unknown`。 |
| `usage events` | 参与聚合的 usage 事件数量。 |
| `tool calls` | 解析到的工具调用事件数量。 |
| `total tokens` | input、cache、output、reasoning 等字段合并后的总 token。 |
| `estimated cost` | 按内置价格表估算的美元成本。 |

### `tkc fetch`

`fetch` 扫描本机 Claude Code、Codex、OpenCode、Pi 和 Kimi Code 数据，并把结果合并保存到 JSON 快照。默认写入：

```text
data/tokencheck.json
```

示例：

```bash
tkc fetch
tkc fetch --source codex
tkc fetch --source opencode
tkc fetch --source pi
tkc fetch --source kimi
tkc fetch --source claude --data-file data/claude.json
tkc fetch --home /Users/yourname --data-file data/backup.json
```

`fetch` 的合并策略是保守的：

- 新 session 会追加。
- 新 usage event 会追加。
- 新 tool event 会追加。
- 同一个 usage event 只有在新扫描的 token 总量更大时才升级。
- 如果新扫描结果缺少旧快照中已有的数据，旧数据会保留。
- 如果没有新增或升级内容，已有 JSON 文件不会被重写。

`fetch` 输出的计数含义：

| 输出 | 含义 |
| --- | --- |
| `snapshot saved` | 快照被创建或更新。 |
| `snapshot unchanged` | 扫描后没有新增或升级内容。 |
| `sessions: A -> B` | 合并前后 session 数量。 |
| `usage events: A -> B (+N, upgraded M)` | 新增 usage 事件数量和升级事件数量。 |
| `tool calls: A -> B (+N)` | 新增工具调用事件数量。 |
| `total tokens: A -> B` | 合并前后的总 token。 |

### `tkc days`

`days` 按日期聚合 token 使用量和估算成本，默认显示最近 20 个有 usage 的日期，最新日期在前。

```bash
tkc days
tkc days --limit 7
tkc days --source codex --limit 30
tkc days --from-json --limit 90
tkc days --since 2026-05-01 --until today
```

表格字段：

| 字段 | 含义 |
| --- | --- |
| `date` | 日期，格式为 `YYYY-MM-DD`。 |
| `sessions` | 当天涉及的 session 数。 |
| `input` | 普通输入 token。 |
| `cached` | cache read / cached input token。 |
| `cache_create` | Claude cache write / cache creation token。 |
| `output` | 输出 token。 |
| `reasoning` | Codex reasoning output token。 |
| `total` | 当天总 token。 |
| `cost` | 当天估算成本。 |

### `tkc days --chant`

`days --chant` 使用 Tokscale 风格的圆角面板和竖直彩色柱状图展示每日 total tokens。

```bash
tkc days --chant
tkc days --chant --limit 60
tkc days --chant --source claude
```

图表会根据终端宽度自动裁剪可显示日期数量：

- 终端足够宽时，显示 `--limit` 指定的全部日期。
- 终端较窄时，只显示能放下的最近日期，并在面板顶部显示 `可见天数/请求天数`。
- 终端过窄时，显示 `Terminal is too narrow for the chart`，避免边框和内容错位。

图表底部依次显示日期、token 数和成本。颜色强度和 `heatmap` 使用同一套等级。

### `tkc heatmap`

`heatmap` 按日聚合 total tokens，并输出周历热力图。横向是周，纵向是星期，颜色越亮表示当天 token 越多。

```bash
tkc heatmap
tkc heatmap --months 6
tkc heatmap --months 24 --from-json
tkc heatmap --source codex
```

规则：

- 默认展示配置中的 `heatmap_months`，初始为最近 12 个月。
- `--months <N>` 控制月份跨度，最小按 1 个月处理。
- 时间范围以数据中的最新日期为结束月份，不一定是今天。
- 色阶按当前可见范围内的最大日用量归一化。
- 终端较窄时会自动裁剪为能显示的最近周数。
- 终端过窄时，显示 `Terminal is too narrow for the heatmap`。

`heatmap` 只展示 token 强度，不展示成本。如果要看每日成本，用 `tkc days`。

### `tkc projects`

`projects` 按 `source + project path` 聚合 usage，按 total tokens 从高到低排序。

```bash
tkc projects
tkc projects --limit 10
tkc projects --source claude
tkc projects --from-json --data-file data/tokencheck.json
tkc projects --since 30d
```

这个命令适合找出最耗 token 的项目。输出字段和 `days` 类似，但第一列是 `source`，第二列是 `project`。

注意：

- Claude Code 的项目通常来自 `~/.claude/projects` 下的项目路径映射。
- Codex 的项目来自 session 记录中的工作目录信息。
- OpenCode、Pi 和 Kimi Code 的项目来自各自 session 记录中的工作目录信息。
- 如果原始数据缺少项目路径，会显示为 `unknown`。

### `tkc models`

`models` 按 `source + model` 聚合 usage，按 total tokens 从高到低排序。

```bash
tkc models
tkc models --limit 20
tkc models --source codex
tkc models --from-json
tkc models --since 7d
```

这个命令适合分析哪些模型用得最多、哪些模型贡献了主要成本，以及是否还有未配置价格的模型名。

如果某个模型没有内置价格，`cost` 会带 `*`，并在命令结束后输出 warning。带 `*` 的成本是部分成本，不包含未定价模型。

### `tkc tools`

`tools` 按 `source + tool name` 统计工具调用次数，按调用次数从高到低排序。

```bash
tkc tools
tkc tools --limit 50
tkc tools --source claude
tkc tools --from-json
tkc tools --since 2026-05-01
```

输出字段：

| 字段 | 含义 |
| --- | --- |
| `source` | `Claude`、`Codex`、`OpenCode`、`Pi` 或 `Kimi Code`。 |
| `tool` | 工具名，例如 `Read`、`Edit`、`Bash`、`apply_patch`。 |
| `calls` | 工具调用次数。 |
| `days` | 该工具出现过的日期数量。 |
| `projects` | 该工具出现过的项目数量。 |

`tools` 不输出 token 或成本，因为工具调用事件本身只记录工具元数据；token 用量来自 usage events。

### `tkc pricing`

`pricing` 用来管理计费来源。不带子命令时等价于 `tkc pricing show`。

```bash
tkc pricing list
tkc pricing show
tkc pricing use personal
tkc pricing use builtin
tkc pricing init
tkc pricing init --force
tkc pricing check
tkc pricing check ~/proxy/claude.json
tkc pricing match gpt-5.6
tkc pricing match gpt-6-astra --source codex
```

`tkc pricing show` 输出示例：

```text
tokencheck pricing
Pricing directory: /Users/you/.config/tokencheck/pricing
Active profile: personal
Embedded defaults: enabled
Priced model entries: 127 entries

Active pricing layers, lowest precedence first:
#  source  models  file
-  ------  ------  ----------------------------------------------------------
1  all     0       defaults
2  all     41      openai
...
8  all     1       /Users/you/.config/tokencheck/pricing/personal/10-official.json
9  claude  2       /Users/you/.config/tokencheck/pricing/personal/20-claude-proxy.json
10 codex   2       /Users/you/.config/tokencheck/pricing/personal/30-codex-flat.json

Accounting:
source                              mode
----------------------------------  ---------------------
all                                 cached_input_separate
codex                               cached_input_separate
legacy single-event Codex sessions  cached_input_subset
```

`tkc pricing match <MODEL>` 会逐个来源列出命中的价格条目和所在文件，是排查“同一个模型为什么两个来源价格不同”的最快方式：

```text
Pricing match
model: gpt-5.6
Active profile: personal

source    matched key  input  cached  output  file
--------  -----------  -----  ------  ------  --------------------------------------------------
claude    gpt-5.6      $6     $0.6    $30     .../personal/20-claude-proxy.json
codex     gpt-5.6      $0      $0      $0      .../personal/30-codex-flat.json
opencode  gpt-5.6      $4     $0.4    $20     .../personal/10-official.json
pi        gpt-5.6      $4     $0.4    $20     .../personal/10-official.json
kimi      gpt-5.6      $4     $0.4    $20     .../personal/10-official.json
```

子命令说明：

| 子命令 | 作用 |
| --- | --- |
| `list` | 列出 `pricing_dir` 里的计费配置和内置配置，并标出当前生效项。计费配置可以是 `<name>.json` 文件，也可以是 `<name>/` 目录。 |
| `show` | 展示真正加载的价格层、加载顺序、每层模型数、计费口径和额外价格文件。 |
| `use <name>` | 校验配置存在后写入配置文件；`tkc pricing use builtin` 回到内置价格。 |
| `init` | 把内置的 `assets/pricing/*.json` 复制到 `<pricing_dir>/default/`，之后可以直接编辑。已存在的文件默认跳过，`--force` 覆盖。 |
| `check [FILE...]` | 解析并校验价格文件；不传文件时校验当前生效配置里的全部文件。校验失败会以非 0 退出码结束。 |
| `match <MODEL>` | 按来源列出命中的价格条目、单价和来源文件；`--source` 可以只看一个来源。 |

### `tkc doctor`

`doctor` 会检查当前环境是否能正常产出报表。它不会写入快照，也不会修改配置。

```bash
tkc doctor
tkc doctor --source codex
tkc doctor --source pi
tkc doctor --source kimi
tkc doctor --home /Users/yourname
tkc doctor --data-file data/workstation.json
tkc doctor --pricing-profile personal
tkc doctor --pricing-file ~/proxy/claude.json
```

诊断项包括：

| 检查项 | 含义 |
| --- | --- |
| `Config file` | 当前配置文件路径是否存在；不存在时会使用内置默认值。 |
| `Home directory` | 实时扫描使用的 home 目录是否存在。 |
| `Source filter` | 当前生效的数据来源过滤：`all`、`claude`、`codex`、`opencode`、`pi` 或 `kimi`。 |
| `Active profile` | 当前生效的计费配置名和计费配置目录。 |
| `Pricing files` | 实际加载了多少价格层、覆盖多少个模型、默认计费口径是什么；有文件解析失败时这里会报错。 |
| `Extra pricing files` | 通过 `pricing_files` 或 `--pricing-file` 指定的额外价格文件是否存在。 |
| `Claude Code data directory` | 是否能找到 `$HOME/.claude/projects`。 |
| `Codex data directory` | 是否能找到 `$HOME/.codex/sessions`。 |
| `OpenCode data directory` | 是否能找到 `$HOME/.local/share/opencode`。 |
| `Pi data directory` | 是否能找到 `$HOME/.pi/agent/sessions`。 |
| `Kimi Code data directory` | 是否能找到 `$HOME/.kimi-code/sessions`。 |
| `Snapshot file` | 配置或命令行指定的 JSON 快照是否存在且可读取。 |
| `Live scan` | 实时扫描当前本机日志后得到的 session、usage、tool 和 token 计数。 |
| `Report data` | 当前配置下是否已经有可用 usage 数据。 |

如果你安装后运行 `tkc` 没有数据，优先运行 `tkc doctor`。常见原因是 Claude Code、Codex、OpenCode、Pi 或 Kimi Code 日志目录不存在、`--home` 指错目录、只配置了某一个 `source`，或者快照文件还没有通过 `tkc fetch` 创建。

## 数据与快照

### 工作方式

报表命令默认会做三件事：

1. 读取 `tkc config` 保存的配置；如果没有配置文件，则使用内置默认值。
2. 扫描当前用户 `$HOME` 下的 Claude Code、Codex、OpenCode、Pi 和 Kimi Code 本地数据。
3. 如果配置的快照文件存在，默认是 `data/tokencheck.json`，把快照数据和实时扫描结果合并后展示。

这样可以保留历史统计：即使 Claude Code、Codex、OpenCode、Pi 或 Kimi Code 的原始日志后续被清理，只要你之前运行过 `tkc fetch`，快照中的旧数据仍然会参与报表。

```text
Claude/Codex/Pi/Kimi Code JSONL + OpenCode SQLite/JSON -> collector -> ReportData
data/tokencheck.json -> snapshot loader -> ReportData
ReportData -> source filter -> command aggregation -> terminal output
```

`fetch` 是唯一会写入快照的命令。其他报表命令只读取和展示数据。

### 数据来源

Claude Code：

```text
~/.claude/projects/**/*.jsonl
```

Codex：

```text
~/.codex/sessions/**/*.jsonl
```

OpenCode：

```text
~/.local/share/opencode/opencode.db
~/.local/share/opencode/storage/message/**/*.json
~/.local/share/opencode/storage/session/**/*.json
```

Pi：

```text
~/.pi/agent/sessions/**/*.jsonl
```

Kimi Code：

```text
~/.kimi-code/sessions/<wd_*>/<session>/agents/*/wire.jsonl
~/.kimi-code/sessions/<wd_*>/<session>/state.json
```

`wire.jsonl` 是 Kimi Code 每个 agent（包括 `main` 和 `agent-N` subagents）的事件日志，token usage 来自其中的 `step.end` 事件；`state.json` 提供 session 的工作目录和创建时间。

当前版本不默认扫描 `~/.codex/log/codex-tui.log`，因为该文件可能非常大，且包含更细的运行日志。

### JSON 快照

默认快照路径来自 `tkc config` 的 `data_file`，初始值是：

```text
data/tokencheck.json
```

推荐用法：

```bash
tkc fetch
tkc summary --from-json
tkc days --from-json --limit 30
```

快照适合这些场景：

- 本机日志可能被清理，但你想保留历史统计。
- 想把多个时间点的扫描结果累计在同一个 JSON 文件中。
- 想在不扫描 `$HOME` 的情况下查看报表。
- 想把统计数据带到另一台机器上查看。

报表命令默认会合并实时扫描和快照。要只看快照，必须加 `--from-json`。要永久修改快照位置，运行 `tkc config` 并设置 `Snapshot data file`；要临时覆盖，使用 `--data-file <PATH>`。

### 输出字段和统计口径

常见 token 字段：

| 字段 | 说明 |
| --- | --- |
| `input` | 普通输入 token。 |
| `cached` | 已缓存输入 token。Claude 侧对应 cache read；Codex 侧对应 cached input。 |
| `cache_create` | Claude cache creation / cache write token。 |
| `output` | 模型输出 token。 |
| `reasoning` | Codex reasoning output token。 |
| `total` | 工具按可用字段计算出的总 token。 |
| `cost` | 按内置价格估算的美元成本。 |

Claude Code：

- 递归扫描 `projects` 下的 JSONL 文件，包括 subagents。
- session 数按 `sessionId` 去重。
- token usage 按 `sessionId + message.id` 去重；没有 `message.id` 时使用 fallback key。
- 统计字段包括 input、cached input、cache creation input、output。

Codex：

- 递归扫描 `sessions` 下的 JSONL 文件。
- 每个 session 只取最后一条非空累计 `token_count.info.total_token_usage`。
- 统计字段包括 input、cached input、output、reasoning output、total。

Kimi Code：

- 递归扫描 `sessions` 下所有 agent 目录中的 `wire.jsonl`，包括 subagents。
- token usage 来自 `step.end` 事件；模型名按 `llm.request` 记录的 `turnStep`（`{turnId}.{step}`）关联到对应 step。
- session 元数据来自同目录的 `state.json`，`cwd` 作为项目路径，`createdAt` 作为 session 日期；缺失时回退为 session 目录名和 `unknown`。
- 统计字段包括 input、cached input、cache creation input、output。

## 计费

- `summary`、`days`、`projects` 和 `models` 会输出 `cost`，单位是美元。
- 所有价格都来自价格文件：内置默认价格是 `assets/pricing/*.json`，用户价格文件是同一套 schema。Rust 代码里没有模型名和价格表。
- Codex 数据里的 cached input 默认视为 input 的子集（旧版单事件会话），其他情况按独立字段计费。这个口径也写在价格文件里。
- Claude Code 数据里的 cache read/cache write 按独立字段计费。
- Claude 模型按 input、cache read、5 分钟 cache write、1 小时 cache write 和 output 分别计费。
- 其他文本模型按 input、cached input/cache hit 和 output 估算。
- 内置价格覆盖 OpenAI GPT-5.6（`gpt-5.6` / `gpt-5.6-sol` / `-terra` / `-luna` / `-cyber`）、GPT-6（`gpt-6-astra` / `gpt-6.1-sol` / `gpt-6-sol` / `gpt-6-luna`）、GPT-5.x / GPT-4.x / o 系列，Claude Opus 5.5 / 5 / 4.x、Sonnet 5.5 / 5 / 4.x、Haiku 5.5 / 4.5 / 3.x、Fable 5.1 / Mythos 5.1，Gemini 3.8 / 3.7 / 3.6 / 3.5 / 3.1 / 3 / 2.5 / 2.0，DeepSeek V4-Pro / Flash、MiMo V2.6 / V2.5、Kimi K3 / K2.x 和 Moonshot V1。
- 只要上层价格文件覆盖了同名模型，就会优先于下层；没有价格时成本显示带 `*`，并在 warning 中说明该模型未计入美元总额。
- 成本只按文本 token 估算，不包含订阅费、Batch/Flex/Priority 折扣、工具调用附加费、图片/音频/视频单独计费、税费或第三方代理加价。

### 价格解析顺序

一次运行实际加载的价格层按优先级从低到高是：

1. `assets/pricing/*.json` 内置默认价格，除非显式使用 `--no-builtin-pricing` 或把 `pricing_builtin` 设为 `false`。
2. 当前计费配置里的价格文件：`<pricing_dir>/<profile>.json`，或者 `<pricing_dir>/<profile>/` 目录下的 `*.json`（按文件名排序）。
3. 配置文件里的 `pricing_file`、`pricing_files`，以及命令行的 `--pricing-file`，按这个顺序追加。

查找某个事件的价格时，从最高优先级往下找，第一个命中的规则生效。同一层内部：

- 精确模型名优先于带 `*` 的通配前缀；通配前缀越长越优先；带 `-2026...` 这类日期后缀的 snapshot 名会回退到基础模型名。
- 对同一个模型名，声明了 `sources` 的条目优先于没有 `sources` 的通用条目；如果该条目只声明了别的来源，就继续往下找。
- `"unpriced": true` 会明确把模型标为未定价，并阻止继续回退到更低优先级的价格。

## 计费配置

计费配置（pricing profile）就是一组价格文件的命名组合。它解决两个问题：不同套餐要切换整套价格，以及同一个模型在不同来源要按不同价格计费。

### 配置文件和计费配置从哪里读

二进制本身不依赖任何外部价格文件：内置价格通过 `include_str!` 编译进可执行文件，把 `tkc` 拷到任何机器上都能直接按内置价格算成本。

配置文件位置按这个顺序确定（第一个命中的生效）：

1. `$TOKENCHECK_CONFIG`：直接指定 `config.json` 的完整路径。
2. `$XDG_CONFIG_HOME/tokencheck/config.json`。
3. `$HOME/.config/tokencheck/config.json`。
4. 都不存在：不读配置，全部使用内置默认值。

计费配置目录按这个顺序确定：

1. `--pricing-dir <DIR>`。
2. 配置文件里的 `pricing_dir`。
3. 配置文件所在目录下的 `pricing/`，也就是默认的 `~/.config/tokencheck/pricing`。
4. 上面都拿不到（没有 `HOME`、`XDG_CONFIG_HOME`、`TOKENCHECK_CONFIG`，也没传 `--pricing-dir`）：没有计费配置目录，只用内置价格，`tkc pricing show` 会显示 `Pricing directory: none`。

需要注意：

- `--home` 只决定扫描哪个目录下的 Claude Code / Codex 等日志，不影响配置文件、计费配置目录和价格文件的查找。
- 配置文件里的相对路径（`pricing_dir`、`pricing_files`、`pricing_file`）按运行 `tkc` 时的当前目录解析，和 `--pricing-file` 的语义一致；`~` 会展开为 `$HOME`。`tkc pricing show` 打印的是解析后的结果，可以直接确认。
- 在 launchd、cron、容器里 `HOME` 经常没有设置。这时报表命令照常可用（走内置价格），但要用 profile 就需要设置 `HOME`、`XDG_CONFIG_HOME`、`TOKENCHECK_CONFIG` 之一，或者显式传 `--pricing-dir`。
- 把整个配置目录搬到别处（例如 U 盘）时，用绝对路径或 `TOKENCHECK_CONFIG` 指向新的 `config.json`，相对路径不会跟着配置文件走。

### 目录结构

计费配置默认放在 `~/.config/tokencheck/pricing`。一个配置可以是单个 JSON 文件，也可以是一个目录：

```text
~/.config/tokencheck/pricing/
├── default/                  # 配置名 default：目录里的文件按文件名排序叠加
│   ├── 00-defaults.json
│   ├── 10-openai.json
│   └── 20-anthropic.json
├── personal/
│   ├── 10-official.json      # 官方价格
│   ├── 20-claude-proxy.json  # 只对 claude 来源生效的网关加价
│   └── 30-codex-flat.json    # 只对 codex 来源生效的包月价
└── team.json                 # 配置名 team：单个文件
```

对应的切换方式：

```bash
tkc pricing list          # 看到 builtin、default、personal、team
tkc pricing use personal  # 写进配置文件，之后所有命令都用它
tkc summary --pricing-profile team   # 只对本次运行生效
```

### 同一个模型，不同来源不同价格

价格文件顶层和模型条目都支持 `sources` / `source`。这样同一个模型可以出现在多个文件里，各自负责一部分来源：

`20-claude-proxy.json`：

```json
{
  "schema": 2,
  "name": "claude-gateway",
  "sources": ["claude"],
  "models": {
    "gpt-5.6": { "input": 6.0, "cached_input": 0.6, "output": 30.0 },
    "gpt-6-astra": { "input": 15.0, "cached_input": 1.5, "output": 75.0 }
  }
}
```

`30-codex-flat.json`：

```json
{
  "schema": 2,
  "name": "codex-subscription",
  "sources": ["codex"],
  "models": {
    "gpt-5.6": { "input": 0.0, "cached_input": 0.0, "output": 0.0 },
    "gpt-6*": { "unpriced": true }
  }
}
```

结果就是 `gpt-5.6` 在 `claude` 来源走网关价、在 `codex` 来源记 0、在 `opencode` / `pi` / `kimi` 来源继续回退到官方价；`gpt-6*` 在 codex 来源被明确标为未定价，不会静默套用官方价。可以用 `tkc pricing match gpt-5.6` 验证。

仓库里的 `examples/pricing/per-source/` 就是这套例子，可以直接拷到 `~/.config/tokencheck/pricing/personal/`。

反过来，如果同一个模型只是因为**渠道档位**不同而价格不同（例如某个渠道的 standard 档和 fast 档差一倍），更适合按档位拆成两个配置，切换而不是叠加：

```text
~/.config/tokencheck/pricing/
├── micu-standard.json   # tkc pricing use micu-standard
└── micu-fast.json       # tkc pricing use micu-fast
```

`examples/pricing/` 里已经带了这两个配置，可以直接拷走。

### 价格文件格式

完整字段如下，`schema: 2` 是当前版本，`schema: 1` 和省略 `schema` 的老格式（只有 `models`）继续兼容。

```json
{
  "schema": 2,
  "name": "personal",
  "description": "自用价格",
  "sources": ["claude", "codex"],
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
      "output": 8.0,
      "note": "网关报价，月结"
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

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `schema` | 否 | 价格文件版本；当前为 `2`，`1` 和省略都按老格式解析。 |
| `name` / `description` | 否 | 展示用的名字和说明，会出现在 `tkc pricing show`。 |
| `sources` | 否 | 文件级来源限定。写了以后，文件内没有自己声明来源的条目只对这个来源生效。 |
| `accounting` | 否 | 计费口径，见下一节。多层会合并，高优先级覆盖低优先级。 |
| `models` | 是 | 模型到价格的映射。键可以是精确模型名，也可以以 `*` 结尾做前缀匹配。 |
| `sources` / `source` | 否 | 条目级来源限定：只对列出的来源（`claude`、`codex`、`opencode`、`pi`、`kimi`）生效。 |
| `input` | 是（除非 `unpriced`） | 普通 input token 单价。 |
| `output` | 是（除非 `unpriced`） | output token 单价。 |
| `cached_input` | 否 | cached input/cache read 单价；省略时等于 `input`。 |
| `cache_creation_5m` | 否 | 5 分钟 cache write 单价；省略时为 `0`。别名：`cache_creation_input_5m`、`cache_write_5m`。 |
| `cache_creation_1h` | 否 | 1 小时 cache write 单价；省略时为 `0`。别名：`cache_creation_input_1h`、`cache_write_1h`。 |
| `tiers` | 否 | 按 prompt token 分档；`prompt_tokens_over` 之上的请求使用这一档价格，未写的字段沿用基础单价。 |
| `note` | 否 | 备注，会出现在 `tkc pricing match` 里，适合标注“促销价”“估算值”“网关加价”。 |
| `unpriced` | 否 | 设为 `true` 表示这个模型（或这个来源）明确不参与美元统计，并阻止回退到低优先级价格。 |

模型名规范化规则：大小写不敏感，`_` 会当成 `-`，`provider/model` 会按最后一段模型名匹配。因此 `openai/gpt-4.1-mini`、`OpenAI/GPT_4.1_Mini` 和 `gpt-4.1-mini` 会命中同一条价格。`unknown` 和 `<synthetic>` 永远不会被定价。

### 计费口径（accounting）

不同工具对 cached input 的记账方式不同，所以 `accounting` 也是配置项：

| 值 | 含义 |
| --- | --- |
| `cached_input_separate` | cached input 是独立计数，和普通 input 分开计费（默认）。 |
| `cached_input_subset` | cached input 已经包含在普通 input 计数里，计费时先从 input 里减掉，避免重复计费。 |

- `default`：没有来源级覆盖时使用的口径。
- `by_source`：按来源覆盖，键是 `claude`、`codex`、`opencode`、`pi`、`kimi`。
- `legacy_codex_event_subset`：默认 `true`。旧版 Codex 会话只会写一条累计事件（`event_id == session_id`），这种事件按 `cached_input_subset` 计费。

### 常用命令

```bash
# 把内置价格落盘成可编辑的 default 配置
tkc pricing init

# 看看某个模型会命中哪一层、什么价格
tkc pricing match gpt-5.6
tkc pricing match claude-opus-5.5 --source claude

# 一个渠道按档位拆成两个配置，按当天走的档位切换
tkc pricing list
tkc pricing use micu-standard
tkc pricing use micu-fast
tkc models --pricing-profile micu-fast

# 校验手写的价格文件
tkc pricing check ~/proxy/claude.json

# 临时只用自己的价格，不回退到内置价格
tkc summary --no-builtin-pricing --pricing-profile personal

# 临时叠加两个价格文件，后面的覆盖前面的
tkc models --pricing-file a.json --pricing-file b.json
```

### 价格数据说明

内置价格来自各厂商官方定价页，`note` 字段里标注了促销价和估算值：

- OpenAI GPT-5.6 系列的 `gpt-5.6` 是 `gpt-5.6-sol` 的别名；这一代用 Sol/Terra/Luna/Cyber 命名，不存在 `gpt-5.6-mini` / `-nano` / `-pro` / `-codex`。`gpt-6` 在官方目录里没有独立条目，这里按该系列的 sol 档（`gpt-6-sol`）定价，并在 `note` 里说明，方便你按自己供应商的实际映射覆盖。
- GPT-5.6 / GPT-6 的 prompt 超过 272k token 时进入高档位（input ×2、output ×1.5），这个规则写在对应模型的 `tiers` 里。
- `gpt-5.6-sol` 的 4/0.4/20 是官方标注的促销价（至少到 2026-11-21）；Gemini 3.8 / 3.6 Flash 的 0.75/0.075/3.75 也是促销价，官方说明 2027-01-01 起翻倍。价格变化时直接改 `assets/pricing/*.json` 或自己的配置目录即可，不需要改代码。
- DeepSeek 区分高峰和低谷价，内置价格用的是高峰价；低谷价是高峰价的一半。usage 日志只有日期没有小时，所以要按低谷价统计就复制 `40-deepseek.json`、把价格减半，作为更高优先级的配置加载。
- MiMo 的内置价格已按小米官方页面更新为 V2.5/V2.6 系列（`MiMo-V2.6-Pro` 0.435/0.0036/0.87、`MiMo-V2.6-Flash` 0.14/0.0028/0.28）。1.3.0 之前的版本这两个系列的价格是错的，升级后成本数字会变化。

## 隐私边界

默认不会输出：

- 用户 prompt 正文。
- assistant 回复正文。
- shell 命令参数。
- 工具调用参数。
- 文件快照内容。
- paste-cache 内容。

当前命令只展示聚合后的元数据和计数。`fetch` 保存的 JSON 快照也用于报表聚合；它不是原始对话导出。

## 已知限制

- 成本是按文本 token 的估算值，不等同于实际账单。
- 内置价格是随包发布的静态数据，厂商调价后需要更新价格文件（`assets/pricing/*.json` 或本地计费配置），不需要改代码。
- 价格文件按“生效中的价格”工作，不记录生效日期；促销价到期需要自己改 `assets/pricing/*.json` 或覆盖配置，`note` 字段里标了已知的到期时间。
- 第三方代理模型名可能不等同于官方模型名；不确定时用 `tkc pricing match <model>` 看命中了哪条价格，再在配置里显式覆盖。
- 来源级价格只按数据来源（`claude`、`codex`、`opencode`、`pi`、`kimi`）区分，不区分同一来源内的多个账号或网关地址。
- `heatmap` 只展示 token 强度，不展示成本。

## 开发

### 项目结构

```text
assets/
└── pricing/         # 内置价格数据文件，运行时通过 include_str! 加载
src/
├── billing.rs       # 价格文件解析、匹配、计费口径、成本估算
├── pricing.rs       # 计费配置发现、目录/文件解析、优先级合并
├── claude_code/     # Claude Code JSONL 数据读取
├── codex/           # Codex sessions JSONL 数据读取
├── opencode/        # OpenCode SQLite 与 JSON 数据读取
├── pi/              # Pi sessions JSONL 数据读取
├── kimi_code/       # Kimi Code sessions wire.jsonl 数据读取
├── lib.rs           # CLI、命令分发、聚合、终端渲染
├── model.rs         # 共享数据模型
├── snapshot.rs      # JSON 快照读写、去重、合并
└── bin/
    ├── tkc.rs
    └── tokencheck.rs
examples/
└── pricing/         # 可直接拷走的计费配置示例
```

核心数据流是：读取本机日志或 JSON 快照，归一化为 `ReportData`，按命令维度聚合，再输出表格、柱状图、热力图或成本估算。计费侧的流程是：计费配置目录 → 选中的价格文件 + 额外文件 → 叠加到内置价格之上 → 按 `(source, model)` 查找价格。

### 开发验证

提交前建议运行：

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

本地冒烟检查：

```bash
cargo run --bin tkc -- summary --limit 5
cargo run --bin tkc -- days --limit 5
cargo run --bin tkc -- days --from-json --limit 20 --chant
cargo run --bin tkc -- heatmap --from-json --months 12
cargo run --bin tkc -- projects --limit 5
cargo run --bin tkc -- models --limit 5
cargo run --bin tkc -- tools --limit 5
cargo run --bin tkc -- pricing list
cargo run --bin tkc -- pricing show
cargo run --bin tkc -- pricing match gpt-5.6
```

## 发布

项目包含两个 GitHub Actions workflows：

```text
.github/workflows/cargo-publish.yml  # 发布 crates.io 包
.github/workflows/release.yml        # 构建并发布 GitHub Release 二进制包
```

### 本地构建可分发二进制

`release` profile 打开了 LTO 并去掉了调试符号，产物可以直接拷到别的机器上运行，不需要装 Rust 或任何第三方动态库：

```bash
cargo build --locked --release --bins
ls -l target/release/tkc target/release/tokencheck
```

指定目标平台进行交叉构建（和 CI 的命名保持一致）：

```bash
cargo build --locked --release --bins --target aarch64-apple-darwin
cargo build --locked --release --bins --target x86_64-apple-darwin
```

macOS 上可以把两个架构合成通用二进制，再打成和 GitHub Release 同名的压缩包：

```bash
mkdir -p dist/universal
for bin in tkc tokencheck; do
  lipo -create "target/aarch64-apple-darwin/release/$bin" \
              "target/x86_64-apple-darwin/release/$bin" \
              -output "dist/universal/$bin"
  codesign --force --sign - "dist/universal/$bin"
done

name="tokencheck-v$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys;print(json.load(sys.stdin)["packages"][0]["version"])')-macos-universal"
mkdir -p "dist/$name"
cp dist/universal/tkc dist/universal/tokencheck README.md LICENSE "dist/$name/"
tar -czf "dist/$name.tar.gz" -C dist "$name"
shasum -a 256 dist/*.tar.gz > dist/SHA256SUMS
```

要点：

- `rusqlite` 用了 `bundled` feature，SQLite 会静态编译进二进制，所以产物没有额外的动态库依赖；`otool -L` 只会看到 `libSystem`、`libiconv` 和 `CoreFoundation` 这些系统库。
- 二进制的 macOS 最低版本是 arm64 的 11.0 和 x86_64 的 10.12，都是各架构的下限。
- 用 `lipo` 合并后要重新 `codesign --force --sign -` 做 ad-hoc 签名，否则签名失效。
- 从网上下载的二进制会被 Gatekeeper 打上隔离属性，用户首次运行需要 `xattr -d com.apple.quarantine tkc`，或右键打开；要彻底避免需要 Apple 开发者证书签名并公证。
- Linux 和 Windows 目标（`x86_64-unknown-linux-gnu`、`x86_64-pc-windows-msvc`）由 `.github/workflows/release.yml` 在推 tag 时构建。

### crates.io 发布

触发方式：

- 推送到 `main`，且本项目源码、manifest、README、LICENSE 或 `cargo-publish.yml` 有变化。
- 手动运行 `workflow_dispatch`。

发布步骤：

1. 读取 `Cargo.toml` 中的 package name 和 version。
2. 运行 `cargo fmt --all -- --check`。
3. 运行 `cargo clippy --all-targets -- -D warnings`。
4. 运行 `cargo test`。
5. 运行 `cargo package`。
6. 查询 crates.io sparse index 是否已有相同 package/version。
7. 如果版本尚未发布，则执行 `cargo publish`。
8. 如果版本已经存在，则跳过发布。

首次启用前，需要在 GitHub 仓库的 Actions secrets 中配置：

```text
CARGO_REGISTRY_TOKEN
```

这个 token 来自 crates.io 账户设置。crates.io 账户必须有已验证邮箱。之后每次要发布新包，需要先提升 `Cargo.toml` 和 `Cargo.lock` 中的版本号，再推送到 `main`。crates.io 不允许覆盖已经发布过的同一版本。

### GitHub Release 二进制包

触发方式：

- 推送 `v*` tag，例如 `v1.0.0`。
- 手动运行 `Build GitHub release` workflow。未填写 tag 时，默认使用 `Cargo.toml` 版本生成 `v<version>`。

release workflow 会先运行 `cargo fmt --all -- --check`、`cargo clippy --all-targets -- -D warnings` 和 `cargo test`，再为以下平台构建并上传归档：

| 平台 | target | 归档 |
| --- | --- | --- |
| Linux x64 | `x86_64-unknown-linux-gnu` | `.tar.gz` |
| Windows x64 | `x86_64-pc-windows-msvc` | `.zip` |
| macOS Intel | `x86_64-apple-darwin` | `.tar.gz` |
| macOS Apple Silicon | `aarch64-apple-darwin` | `.tar.gz` |

每个归档内包含 `tkc`、`tokencheck`、`README.md` 和 `LICENSE`。workflow 还会上传 `SHA256SUMS` 校验文件。GitHub Release 使用仓库内置的 `GITHUB_TOKEN`，不需要额外配置 secret。

## License

MIT. See [LICENSE](LICENSE).
