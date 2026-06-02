use clap::{Parser, ValueEnum};

#[derive(Debug, Clone, Parser)]
#[command(
    name = "rfuzz",
    version,
    about = "Rust Web Fuzzer，仅用于授权测试 / Rust web fuzzer for authorized testing",
    after_help = "示例 / Examples:
  基础目录扫描 / Basic directory fuzzing:
    rfuzz -w dirs.txt:DIR -u 'https://example.com/${{DIR}}$' -fc 404

  Burp 请求文件 / Burp raw request file:
    rfuzz -request login.txt -request-proto https -w passwords.txt:PASS -fc 401

  按 scope 命中后停止 / Stop after matches per scope:
    rfuzz -w urls.txt:TARGET -w values.txt:VALUE -u '${{TARGET}}$/?q=${{VALUE}}$' -stop-scope TARGET -stop-on-match 2

免责声明 / Disclaimer:
  仅在明确授权的目标上使用。/ Use only against targets you are authorized to test."
)]
pub struct Cli {
    #[arg(short = 'u', value_name = "URL", help = "URL 模板 / URL template")]
    pub url: Option<String>,

    #[arg(
        short = 'w',
        value_name = "WORDLIST[:KEYWORD]",
        help = "字典文件和可选 keyword，可重复 / Wordlist with optional keyword, repeatable"
    )]
    pub wordlists: Vec<String>,

    #[arg(short = 'X', default_value = "GET", help = "HTTP 方法 / HTTP method")]
    pub method: String,

    #[arg(
        short = 'H',
        value_name = "HEADER",
        help = "HTTP Header，可重复 / HTTP header, repeatable"
    )]
    pub headers: Vec<String>,

    #[arg(
        short = 'd',
        value_name = "BODY",
        help = "请求体模板 / Request body template"
    )]
    pub data: Option<String>,

    #[arg(
        short = 'b',
        value_name = "COOKIE",
        help = "Cookie，可重复 / Cookie, repeatable"
    )]
    pub cookies: Vec<String>,

    #[arg(
        short = 'x',
        value_name = "PROXY",
        help = "发送请求使用的代理 / Proxy used for requests"
    )]
    pub proxy: Option<String>,

    #[arg(
        short = 'r',
        default_value_t = false,
        help = "跟随重定向 / Follow redirects"
    )]
    pub follow_redirects: bool,

    #[arg(
        long = "raw",
        default_value_t = false,
        help = "禁止 URI 编码 / Disable URI encoding"
    )]
    pub raw_uri: bool,

    #[arg(
        long = "sni",
        help = "指定 TLS SNI（当前为兼容预留）/ Set TLS SNI (compatibility placeholder)"
    )]
    pub sni: Option<String>,

    #[arg(
        long = "http2",
        default_value_t = false,
        help = "显式使用 HTTP/2 / Force HTTP/2"
    )]
    pub http2: bool,

    #[arg(
        long = "ssl-verify",
        default_value = "off",
        help = "SSL/TLS 证书校验开关：on/off，默认 off / SSL/TLS certificate verification: on/off, default off"
    )]
    pub ssl_verify: String,

    #[arg(
        long = "keepalive",
        default_value = "on",
        help = "HTTP keep-alive 连接复用：on/off，默认 on / HTTP keep-alive connection reuse: on/off, default on"
    )]
    pub keepalive: String,

    #[arg(
        long = "dns-cache",
        default_value = "on",
        help = "DNS 解析缓存：on/off，默认 on / DNS resolution cache: on/off, default on"
    )]
    pub dns_cache: String,

    #[arg(
        long = "dns-cache-ttl",
        default_value_t = 300,
        help = "DNS 解析缓存 TTL 秒数 / DNS cache TTL in seconds"
    )]
    pub dns_cache_ttl_secs: u64,

    #[arg(
        long = "dns-negative-cache-ttl",
        default_value_t = 30,
        help = "DNS 失败缓存 TTL 秒数，0 禁用 / DNS failure cache TTL in seconds, 0 disables"
    )]
    pub dns_negative_cache_ttl_secs: u64,

    #[arg(
        long = "dns-max-concurrent",
        default_value_t = 64,
        help = "最大并发 DNS 解析数 / Maximum concurrent DNS lookups"
    )]
    pub dns_max_concurrent: usize,

    #[arg(
        long = "cc",
        help = "客户端证书 PEM 路径 / Client certificate PEM path"
    )]
    pub client_cert: Option<String>,

    #[arg(
        long = "ck",
        help = "客户端私钥 PEM 路径 / Client private key PEM path"
    )]
    pub client_key: Option<String>,

    #[arg(
        long = "request",
        help = "Burp raw request txt 文件 / Burp raw request txt file"
    )]
    pub request: Option<String>,

    #[arg(
        long = "request-proto",
        default_value = "https",
        help = "raw request 使用的协议 / Protocol for raw request"
    )]
    pub request_proto: String,

    #[arg(
        long = "replay-proxy",
        help = "命中后 replay 到代理 / Replay matched requests through proxy"
    )]
    pub replay_proxy: Option<String>,

    #[arg(
        short = 't',
        default_value_t = 20,
        help = "并发数，Unix/Linux 会按 ulimit -n 自动下调 / Concurrent workers; Unix/Linux may cap by ulimit -n"
    )]
    pub concurrency: usize,

    #[arg(
        long = "rate",
        default_value_t = 0,
        help = "每秒请求限制，0 为不限 / Requests per second limit, 0 disables"
    )]
    pub rate: u64,

    #[arg(
        long = "timeout",
        default_value_t = 10,
        help = "请求超时秒数 / Request timeout in seconds"
    )]
    pub timeout_secs: u64,

    #[arg(long = "mode", default_value_t = ModeArg::Clusterbomb, help = "多字典模式 / Multi-wordlist mode")]
    pub mode: ModeArg,

    #[arg(
        long = "order",
        help = "clusterbomb keyword 生成顺序，最后一个变化最快 / Clusterbomb keyword generation order, last changes fastest"
    )]
    pub order: Option<String>,

    #[arg(
        long = "schedule",
        default_value_t = ScheduleArg::Default,
        help = "请求生成调度：default/rotate-window / Request scheduling: default/rotate-window"
    )]
    pub schedule: ScheduleArg,

    #[arg(
        long = "target-key",
        help = "rotate-window 使用的 URL/目标 keyword / URL/target keyword used by rotate-window"
    )]
    pub target_key: Option<String>,

    #[arg(
        long = "target-window",
        default_value_t = 100,
        help = "rotate-window 同一批轮转目标数量 / Number of targets kept in each rotate-window batch"
    )]
    pub target_window: usize,

    #[arg(
        long = "target-burst",
        default_value_t = 3,
        help = "rotate-window 中每个目标连续请求数 / Consecutive requests per target in rotate-window"
    )]
    pub target_burst: usize,

    #[arg(short = 'e', help = "扩展名追加列表 / Extension list to append")]
    pub extensions: Option<String>,

    #[arg(
        long = "ic",
        default_value_t = false,
        help = "忽略字典注释行 / Ignore wordlist comment lines"
    )]
    pub ignore_wordlist_comments: bool,

    #[arg(long = "enc", help = "keyword 编码链 / Keyword encoder chain")]
    pub encoders: Vec<String>,

    #[arg(long = "mc", help = "匹配状态码 / Match status codes")]
    pub match_status: Option<String>,
    #[arg(long = "fc", help = "过滤状态码 / Filter status codes")]
    pub filter_status: Option<String>,
    #[arg(long = "ms", help = "匹配响应大小 / Match response size")]
    pub match_size: Option<String>,
    #[arg(long = "fs", help = "过滤响应大小 / Filter response size")]
    pub filter_size: Option<String>,
    #[arg(long = "mw", help = "匹配单词数 / Match word count")]
    pub match_words: Option<String>,
    #[arg(long = "fw", help = "过滤单词数 / Filter word count")]
    pub filter_words: Option<String>,
    #[arg(long = "ml", help = "匹配行数 / Match line count")]
    pub match_lines: Option<String>,
    #[arg(long = "fl", help = "过滤行数 / Filter line count")]
    pub filter_lines: Option<String>,
    #[arg(long = "mr", help = "匹配正则 / Match regex")]
    pub match_regex: Vec<String>,
    #[arg(long = "fr", help = "过滤正则 / Filter regex")]
    pub filter_regex: Vec<String>,
    #[arg(
        long = "mt",
        help = "匹配响应时间，例如 >100 / Match response time, e.g. >100"
    )]
    pub match_time: Option<String>,
    #[arg(
        long = "ft",
        help = "过滤响应时间，例如 <50 / Filter response time, e.g. <50"
    )]
    pub filter_time: Option<String>,
    #[arg(long = "mmode", default_value_t = SetModeArg::Or, help = "matcher 组合模式 / Matcher set mode")]
    pub matcher_mode: SetModeArg,
    #[arg(long = "fmode", default_value_t = SetModeArg::Or, help = "filter 组合模式 / Filter set mode")]
    pub filter_mode: SetModeArg,

    #[arg(short = 'o', help = "输出文件 / Output file")]
    pub output: Option<String>,

    #[arg(long = "of", default_value_t = OutputFormatArg::Console, help = "输出格式 / Output format")]
    pub output_format: OutputFormatArg,

    #[arg(
        long = "budget-requests",
        help = "最大请求预算 / Maximum request budget"
    )]
    pub budget_requests: Option<usize>,

    #[arg(
        short = 'p',
        help = "请求间延迟或随机范围 / Delay or random delay range between requests"
    )]
    pub delay: Option<String>,

    #[arg(
        short = 's',
        default_value_t = false,
        help = "静默模式，只输出 URL / Silent mode, output URLs only"
    )]
    pub silent: bool,

    #[arg(
        long = "od",
        help = "保存命中请求/响应原文目录 / Directory for matched raw request/response"
    )]
    pub output_directory: Option<String>,

    #[arg(
        long = "error-log",
        help = "保存请求失败 payload 日志 JSONL / Save failed request payload log as JSONL"
    )]
    pub error_log: Option<String>,

    #[arg(
        long = "precheck",
        default_value = "on",
        help = "预检查开关：on/off，默认 on / Precheck switch: on/off, default on"
    )]
    pub precheck: String,

    #[arg(
        long = "precheck-key",
        help = "预检查使用的 URL 目标 keyword / URL target keyword used by precheck"
    )]
    pub precheck_key: Option<String>,

    #[arg(
        long = "precheck-report-only",
        default_value_t = false,
        help = "只报告预检查错误，不跳过 payload / Only report precheck errors, do not skip payloads"
    )]
    pub precheck_report_only: bool,

    #[arg(
        long = "no-progress",
        default_value_t = false,
        help = "禁用进度条 / Disable progress bar"
    )]
    pub no_progress: bool,

    #[arg(
        long = "stop-on-match",
        help = "每个 scope 命中 N 次后停止 / Stop each scope after N matches"
    )]
    pub stop_on_match: Option<usize>,

    #[arg(long = "stop-scope", help = "停止作用域 keyword / Stop-scope keyword")]
    pub stop_scope: Vec<String>,

    #[arg(
        long = "ac",
        default_value_t = false,
        help = "自动校准预留 / Auto-calibration placeholder"
    )]
    pub auto_calibration: bool,
    #[arg(
        long = "ac-scope",
        default_value = "job",
        help = "自动校准作用域预留 / Auto-calibration scope placeholder"
    )]
    pub auto_calibration_scope: String,
    #[arg(
        long = "ac-ignore",
        help = "自动校准忽略 keyword 预留 / Auto-calibration ignored keyword placeholder"
    )]
    pub auto_calibration_ignore: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ModeArg {
    Sniper,
    Pitchfork,
    Clusterbomb,
}

