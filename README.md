# rfuzz

`rfuzz` is a Rust command-line web fuzzer compatible with common `ffuf`
workflows.

`rfuzz` 是一个使用 Rust 编写的命令行 Web Fuzzer，兼容常见 `ffuf`
使用习惯，适用于授权安全测试、企业资产自查、靶场和研究环境。

## Safety / 授权测试说明

Use `rfuzz` only on targets you are explicitly authorized to test.

仅在你拥有明确授权的目标上使用 `rfuzz`。扫描行为可能对服务造成压力，请遵守当地法律、组织规则和授权范围。

## Build / 编译

```bash
cargo build --release
```

Release binary / Release 二进制：

```text
target/release/rfuzz
```

Debug binary / Debug 二进制：

```text
target/debug/rfuzz
```

After building, run examples with `rfuzz`. If it is not in `PATH`, use the local
binary path such as `target/debug/rfuzz`.

编译后示例统一使用 `rfuzz`。如果没有加入 `PATH`，请使用本地二进制路径，例如 `target/debug/rfuzz`。

## Quick Examples / 快速示例

### Directory Fuzz / 目录扫描

```bash
rfuzz -w dirs.txt:DIR \
  -u 'https://example.com/${{DIR}}$' \
  -fc 404
```

### POST Fuzz / POST 表单爆破

```bash
rfuzz -w passwords.txt:PASS \
  -u https://example.com/login \
  -X POST \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -d 'username=admin&password=${{PASS}}$' \
  -fc 401
```

### Multi-Wordlist Login Fuzz / 多字典登录爆破

If `3xui_http_vpn.txt` contains host names or `host:port` values, put the scheme
in `-u`:

如果 `3xui_http_vpn.txt` 里是域名、IP 或 `host:port`，在 `-u` 里补协议：

```bash
rfuzz -u https://URLFUZZ/login \
  -H "Content-Type: application/x-www-form-urlencoded; charset=UTF-8" \
  -X POST \
  -d "username=UFUZZ&password=PFUZZ&twoFactorCode=" \
  -w 3xui_http_vpn.txt:URLFUZZ \
  -w dir/50_name.txt:UFUZZ \
  -w dir/top100.txt:PFUZZ \
  -precheck-key URLFUZZ \
  -schedule rotate-window \
  -target-key URLFUZZ \
  -target-window 100 \
  -target-burst 3 \
  -mr "Set-Cookie: 3x-ui=" \
  -stop-scope URLFUZZ \
  -stop-on-match 1 \
  -o 3xui_result.jsonl \
  -of jsonl \
  -t 100
```

If the URL wordlist already contains full URLs such as
`https://example.com:54321`, use `-u URLFUZZ/login`.

如果 URL 字典每行已经是 `https://example.com:54321` 这种完整 URL，可以使用
`-u URLFUZZ/login`。

推荐使用 `-schedule rotate-window` 做 URL 轮转。上面的命令会在 100 个
`URLFUZZ` 目标组成的窗口内轮转，每个目标连续尝试 3 个账号/密码组合后切到下一个目标；同时
`-stop-scope URLFUZZ -stop-on-match 1` 会让某个 URL 命中一次后跳过该 URL 的剩余组合。

`-precheck-key URLFUZZ` 会先探测目标是否能收到 HTTP 响应，预检查失败的
`URLFUZZ` payload 在正式 fuzz 时会被跳过。

### Stop After One Hit Per URL / 每个 URL 命中一次后停止

Stop the remaining username/password combinations for a URL after the first
matched credential:

某个 `URLFUZZ` 命中一次后，跳过该 URL 后续账号密码组合，继续下一个 URL：

```bash
rfuzz -u https://URLFUZZ/login \
  -H "Content-Type: application/x-www-form-urlencoded; charset=UTF-8" \
  -X POST \
  -d "username=UFUZZ&password=PFUZZ&twoFactorCode=" \
  -w 3xui_http_vpn.txt:URLFUZZ \
  -w dir/50_name.txt:UFUZZ \
  -w dir/top100.txt:PFUZZ \
  -mr "Set-Cookie: 3x-ui=" \
  -stop-scope URLFUZZ \
  -stop-on-match 1 \
  -o 3xui_result.jsonl \
  -of jsonl
```

### Stop After One Password Per URL And User / 每个 URL+用户命中一次后停止

Stop only the current URL and username pair after one matched password:

某个 `URLFUZZ + UFUZZ` 找到一个密码后，跳过该 URL 下该用户的后续密码，继续其他用户或 URL：

```bash
rfuzz -u https://URLFUZZ/login \
  -H "Content-Type: application/x-www-form-urlencoded; charset=UTF-8" \
  -X POST \
  -d "username=UFUZZ&password=PFUZZ&twoFactorCode=" \
  -w 3xui_http_vpn.txt:URLFUZZ \
  -w dir/50_name.txt:UFUZZ \
  -w dir/top100.txt:PFUZZ \
  -mr "Set-Cookie: 3x-ui=" \
  -stop-scope URLFUZZ,UFUZZ \
  -stop-on-match 1 \
  -o 3xui_result.jsonl \
  -of jsonl
```

