# rfuzz 与 ffuf / Wfuzz 功能差距分析

本文档用于记录 `rfuzz` 与 `ffuf`、Wfuzz 的主要功能差距，作为后续路线图参考。

这里的目标不是完全复制上游工具，而是判断哪些能力值得优先补齐，让 `rfuzz` 在保持轻量、高并发、低内存占用的同时，逐步覆盖常见实战场景。

## 当前 rfuzz 基线

`rfuzz` 当前已经支持核心 Web fuzz 流程：

- URL、Header、Cookie、Body 模板替换
- 裸 keyword 占位符，例如 `FUZZ`、`PASS`、`URLFUZZ`
- 原生显式占位符，例如 `${{FUZZ}}$`
- 多字典输入
- `clusterbomb` 和 `pitchfork`
- `-order` 自定义 `clusterbomb` 组合顺序
- `-stop-scope` 和 `-stop-on-match` 按 scope 命中后停止
- POST、JSON、Burp raw request fuzz
- 状态码、响应大小、单词数、行数、响应时间、正则匹配/过滤
- `-mr` / `-fr` 对完整 raw response 匹配，包括响应头和响应体
- 并发、延迟、全局速率限制、超时、代理、replay proxy
- 带 ETA 和 req/s 的进度条
- `console`、`jsonl`、`csv` 输出
- 命中结果 raw request/response 保存
- 请求错误计数和可选错误 payload 日志

## 相比 ffuf 缺失的能力

| 方向 | ffuf 能力 | rfuzz 当前状态 | 优先级 |
| --- | --- | --- | --- |
| 输入模式 | 真正的 `sniper` 模式 | 当前 `sniper` 只是降级为 `pitchfork` | 高 |
| 自动校准 | `-ac`、`-acc`、`-ach`、`-ack`、`-acs` | 参数已预留，但未实现 | 高 |
| 递归扫描 | `-recursion`、`-recursion-depth`、递归策略 | 未实现 | 高 |
| 忽略响应体 | `-ignore-body` | 未实现 | 高 |
| 外部变异器 | `--input-cmd`、`--input-num`、`--input-shell` | 未实现 | 中 |
| 运行时间限制 | `-maxtime`、`-maxtime-job` | 未实现 | 中 |
| 配置文件 | `ffufrc`、`-config` | 未实现 | 中 |
| 交互模式 | 运行中修改 filter/rate、管理队列、保存结果 | 未实现 | 中 |
| 响应提取 | `-scraperfile`、`-scrapers` | 未实现 | 中 |
| 错误停止策略 | 错误过多、过滤率过高时自动停止 | 未实现 | 中 |
| 更多输出格式 | `json`、`ejson`、`html`、`md`、`ecsv`、`all` | 当前只有 `console`、`jsonl`、`csv` | 中 |
| 调试日志 | `-debug-log` | 未实现 | 低 |
| 终端体验 | 彩色输出、verbose 输出 | 当前较基础 | 低 |
| DirSearch 兼容 | `-D` | 未实现 | 低 |
| 历史结果搜索 | `-search FFUFHASH` | 未实现 | 低 |

## 相比 Wfuzz 缺失的能力

| 方向 | Wfuzz 能力 | rfuzz 当前状态 | 优先级 |
| --- | --- | --- | --- |
| Payload 系统 | `-z payload,params`，支持插件化 payload 来源 | 当前主要是文件字典 | 高 |
| 内置 payload | `range`、`list`、`stdin`、`dirwalk` 等 | 未系统实现 | 高 |
| 组合器系统 | `product`、`zip`、`chain` | 已有 `clusterbomb` 和 `pitchfork`，没有 `chain` | 中 |
| 过滤表达式语言 | `--filter`、`--prefilter`、字段表达式 | 仅在路线图中 | 高 |
| Payload 改写 | `--slice`、payload 重写、prefilter | 未实现 | 中 |
| 编码插件模型 | 可列出、可组合的 encoder | 当前是固定 encoder 集合 | 中 |
| 扫描/解析插件 | active/passive/discovery scripts | 未实现 | 中 |
| Recipe | `--dump-recipe`、`--recipe` | 未实现 | 中 |
| 基线过滤 | baseline response 对比过滤 | 未实现 | 中 |
| 多代理轮转 | 多代理、按请求轮转代理 | 当前只有单代理 | 中 |
| 认证辅助 | Basic、NTLM、Digest 专用参数 | 可手写 Header，但没有专用参数 | 低 |
| 指定连接 IP | 使用某个 Host，但连接到指定 IP | 未实现 | 低 |
| 独立超时控制 | 连接延迟、请求延迟等更细控制 | 当前是总 timeout 加请求间 delay | 低 |
| 库模式 | Python library API | 当前只有 CLI | 低 |

