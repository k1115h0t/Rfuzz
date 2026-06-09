[English](README.md)

# Rfuzz

<div align="center">

**Rust 编写的命令行 Web Fuzzer，面向授权安全测试、资产自查和靶场研究。**

English: a conservative Rust web fuzzer for authorized testing, with familiar ffuf-style workflows.

[![Rust](https://img.shields.io/badge/Rust-2021-orange.svg)](https://www.rust-lang.org/)
[![Version](https://img.shields.io/badge/version-0.1.6-blue.svg)](Cargo.toml)
[![License](https://img.shields.io/badge/license-AGPL--3.0-green.svg)](LICENSE)

</div>

---

## 这是什么？

`rfuzz` 是一个轻量、可脚本化的 Web fuzzing 工具。它保留了常见 `ffuf` 使用习惯，同时针对大规模多目标任务补充了预检查、目标轮转、scope 级停止、DNS 缓存、错误日志和结构化输出能力。

适合用于：

- 授权范围内的目录、参数、接口、表单和弱口令验证；
- 企业内部资产自查、持续安全测试和回归验证；
- CTF、靶场、实验环境和安全研究；
- 将 Burp Suite raw request 快速转成可重复运行的 fuzz 任务。

> **授权说明**
> 仅在你拥有明确授权的目标上使用 `rfuzz`。扫描和 fuzzing 可能给服务带来压力，请严格遵守当地法律、组织规则和授权范围。

---

## 核心特性

| 能力 | 说明 |
| --- | --- |
| ffuf 风格 CLI | 支持 `-u`、`-w`、`-H`、`-X`、`-d`、`-mc`、`-fc`、`-mr` 等常见参数风格。 |
| 多字典组合 | 支持 `clusterbomb`、`pitchfork`，并预留 `sniper`。 |
| 惰性生成 | 大规模笛卡尔积不会一次性加载全部请求组合。 |
| 目标预检查 | `-precheck-key` 先探测目标是否可达，失败 payload 默认跳过。 |
| 目标轮转 | `-schedule rotate-window` 适合多 URL、多账号、多密码场景，避免长时间打同一个目标。 |
| scope 级停止 | `-stop-scope` + `-stop-on-match` 可以让某个 URL、用户或组合命中后提前跳过剩余请求。 |
| HTTP 调优 | 支持代理、重定向、HTTP/2、keep-alive、DNS 缓存、超时、延迟、全局限速。 |
| 结构化输出 | 支持 console、silent URL、JSONL、CSV、原始请求/响应保存和错误 JSONL 日志。 |

---

## 安装与编译

### 从源码编译

```bash
git clone https://github.com/k1115h0t/Rfuzz.git
cd Rfuzz
cargo build --release
```

Release 二进制位于：

```text
target/release/rfuzz
```

开发调试时也可以直接运行：

```bash
cargo run -- -h
```

或者安装到本机 Cargo bin 目录：

```bash
cargo install --path .
```

---

## 快速开始

### 1. 目录扫描

```bash
rfuzz -w dirs.txt:DIR \
  -u 'https://example.com/${{DIR}}$' \
  -fc 404
```

也可以使用更接近 ffuf 的裸 keyword 写法：

```bash
rfuzz -w dirs.txt:FUZZ -u https://example.com/FUZZ -fc 404
```

### 2. POST 表单 fuzz

```bash
rfuzz -w passwords.txt:PASS \
  -u https://example.com/login \
  -X POST \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -d 'username=admin&password=${{PASS}}$' \
  -fc 401
```

### 3. 多字典组合

```bash
rfuzz -u https://example.com/login \
  -X POST \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -d 'username=${{USER}}$&password=${{PASS}}$' \
  -w users.txt:USER \
  -w passwords.txt:PASS \
  -mode clusterbomb \
  -mr 'Set-Cookie: session_id='
```

默认组合模式是 `clusterbomb`，即多个字典做笛卡尔积。

### 4. Burp raw request 文件

把 Burp Suite 复制出来的请求保存为 `login.txt`：

```http
POST /login HTTP/1.1
Host: example.com
Content-Type: application/x-www-form-urlencoded

username=admin&password=${{PASS}}$
```

然后运行：

```bash
rfuzz -request login.txt \
  -request-proto https \
  -w passwords.txt:PASS \
  -fc 401
```

`-request-proto` 只对 raw request 文件生效，不会给 `-u` 模板自动补协议。

---

## 关键概念

### 占位符

`rfuzz` 支持两种占位符风格：

| 写法 | 示例 | 说明 |
| --- | --- | --- |
| 原生显式 | `${{FUZZ}}$`、`${{PASS}}$` | 精确，不容易误替换普通文本。 |
| 裸 keyword | `FUZZ`、`PASS`、`URLFUZZ` | 更接近 ffuf，用在命令行里更省心。 |

在 Bash、zsh、PowerShell 中使用 `${{KEYWORD}}$` 时，建议用单引号包裹模板，避免 shell 提前解释 `$`：

```bash
rfuzz -w dirs.txt:DIR -u 'https://example.com/${{DIR}}$'
```

### 字典组合模式

| 模式 | 行为 | 适合场景 |
| --- | --- | --- |
| `clusterbomb` | 所有字典做笛卡尔积。 | 用户名 x 密码、路径 x 扩展名、多参数组合。 |
| `pitchfork` | 多个字典按行号同步读取。 | 一一对应的账号/密码、参数名/参数值。 |
| `sniper` | v0.1 预留，目前降级为 pitchfork 行为。 | 后续兼容扩展。 |

控制 `clusterbomb` 生成顺序：

```bash
rfuzz -u https://example.com/login \
  -w users.txt:USER \
  -w passwords.txt:PASS \
  -w hosts.txt:HOST \
  -order USER,PASS,HOST
```

`-order` 必须包含所有 keyword，且最后一个 keyword 变化最快。

---

## 大规模任务推荐配置

### 目标预检查

当一个字典代表目标 URL、域名或 `host:port` 时，建议先启用目标预检查：

```bash
rfuzz -u https://TARGET/login \
  -w targets.txt:TARGET \
  -w users.txt:USER \
  -w passwords.txt:PASS \
  -d 'username=${{USER}}$&password=${{PASS}}$' \
  -precheck-key TARGET
```

预检查只遍历 `-precheck-key` 对应 payload，不会组合其他字典。只要能收到 HTTP 响应，就视为目标可达；`200`、`301`、`401`、`403`、`404`、`500` 等状态码都算可达。

关闭预检查：

```bash
rfuzz -u https://TARGET/login -w targets.txt:TARGET -precheck off
```

只报告预检查错误，但不跳过失败 payload：

```bash
rfuzz -u https://TARGET/login \
  -w targets.txt:TARGET \
  -precheck-key TARGET \
  -precheck-report-only
```

预检查默认按轮次遍历所有目标 3 次。也就是说 3 个目标会按 `1,2,3,1,2,3,1,2,3` 的顺序重试，而不是 `1,1,1,2,2,2,3,3,3`。可以指定轮询次数：

```bash
rfuzz -u https://TARGET/login \
  -w targets.txt:TARGET \
  -precheck-key TARGET \
  -precheck-attempts 5
```

### 目标轮转

当目标很多时，可以用 `rotate-window` 在目标之间轮转，减少对单个目标的持续压力：

```bash
rfuzz -u https://TARGET/login \
  -X POST \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -d 'username=${{USER}}$&password=${{PASS}}$' \
  -w targets.txt:TARGET \
  -w users.txt:USER \
  -w passwords.txt:PASS \
  -precheck-key TARGET \
  -schedule rotate-window \
  -target-key TARGET \
  -target-window 100 \
  -target-burst 3 \
  -mr 'Set-Cookie: session_id=' \
  -o result.jsonl \
  -of jsonl \
  -t 100
```

参数含义：

| 参数 | 说明 |
| --- | --- |
| `-schedule rotate-window` | 启用目标窗口轮转。 |
| `-target-key TARGET` | 指定哪个 keyword 代表目标。 |
| `-target-window 100` | 每批保持多少个目标参与轮转。 |
| `-target-burst 3` | 每个目标连续请求多少个组合后切换。 |

### 命中后停止

命中一次后跳过该目标的剩余组合：

```bash
rfuzz -u https://TARGET/login \
  -w targets.txt:TARGET \
  -w users.txt:USER \
  -w passwords.txt:PASS \
  -mr 'Set-Cookie: session_id=' \
  -stop-scope TARGET \
  -stop-on-match 1
```

只跳过当前目标 + 当前用户的剩余密码：

```bash
rfuzz -u https://TARGET/login \
  -w targets.txt:TARGET \
  -w users.txt:USER \
  -w passwords.txt:PASS \
  -mr 'Set-Cookie: session_id=' \
  -stop-scope TARGET,USER \
  -stop-on-match 1
```

`-stop-scope` 也可以和 `-order`、`rotate-window`、`precheck` 一起使用。达到停止阈值后，`rfuzz` 会快进跳过该 scope 的剩余组合。

---

## 输出与进度

默认情况下，`rfuzz` 在 `stderr` 显示进度条，不污染写到 `stdout` 或 `-o` 的结果。

进度字段：

```text
done/total | percent | matched | errors | skipped | err | ETA
```

`ETA` 使用最近完成的真实 case 速度估算，会包含限速、延迟、跳过、匹配和输出写入带来的实际影响。

关闭进度条：

```bash
rfuzz -w dirs.txt:DIR -u 'https://example.com/${{DIR}}$' -no-progress
```

JSONL 输出示例：

```json
{"url":"https://example.com/admin","status":200,"size":1234,"words":100,"lines":20,"time_ms":42,"input":"admin","input_values":{"DIR":"admin"},"location":null,"title":"Admin","body_hash":123}
```

保存匹配结果：

```bash
rfuzz -w dirs.txt:DIR \
  -u 'https://example.com/${{DIR}}$' \
  -mc 200,204,301-302 \
  -o result.jsonl \
  -of jsonl
```

保存请求错误日志，便于后续复盘失败 payload：

```bash
rfuzz -u https://TARGET/login \
  -w targets.txt:TARGET \
  -error-log request_errors.jsonl
```

---

## 性能与稳定性建议

### 并发与文件描述符

Unix/Linux 下，`rfuzz` 启动时会检查 `ulimit -n`，并估算当前文件描述符上限是否足够支撑 `-t` 并发。如果上限太低，会自动下调实际并发并打印提示。

大量目标或高并发任务前建议：

```bash
ulimit -n 8192
```

如果系统不允许提高上限，就降低 `-t`，或通过系统/服务配置提高 hard limit。

### Keep-alive 与 DNS 缓存

默认配置：

| 参数 | 默认值 | 说明 |
| --- | --- | --- |
| `-keepalive` | `on` | 复用 HTTP 连接，HTTPS 场景通常更快。 |
| `-dns-cache` | `on` | 开启进程内 DNS 缓存。 |
| `-dns-cache-ttl` | `300` | 成功解析缓存 300 秒。 |
| `-dns-negative-cache-ttl` | `30` | DNS 失败缓存 30 秒。 |
| `-dns-max-concurrent` | `64` | 同时进行的真实 DNS 解析上限。 |

低文件描述符环境可以关闭 keep-alive：

```bash
rfuzz -u https://TARGET/login -w targets.txt:TARGET -keepalive off
```

这会更省 FD，但 HTTPS 吞吐可能下降，因为请求需要更频繁地重新建立 TCP/TLS 连接。

---

## 参数速查

### 请求参数

| 参数 | 说明 | 示例 |
| --- | --- | --- |
| `-u` | URL 模板。 | `-u https://HOST/FUZZ` |
| `-request` | Burp raw request 文件。 | `-request login.txt` |
| `-request-proto` | raw request 协议。 | `-request-proto https` |
| `-X` | HTTP 方法。 | `-X POST` |
| `-H` | Header 模板，可重复。 | `-H "Content-Type: application/json"` |
| `-d` | 请求体模板。 | `-d 'q=${{FUZZ}}$'` |
| `-b` | Cookie 模板，可重复。 | `-b 'sid=${{SID}}$'` |

### 输入与调度

| 参数 | 说明 | 示例 |
| --- | --- | --- |
| `-w` | 字典文件和可选 keyword，可重复。 | `-w users.txt:USER` |
| `-mode` | 多字典模式。 | `-mode clusterbomb` |
| `-order` | clusterbomb 生成顺序。 | `-order USER,PASS,TARGET` |
| `-schedule` | 请求生成调度。 | `-schedule rotate-window` |
| `-target-key` | 轮转目标 keyword。 | `-target-key TARGET` |
| `-target-window` | 每批轮转目标数量。 | `-target-window 100` |
| `-target-burst` | 每个目标连续请求数。 | `-target-burst 3` |
| `-e` | 给字典项追加扩展名。 | `-e .php,.bak` |
| `-ic` | 忽略 `#` 开头注释行。 | `-ic` |
| `-enc` | 对 keyword 应用编码链。 | `-enc 'DIR:urlencode'` |
| `-budget-requests` | 最大请求预算。 | `-budget-requests 1000` |

支持的 encoder：`urlencode`、`b64encode` / `base64`、`hex`、`lower`、`upper`、`md5`、`sha1`。

### 执行与 HTTP

| 参数 | 说明 | 示例 |
| --- | --- | --- |
| `-t` | 并发 worker 数。 | `-t 100` |
| `-rate` | 全局每秒请求数，`0` 表示不限速。 | `-rate 50` |
| `-timeout` | 请求超时秒数。 | `-timeout 10` |
| `-p` | 请求间延迟或随机范围。 | `-p 0.1-0.5` |
| `-no-progress` | 关闭进度条。 | `-no-progress` |
| `-precheck` | 开关预检查。 | `-precheck off` |
| `-precheck-key` | 目标预检查 keyword。 | `-precheck-key TARGET` |
| `-precheck-report-only` | 只报告，不跳过失败 payload。 | `-precheck-report-only` |
| `-precheck-attempts` | 预检查按轮次遍历目标的次数。 | `-precheck-attempts 5` |
| `-r` | 跟随重定向。 | `-r` |
| `-raw` | 禁用 URI 空格编码。 | `-raw` |
| `-x` | 请求代理。 | `-x http://127.0.0.1:8080` |
| `-replay-proxy` | 命中后 replay 到代理。 | `-replay-proxy http://127.0.0.1:8081` |
| `-http2` | 强制 HTTP/2 prior knowledge。 | `-http2` |
| `-ssl-verify` | 开关证书校验，默认 off。 | `-ssl-verify on` |
| `-keepalive` | 开关 HTTP keep-alive。 | `-keepalive off` |
| `-dns-cache` | 开关 DNS 缓存。 | `-dns-cache on` |
| `-sni` | 兼容参数，当前不支持任意覆盖 SNI。 | `-sni example.com` |
| `-cc` / `-ck` | 客户端证书和私钥。 | `-cc client.crt -ck client.key` |

### 匹配与过滤

过滤器优先于匹配器。响应命中过滤条件时，不会输出。

| 参数 | 说明 | 示例 |
| --- | --- | --- |
| `-mc` / `-fc` | 匹配/过滤状态码。 | `-mc 200,204,300-399` |
| `-ms` / `-fs` | 匹配/过滤响应大小。 | `-fs 0` |
| `-mw` / `-fw` | 匹配/过滤单词数。 | `-mw 10-30` |
| `-ml` / `-fl` | 匹配/过滤行数。 | `-ml 5-20` |
| `-mr` / `-fr` | 匹配/过滤完整 raw response 正则。 | `-mr 'Set-Cookie: session_id='` |
| `-mt` / `-ft` | 匹配/过滤响应时间，单位 ms。 | `-mt '>100'` |
| `-mmode` / `-fmode` | matcher/filter 组合模式。 | `-mmode and` |

状态码支持 `all`、单值、逗号列表和范围。时间条件支持 `>N`、`<N`、单值、逗号列表和范围。

### 输出与停止控制

| 参数 | 说明 | 示例 |
| --- | --- | --- |
| `-o` | 输出文件。 | `-o result.jsonl` |
| `-of` | 输出格式：`console`、`jsonl`、`csv`。 | `-of jsonl` |
| `-s` | 静默模式，只输出 URL。 | `-s` |
| `-od` | 保存命中请求/响应原文目录。 | `-od raw-results` |
| `-error-log` | 保存失败请求 payload 日志。 | `-error-log errors.jsonl` |
| `-stop-scope` | 停止计数的分组 key。 | `-stop-scope TARGET,USER` |
| `-stop-on-match` | scope 命中 N 次后停止。 | `-stop-on-match 1` |

---

## 当前实现状态

- CLI：请求、输入、执行、matcher/filter、输出、预算控制等主要参数。
- 模板：`${{KEYWORD}}$` 原生占位符和裸 keyword 兼容。
- 输入模式：`clusterbomb`、`pitchfork`；`sniper` 当前降级为 pitchfork。
- 调度：自定义 `-order`、`rotate-window` 目标轮转。
- HTTP：tokio + reqwest，支持代理、replay proxy、重定向、超时、keep-alive、DNS 缓存、全局限速。
- 预检查：目标 payload 连通性检查，支持按轮次重试、只报告模式，失败 payload 默认跳过。
- 匹配/过滤：状态码、大小、单词数、行数、响应时间、完整 raw response 正则、and/or 组合。
- 输出：console、silent URL、JSONL、CSV、raw request/response 保存、错误 JSONL 日志。
- 停止控制：`-stop-scope` + `-stop-on-match`。

---

## Roadmap

- `-ac` 自动校准；
- dynamic auto-filter；
- `filter-expr` / `match-expr`；
- recursion queue；
- `save-state` / `resume`；
- 更完整的 concurrent scoped stop execution。

---

## License

本项目使用 [GNU Affero General Public License v3.0](LICENSE)。