Scoped stop mode runs multiple scopes concurrently. A single scope is processed
sequentially so `rfuzz` can stop that scope as soon as its match threshold is
reached. With `-stop-scope URLFUZZ`, `-t 100` can process up to 100 URLs in
parallel, while each individual URL stops cleanly after its configured match
count.

scoped stop 模式会并发处理多个 scope。单个 scope 内部按顺序执行，这样某个 scope 达到命中阈值后可以立刻停止。使用 `-stop-scope URLFUZZ` 时，`-t 100` 可以同时处理最多 100 个 URL，同时每个 URL 会在达到配置的命中次数后干净停止。

When a scoped stop threshold is reached, `rfuzz` fast-forwards over the remaining
combinations in that scope. For example, `-stop-scope URLFUZZ,UFUZZ
-stop-on-match 1` jumps over the remaining passwords for the current URL and
user after one match.

当某个 scope 达到停止阈值后，`rfuzz` 会快进跳过该 scope 的剩余组合。例如
`-stop-scope URLFUZZ,UFUZZ -stop-on-match 1` 在某个 URL+用户命中一次后，会直接跳过该用户剩余密码。

When `-precheck-key` is inside the scoped stop key, failed precheck scopes are
also fast-forwarded as a whole. For example, with `-precheck-key URLFUZZ
-stop-scope URLFUZZ`, a dead URL is skipped once as a full URL scope instead of
refreshing progress for every username/password combination under that URL.

当 `-precheck-key` 包含在 scoped stop 的 key 里时，预检查失败的 scope 也会整组快进。例如
`-precheck-key URLFUZZ -stop-scope URLFUZZ` 下，失效 URL 会作为一个完整 URL scope 被跳过，而不是对该 URL 下每个用户名/密码组合逐条刷新进度。

## Progress Bar / 进度条

`rfuzz` shows a progress bar on `stderr` by default. It does not pollute JSONL,
CSV, or console result output written to `stdout` or `-o`.

`rfuzz` 默认在 `stderr` 显示进度条，不会污染写到 `stdout` 或 `-o` 的 JSONL、CSV、
console 结果。

Progress fields:

进度字段：

```text
done/total | percent | matched | errors | skipped | avg | req/s | err | ETA
```

Example:

示例：

```text
[====================>-------------------] 6140/10000 61% | matched 3 | errors 12 | skipped 0 | avg 120ms | 98.7 req/s | err timeout 9,connect 3 | ETA 00:39
```

`req/s` is calculated from actual completed HTTP requests, including failed
requests. In scoped stop mode, skipped combinations advance `done/total` and
`skipped`, but they are not counted as HTTP requests.

`req/s` 按实际完成的 HTTP 请求计算，包括请求失败。scoped stop 模式下，被跳过的组合会推进
`done/total` 和 `skipped`，但不会计入 HTTP 请求数。

`avg` is the average HTTP request time for completed requests. If `avg` is
several seconds while `-t` is high, workers are waiting on slow network failures
or slow target responses. `err` shows the most common request error classes:
`timeout`, `dns`, `connect`, `tls`, `redirect`, `fd`, `request`, and `other`.

`avg` 是已完成 HTTP 请求的平均耗时。如果 `-t` 很高但 `avg` 达到几秒，说明 worker 主要卡在慢网络失败或慢目标响应上。`err` 显示常见请求错误分类：
`timeout`、`dns`、`connect`、`tls`、`redirect`、`fd`、`request`、`other`。

Disable progress for scripts, logs, or CI:

脚本、日志或 CI 场景可以关闭进度条：

```bash
rfuzz -w dirs.txt:DIR -u 'https://example.com/${{DIR}}$' -no-progress
```

## ulimit And Concurrency / ulimit 与并发

On Unix/Linux, `rfuzz` checks `ulimit -n` at startup and estimates whether the
requested `-t` value is safe for the current file-descriptor limit. If the limit
is too low, `rfuzz` automatically lowers the effective concurrency and prints a
startup warning.

在 Unix/Linux 上，`rfuzz` 启动时会检查 `ulimit -n`，估算当前文件描述符上限是否足够支撑用户设置的 `-t` 并发。如果上限太低，`rfuzz` 会自动下调实际并发，并在启动时打印提示。

Example startup messages:

启动提示示例：

```text
# fd-limit: ulimit -n=1024, -t=100, 估算至少需要 528。大量 URLFUZZ/预检查建议先设置：ulimit -n 8192
 WARN ulimit -n=256 较低，已将并发 -t 从 100 下调到 32。建议运行前设置：ulimit -n 8192，或继续降低 -t。
```

Recommended Linux setting before large URL or password fuzzing:

大量 URL 或密码爆破前，建议先在 Linux shell 中设置：

```bash
ulimit -n 8192
```

If the shell reports that the limit cannot be raised, lower `-t` instead, or
raise the hard limit through the system/service configuration.

如果 shell 提示无法提高上限，就降低 `-t`；或者通过系统、服务配置提高 hard limit。

`rfuzz` keeps HTTP keep-alive enabled by default because HTTPS fuzzing is much
faster when TCP/TLS connections can be reused. If a very large multi-target run
still exhausts file descriptors, use `-keepalive off` as a low-FD fallback; it is
safer on small `ulimit -n` values, but HTTPS throughput can drop because every
request may need a new TCP/TLS handshake.