## 推荐路线图

### 第一阶段：补齐 ffuf 高频体验

这些能力最贴近日常 Web fuzz 使用场景，建议优先实现。

1. `-ignore-body`
   - 当只需要状态码、响应头、耗时等信息时，避免读取和拼接完整响应体。
   - 对大规模扫描、高并发、内存控制都很有帮助。
   - 需要明确文档说明：开启后依赖 body 的 `-mr` / `-fr` / size / words / lines 行为会受到影响。

2. 真正的 `sniper` 模式
   - 每次只 fuzz 一个 keyword，其他 keyword 使用默认值或基准值。
   - 让 `-mode sniper` 的行为真正符合用户预期。

3. 自动校准
   - 实现已预留的 `-ac`、`-ac-scope`、`-ac-ignore`。
   - 支持按 host/job/global 采样基线响应，自动生成过滤规则。

4. 递归扫描
   - 对发现的目录继续递归 fuzz。
   - 支持最大递归深度、递归队列、递归策略。

### 第二阶段：增强匹配和输入能力

1. `-match-expr` 和 `-filter-expr`
   - 支持基于状态码、大小、单词数、行数、耗时、响应头、body hash、title 等字段的布尔表达式。

2. 外部变异器支持
   - 增加类似 `--input-cmd` 的能力，支持 Radamsa 这类外部生成器。
   - 尽量支持确定性 seed，方便复现。

3. 内置 payload 生成器
   - 增加 `range`、`list`、`stdin`、`dirwalk` 等输入来源。
   - 文件字典仍然保持默认和最常用路径。

4. 多代理轮转
   - 支持代理池。
   - 支持按请求轮转代理。

### 第三阶段：工作流和生态能力

1. 配置文件
   - 保存常用 Header、rate、proxy、输出格式、过滤规则等默认配置。

2. 更多输出格式
   - 优先增加 Markdown 和 HTML。
   - 后续考虑 `all`，一次写出多个格式。

3. 运行时控制
   - 增加总运行时间限制。
   - 增加单 job 运行时间限制。
   - 增加错误率过高时自动停止策略。

4. Recipe
   - 支持导出当前命令配置。
   - 支持从 recipe 重新运行任务。

5. 交互模式
   - 可作为后期功能。
   - 复杂度较高，因为会涉及运行时修改过滤器、速率、队列和输出。

## 设计注意事项

- 保持 `rfuzz` 的低内存特性，避免预先收集大规模请求组合。
- 优先使用流式迭代器和有界任务队列。
- 已经与 ffuf 接近的参数名，尽量继续保持 ffuf 风格。
- 不要过早做复杂插件系统。可以先做小而清晰的内置 payload 抽象。
- 继续保持 `-mr` / `-fr` 语义清晰：当前匹配完整 raw response。
- 如果实现 `-ignore-body`，必须明确它对 body 正则、size、words、lines 的影响。
- 如果实现自动校准，需要避免引入大量额外请求导致用户误判扫描规模。
- 如果实现递归扫描，需要保证递归队列不会无限增长。

## 参考资料

- ffuf GitHub README: https://github.com/ffuf/ffuf
- ffuf Wiki: https://github.com/ffuf/ffuf/wiki
- Wfuzz 文档首页: https://wfuzz.readthedocs.io/
- Wfuzz Getting Started: https://wfuzz.readthedocs.io/en/latest/user/getting.html
- Wfuzz Advanced Usage: https://wfuzz.readthedocs.io/en/latest/user/advanced.html
