# 计费配置示例：同一模型，不同来源，不同价格

这个目录是一个可以直接使用的计费配置，配置名建议叫 `personal`。

```bash
mkdir -p ~/.config/tokencheck/pricing/personal
cp examples/pricing/per-source/*.json ~/.config/tokencheck/pricing/personal/

tkc pricing list
tkc pricing use personal
tkc pricing match gpt-5.6
```

三个文件的优先级从低到高（按文件名排序）：

| 文件 | 来源限定 | 作用 |
| --- | --- | --- |
| `10-official.json` | 全部 | 兜底的官方价，覆盖没有被网关接管的来源。 |
| `20-claude-gateway.json` | `claude` | Claude Code 走网关，同模型按加价后的价格计费。 |
| `30-codex-flat-rate.json` | `codex` | Codex 是包月套餐：已覆盖的模型记 0，未覆盖的 `gpt-6*` 明确标成未定价。 |

`tkc pricing match gpt-5.6` 会逐个来源展示命中的文件和单价，可以直接验证“同一个模型来自不同来源收费不同”是否符合预期。

想临时试这套价格而不改配置：

```bash
tkc models \
  --pricing-dir examples/pricing \
  --pricing-profile per-source
```