`rfuzz` 默认开启 HTTP keep-alive，因为 HTTPS 爆破可以复用 TCP/TLS 连接时会快很多。如果超大规模多目标任务仍然耗尽文件描述符，可以使用 `-keepalive off` 作为低 FD 模式；它在较小的
`ulimit -n` 下更稳，但 HTTPS 吞吐会下降，因为每个请求可能都要重新建立 TCP/TLS 连接。

`rfuzz` 默认开启进程内 DNS 缓存：`-dns-cache on`，默认成功解析 TTL 为 300
秒，失败解析 TTL 为 30 秒。预检查和正式 fuzz 共用同一个 HTTP client，所以预检查阶段解析过的域名可以在正式 fuzz 阶段复用。

同一个 host 的并发 DNS 请求会合并成一次真实解析，不同 host 的真实解析默认最多同时进行
64 个。这样可以降低大规模 `URLFUZZ` 对系统 resolver 的瞬时压力，也可以避免不存在域名在海量组合中被反复解析。

可以按需调整：

```bash
rfuzz -u https://URLFUZZ/login -w urls.txt:URLFUZZ \
  -dns-cache on \
  -dns-cache-ttl 300 \
  -dns-negative-cache-ttl 30 \
  -dns-max-concurrent 64
```

DNS 缓存只能减少重复解析开销，不能把连接超时、目标限速、目标服务慢响应变快。进度条里的
`err dns` 仍然表示该请求因为 DNS 失败而结束；如果命中失败缓存，它不会再次发起真实系统解析。如果进度条里 `avg` 很高并且 `err timeout` 占大头，瓶颈通常是目标不可达或超时等待，而不是 DNS。

## Precheck / 预检查

`rfuzz` supports target precheck before the main fuzz run. The precheck switch is
on by default, and target precheck runs when `-precheck-key` is provided.

`rfuzz` 支持在正式 fuzz 前先做目标预检查。预检查开关默认开启；当指定
`-precheck-key` 后，会先对这个 keyword 对应的目标 payload 做连通性检查。

Example:

示例：

```bash
rfuzz -u URLFUZZ/login \
  -H "Content-Type: application/x-www-form-urlencoded; charset=UTF-8" \
  -X POST \
  -d "username=UFUZZ&password=PFUZZ&twoFactorCode=" \
  -w 3xui_http_vpn.txt:URLFUZZ \
  -w dir/50_name.txt:UFUZZ \
  -w dir/top100.txt:PFUZZ \
  -precheck-key URLFUZZ \
  -schedule rotate-window \
  -target-key URLFUZZ \
  -target-window 100 \
  -target-burst 3 \
  -mr "Set-Cookie: 3x-ui=" \
  -o 3xui_result.jsonl \
  -of jsonl \
  -t 100
```

Precheck only iterates the payload values of `-precheck-key`. It does not combine
the other fuzz payloads. With the example above, it checks each `URLFUZZ` value
once instead of checking every `URLFUZZ + UFUZZ + PFUZZ` combination.

预检查只遍历 `-precheck-key` 指定的 payload 值，不会组合其他 fuzz payload。以上面的命令为例，它只会对每个 `URLFUZZ` 值检查一次，不会生成
`URLFUZZ + UFUZZ + PFUZZ` 的完整组合。

Supported URL template forms:

支持的 URL 模板形式：

```text
URLFUZZ/login
http://URLFUZZ/login
https://URLFUZZ/login
http://asdsadasd.URLFUZZ/login
```

If the rendered precheck URL already starts with `http://` or `https://`, rfuzz
checks that URL as-is. If no scheme is present, rfuzz tries `https://...` first
and then `http://...`.

如果渲染后的预检查 URL 已经带 `http://` 或 `https://`，rfuzz 会按原样检查。如果没有协议，rfuzz 会先尝试 `https://...`，再尝试 `http://...`。

Precheck only cares whether an HTTP response can be received. Status codes such
as `200`, `301`, `401`, `403`, `404`, and `500` all count as reachable.

预检查只关心是否能正常收到 HTTP 响应，不关心状态码。`200`、`301`、`401`、`403`、`404`、`500` 都算目标可达。

By default, rfuzz does not verify SSL/TLS certificates. Self-signed certificates,
expired certificates, and hostname mismatches are accepted. Use `-ssl-verify on`
only when strict certificate verification is required.

默认情况下，rfuzz 不校验 SSL/TLS 证书。自签名证书、过期证书、证书域名不匹配都会被接受。只有需要严格证书校验时，才使用 `-ssl-verify on`。

These count as precheck failures:

以下情况会被视为预检查失败：

```text
DNS 解析失败
连接超时
连接被拒绝
TLS/证书错误
代理连接失败
URL 格式错误
```

Failed precheck values are printed to the terminal with a short reason:

预检查失败会在终端打印失败 URL 和简短原因：

```text
PRECHECK ERROR URLFUZZ=101.36.104.56:2053 https://101.36.104.56:2053/login: timeout; http://101.36.104.56:2053/login: connection refused
```

