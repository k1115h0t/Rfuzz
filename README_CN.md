[English](README.md)

# Rfuzz

<div align="center">

**Rust 编写的命令行 Web Fuzzer，面向授权安全测试、资产自查和靶场研究。**

English: a conservative Rust web fuzzer for authorized testing, with familiar ffuf-style workflows.

[![Rust](https://img.shields.io/badge/Rust-2021-orange.svg)](https://www.rust-lang.org/)
[![Version](https://img.shields.io/badge/version-0.2.3-blue.svg)](Cargo.toml)
[![License](https://img.shields.io/badge/license-AGPL--3.0-green.svg)](LICENSE)

</div>

---

## 这是什么？

`rfuzz` 是一个轻量、可脚本化的 Web fuzzing 工具。它保留了常见 `ffuf` 使用习惯，同时针对大规模多目标任务补充了安全预演、预检查、目标轮转、按作用域停止、DNS 缓存、响应 body 限制、任务结束摘要、错误日志和结构化输出能力。

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
| lazy raw request | raw HTTP 请求字符串只在 dry-run、request-dry-run 和命中 raw 保存时构造，不再每个请求都拼。 |
| header-only 匹配 | `-mhr` / `-fhr` 只匹配/过滤响应头，不读取响应 body。 |
| 目标预检查 | `-precheck-key` 用短超时和 HEAD 优先 fallback 探测目标是否可达，失败 payload 默认跳过。 |
| 目标轮转 | `-schedule rotate-window` 适合多 URL、多账号、多密码场景，避免长时间打同一个目标。 |
| 按作用域停止 | 命中后可按 `TARGET`、`USER` 或 `TARGET,USER` 这样的分组提前跳过剩余组合。详见“scope：停止计数的分组键”。 |
| 安全预演 | `-dry-run` / `-explain` / `-request-dry-run` 可在不发送请求的情况下检查计划和最终请求。 |
| HTTP 调优 | 支持代理、重定向、HTTP/2、keep-alive、DNS 缓存、超时、延迟、全局限速。 |
| 结构化输出 | 支持可读的 `[MATCH]` 命中行、silent URL、JSONL、CSV、原始请求/响应保存、错误 JSONL 日志和 JSON 任务摘要。 |

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
  -mhr 'Set-Cookie: session_id='
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

`-request-proto` 只对 raw request 文件生效，不会给 `-u` 模板自动补协议；取值只能是 `http` 或 `https`。

raw request 渲染会校验相对请求行必须有 `Host`，并在 placeholder 渲染后自动重算 `Content-Length`。v0.1.9 起，解析 Burp raw request 时会保留 body 内部的 CRLF 换行；但 `-request` 仍面向文本型 raw request，不保证保留二进制 body。如果只想查看最终请求而不发送：

```bash
rfuzz -request login.txt \
  -request-proto https \
  -w passwords.txt:PASS \
  -request-dry-run
```

---

## 容易混淆的概念

### 安全预演：dry-run / explain / request-dry-run

dry-run 可以理解为“只预演，不执行”。

启用 `-dry-run` 后，`rfuzz` 会加载字典、解析模板、计算预计请求数、应用组合模式、调度策略、并发、限速、预检查配置和输出配置，并渲染首个最终请求供你检查；但不会发送任何 HTTP 请求。

这适合在大任务开始前确认三件事：

1. placeholder 是否替换正确；
2. 请求数量是否符合预期；
3. 最终请求是否会打到正确目标。

| 参数 | 适合什么时候用 | 做什么 |
| --- | --- | --- |
| `-dry-run` | 普通 URL 模板任务 | 打印完整执行计划和首个渲染请求。 |
| `-explain` | 想确认任务配置 | 和 `-dry-run` 类似，强调解释计划。 |
| `-request-dry-run` | 使用 Burp raw request 文件时 | 渲染并打印首个最终 raw request，不发送请求。 |

### request-dry-run：只渲染 raw request，不发请求

`-request-dry-run` 是 raw request 文件场景下的安全预演。它会读取 Burp 导出的请求、替换 placeholder、补全目标协议、重算 `Content-Length`，然后打印首个最终 raw request；打印后任务结束，不会发出网络请求。

### scope：停止计数的分组键

在 `rfuzz` 里，scope 指的是由一个或多个 wordlist keyword 组成的“分组键”。

`-stop-scope TARGET` 表示：每个 `TARGET` 单独计数，某个 `TARGET` 命中达到阈值后，跳过这个 `TARGET` 的剩余组合。

`-stop-scope TARGET,USER` 表示：每个 `TARGET + USER` 组合单独计数，某个目标上的某个用户命中后，只跳过这个用户的剩余密码，不影响同一目标上的其他用户。

`-stop-scope` 里的 keyword 必须来自 `-w file:KEYWORD` 定义的字典。运行时，`rfuzz` 会从当前 case 中取出这些 keyword 的值组成 key；命中数达到 `-stop-on-match` 后，相同 key 的后续 case 会被跳过。

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

预检查只遍历 `-precheck-key` 对应 payload，不会组合其他字典。预检查 URL 会和主请求一样应用 `-enc` 编码链，并使用同样的 URL 空格归一化逻辑。预检查使用独立超时，默认 `-precheck-timeout 3`，并优先用 `HEAD` 探测；如果 `HEAD` 不允许或失败，再 fallback 到 `GET`。只要能收到 HTTP 响应，就视为目标可达；`200`、`301`、`401`、`403`、`404`、`500` 等状态码都算可达。

预检查会去重相同的目标值，并在 `stderr` 显示独立的逐轮进度条。显式的 `HEAD`/`GET` 回退请求以及协议候选都会分别计入全局 `-rate` 限速；`-no-progress` 会同时关闭预检查和主任务进度条。

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
  -precheck-attempts 5 \
  -precheck-timeout 2
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
  -mhr 'Set-Cookie: session_id=' \
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

假设有：

```text
TARGET = [a.com, b.com]
USER   = [alice, bob]
PASS   = [123456, admin, qwerty]
```

#### 示例 A：`-stop-scope TARGET -stop-on-match 1`

含义：每个目标只要命中 1 次，就跳过该目标的所有剩余 `USER/PASS` 组合。

如果 `a.com + alice + admin` 命中，那么：

- `a.com + alice + qwerty` 会跳过；
- `a.com + bob + 123456/admin/qwerty` 也会跳过；
- `b.com` 不受影响，继续跑。

对应命令：

```bash
rfuzz -u https://TARGET/login \
  -w targets.txt:TARGET \
  -w users.txt:USER \
  -w passwords.txt:PASS \
  -mhr 'Set-Cookie: session_id=' \
  -stop-scope TARGET \
  -stop-on-match 1
```

#### 示例 B：`-stop-scope TARGET,USER -stop-on-match 1`

含义：每个“目标 + 用户”组合只要命中 1 次，就跳过该用户在该目标上的剩余密码。

如果 `a.com + alice + admin` 命中，那么：

- `a.com + alice + qwerty` 会跳过；
- `a.com + bob + ...` 继续跑；
- `b.com + alice + ...` 也继续跑。

对应命令：

```bash
rfuzz -u https://TARGET/login \
  -w targets.txt:TARGET \
  -w users.txt:USER \
  -w passwords.txt:PASS \
  -mhr 'Set-Cookie: session_id=' \
  -stop-scope TARGET,USER \
  -stop-on-match 1
```

`-stop-scope` 也可以和 `-order`、`rotate-window`、`precheck` 一起使用。达到停止阈值后，`rfuzz` 会快进跳过相同分组键的剩余组合。

注意：`-stop-on-match` 不会撤回已经发出的请求。在高并发或 `rotate-window` 调度下，某个分组命中后，`rfuzz` 会停止调度新的同分组请求，但已经在飞的同分组请求仍可能完成。

### 安全预演：dry-run / explain / request-dry-run

大任务开始前，可以用安全预演检查请求模板、placeholder、字典大小、预计请求数、并发、限速、预检查模式、输出路径、响应 body 限制，以及首个渲染后的最终请求：

```bash
rfuzz -w dirs.txt:DIR \
  -u 'https://example.com/${{DIR}}$' \
  -fc 404 \
  -dry-run
```

`-explain` 也是不发送网络请求的计划说明视图；使用 Burp raw request 文件时，用 `-request-dry-run` 查看首个最终 raw request。

---

## 输出与进度

默认情况下，`rfuzz` 在 `stderr` 显示预检查和主任务进度条，不污染写到 `stdout` 或 `-o` 的结果。

进度字段：

```text
done/total | percent | matched | errors | skipped | err | ETA
```

`ETA` 使用最近完成的真实 case 速度估算，会包含限速、延迟、跳过、匹配和输出写入带来的实际影响。

响应命中时，console 输出会显示命中的组合、最终渲染 URL 和响应摘要：

```text
[MATCH] PASS=admin,URLFUZZ=https://example.com,USER=alice -> https://example.com/login [Status: 200, Size: 12, Words: 2, Lines: 1, Time: 35ms]
```

如果 `-o` 把 JSONL、CSV 或 console 输出写入文件，`rfuzz` 仍会把简洁的 `[MATCH]` 命中行同步打印到 `stderr`，方便运行时直接看到命中结果。开启进度条时，命中行会通过进度条渲染器安全打印，不会被动态刷新覆盖。静默模式（`-s`）仍保持只输出 URL。

输出文件会在每条命中记录写入后立即 flush，因此扫描尚未结束时也可以读取 JSONL/CSV 结果，例如使用 `tail -f result.jsonl`。

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

任务结束时，`rfuzz` 会在 `stderr` 打印摘要：total、matched、filtered、error、skipped、主要错误分类、高频响应签名、输出路径，以及 `-stop-on-match` 是否真的触发。也可以保存为 JSON：

```bash
rfuzz -w dirs.txt:DIR \
  -u 'https://example.com/${{DIR}}$' \
  -summary-json summary.json
```

---

## 性能与稳定性建议

### 并发与文件描述符

Unix/Linux 下，`rfuzz` 启动时会检查 `ulimit -n`，并估算当前参数是否能放进文件描述符上限。估算会包含 worker 并发、DNS 并发、输出文件，以及多目标预检查/轮转时 keep-alive 连接池可能累积的连接数。如果当前值太低，`rfuzz` 会打印当前值、估算需要值，以及启动前应该执行的 `ulimit` 命令。

大量目标或高并发任务前，按启动警告打印的估算值设置：

```bash
ulimit -n <估算需要值>
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

### 响应 Body 限制

`rfuzz` 不再无上限读取响应 body。默认每个响应最多读取 2 MiB，raw response 输出默认展示 4096 字节预览。目录枚举、二进制接口或大文件下载场景可以按需调整：

```bash
rfuzz -w dirs.txt:DIR \
  -u 'https://example.com/${{DIR}}$' \
  -ignore-body
```

```bash
rfuzz -w files.txt:FILE \
  -u 'https://example.com/${{FILE}}$' \
  -max-body 1048576 \
  -body-preview 2048
```

---

## 参数速查

### 请求参数

| 参数 | 说明 | 示例 |
| --- | --- | --- |
| `-u` | URL 模板。 | `-u https://HOST/FUZZ` |
| `-request` | Burp raw request 文件。 | `-request login.txt` |
| `-request-proto` | raw request 协议，只接受 `http` / `https`。 | `-request-proto https` |
| `-request-dry-run` | 安全预演 raw request：渲染并打印首个最终请求，不发送请求。 | `-request-dry-run` |
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
| `-target-window` | 每批轮转目标数量；默认值：`100`。 | `-target-window 100` |
| `-target-burst` | 每个目标连续请求数；默认值：`3`。 | `-target-burst 3` |
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
| `-p` | 请求间有限延迟或随机范围；拒绝 NaN/inf。 | `-p 0.1-0.5` |
| `-dry-run` | 安全预演：输出计划和首个渲染请求，不发送请求。 | `-dry-run` |
| `-explain` | 解释安全预演计划，不发送请求。 | `-explain` |
| `-no-progress` | 关闭预检查和主任务进度条。 | `-no-progress` |
| `-precheck` | 开关预检查。 | `-precheck off` |
| `-precheck-key` | 目标预检查 keyword。 | `-precheck-key TARGET` |
| `-precheck-report-only` | 只报告，不跳过失败 payload。 | `-precheck-report-only` |
| `-precheck-attempts` | 预检查按轮次遍历目标的次数。 | `-precheck-attempts 5` |
| `-precheck-timeout` | 每个预检查请求的超时秒数；默认值：`3`。 | `-precheck-timeout 2` |
| `-r` | 跟随重定向。 | `-r` |
| `-raw` | 禁用 URI 空格编码。 | `-raw` |
| `-x` | 请求代理。 | `-x http://127.0.0.1:8080` |
| `-replay-proxy` | 命中后 replay 到代理。 | `-replay-proxy http://127.0.0.1:8081` |
| `-http2` | 强制 HTTP/2 prior knowledge。 | `-http2` |
| `-ssl-verify` | 开关证书校验，默认 off。 | `-ssl-verify on` |
| `-keepalive` | 开关 HTTP keep-alive。 | `-keepalive off` |
| `-dns-cache` | 开关 DNS 缓存。 | `-dns-cache on` |
| `-dns-cache-ttl` | 成功 DNS 缓存 TTL 秒数。 | `-dns-cache-ttl 300` |
| `-dns-negative-cache-ttl` | DNS 失败缓存 TTL 秒数，`0` 表示禁用失败缓存。 | `-dns-negative-cache-ttl 30` |
| `-dns-max-concurrent` | 最大并发真实 DNS 解析数。 | `-dns-max-concurrent 64` |
| `-sni` | 兼容参数，当前不支持任意覆盖 SNI。 | `-sni example.com` |
| `-cc` / `-ck` | 客户端证书和私钥，必须成对提供。 | `-cc client.crt -ck client.key` |
| `-ignore-body` | 不读取响应 body，仍保留状态码和 header。 | `-ignore-body` |
| `-max-body` | 每个响应最多读取的 body 字节数；默认值：`2097152`。 | `-max-body 1048576` |
| `-body-preview` | raw response 中最多展示的 body 字节数；默认值：`4096`。 | `-body-preview 2048` |
| `-ac` | 自动校准预留开关。 | `-ac` |
| `-ac-scope` | 自动校准作用域预留参数：`host`、`job` 或 `global`。 | `-ac-scope job` |
| `-ac-ignore` | 自动校准忽略 keyword 预留参数，可重复。 | `-ac-ignore URLFUZZ` |

### 匹配与过滤

过滤器优先于匹配器。响应命中过滤条件时，不会输出。

| 参数 | 说明 | 示例 |
| --- | --- | --- |
| `-mc` / `-fc` | 匹配/过滤状态码。 | `-mc 200,204,300-399` |
| `-ms` / `-fs` | 匹配/过滤响应大小。 | `-fs 0` |
| `-mw` / `-fw` | 匹配/过滤单词数。 | `-mw 10-30` |
| `-ml` / `-fl` | 匹配/过滤行数。 | `-ml 5-20` |
| `-mhr` / `-fhr` | 匹配/过滤响应头正则，不读取 body。 | `-mhr 'Set-Cookie: session_id='` |
| `-mr` / `-fr` | 匹配/过滤完整 raw response 正则，会读取响应 body。 | `-mr 'welcome'` |
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
| `-summary-json` | 保存任务结束摘要 JSON。 | `-summary-json summary.json` |
| `-stop-scope` | 停止计数的分组 key。 | `-stop-scope TARGET,USER` |
| `-stop-on-match` | scope 命中 N 次后停止调度新请求；不取消已在飞请求。 | `-stop-on-match 1` |

---

## 当前实现状态

- CLI：请求、输入、执行、matcher/filter、header-only matcher、输出、dry-run、body limit、summary、预算控制等主要参数。
- 模板：`${{KEYWORD}}$` 原生占位符和裸 keyword 兼容，并在启动前校验未知 placeholder 和未使用的 wordlist keyword。
- 输入模式：`clusterbomb`、`pitchfork`；`sniper` 当前降级为 pitchfork。
- 调度：自定义 `-order`、`rotate-window` 目标轮转。
- HTTP：tokio + reqwest，支持代理、replay proxy、重定向、超时、keep-alive、DNS 缓存、全局限速、lazy raw request 构造、raw request `Content-Length` 重算、body CRLF 保留、header-only 响应匹配、precheck 独立超时、HEAD 优先预检查，以及有上限的响应 body 读取。
- 预检查：目标 payload 连通性检查，支持按轮次重试、只报告模式，失败 payload 默认跳过。
- 匹配/过滤：状态码、大小、单词数、行数、响应时间、完整 raw response 正则、and/or 组合。
- 输出：可读命中行、silent URL、JSONL、CSV、raw request/response 保存、错误 JSONL 日志和任务结束摘要。
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
