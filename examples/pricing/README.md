# 计费配置示例

这里的每个条目都是一个可以直接用的计费配置（pricing profile）。把它们拷到 `~/.config/tokencheck/pricing/` 之后，`tkc pricing list` 就能看到。

## 米醋 API 渠道：fast / standard 两档

| 配置 | 文件 | gpt-6.1-sol 单价（每 1M tokens） |
| --- | --- | --- |
| `micu-standard` | `micu-standard.json` | input ¥0.4 / output ¥2 / cached ¥0.02 / cache write ¥0.5 |
| `micu-fast` | `micu-fast.json` | input ¥0.8 / output ¥4 / cached ¥0.04 / cache write ¥1 |

fast 档四项都是 standard 档的 2 倍。两档只覆盖 gpt-6.1-sol，其他模型继续回退到内置价格。

```bash
tkc pricing use micu-standard     # 平时用标准档
tkc pricing use micu-fast         # 走 fast 档时切过去
tkc pricing match gpt-6.1-sol     # 确认命中的是哪个档

tkc models --pricing-profile micu-fast   # 只对本次运行生效
```

注意：价格文件里的数字按米醋渠道的 ¥ 报价原样保存，而 `tkc` 的成本列统一按美元累加和显示（会显示成 `$0.6`）。同一次报表里如果既有米醋价又有内置美元价，两者会被直接相加，看总额时留意这一点。

## 同一模型，不同来源，不同价格

`per-source/` 目录演示用 `sources` 把同一个模型按数据来源拆到不同文件里，详见 `per-source/README.md`。