If `curl` or `nslookup` works for a single hostname but precheck reports a DNS
failure during a large run, the cause is often transient resolver pressure or
file-descriptor exhaustion during concurrent precheck, not a permanently broken
domain. `rfuzz` retries temporary DNS/FD-related precheck errors briefly and
classifies FD exhaustion separately as `file descriptor exhausted`.

如果单独用 `curl` 或 `nslookup` 能解析某个域名，但大批量预检查时显示 DNS 失败，通常是并发预检查给解析器或文件描述符带来的临时压力，不一定代表域名永久失效。`rfuzz` 会对临时 DNS/FD 类预检查错误做短重试，并把 FD 耗尽单独显示为
`file descriptor exhausted`。

By default, a failed precheck payload value is skipped during the main fuzz run.
This skip is based on the `-precheck-key` payload value, not on a complete URL
string. This is important for templates such as
`http://asdsadasd.URLFUZZ/login`, where `URLFUZZ` is only part of the final URL.

默认情况下，预检查失败的 payload 值会在正式 fuzz 阶段被跳过。跳过依据是
`-precheck-key` 对应的 payload 值，而不是完整 URL 字符串。这样对
`http://asdsadasd.URLFUZZ/login` 这类拼接 URL 更严谨。

Disable precheck:

关闭预检查：

```bash
rfuzz -u URLFUZZ/login -w urls.txt:URLFUZZ -precheck off
```

Only report precheck errors, but do not skip failed payload values:

只报告预检查错误，不跳过失败 payload 值：

```bash
rfuzz -u URLFUZZ/login -w urls.txt:URLFUZZ -precheck-key URLFUZZ -precheck-report-only
```

## Request Errors / 请求错误

Request failures such as DNS errors, connection failures, and timeouts are not
printed for every request by default. They are counted in the progress bar as
`errors` so the terminal stays readable during large runs.

DNS 错误、连接失败、超时等请求失败默认不会逐条打印出来，只会计入进度条里的
`errors`，避免大任务运行时刷屏。

To keep failed payload combinations for later analysis, write an error JSONL log:

如需后续分析失败的 payload 组合，可以写入错误 JSONL 日志：

```bash
rfuzz -u https://URLFUZZ/login \
  -H "Content-Type: application/x-www-form-urlencoded; charset=UTF-8" \
  -X POST \
  -d "username=UFUZZ&password=PFUZZ&twoFactorCode=" \
  -w 3xui_http_vpn.txt:URLFUZZ \
  -w dir/50_name.txt:UFUZZ \
  -w dir/top100.txt:PFUZZ \
  -mr "Set-Cookie: 3x-ui=" \
  -error-log request_errors.jsonl \
  -o 3xui_result.jsonl \
  -of jsonl \
  -t 100
```

Each error log record contains `url`, `input`, `input_values`, and `error`.

每条错误日志包含 `url`、`input`、`input_values` 和 `error`。

## Placeholders And Shell Quoting / 占位符与 Shell 引号

`rfuzz` supports two placeholder styles:

`rfuzz` 支持两种占位符写法：

| Style / 类型 | Example / 示例 | Notes / 说明 |
| --- | --- | --- |
| Native explicit / 原生显式 | `${{FUZZ}}$`, `${{PASS}}$` | Precise, avoids accidental replacement. 精确，不容易误替换普通文本。 |
| Bare keyword / 裸 keyword | `FUZZ`, `PASS`, `URLFUZZ` | ffuf-compatible and shell-friendly. 兼容 ffuf，命令行里更省心。 |

When using `${{KEYWORD}}$`, wrap the template in single quotes in Bash, zsh, and
PowerShell so the shell does not interpret `$` or `${...}`:

使用 `${{KEYWORD}}$` 时，建议在 Bash、zsh、PowerShell 中用单引号包裹模板，避免
`$` 或 `${...}` 被终端提前解释：

```bash
rfuzz -w dirs.txt:FUZZ -u 'https://example.com/${{FUZZ}}$'
```

For maximum ffuf-style compatibility, use bare keywords:

需要最大化兼容 ffuf 命令习惯时，可以使用裸 keyword：

```bash
rfuzz -w dirs.txt:FUZZ -u https://example.com/FUZZ
```

## Wordlist Modes / 字典组合模式

| Mode / 模式 | Behavior / 行为 | Example / 示例 |
| --- | --- | --- |
| `clusterbomb` | Cartesian product of all wordlists. 所有字典做笛卡尔积。 | `-mode clusterbomb` |
| `pitchfork` | Read multiple wordlists by row index. 多个字典按行同步读取。 | `-mode pitchfork` |
| `sniper` | Reserved in v0.1, currently uses pitchfork behavior. v0.1 预留，目前降级为 pitchfork。 | `-mode sniper` |

Default mode is `clusterbomb`.

默认模式是 `clusterbomb`。

### Custom Order / 自定义组合顺序

Use `-order <KEYWORD,...>` to control `clusterbomb` generation order. The list
must include every wordlist keyword exactly once. The last keyword changes
fastest.

使用 `-order <KEYWORD,...>` 控制 `clusterbomb` 的组合生成顺序。列表必须且只能包含所有字典 keyword
各一次。最后一个 keyword 变化最快。

