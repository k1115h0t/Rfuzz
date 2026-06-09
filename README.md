[中文文档](README_CN.md)

# Rfuzz

<div align="center">

**A command-line web fuzzer written in Rust for authorized security testing, asset self-auditing, and lab research.**

A conservative Rust web fuzzer for authorized testing, with familiar ffuf-style workflows.

[![Rust](https://img.shields.io/badge/Rust-2021-orange.svg)](https://www.rust-lang.org/)
[![Version](https://img.shields.io/badge/version-0.1.6-blue.svg)](Cargo.toml)
[![License](https://img.shields.io/badge/license-AGPL--3.0-green.svg)](LICENSE)

</div>

---

## What Is This?

`rfuzz` is a lightweight, scriptable web fuzzing tool. It keeps familiar `ffuf`-style workflows while adding target prechecks, target rotation, scope-level stop controls, DNS caching, error logs, and structured output for large multi-target jobs.

It is useful for:

- Directory, parameter, endpoint, form, and weak-credential validation within an authorized scope.
- Internal asset self-audits, continuous security testing, and regression checks.
- CTFs, labs, test environments, and security research.
- Turning Burp Suite raw requests into repeatable fuzzing jobs.

> **Authorization Notice**
> Use `rfuzz` only against targets you are explicitly authorized to test. Scanning and fuzzing can stress services. Follow local laws, organization rules, and the exact scope of your authorization.

---

## Core Features

| Capability | Description |
| --- | --- |
| ffuf-style CLI | Supports common options such as `-u`, `-w`, `-H`, `-X`, `-d`, `-mc`, `-fc`, and `-mr`. |
| Multi-wordlist modes | Supports `clusterbomb` and `pitchfork`, with `sniper` reserved. |
| Lazy generation | Large Cartesian products are streamed instead of materialized up front. |
| Target precheck | `-precheck-key` probes target reachability before the full run; failed payloads are skipped by default. |
| Target rotation | `-schedule rotate-window` is designed for many URLs, users, and passwords without hammering one target continuously. |
| Scope-level stop | `-stop-scope` plus `-stop-on-match` can skip remaining requests for a URL, user, or grouped combination after a hit. |
| HTTP tuning | Supports proxies, redirects, HTTP/2, keep-alive, DNS cache, timeouts, delays, and global rate limits. |
| Structured output | Supports console output, silent URL output, JSONL, CSV, raw request/response capture, and JSONL error logs. |

---

## Installation

### Build From Source

```bash
git clone https://github.com/k1115h0t/Rfuzz.git
cd Rfuzz
cargo build --release
```

The release binary is written to:

```text
target/release/rfuzz
```

For development, you can run it directly:

```bash
cargo run -- -h
```

Or install it into your local Cargo bin directory:

```bash
cargo install --path .
```

---

## Quick Start

### 1. Directory Fuzzing

```bash
rfuzz -w dirs.txt:DIR \
  -u 'https://example.com/${{DIR}}$' \
  -fc 404
```

You can also use the bare keyword syntax, which is closer to `ffuf`:

```bash
rfuzz -w dirs.txt:FUZZ -u https://example.com/FUZZ -fc 404
```

### 2. POST Form Fuzzing

```bash
rfuzz -w passwords.txt:PASS \
  -u https://example.com/login \
  -X POST \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -d 'username=admin&password=${{PASS}}$' \
  -fc 401
```

### 3. Multi-Wordlist Combinations

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

The default combination mode is `clusterbomb`, meaning all wordlists are combined as a Cartesian product.

### 4. Burp Raw Request Files

Save a request copied from Burp Suite as `login.txt`:

```http
POST /login HTTP/1.1
Host: example.com
Content-Type: application/x-www-form-urlencoded

username=admin&password=${{PASS}}$
```

Then run:

```bash
rfuzz -request login.txt \
  -request-proto https \
  -w passwords.txt:PASS \
  -fc 401
```

`-request-proto` only applies to raw request files. It does not add a scheme to `-u` templates.

---

## Key Concepts

### Placeholders

`rfuzz` supports two placeholder styles:

| Syntax | Example | Notes |
| --- | --- | --- |
| Native explicit | `${{FUZZ}}$`, `${{PASS}}$` | Precise and less likely to replace normal text accidentally. |
| Bare keyword | `FUZZ`, `PASS`, `URLFUZZ` | Closer to `ffuf` and convenient in command lines. |

When using `${{KEYWORD}}$` in Bash, zsh, or PowerShell, quote the template with single quotes so the shell does not interpret `$` first:

```bash
rfuzz -w dirs.txt:DIR -u 'https://example.com/${{DIR}}$'
```

### Wordlist Modes

| Mode | Behavior | Good For |
| --- | --- | --- |
| `clusterbomb` | Cartesian product of all wordlists. | Users x passwords, paths x extensions, multi-parameter combinations. |
| `pitchfork` | Reads multiple wordlists by synchronized row index. | One-to-one username/password or parameter/value pairs. |
| `sniper` | Reserved in v0.1 and currently falls back to pitchfork behavior. | Future compatibility. |

Control `clusterbomb` generation order:

```bash
rfuzz -u https://example.com/login \
  -w users.txt:USER \
  -w passwords.txt:PASS \
  -w hosts.txt:HOST \
  -order USER,PASS,HOST
```

`-order` must include every keyword. The last keyword changes fastest.

---

## Large Job Patterns

### Target Precheck

When a wordlist represents target URLs, domains, or `host:port` values, enable target precheck first:

```bash
rfuzz -u https://TARGET/login \
  -w targets.txt:TARGET \
  -w users.txt:USER \
  -w passwords.txt:PASS \
  -d 'username=${{USER}}$&password=${{PASS}}$' \
  -precheck-key TARGET
```

Precheck only iterates the payloads for `-precheck-key`; it does not combine other wordlists. Any received HTTP response counts as reachable, including `200`, `301`, `401`, `403`, `404`, and `500`.

Disable precheck:

```bash
rfuzz -u https://TARGET/login -w targets.txt:TARGET -precheck off
```

Report precheck failures without skipping payloads:

```bash
rfuzz -u https://TARGET/login \
  -w targets.txt:TARGET \
  -precheck-key TARGET \
  -precheck-report-only
```

Precheck scans all targets by rounds. With the default 3 attempts, three targets are retried as `1,2,3,1,2,3,1,2,3`, not as `1,1,1,2,2,2,3,3,3`. You can set the round count:

```bash
rfuzz -u https://TARGET/login \
  -w targets.txt:TARGET \
  -precheck-key TARGET \
  -precheck-attempts 5
```

### Target Rotation

For many targets, use `rotate-window` to rotate through targets and avoid sending a long continuous burst to one target:

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

Option meanings:

| Option | Description |
| --- | --- |
| `-schedule rotate-window` | Enables target window rotation. |
| `-target-key TARGET` | Selects the keyword that represents the target. |
| `-target-window 100` | Keeps this many targets in each rotation batch. |
| `-target-burst 3` | Sends this many consecutive combinations per target before switching. |

### Stop After Match

Skip remaining combinations for a target after one hit:

```bash
rfuzz -u https://TARGET/login \
  -w targets.txt:TARGET \
  -w users.txt:USER \
  -w passwords.txt:PASS \
  -mr 'Set-Cookie: session_id=' \
  -stop-scope TARGET \
  -stop-on-match 1
```

Skip remaining passwords for the current target and user:

```bash
rfuzz -u https://TARGET/login \
  -w targets.txt:TARGET \
  -w users.txt:USER \
  -w passwords.txt:PASS \
  -mr 'Set-Cookie: session_id=' \
  -stop-scope TARGET,USER \
  -stop-on-match 1
```

`-stop-scope` can be combined with `-order`, `rotate-window`, and `precheck`. Once the threshold is reached, `rfuzz` fast-forwards through the remaining cases for that scope.

---

## Output And Progress

By default, `rfuzz` writes its progress bar to `stderr`, so it does not pollute `stdout` or files written with `-o`.

Progress fields:

```text
done/total | percent | matched | errors | skipped | err | ETA
```

`ETA` is estimated from the recent real case completion rate. It includes practical runtime effects such as rate limiting, delays, skipped cases, matching, and output handling.

Disable progress:

```bash
rfuzz -w dirs.txt:DIR -u 'https://example.com/${{DIR}}$' -no-progress
```

Example JSONL record:

```json
{"url":"https://example.com/admin","status":200,"size":1234,"words":100,"lines":20,"time_ms":42,"input":"admin","input_values":{"DIR":"admin"},"location":null,"title":"Admin","body_hash":123}
```

Save matching results:

```bash
rfuzz -w dirs.txt:DIR \
  -u 'https://example.com/${{DIR}}$' \
  -mc 200,204,301-302 \
  -o result.jsonl \
  -of jsonl
```

Save request error logs for later review:

```bash
rfuzz -u https://TARGET/login \
  -w targets.txt:TARGET \
  -error-log request_errors.jsonl
```

---

## Performance And Stability

### Concurrency And File Descriptors

On Unix/Linux, `rfuzz` checks `ulimit -n` at startup and estimates whether the file descriptor limit can support the requested `-t` concurrency. If the limit is too low, it lowers the effective concurrency and prints a notice.

Before large or highly concurrent jobs, consider:

```bash
ulimit -n 8192
```

If the system does not allow a higher limit, lower `-t` or adjust the system/service hard limit.

### Keep-Alive And DNS Cache

Default settings:

| Option | Default | Description |
| --- | --- | --- |
| `-keepalive` | `on` | Reuses HTTP connections and is usually faster for HTTPS. |
| `-dns-cache` | `on` | Enables the in-process DNS cache. |
| `-dns-cache-ttl` | `300` | Caches successful DNS resolutions for 300 seconds. |
| `-dns-negative-cache-ttl` | `30` | Caches DNS failures for 30 seconds. |
| `-dns-max-concurrent` | `64` | Caps concurrent real DNS lookups. |

In low-file-descriptor environments, you can disable keep-alive:

```bash
rfuzz -u https://TARGET/login -w targets.txt:TARGET -keepalive off
```

This saves file descriptors, but HTTPS throughput can drop because TCP/TLS connections are created more often.

---

## Option Reference

### Request Options

| Option | Description | Example |
| --- | --- | --- |
| `-u` | URL template. | `-u https://HOST/FUZZ` |
| `-request` | Burp raw request file. | `-request login.txt` |
| `-request-proto` | Protocol for raw request files. | `-request-proto https` |
| `-X` | HTTP method. | `-X POST` |
| `-H` | Header template, repeatable. | `-H "Content-Type: application/json"` |
| `-d` | Request body template. | `-d 'q=${{FUZZ}}$'` |
| `-b` | Cookie template, repeatable. | `-b 'sid=${{SID}}$'` |

### Input And Scheduling

| Option | Description | Example |
| --- | --- | --- |
| `-w` | Wordlist file and optional keyword, repeatable. | `-w users.txt:USER` |
| `-mode` | Multi-wordlist mode. | `-mode clusterbomb` |
| `-order` | Clusterbomb generation order. | `-order USER,PASS,TARGET` |
| `-schedule` | Request generation schedule. | `-schedule rotate-window` |
| `-target-key` | Rotation target keyword. | `-target-key TARGET` |
| `-target-window` | Targets per rotation window. | `-target-window 100` |
| `-target-burst` | Consecutive requests per target. | `-target-burst 3` |
| `-e` | Append extensions to wordlist entries. | `-e .php,.bak` |
| `-ic` | Ignore comment lines starting with `#`. | `-ic` |
| `-enc` | Apply an encoder chain to a keyword. | `-enc 'DIR:urlencode'` |
| `-budget-requests` | Maximum request budget. | `-budget-requests 1000` |

Supported encoders: `urlencode`, `b64encode` / `base64`, `hex`, `lower`, `upper`, `md5`, `sha1`.

### Execution And HTTP

| Option | Description | Example |
| --- | --- | --- |
| `-t` | Concurrent worker count. | `-t 100` |
| `-rate` | Global requests per second; `0` disables the limit. | `-rate 50` |
| `-timeout` | Request timeout in seconds. | `-timeout 10` |
| `-p` | Fixed or random request delay range. | `-p 0.1-0.5` |
| `-no-progress` | Disables the progress bar. | `-no-progress` |
| `-precheck` | Enables or disables precheck. | `-precheck off` |
| `-precheck-key` | Target precheck keyword. | `-precheck-key TARGET` |
| `-precheck-report-only` | Reports failures without skipping failed payloads. | `-precheck-report-only` |
| `-precheck-attempts` | Number of target precheck rounds. | `-precheck-attempts 5` |
| `-r` | Follows redirects. | `-r` |
| `-raw` | Disables URI space encoding. | `-raw` |
| `-x` | Request proxy. | `-x http://127.0.0.1:8080` |
| `-replay-proxy` | Replays matches through another proxy. | `-replay-proxy http://127.0.0.1:8081` |
| `-http2` | Forces HTTP/2 prior knowledge. | `-http2` |
| `-ssl-verify` | Toggles TLS certificate verification; default is off. | `-ssl-verify on` |
| `-keepalive` | Toggles HTTP keep-alive. | `-keepalive off` |
| `-dns-cache` | Toggles DNS cache. | `-dns-cache on` |
| `-sni` | Compatibility option; arbitrary SNI override is not currently supported. | `-sni example.com` |
| `-cc` / `-ck` | Client certificate and private key. | `-cc client.crt -ck client.key` |

### Matching And Filtering

Filters take precedence over matchers. If a response matches a filter condition, it is not output.

| Option | Description | Example |
| --- | --- | --- |
| `-mc` / `-fc` | Match/filter status codes. | `-mc 200,204,300-399` |
| `-ms` / `-fs` | Match/filter response size. | `-fs 0` |
| `-mw` / `-fw` | Match/filter word count. | `-mw 10-30` |
| `-ml` / `-fl` | Match/filter line count. | `-ml 5-20` |
| `-mr` / `-fr` | Match/filter full raw response regex. | `-mr 'Set-Cookie: session_id='` |
| `-mt` / `-ft` | Match/filter response time in ms. | `-mt '>100'` |
| `-mmode` / `-fmode` | Matcher/filter set mode. | `-mmode and` |

Status codes support `all`, single values, comma-separated lists, and ranges. Time conditions support `>N`, `<N`, single values, comma-separated lists, and ranges.

### Output And Stop Controls

| Option | Description | Example |
| --- | --- | --- |
| `-o` | Output file. | `-o result.jsonl` |
| `-of` | Output format: `console`, `jsonl`, or `csv`. | `-of jsonl` |
| `-s` | Silent mode, output URLs only. | `-s` |
| `-od` | Directory for matched raw request/response captures. | `-od raw-results` |
| `-error-log` | Saves failed request payload logs. | `-error-log errors.jsonl` |
| `-stop-scope` | Grouping key for stop counters. | `-stop-scope TARGET,USER` |
| `-stop-on-match` | Stops a scope after N matches. | `-stop-on-match 1` |

---

## Implementation Status

- CLI: request, input, execution, matcher/filter, output, and request budget options.
- Templates: native `${{KEYWORD}}$` placeholders and bare keyword compatibility.
- Input modes: `clusterbomb`, `pitchfork`; `sniper` currently falls back to pitchfork.
- Scheduling: custom `-order` and `rotate-window` target rotation.
- HTTP: tokio + reqwest with proxy, replay proxy, redirects, timeout, keep-alive, DNS cache, and global rate limiting.
- Precheck: target payload reachability checks with round-based retries and report-only mode; failed payloads are skipped by default.
- Matching/filtering: status, size, words, lines, response time, full raw response regex, and/or modes.
- Output: console, silent URL, JSONL, CSV, raw request/response capture, and JSONL error logs.
- Stop control: `-stop-scope` plus `-stop-on-match`.

---

## Roadmap

- `-ac` auto-calibration.
- Dynamic auto-filter.
- `filter-expr` / `match-expr`.
- Recursion queue.
- `save-state` / `resume`.
- More complete concurrent scoped stop execution.

---

## License

This project is licensed under the [GNU Affero General Public License v3.0](LICENSE).