impl std::fmt::Display for ModeArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sniper => write!(f, "sniper"),
            Self::Pitchfork => write!(f, "pitchfork"),
            Self::Clusterbomb => write!(f, "clusterbomb"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ScheduleArg {
    Default,
    RotateWindow,
}

impl std::fmt::Display for ScheduleArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Default => write!(f, "default"),
            Self::RotateWindow => write!(f, "rotate-window"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormatArg {
    Console,
    Jsonl,
    Csv,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SetModeArg {
    Or,
    And,
}

impl std::fmt::Display for SetModeArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Or => write!(f, "or"),
            Self::And => write!(f, "and"),
        }
    }
}

impl std::fmt::Display for OutputFormatArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Console => write!(f, "console"),
            Self::Jsonl => write!(f, "jsonl"),
            Self::Csv => write!(f, "csv"),
        }
    }
}

pub fn parse() -> Cli {
    Cli::parse_from(normalize_ffuf_style_args(std::env::args()))
}

fn normalize_ffuf_style_args<I>(args: I) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    const SINGLE_DASH_LONGS: &[&str] = &[
        "-rate",
        "-timeout",
        "-mode",
        "-order",
        "-schedule",
        "-target-key",
        "-target-window",
        "-target-burst",
        "-raw",
        "-sni",
        "-http2",
        "-ssl-verify",
        "-keepalive",
        "-dns-cache",
        "-dns-cache-ttl",
        "-dns-negative-cache-ttl",
        "-dns-max-concurrent",
        "-cc",
        "-ck",
        "-request",
        "-request-proto",
        "-replay-proxy",
        "-ic",
        "-enc",
        "-mc",
        "-fc",
        "-ms",
        "-fs",
        "-mw",
        "-fw",
        "-ml",
        "-fl",
        "-mr",
        "-fr",
        "-mt",
        "-ft",
        "-mmode",
        "-fmode",
        "-of",
        "-budget-requests",
        "-od",
        "-error-log",
        "-precheck",
        "-precheck-key",
        "-precheck-report-only",
        "-no-progress",
        "-stop-on-match",
        "-stop-scope",
        "-ac",
        "-ac-scope",
        "-ac-ignore",
    ];

    args.into_iter()
        .map(|arg| {
            if SINGLE_DASH_LONGS.contains(&arg.as_str()) {
                format!("-{}", arg)
            } else {
                arg
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_single_dash_order_option() {
        let args = normalize_ffuf_style_args([
            "rfuzz".to_string(),
            "-order".to_string(),
            "UFUZZ,PFUZZ,URLFUZZ".to_string(),
        ]);

        assert_eq!(args, vec!["rfuzz", "--order", "UFUZZ,PFUZZ,URLFUZZ"]);
    }

    #[test]
    fn normalizes_single_dash_precheck_options() {
        let args = normalize_ffuf_style_args([
            "rfuzz".to_string(),
            "-precheck".to_string(),
            "off".to_string(),
            "-precheck-key".to_string(),
            "URLFUZZ".to_string(),
            "-precheck-report-only".to_string(),
        ]);

        assert_eq!(
            args,
            vec![
                "rfuzz",
                "--precheck",
                "off",
                "--precheck-key",
                "URLFUZZ",
                "--precheck-report-only",
            ]
        );
    }

    #[test]
    fn normalizes_single_dash_ssl_verify_option() {
        let args = normalize_ffuf_style_args([
            "rfuzz".to_string(),
            "-ssl-verify".to_string(),
            "on".to_string(),
        ]);

        assert_eq!(args, vec!["rfuzz", "--ssl-verify", "on"]);
    }

    #[test]
    fn normalizes_single_dash_keepalive_option() {
        let args = normalize_ffuf_style_args([
            "rfuzz".to_string(),
            "-keepalive".to_string(),
            "off".to_string(),
        ]);

        assert_eq!(args, vec!["rfuzz", "--keepalive", "off"]);
    }

    #[test]
    fn normalizes_single_dash_rotate_window_options() {
        let args = normalize_ffuf_style_args([
            "rfuzz".to_string(),
            "-schedule".to_string(),
            "rotate-window".to_string(),
            "-target-window".to_string(),
            "100".to_string(),
            "-target-burst".to_string(),
            "3".to_string(),
            "-dns-cache".to_string(),
            "on".to_string(),
            "-dns-cache-ttl".to_string(),
            "300".to_string(),
            "-dns-negative-cache-ttl".to_string(),
            "30".to_string(),
            "-dns-max-concurrent".to_string(),
            "64".to_string(),
        ]);

        assert_eq!(
            args,
            vec![
                "rfuzz",
                "--schedule",
                "rotate-window",
                "--target-window",
                "100",
                "--target-burst",
                "3",
                "--dns-cache",
                "on",
                "--dns-cache-ttl",
                "300",
                "--dns-negative-cache-ttl",
                "30",
                "--dns-max-concurrent",
                "64",
            ]
        );
    }
}