Example:

示例：

```bash
rfuzz -u URLFUZZ/login \
  -H "Content-Type: application/x-www-form-urlencoded; charset=UTF-8" \
  -X POST \
  -d "username=UFUZZ&password=PFUZZ&twoFactorCode=" \
  -w 3xui_http_vpn.txt:URLFUZZ \
  -w dir/50_name.txt:UFUZZ \
  -w dir/top100.txt:PFUZZ \
  -mode clusterbomb \
  -order UFUZZ,PFUZZ,URLFUZZ \
  -mr "Set-Cookie: 3x-ui=" \
  -o 3xui_result.jsonl \
  -of jsonl \
  -t 100
```

Generated request order:

请求顺序：

```text
UFUZZ=user1 PFUZZ=pass1 URLFUZZ=url1
UFUZZ=user1 PFUZZ=pass1 URLFUZZ=url2
UFUZZ=user1 PFUZZ=pass1 URLFUZZ=url3
...
UFUZZ=user1 PFUZZ=pass2 URLFUZZ=url1
UFUZZ=user1 PFUZZ=pass2 URLFUZZ=url2
...
```

When `-order` is set, `rfuzz` uses the order as generation priority but does not
wait for each order batch to drain before scheduling the next work. Cases are
fed through a bounded in-memory queue, so large jobs keep high concurrency
without preloading the whole Cartesian product.

设置 `-order` 后，`rfuzz` 会把它作为组合生成优先级，但不会等待每个 order
批次全部完成后才调度下一批。请求会通过一个有界内存队列送入 worker，因此大任务可以保持高并发，同时不会把整个笛卡尔积预加载进内存。

`-order` currently applies to `clusterbomb` mode only.

`-order` 当前仅适用于 `clusterbomb` 模式。

### Rotate Window Schedule / 轮转窗口调度

当目标数量很多、又希望轮转 URL 避免长时间打同一个目标时，推荐使用
`-schedule rotate-window`。它仍然是懒生成组合，不会把所有请求预加载到内存。

```bash
rfuzz -u URLFUZZ/login \
  -H "Content-Type: application/x-www-form-urlencoded; charset=UTF-8" \
  -X POST \
  -d "username=UFUZZ&password=PFUZZ&twoFactorCode=" \
  -w 3xui_http_vpn.txt:URLFUZZ \
  -w dir/50_name.txt:UFUZZ \
  -w dir/top100.txt:PFUZZ \
  -precheck-key URLFUZZ \
  -schedule rotate-window \
  -target-key URLFUZZ \
  -target-window 100 \
  -target-burst 3 \
  -stop-scope URLFUZZ \
  -stop-on-match 1 \
  -mr "Set-Cookie: 3x-ui=" \
  -o 3xui_result.jsonl \
  -of jsonl \
  -t 100
```

参数含义：

| 参数 | 说明 | 示例 |
| --- | --- | --- |
| `-schedule rotate-window` | 启用轮转窗口调度。 | `-schedule rotate-window` |
| `-target-key` | 指定哪个 keyword 是 URL/目标。未指定时依次尝试使用 `-precheck-key`、`-stop-scope` 第一个 keyword、第一组字典 keyword。 | `-target-key URLFUZZ` |
| `-target-window` | 每批保持多少个目标参与轮转。窗口越大，越分散；窗口越小，连接复用更容易命中。 | `-target-window 100` |
| `-target-burst` | 每个目标连续发多少个组合后切换到下一个目标。 | `-target-burst 3` |

生成顺序示意：

```text
URLFUZZ=url1 UFUZZ=user1 PFUZZ=pass1
URLFUZZ=url1 UFUZZ=user1 PFUZZ=pass2
URLFUZZ=url1 UFUZZ=user1 PFUZZ=pass3
URLFUZZ=url2 UFUZZ=user1 PFUZZ=pass1
URLFUZZ=url2 UFUZZ=user1 PFUZZ=pass2
URLFUZZ=url2 UFUZZ=user1 PFUZZ=pass3
...
```

这个模式适合“既要轮转 URL，又希望保持较高请求速率”的场景。它可以和
`-order` 一起使用；当 `-order` 配合 `-precheck-key`、`-target-key` 或
`-stop-scope` 指定了目标 keyword 时，`rfuzz` 会自动使用目标窗口轮转，避免等一个账号/密码组合扫完整个 URL 列表后才进入下一个组合，因此更容易让活跃窗口内的 DNS 缓存、TCP/TLS keep-alive 被复用。

`-schedule rotate-window` 只适用于 `clusterbomb` 模式。

Input combinations are generated lazily. Even when `clusterbomb` produces a very
large total such as hundreds of millions of combinations, `rfuzz` does not
preload all request cases into memory. Runtime memory is mainly determined by
loaded wordlists, in-flight requests, response bodies, and output buffering.

输入组合是惰性生成的。即使 `clusterbomb` 的总组合数达到数亿级，`rfuzz` 也不会把所有请求组合预加载进内存。运行时内存主要由已加载字典、并发中的请求、响应体和输出缓冲决定。

## Raw Request Mode / Raw 请求文件模式

Save a Burp-style request to a text file and load it with `-request`.

