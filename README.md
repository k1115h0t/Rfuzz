[中文文档](README_CN.md)

# Rfuzz

<div align="center">

**A command-line web fuzzer written in Rust for authorized security testing, asset self-auditing, and lab research.**

A conservative Rust web fuzzer for authorized testing, with familiar ffuf-style workflows.

[![Rust](https://img.shields.io/badge/Rust-2021-orange.svg)](https://www.rust-lang.org/)
[![Version](https://img.shields.io/badge/version-0.2.0-blue.svg)](Cargo.toml)
[![License](https://img.shields.io/badge/license-AGPL--3.0-green.svg)](LICENSE)

</div>

---

## What Is This?

`rfuzz` is a lightweight, scriptable web fuzzing tool. It keeps familiar `ffuf`-style workflows while adding safe previews, target prechecks, target rotation, scoped stopping, DNS caching, response body limits, end-of-run summaries, error logs, and structured output for large multi-target jobs.

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
| Lazy raw requests | Raw HTTP request strings are built only for dry-run, request-dry-run, and matched raw captures instead of every request. |
| Header-only matching | `-mhr` / `-fhr` match or filter response headers without reading response bodies. |
| Target precheck | `-precheck-key` probes target reachability before the full run with a short timeout and HEAD-first fallback; failed payloads are skipped by default. |
| Target rotation | `-schedule rotate-window` is designed for many URLs, users, and passwords without hammering one target continuously. |
| Scoped stopping | After a hit, skip remaining combinations by grouping keys such as `TARGET`, `USER`, or `TARGET,USER`. See “scope: the grouping key for stop counters”. |
| Safe preview | `-dry-run` / `-explain` / `-request-dry-run` inspect the plan and final request without sending network traffic. |
| HTTP tuning | Supports proxies, redirects, HTTP/2, keep-alive, DNS cache, timeouts, delays, and global rate limits. |
| Structured output | Supports readable `[MATCH]` console lines, silent URL output, JSONL, CSV, raw request/response capture, JSONL error logs, and JSON run summaries. |

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
  -mhr 'Set-Cookie: session_id='
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

`-request-proto` only applies to raw request files. It does not add a scheme to `-u` templates, and it only accepts `http` or `https`.

Raw request rendering validates `Host` for relative request lines and recalculates `Content-Length` after placeholders are rendered. Starting in v0.1.9, Burp raw request parsing preserves CRLF line endings inside the body; `-request` is still intended for text raw requests and does not guarantee binary body preservation. To inspect the final request without sending it:

```bash
rfuzz -request login.txt \
  -request-proto https \
  -w passwords.txt:PASS \
  -request-dry-run
```

---

## Commonly Confused Concepts

### Safe preview: dry-run / explain / request-dry-run

Think of dry-run as “preview only, do not execute.”

With `-dry-run`, `rfuzz` loads wordlists, parses templates, estimates request counts, applies the combination mode, scheduler, concurrency, rate limit, precheck settings, and output settings, then renders the first final request for inspection. It does not send any HTTP requests.

This is useful before a large job because it answers three practical questions:

1. Are placeholders being replaced correctly?
2. Is the request count what you expected?
3. Will the final request hit the intended target?

| Option | Use When | What It Does |
| --- | --- | --- |
| `-dry-run` | Normal URL-template jobs | Prints the full execution plan and first rendered request. |
| `-explain` | You want to confirm job configuration | Similar to `-dry-run`, with emphasis on explaining the plan. |
| `-request-dry-run` | You use a Burp raw request file | Renders and prints the first final raw request without sending it. |

### request-dry-run: render a raw request, send nothing

`-request-dry-run` is the safe preview mode for raw request files. It reads the Burp-exported request, replaces placeholders, applies the target protocol, recalculates `Content-Length`, and prints the first final raw request. After printing, the job exits without network traffic.

### scope: the grouping key for stop counters

In `rfuzz`, scope means a “grouping key” built from one or more wordlist keywords.

`-stop-scope TARGET` means each `TARGET` is counted independently. Once a target reaches the match threshold, the remaining combinations for that target are skipped.

`-stop-scope TARGET,USER` means each `TARGET + USER` pair is counted independently. If one user hits on one target, only that user’s remaining passwords on that target are skipped; other users on the same target are not affected.

Keywords in `-stop-scope` must come from wordlists declared with `-w file:KEYWORD`. At runtime, `rfuzz` takes those keyword values from the current case and builds a key. Once the key reaches `-stop-on-match`, later cases with the same key are skipped.

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

Precheck only iterates the payloads for `-precheck-key`; it does not combine other wordlists. Precheck URLs apply the same `-enc` encoder chain and URL space normalization as normal requests. Precheck uses its own timeout, `-precheck-timeout 3` by default, and probes with `HEAD` first before falling back to `GET` when `HEAD` is not allowed or fails. Any received HTTP response counts as reachable, including `200`, `301`, `401`, `403`, `404`, and `500`.

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
  -precheck-attempts 5 \
  -precheck-timeout 2
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
  -mhr 'Set-Cookie: session_id=' \
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

Assume the wordlists are:

```text
TARGET = [a.com, b.com]
USER   = [alice, bob]
PASS   = [123456, admin, qwerty]
```

#### Example A: `-stop-scope TARGET -stop-on-match 1`

Meaning: each target stops independently after 1 match, skipping all remaining `USER/PASS` combinations for that target.

If `a.com + alice + admin` matches, then:

- `a.com + alice + qwerty` is skipped.
- `a.com + bob + 123456/admin/qwerty` is also skipped.
- `b.com` is unaffected and continues.

Command:

```bash
rfuzz -u https://TARGET/login \
  -w targets.txt:TARGET \
  -w users.txt:USER \
  -w passwords.txt:PASS \
  -mhr 'Set-Cookie: session_id=' \
  -stop-scope TARGET \
  -stop-on-match 1
```

#### Example B: `-stop-scope TARGET,USER -stop-on-match 1`

Meaning: each “target + user” pair stops independently after 1 match, skipping the remaining passwords for that user on that target.

If `a.com + alice + admin` matches, then:

- `a.com + alice + qwerty` is skipped.
- `a.com + bob + ...` continues.
- `b.com + alice + ...` also continues.

Command:

```bash
rfuzz -u https://TARGET/login \
  -w targets.txt:TARGET \
  -w users.txt:USER \
  -w passwords.txt:PASS \
  -mhr 'Set-Cookie: session_id=' \
  -stop-scope TARGET,USER \
  -stop-on-match 1
```

`-stop-scope` can be combined with `-order`, `rotate-window`, and `precheck`. Once the threshold is reached, `rfuzz` fast-forwards through the remaining cases for the same grouping key.

Note: `-stop-on-match` does not cancel requests that are already in flight. With high concurrency or `rotate-window` scheduling, once a grouping key reaches the threshold, `rfuzz` stops scheduling new requests for that key, but already-started requests for the same key may still finish.

### Safe Preview: dry-run / explain / request-dry-run

Before a large scan, use safe preview mode to verify the request template, placeholders, wordlist sizes, estimated request count, concurrency, rate limit, precheck mode, output paths, response body limits, and the first rendered request:

```bash
rfuzz -w dirs.txt:DIR \
  -u 'https://example.com/${{DIR}}$' \
  -fc 404 \
  -dry-run
```

`-explain` is an alias-style planning view for the same no-network safety check. With Burp raw request files, use `-request-dry-run` to inspect the first final raw request.

---

## Output And Progress

By default, `rfuzz` writes its progress bar to `stderr`, so it does not pollute `stdout` or files written with `-o`.

Progress fields:

```text
done/total | percent | matched | errors | skipped | err | ETA
```

`ETA` is estimated from the recent real case completion rate. It includes practical runtime effects such as rate limiting, delays, skipped cases, matching, and output handling.

When a response matches, console output shows the matched combination, final rendered URL, and response summary:

```text
[MATCH] PASS=admin,URLFUZZ=https://example.com,USER=alice -> https://example.com/login [Status: 200, Size: 12, Words: 2, Lines: 1, Time: 35ms]
```

If `-o` writes JSONL, CSV, or console output to a file, `rfuzz` still mirrors a concise `[MATCH]` line to `stderr` so matches are visible during the run. Silent mode (`-s`) still prints URLs only.

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

At the end of a run, `rfuzz` prints a summary to `stderr` with total, matched, filtered, error, and skipped counts, top error categories, top response signatures, output paths, and whether `-stop-on-match` actually triggered. Save the same summary as JSON:

```bash
rfuzz -w dirs.txt:DIR \
  -u 'https://example.com/${{DIR}}$' \
  -summary-json summary.json
```

---

## Performance And Stability

### Concurrency And File Descriptors

On Unix/Linux, `rfuzz` checks `ulimit -n` at startup and estimates whether the current options can fit within the file descriptor limit. The estimate includes worker concurrency, DNS concurrency, output files, and the keep-alive connection pool that can build up during multi-target precheck/rotation. If the current value is too low, `rfuzz` prints the current value, the estimated required value, and the exact `ulimit` command to run before starting the job.

Before large or highly concurrent jobs, use the value printed by the startup warning:

```bash
ulimit -n <estimated-required-value>
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

### Response Body Limits

`rfuzz` no longer reads response bodies without a cap. By default each response body is limited to 2 MiB, and raw response output shows a 4096-byte preview. Tune this for directory fuzzing, binary endpoints, or huge downloads:

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

## Option Reference

### Request Options

| Option | Description | Example |
| --- | --- | --- |
| `-u` | URL template. | `-u https://HOST/FUZZ` |
| `-request` | Burp raw request file. | `-request login.txt` |
| `-request-proto` | Protocol for raw request files; only `http` / `https` are accepted. | `-request-proto https` |
| `-request-dry-run` | Safe preview for raw requests: renders and prints the first final request without sending it. | `-request-dry-run` |
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
| `-target-window` | Targets per rotation window; default: `100`. | `-target-window 100` |
| `-target-burst` | Consecutive requests per target; default: `3`. | `-target-burst 3` |
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
| `-p` | Fixed or random finite request delay range; rejects NaN/inf. | `-p 0.1-0.5` |
| `-dry-run` | Safe preview: prints the plan and first rendered request without sending requests. | `-dry-run` |
| `-explain` | Explains the safe-preview plan without sending requests. | `-explain` |
| `-no-progress` | Disables the progress bar. | `-no-progress` |
| `-precheck` | Enables or disables precheck. | `-precheck off` |
| `-precheck-key` | Target precheck keyword. | `-precheck-key TARGET` |
| `-precheck-report-only` | Reports failures without skipping failed payloads. | `-precheck-report-only` |
| `-precheck-attempts` | Number of target precheck rounds. | `-precheck-attempts 5` |
| `-precheck-timeout` | Per-request precheck timeout in seconds; default: `3`. | `-precheck-timeout 2` |
| `-r` | Follows redirects. | `-r` |
| `-raw` | Disables URI space encoding. | `-raw` |
| `-x` | Request proxy. | `-x http://127.0.0.1:8080` |
| `-replay-proxy` | Replays matches through another proxy. | `-replay-proxy http://127.0.0.1:8081` |
| `-http2` | Forces HTTP/2 prior knowledge. | `-http2` |
| `-ssl-verify` | Toggles TLS certificate verification; default is off. | `-ssl-verify on` |
| `-keepalive` | Toggles HTTP keep-alive. | `-keepalive off` |
| `-dns-cache` | Toggles DNS cache. | `-dns-cache on` |
| `-dns-cache-ttl` | Successful DNS cache TTL in seconds. | `-dns-cache-ttl 300` |
| `-dns-negative-cache-ttl` | Failed DNS cache TTL in seconds; `0` disables negative caching. | `-dns-negative-cache-ttl 30` |
| `-dns-max-concurrent` | Maximum concurrent real DNS lookups. | `-dns-max-concurrent 64` |
| `-sni` | Compatibility option; arbitrary SNI override is not currently supported. | `-sni example.com` |
| `-cc` / `-ck` | Client certificate and private key; must be provided together. | `-cc client.crt -ck client.key` |
| `-ignore-body` | Does not read response bodies; status and headers are still available. | `-ignore-body` |
| `-max-body` | Maximum response body bytes read per response; default: `2097152`. | `-max-body 1048576` |
| `-body-preview` | Maximum body bytes shown in raw response output; default: `4096`. | `-body-preview 2048` |
| `-ac` | Reserved auto-calibration switch. | `-ac` |
| `-ac-scope` | Reserved auto-calibration scope: `host`, `job`, or `global`. | `-ac-scope job` |
| `-ac-ignore` | Reserved auto-calibration ignored keyword, repeatable. | `-ac-ignore URLFUZZ` |

### Matching And Filtering

Filters take precedence over matchers. If a response matches a filter condition, it is not output.

| Option | Description | Example |
| --- | --- | --- |
| `-mc` / `-fc` | Match/filter status codes. | `-mc 200,204,300-399` |
| `-ms` / `-fs` | Match/filter response size. | `-fs 0` |
| `-mw` / `-fw` | Match/filter word count. | `-mw 10-30` |
| `-ml` / `-fl` | Match/filter line count. | `-ml 5-20` |
| `-mhr` / `-fhr` | Match/filter response header regex without reading the body. | `-mhr 'Set-Cookie: session_id='` |
| `-mr` / `-fr` | Match/filter full raw response regex; reads the response body. | `-mr 'welcome'` |
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
| `-summary-json` | Saves the end-of-run summary as JSON. | `-summary-json summary.json` |
| `-stop-scope` | Grouping key for stop counters. | `-stop-scope TARGET,USER` |
| `-stop-on-match` | Stops scheduling new requests for a scope after N matches; does not cancel in-flight requests. | `-stop-on-match 1` |

---

## Implementation Status

- CLI: request, input, execution, matcher/filter, header-only matcher, output, dry-run, body-limit, summary, and request budget options.
- Templates: native `${{KEYWORD}}$` placeholders and bare keyword compatibility, with startup validation for unknown placeholders and unused wordlist keywords.
- Input modes: `clusterbomb`, `pitchfork`; `sniper` currently falls back to pitchfork.
- Scheduling: custom `-order` and `rotate-window` target rotation.
- HTTP: tokio + reqwest with proxy, replay proxy, redirects, timeout, keep-alive, DNS cache, global rate limiting, lazy raw request construction, raw request `Content-Length` recalculation, body CRLF preservation, header-only response matching, precheck timeout, HEAD-first precheck, and bounded response body reads.
- Precheck: target payload reachability checks with round-based retries and report-only mode; failed payloads are skipped by default.
- Matching/filtering: status, size, words, lines, response time, full raw response regex, and/or modes.
- Output: readable match lines, silent URL, JSONL, CSV, raw request/response capture, JSONL error logs, and end-of-run summaries.
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