可以把 Burp Suite 中复制出来的 HTTP 请求保存为 txt，然后用 `-request` 读取。请求行、Header
和 Body 都支持模板占位符。

```bash
rfuzz -request login.txt \
  -request-proto https \
  -w passwords.txt:PASS \
  -fc 401
```

`login.txt`:

```http
POST /login HTTP/1.1
Host: example.com
Content-Type: application/x-www-form-urlencoded

username=admin&password=${{PASS}}$
```

`-request-proto` only applies to raw request files. It does not add a scheme to
`-u` templates.

`-request-proto` 只对 `-request` raw request 文件生效，不会给 `-u` 模板自动补协议。

## Matching And Filtering / 匹配与过滤

Filters run before matchers. If a response matches any active filter, it is not
output.

过滤器优先于匹配器。响应命中过滤条件时，不会输出。

Regex matchers and filters (`-mr` / `-fr`) match the complete raw response:
status line, response headers, blank line, and body.

正则匹配和过滤（`-mr` / `-fr`）匹配完整响应原文：状态行、响应头、空行和响应体。

```bash
rfuzz -w users.txt:USER -w passwords.txt:PASS \
  -u https://example.com/login \
  -X POST \
  -d 'username=${{USER}}$&password=${{PASS}}$' \
  -mr 'Set-Cookie: 3x-ui='
```

## Parameter Reference / 参数参考

ffuf-style single-dash long options such as `-mr` and `-request-proto` are
accepted. Standard double-dash forms such as `--mr` also work where applicable.

支持 ffuf 风格的单横线长参数，例如 `-mr`、`-request-proto`。对应的双横线形式在适用时也可用。

### Request / 请求参数

| Option / 参数 | Value / 取值 | Description / 说明 | Example / 示例 |
| --- | --- | --- | --- |
| `-u` | URL template / URL 模板 | Target URL template. 目标 URL 模板。 | `-u https://HOST/FUZZ` |
| `-request` | File path / 文件路径 | Burp raw request file. Burp raw 请求文件。 | `-request login.txt` |
| `-request-proto` | `http`, `https` | Scheme for raw request files. raw 请求文件使用的协议。 | `-request-proto https` |
| `-X` | HTTP method / HTTP 方法 | Request method. 请求方法。 | `-X POST` |
| `-H` | `Name: value` | Header template, repeatable. Header 模板，可重复。 | `-H "Content-Type: application/json"` |
| `-d` | Body template / Body 模板 | Request body template. 请求体模板。 | `-d 'q=${{FUZZ}}$'` |
| `-b` | Cookie string / Cookie 字符串 | Cookie template, repeatable. Cookie 模板，可重复。 | `-b 'sid=${{SID}}$'` |

### Input / 输入参数

| Option / 参数 | Value / 取值 | Description / 说明 | Example / 示例 |
| --- | --- | --- | --- |
| `-w` | `file[:KEYWORD]` | Wordlist with optional keyword, repeatable. 字典和可选 keyword，可重复。 | `-w users.txt:USER` |
| `-mode` | `clusterbomb`, `pitchfork`, `sniper` | Multi-wordlist mode. 多字典组合模式。 | `-mode clusterbomb` |
| `-order` | Keyword list / keyword 列表 | Clusterbomb generation order; last keyword changes fastest. clusterbomb 生成顺序，最后一个 keyword 变化最快。 | `-order UFUZZ,PFUZZ,URLFUZZ` |
| `-schedule` | `default`, `rotate-window` | Request generation schedule. 请求生成调度。 | `-schedule rotate-window` |
| `-target-key` | Keyword / keyword | URL/target keyword used by rotate-window. rotate-window 使用的 URL/目标 keyword。 | `-target-key URLFUZZ` |
| `-target-window` | Number / 数字 | Number of targets kept in each rotate-window batch. 每个轮转窗口里的目标数量。 | `-target-window 100` |
| `-target-burst` | Number / 数字 | Consecutive requests per target before switching. 每个目标连续请求多少次后切换。 | `-target-burst 3` |
| `-e` | Comma list / 逗号列表 | Append extensions to each wordlist item. 给字典项追加扩展名。 | `-e .php,.bak` |
| `-ic` | Flag / 开关 | Ignore comment lines starting with `#`. 忽略 `#` 开头的注释行。 | `-ic` |
| `-enc` | `KEYWORD:chain` | Apply encoders to a keyword. 对 keyword 应用编码链。 | `-enc 'DIR:urlencode'` |
| `-budget-requests` | Number / 数字 | Maximum generated request count. 最大请求预算。 | `-budget-requests 1000` |

Supported encoders: `urlencode`, `b64encode` / `base64`, `hex`, `lower`,
`upper`, `md5`, `sha1`.

支持的 encoder：`urlencode`、`b64encode` / `base64`、`hex`、`lower`、`upper`、`md5`、`sha1`。

### Execution And HTTP / 执行与 HTTP 参数

| Option / 参数 | Value / 取值 | Description / 说明 | Example / 示例 |
| --- | --- | --- | --- |
| `-t` | Number / 数字 | Concurrent workers; Unix/Linux startup may cap it by `ulimit -n`. 并发数；Unix/Linux 启动时可能按 `ulimit -n` 自动下调。 | `-t 100` |
| `-rate` | Number / 数字 | Global requests per second, `0` disables. 全局每秒请求数，`0` 表示不限速。 | `-rate 50` |
| `-timeout` | Seconds / 秒 | Request timeout. 请求超时。 | `-timeout 10` |
| `-p` | Seconds or range / 秒或范围 | Delay between requests. 请求间延迟。 | `-p 0.1-0.5` |
| `-no-progress` | Flag / 开关 | Disable progress bar. 禁用进度条。 | `-no-progress` |
| `-precheck` | `on`, `off` | Enable or disable precheck. 开启或关闭预检查。 | `-precheck off` |
| `-precheck-key` | Keyword / keyword | Payload keyword used as target precheck value. 作为目标预检查值的 payload keyword。 | `-precheck-key URLFUZZ` |
| `-precheck-report-only` | Flag / 开关 | Report precheck errors but do not skip failed payload values. 只报告预检查错误，不跳过失败 payload 值。 | `-precheck-report-only` |
| `-r` | Flag / 开关 | Follow redirects. 跟随重定向。 | `-r` |
| `-raw` | Flag / 开关 | Disable URI space encoding. 禁用 URI 空格编码。 | `-raw` |
| `-x` | Proxy URL / 代理 URL | Proxy for requests. 请求代理。 | `-x http://127.0.0.1:8080` |
| `-replay-proxy` | Proxy URL / 代理 URL | Replay matched requests through proxy. 命中后 replay 到代理。 | `-replay-proxy http://127.0.0.1:8081` |
| `-http2` | Flag / 开关 | Force HTTP/2 prior knowledge. 强制 HTTP/2 prior knowledge。 | `-http2` |
| `-ssl-verify` | `on`, `off` | Enable or disable SSL/TLS certificate verification. 开启或关闭 SSL/TLS 证书校验，默认关闭。 | `-ssl-verify on` |
| `-keepalive` | `on`, `off` | HTTP keep-alive connection reuse, default on. HTTP keep-alive 连接复用，默认开启。 | `-keepalive off` |
| `-dns-cache` | `on`, `off` | DNS cache, default on. DNS 解析缓存，默认开启。 | `-dns-cache on` |
| `-dns-cache-ttl` | Seconds / 秒 | DNS cache TTL. DNS 解析缓存有效期。 | `-dns-cache-ttl 300` |
| `-dns-negative-cache-ttl` | Seconds / 秒 | DNS failure cache TTL, `0` disables. DNS 失败缓存有效期，`0` 禁用。 | `-dns-negative-cache-ttl 30` |
| `-dns-max-concurrent` | Number / 数字 | Maximum concurrent real DNS lookups. 最大并发真实 DNS 解析数。 | `-dns-max-concurrent 64` |
| `-sni` | Hostname / 主机名 | Accepted for compatibility; arbitrary SNI override is not implemented. 兼容参数，当前不支持任意覆盖 SNI。 | `-sni example.com` |
| `-cc` | PEM path / PEM 路径 | Client certificate. 客户端证书。 | `-cc client.crt` |
| `-ck` | PEM path / PEM 路径 | Client private key. 客户端私钥。 | `-ck client.key` |

### Matchers And Filters / 匹配与过滤参数

| Option / 参数 | Value / 取值 | Description / 说明 | Example / 示例 |
| --- | --- | --- | --- |
| `-mc` | Status list/range / 状态码列表或范围 | Match status code. 匹配状态码。 | `-mc 200,204,300-399` |
| `-fc` | Status list/range / 状态码列表或范围 | Filter status code. 过滤状态码。 | `-fc 404` |
| `-ms` | Number/range / 数字或范围 | Match response size. 匹配响应大小。 | `-ms 100-500` |
| `-fs` | Number/range / 数字或范围 | Filter response size. 过滤响应大小。 | `-fs 0` |
| `-mw` | Number/range / 数字或范围 | Match word count. 匹配单词数。 | `-mw 10-30` |
| `-fw` | Number/range / 数字或范围 | Filter word count. 过滤单词数。 | `-fw 1` |
| `-ml` | Number/range / 数字或范围 | Match line count. 匹配行数。 | `-ml 5-20` |
| `-fl` | Number/range / 数字或范围 | Filter line count. 过滤行数。 | `-fl 0` |
| `-mr` | Regex / 正则 | Match complete raw response. 匹配完整响应原文。 | `-mr 'Set-Cookie: sid='` |
| `-fr` | Regex / 正则 | Filter complete raw response. 过滤完整响应原文。 | `-fr 'Not Found'` |
| `-mt` | Number/range or comparator / 数字、范围或比较符 | Match response time in ms. 匹配响应时间，单位 ms。 | `-mt '>100'` |
| `-ft` | Number/range or comparator / 数字、范围或比较符 | Filter response time in ms. 过滤响应时间，单位 ms。 | `-ft '<10'` |
| `-mmode` | `or`, `and` | Matcher set mode. matcher 组合模式。 | `-mmode and` |
| `-fmode` | `or`, `and` | Filter set mode. filter 组合模式。 | `-fmode or` |

Status specs support `all`, single values, comma lists, and ranges.

状态码支持 `all`、单值、逗号列表和范围。

Number specs support single values, comma lists, and ranges.

数字条件支持单值、逗号列表和范围。

Time specs support `>N`, `<N`, single values, comma lists, and ranges.

时间条件支持 `>N`、`<N`、单值、逗号列表和范围。

### Output / 输出参数

| Option / 参数 | Value / 取值 | Description / 说明 | Example / 示例 |
| --- | --- | --- | --- |
| `-o` | File path / 文件路径 | Output file. 输出文件。 | `-o result.jsonl` |
| `-of` | `console`, `jsonl`, `csv` | Output format. 输出格式。 | `-of jsonl` |
| `-s` | Flag / 开关 | Silent mode, output URLs only. 静默模式，只输出 URL。 | `-s` |
| `-od` | Directory / 目录 | Save matched raw request/response. 保存命中请求和响应原文。 | `-od raw-results` |
| `-error-log` | File path / 文件路径 | Save failed request payloads as JSONL. 保存失败请求 payload 组合日志。 | `-error-log errors.jsonl` |

`-of md` is not implemented in v0.1. Use `jsonl` or `csv` for machine-readable
results.

v0.1 暂不支持 `-of md`。需要结构化结果时建议使用 `jsonl` 或 `csv`。

### Stop Control / 停止控制参数

| Option / 参数 | Value / 取值 | Description / 说明 | Example / 示例 |
| --- | --- | --- | --- |
| `-stop-scope` | Keyword list / keyword 列表 | Group key for stop counting. 停止计数的分组 key。 | `-stop-scope URLFUZZ,UFUZZ` |
| `-stop-on-match` | Number / 数字 | Stop a scope after N matched results. 某个 scope 命中 N 次后停止。 | `-stop-on-match 1` |

Examples:

示例：

| Goal / 目标 | Parameters / 参数 |
| --- | --- |
| Stop a URL after one valid credential. 一个 URL 找到任意有效账号后停止。 | `-stop-scope URLFUZZ -stop-on-match 1` |
| Stop a URL+user after one valid password. 一个 URL+用户找到一个有效密码后停止。 | `-stop-scope URLFUZZ,UFUZZ -stop-on-match 1` |
| Stop a target after two matches. 一个目标命中两次后停止。 | `-stop-scope TARGET -stop-on-match 2` |

`-stop-scope` also works when `-order` makes a scope non-contiguous. For example,
`-order UFUZZ,PFUZZ,URLFUZZ -stop-scope URLFUZZ -stop-on-match 1` skips future
combinations for a URL after that URL has one match, even though URL requests are
rotated across batches.

当 `-order` 让某个 scope 不再连续时，`-stop-scope` 仍然生效。例如
`-order UFUZZ,PFUZZ,URLFUZZ -stop-scope URLFUZZ -stop-on-match 1` 会在某个 URL
命中一次后，跳过该 URL 后续组合，即使 URL 请求是轮转分散的。

## Output Records / 输出记录

JSONL record example / JSONL 输出示例：

```json
{"url":"https://example.com/admin","status":200,"size":1234,"words":100,"lines":20,"time_ms":42,"input":"admin","input_values":{"DIR":"admin"},"location":null,"title":"Admin","body_hash":123}
```

## Implemented In v0.1 / v0.1 已实现功能

- CLI: `-u`, `-w`, `-X`, `-H`, `-d`, `-b`, `-x`, `-t`, `-rate`,
  `-timeout`, `-mode`, matcher/filter options, `-o`, `-of`,
  `-budget-requests`
- ffuf-compatible options: `-r`, `-raw`, `-sni`, `-http2`, `-cc`, `-ck`,
  `-request`, `-request-proto`, `-replay-proxy`, `-e`, `-ic`, `-enc`,
  `-mt`, `-ft`, `-mmode`, `-fmode`, `-p`, `-s`, `-od`
- Templates: native `${{KEYWORD}}$`, bare keyword compatibility, startup keyword
  validation
- Input modes: `pitchfork`, `clusterbomb`; custom `clusterbomb` order;
  `rotate-window` target scheduling; `sniper` currently falls back to pitchfork
  behavior
- HTTP: tokio + reqwest, method, headers, cookies, body, proxy, replay proxy,
  redirects, timeout, disabled SSL/TLS certificate verification by default,
  keep-alive, DNS cache, concurrency, ulimit-aware startup concurrency cap,
  delay, and global rate limiting
- Precheck: target payload connectivity check with failed payload skipping
- Response summary: status, size, words, lines, elapsed time, location, title,
  body hash
- Matching/filtering: status, size, words, lines, response time, regex over full
  raw response; `and` / `or`; filters run first
- Output: console, silent URL, JSONL, CSV, raw request/response saving, request
  error payload logging
- Stop control: `-stop-scope` + `-stop-on-match`

## Roadmap / 后续路线图

- auto-calibration: `-ac`, `-ac-scope host|job|global`, `-ac-ignore KEYWORD`
- dynamic auto-filter
- `filter-expr` / `match-expr`
- recursion queue
- `save-state` / `resume`
- concurrent scoped stop execution
