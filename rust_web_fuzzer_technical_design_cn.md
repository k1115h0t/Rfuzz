# Rust Web Fuzzer 技术方案与架构设计

## 1. 项目定位

本项目计划实现一个基于 Rust 的高性能 Web Fuzzer，目标不是简单复刻 ffuf，而是在兼容 ffuf 常用使用习惯的基础上，针对大规模资产筛查、多目标扫描、自动校准、递归扫描、结果过滤和任务恢复等场景做增强。

工具暂定名：`rfuzz`。

核心定位：

```text
一个兼容 ffuf 使用习惯、面向大规模 Web 资产 fuzz 的 Rust 命令行工具。
```

设计原则：

```text
1. 使用 `${{KEYWORD}}$` 作为原生显式占位符语法
2. 兼容 ffuf 风格的裸 keyword 替换模型
3. 不强行理解 keyword 的业务语义
4. 优先提升多目标扫描、校准、过滤、递归和恢复能力
5. 使用 Rust 异步模型提升稳定性、并发能力和资源控制能力
6. 默认输出可复现、可审计、可继续处理的数据
```

本工具仅面向授权安全测试、企业资产自查、靶场和研究环境使用。

---

## 2. 背景与问题分析

ffuf 是目前 Web 安全测试中非常常用的字典型 fuzz 工具。它的核心模型很简单：

```text
wordlist 输入 -> 替换 `${{KEYWORD}}$` 占位符 -> 发送 HTTP 请求 -> 匹配/过滤响应 -> 输出结果
```

这种设计非常适合目录扫描、参数 fuzz、vhost fuzz、POST 数据 fuzz、Header fuzz 等场景。

但是在复杂场景下，ffuf 仍然存在一些可以改进的地方：

```text
1. 多目标、多 wordlist 场景下 auto-calibration 容易混乱
2. 递归扫描输出不够清晰，难以判断结果属于哪个父路径
3. 递归队列控制能力不足，长时间扫描不够可控
4. 过滤表达式能力有限，难以表达复杂的 AND/OR 条件
5. 大字典和长任务场景下内存占用、任务恢复能力不足
6. HTTP/2 下 Host / :authority 处理需要更明确
7. 插件化和变换能力有需求，但不适合第一版做得过重
8. replay proxy、raw request、输出格式需要更强的可复现能力
```

因此，本项目的突破点不应该是“单纯比 ffuf 快”，而应该是：

```text
1. 更稳定的多目标自动校准
2. 更强的结果过滤表达式
3. 更清晰的递归队列和结果输出
4. 更好的任务恢复和断点续扫
5. 更低的内存占用和更可控的资源管理
6. 更适合批量资产筛查的输出格式
```

---

## 3. 功能目标

### 3.1 必须兼容的 ffuf 基础能力

第一阶段需要支持以下基础能力：

```text
-u URL 模板
-w wordlist[:KEYWORD]
-X HTTP 方法
-H Header
-d 请求体
-b Cookie
-x 代理
-t 并发数
-rate 每秒请求数限制
-timeout 请求超时
-mc / -fc 状态码匹配和过滤
-ms / -fs 响应大小匹配和过滤
-mw / -fw 单词数匹配和过滤
-ml / -fl 行数匹配和过滤
-mr / -fr 正则匹配和过滤
-o 输出文件
-of 输出格式
```

基础示例：

```bash
rfuzz -w dirs.txt:DIR -u 'https://example.com/${{DIR}}$' -fc 404
```

多字典示例：

```bash
rfuzz -w hosts.txt:HOST -w dirs.txt:DIR -u 'https://${{HOST}}$/${{DIR}}$' -mode pitchfork
```

POST fuzz 示例：

```bash
rfuzz -w passwords.txt:PASS \
  -u https://example.com/login \
  -X POST \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -d 'username=admin&password=${{PASS}}$' \
  -fc 401
```

---

### 3.2 本项目增强能力

相比传统 ffuf，本项目重点增强以下能力：

```text
1. 多目标 auto-calibration
2. 校准时忽略指定 wordlist
3. 动态响应自动过滤
4. 嵌套布尔过滤表达式
5. 递归扫描完整路径输出
6. 递归队列可控、可跳过、可恢复
7. JSONL 实时输出
8. 断点续扫
9. HTTP/2 :authority 明确支持
10. 低内存流式 wordlist 读取
11. 按目标或自定义作用域命中后提前停止
```

建议命令参数：

```text
-ac                         开启自动校准
-ac-scope host|job|global   校准作用域
-ac-ignore KEYWORD          校准时忽略某个 keyword
-ac-keyword KEYWORD         指定被校准污染的 keyword，默认 FUZZ
-ac-samples N               每个 scope 的校准请求数
-auto-filter dynamic        动态响应过滤
-filter-expr EXPR           高级过滤表达式
-match-expr EXPR            高级匹配表达式
-recursion                  开启递归
-recursion-depth N          最大递归深度
-recursion-seed PATHS       递归初始路径
-recursion-seed-file FILE   递归初始路径文件
-recursion-show full        递归结果显示完整路径
-save-state FILE            保存扫描状态
-resume FILE                从状态文件恢复扫描
-authority VALUE            HTTP/2 :authority 或 vhost fuzz
-stop-on-match N            每个作用域命中 N 个有效结果后停止该作用域
-stop-scope KEYWORDS        提前停止作用域，如 TARGET 或 TARGET,USER
-stop-match-source matched|expr
                            使用普通 matcher 结果或独立表达式作为停止依据
-stop-match-expr EXPR       独立的提前停止命中表达式
-per-scope-concurrency N    每个停止作用域最多同时在飞的请求数
```

---

## 4. 非目标

第一阶段不做以下事情：

```text
1. 不做完整的 Web 漏洞扫描器
2. 不做 nuclei 模板扫描能力
3. 不做 sqlmap 类型的漏洞利用能力
4. 不做复杂浏览器自动化
5. 不默认理解 JSON、GraphQL、JWT 等字段语义
6. 不做动态插件系统
7. 不做图形化界面
```

本工具保持字典型 fuzzer 定位。

关于占位符设计：

```text
`${{KEYWORD}}$` 是 rfuzz 原生推荐占位符语法，例如 `${{DIR}}$`、`${{USER}}$`、`${{PASS}}$`。
FUZZ / USER / PASS / HOST / DIR 等 keyword 只作为字符串替换点。
工具不需要知道占位符代表路径、参数、Header、Cookie 还是 JSON 字段。
编码、转义、payload 合法性由用户控制。
```

为兼容 ffuf，第一版可以继续支持裸 keyword 模式，例如 `FUZZ`、`HOST`、`DIR`。
但文档、README 和新示例应优先使用 `${{KEYWORD}}$`，避免模板中的普通文本与占位符混淆。
命令行中包含 `$` 的模板建议使用单引号包裹，避免被 shell 解释。

可以支持 encoder，但 encoder 只是字符串变换管道，不代表工具理解业务语义。

---

## 5. 总体架构

整体架构分为九层：

```text
CLI 参数层
配置归一化层
输入生成层
模板渲染层
HTTP 执行层
响应分析层
匹配过滤层
任务调度层
输出与状态层
```

整体流程：

```text
1. 解析 CLI 参数
2. 加载配置文件
3. 编译请求模板
4. 加载 wordlist / stdin / input-cmd
5. 根据 fuzz mode 生成 payload 组合
6. 根据 stop-scope 判断是否跳过该 payload
7. 渲染 HTTP 请求
8. 发送请求
9. 分析响应签名
10. 执行 calibration / filter / matcher
11. 判断是否输出结果
12. 更新 stop-on-match 作用域状态
13. 判断是否加入递归队列
14. 写入输出文件
15. 定期保存状态
```

---

## 6. 模块设计

推荐目录结构：

```text
rfuzz/
  Cargo.toml
  src/
    main.rs
    cli.rs
    config.rs

    engine/
      mod.rs
      scheduler.rs
      worker.rs
      rate_limiter.rs
      state.rs

    input/
      mod.rs
      wordlist.rs
      stdin.rs
      command.rs
      modes.rs

    template/
      mod.rs
      parser.rs
      render.rs
      encoder.rs

    http/
      mod.rs
      request.rs
      raw_request.rs
      client.rs
      response.rs

    matcher/
      mod.rs
      legacy.rs
      expr.rs
      signature.rs

    calibration/
      mod.rs
      baseline.rs
      dynamic.rs

    recursion/
      mod.rs
      queue.rs
      detector.rs

    output/
      mod.rs
      console.rs
      json.rs
      jsonl.rs
      csv.rs
      markdown.rs

    transform/
      mod.rs
      builtin.rs
```

---

## 7. CLI 与配置层

### 7.1 CLI 解析

使用 `clap` 作为命令行解析库。

核心结构：

```rust
#[derive(clap::Parser, Debug)]
pub struct Cli {
    pub url: Option<String>,
    pub wordlists: Vec<String>,
    pub method: Option<String>,
    pub headers: Vec<String>,
    pub data: Option<String>,
    pub cookies: Vec<String>,
    pub proxy: Option<String>,
    pub threads: usize,
    pub rate: Option<u64>,
    pub timeout: u64,
    pub mode: FuzzMode,
    pub output: Option<String>,
    pub output_format: OutputFormat,
    pub auto_calibration: bool,
    pub recursion: bool,
    pub stop_on_match: Option<usize>,
    pub stop_scope: Vec<String>,
    pub stop_match_source: StopMatchSource,
    pub stop_match_expr: Option<String>,
    pub per_scope_concurrency: Option<usize>,
}
```

### 7.2 配置归一化

CLI 参数、配置文件、默认值最终合并为统一的 `Config`：

```rust
pub struct Config {
    pub request: RequestTemplateConfig,
    pub inputs: Vec<InputConfig>,
    pub engine: EngineConfig,
    pub matcher: MatcherConfig,
    pub filter: FilterConfig,
    pub calibration: CalibrationConfig,
    pub recursion: RecursionConfig,
    pub stop_policy: StopPolicyConfig,
    pub output: OutputConfig,
}
```

设计要求：

```text
1. CLI 参数优先级最高
2. 配置文件次之
3. 内置默认值最低
4. 所有配置在启动阶段完成校验
5. 缺少 `${{KEYWORD}}$` 或兼容模式指定 keyword 时必须给出明确错误
6. `-stop-scope` 中的 keyword 必须存在于输入或模板中
7. `-stop-on-match` 只影响对应作用域，不应触发全局停止
```

---

## 8. 输入生成层

### 8.1 Wordlist 输入

要求支持：

```text
1. 普通文件
2. stdin
3. 外部命令 input-cmd
4. 多 wordlist
5. 每个 wordlist 绑定 keyword
6. 流式读取，避免一次性载入大字典
```

数据结构：

```rust
pub struct WordlistSource {
    pub keyword: String,
    pub source: InputSource,
}

pub enum InputSource {
    File(PathBuf),
    Stdin,
    Command(String),
}
```

### 8.2 Fuzz 模式

支持三种基本模式：

```text
sniper      每次只 fuzz 一个 keyword，其余使用默认值
pitchfork   多个 wordlist 按行同步读取
clusterbomb 多个 wordlist 做笛卡尔积组合
```

数据结构：

```rust
pub enum FuzzMode {
    Sniper,
    Pitchfork,
    Clusterbomb,
}

pub struct PayloadSet {
    pub values: HashMap<String, Bytes>,
}
```

### 8.3 内存控制

设计要求：

```text
1. pitchfork 可以完全流式
2. sniper 可以按 keyword 分批流式
3. clusterbomb 对大字典要谨慎，不能盲目全量载入内存
4. 对 clusterbomb 提供请求预算和警告
```

建议参数：

```text
-budget-requests N     限制总请求数
-budget-per-job N      限制单个递归 job 请求数
-max-memory MB         软性内存限制
```

---

## 9. 模板解析与渲染层

### 9.1 占位符模型

占位符采用 rfuzz 原生显式语法：

```text
${{FUZZ}}$
${{USER}}$
${{PASS}}$
${{HOST}}$
${{DIR}}$
${{TOKEN}}$
```

示例：

```bash
rfuzz -w hosts.txt:HOST -w dirs.txt:DIR -u 'https://${{HOST}}$/${{DIR}}$'
```

工具不理解 `HOST`、`DIR` 的业务语义，只做确定性的字符串替换。

兼容策略：

```text
1. 原生模式只替换 `${{KEYWORD}}$`，不会误替换正文中出现的普通单词。
2. 如果模板中没有 `${{...}}$`，可以进入 ffuf 兼容模式，支持裸 `FUZZ`、`HOST`、`DIR` 等 keyword。
3. 如果同一模板同时出现原生占位符和裸 keyword，默认只按原生占位符解析，并给出兼容提示。
4. 新文档和新功能示例统一使用 `${{KEYWORD}}$`。
5. 命令行里建议用单引号包裹含 `$` 的模板，例如 `'https://example.com/${{DIR}}$'`。
```

### 9.2 模板编译

不能简单使用全局 `String::replace()`。

原因：

```text
1. 多个 keyword 可能存在包含关系
2. 同一个 keyword 可能在多个位置出现
3. 需要支持 raw request 模板
4. 需要避免替换顺序导致错误
```

原生占位符解析规则：

```text
1. 起始标记为 `${{`
2. 结束标记为 `}}$`
3. 中间内容为 keyword，建议只允许 `[A-Z][A-Z0-9_]*`
4. `${{DIR}}$` 编译为 Placeholder { keyword: "DIR" }
5. 未闭合的 `${{` 或空 keyword 必须在启动阶段报错
```

建议先把模板编译为片段：

```rust
pub struct Template {
    pub parts: Vec<TemplatePart>,
}

pub enum TemplatePart {
    Literal(Bytes),
    Placeholder { keyword: String },
}
```

渲染时：

```rust
impl Template {
    pub fn render(&self, payloads: &PayloadSet) -> Bytes {
        // 按 parts 顺序拼接 Literal 和 Placeholder 的值
    }
}
```

### 9.3 Encoder 设计

支持简单 encoder：

```text
urlencode
base64
hex
lower
upper
md5
sha1
double-urlencode
```

参数示例：

```bash
rfuzz -w payloads.txt:Q \
  -u 'https://example.com/search?q=${{Q}}$' \
  -enc 'Q:urlencode'
```

数据结构：

```rust
pub trait Encoder {
    fn name(&self) -> &'static str;
    fn encode(&self, input: &[u8]) -> Vec<u8>;
}
```

注意：encoder 只是字符串变换，不代表工具理解字段语义。

---

## 10. HTTP 执行层

### 10.1 HTTP 客户端选择

第一版可以使用 `reqwest` 快速实现：

```text
优点：开发快、支持代理、HTTP/2、cookies、TLS 较方便
缺点：对 raw request、HTTP/2 :authority、SNI、连接细节控制较弱
```

长期建议切换或封装到 `hyper`：

```text
优点：更底层、更可控、更适合专业 fuzzer
缺点：开发成本更高
```

建议策略：

```text
v0.1 使用 reqwest 实现基础能力
v0.3 抽象 HttpClient trait
v0.4 逐步替换为 hyper / h2 实现高级能力
```

### 10.2 HTTP 请求结构

```rust
pub struct RenderedRequest {
    pub method: Method,
    pub url: Url,
    pub headers: HeaderMap,
    pub body: Option<Bytes>,
    pub metadata: RequestMetadata,
}

pub struct RequestMetadata {
    pub payloads: PayloadSet,
    pub job_id: u64,
    pub depth: usize,
    pub raw_template_id: Option<String>,
}
```

### 10.3 HTTP/2 Authority 支持

HTTP/1.1 中使用 `Host` Header。HTTP/2 中应明确支持 `:authority`。

建议参数：

```bash
rfuzz -u 'https://1.2.3.4/${{DIR}}$' -http2 -authority example.com -w dirs.txt:DIR
```

vhost fuzz 示例：

```bash
rfuzz -u https://1.2.3.4/ -http2 -w vhosts.txt:VHOST -authority '${{VHOST}}$'
```

设计要求：

```text
1. HTTP/1.1 下 -H "Host: xxx" 正常处理
2. HTTP/2 下优先使用 -authority
3. 如果用户在 HTTP/2 下传入 Host Header，应给出提示
4. SNI、Host、authority 三者需要允许分别指定
```

---

## 11. 响应分析层

响应分析不只保存原始 body，还要生成可过滤的响应签名。

```rust
pub struct ResponseRecord {
    pub request: RequestSummary,
    pub response: ResponseSummary,
    pub signature: ResponseSignature,
    pub matched: bool,
    pub filtered: bool,
}

pub struct ResponseSummary {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: Option<Bytes>,
    pub elapsed_ms: u128,
}

pub struct ResponseSignature {
    pub status: u16,
    pub size: usize,
    pub words: usize,
    pub lines: usize,
    pub title: Option<String>,
    pub location: Option<String>,
    pub body_hash: u64,
}
```

### 11.1 Body 保存策略

为了控制内存：

```text
1. 默认不长期保存完整 body
2. matcher/filter 需要时可以短暂读取 body
3. 输出 JSONL 默认只输出摘要
4. 用户显式指定时才保存 body 片段或完整 body
```

参数建议：

```text
-ignore-body          不读取响应 body
-save-body            保存命中结果 body
-save-body-dir DIR    body 单独落盘
-body-preview N       JSONL 中保存前 N 字节预览
```

---

## 12. 匹配与过滤层

### 12.1 兼容 ffuf 的基础 matcher/filter

基础规则：

```text
-mc 匹配状态码
-fc 过滤状态码
-ms 匹配响应大小
-fs 过滤响应大小
-mw 匹配单词数
-fw 过滤单词数
-ml 匹配行数
-fl 过滤行数
-mr 匹配正则
-fr 过滤正则
```

支持范围表达式：

```text
200,204,301,302
400-499
all
```

### 12.2 高级表达式

新增：

```bash
-filter-expr '(status == 200 && size == 4242) || body =~ "not found"'
-match-expr 'status in [200,301,302] && words > 10'
```

表达式支持字段：

```text
status
size
words
lines
time_ms
body
header["Location"]
header["Content-Type"]
title
url
redirect
```

表达式 AST：

```rust
pub enum Expr {
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    Predicate(Predicate),
}

pub enum Predicate {
    Status(Cmp),
    Size(Cmp),
    Words(Cmp),
    Lines(Cmp),
    TimeMs(Cmp),
    BodyRegex(Regex),
    HeaderRegex(String, Regex),
    TitleRegex(Regex),
}
```

第一版可以先不写完整脚本语言，只实现一个简单表达式解析器。

---

## 13. Auto-calibration 设计

### 13.1 问题

传统 auto-calibration 在以下场景容易出问题：

```text
1. HOST 本身来自 wordlist
2. TARGET 是目标列表，不应该被随机污染
3. 多 keyword 同时存在时，不知道应该替换哪个 keyword 做校准
4. 多域名扫描时，每个域名的 404 / WAF / fallback 页面不同
```

### 13.2 设计目标

```text
1. 校准时明确区分目标维度和 fuzz 维度
2. 支持忽略指定 keyword
3. 支持按 host / job / global 维护 baseline
4. baseline 不应该无限增长
5. 校准过程必须可解释、可输出
```

### 13.3 参数设计

```text
-ac                         开启自动校准
-ac-scope host|job|global   baseline 作用域
-ac-ignore KEYWORD          校准时不替换该 keyword
-ac-keyword KEYWORD         校准时主要污染哪个 keyword
-ac-samples N               每个 scope 校准样本数
-ac-random-len N            随机字符串长度
-ac-debug                   输出校准详情
```

示例：

```bash
rfuzz -w targets.txt:TARGET -w dirs.txt:FUZZ \
  -u '${{TARGET}}$/${{FUZZ}}$' \
  -ac \
  -ac-ignore TARGET \
  -ac-scope host
```

含义：

```text
TARGET 是真实目标，不参与随机替换。
FUZZ 是路径 fuzz 点，校准时只污染 `${{FUZZ}}$` 对应的 payload。
每个 host 单独维护 baseline。
```

### 13.4 数据结构

```rust
#[derive(Hash, Eq, PartialEq, Clone)]
pub struct CalibrationScope {
    pub scheme: String,
    pub host: String,
    pub port: Option<u16>,
    pub method: String,
    pub path_prefix: String,
}

pub struct Baseline {
    pub scope: CalibrationScope,
    pub signatures: Vec<ResponseSignature>,
    pub created_at: Instant,
}
```

### 13.5 判断逻辑

```text
1. 新 host / job 出现时，先生成 N 个随机 payload
2. 发送校准请求
3. 提取 status / size / words / lines / title / body_hash
4. 生成 baseline
5. 后续响应如果与 baseline 高度相似，则自动过滤
```

相似度判断：

```text
1. status 相同
2. size 差异小于阈值
3. words / lines 差异小于阈值
4. title 相同或缺失
5. body simhash 相似，可作为后续增强
```

---

## 14. 动态响应自动过滤

### 14.1 背景

很多 Web 站点会对不存在路径返回统一的 200 页面，或者 WAF 返回统一页面。手动设置 `-fs`、`-fw` 很麻烦。

### 14.2 功能设计

参数：

```text
-auto-filter dynamic
-af-window N
-af-confirm-junk N
-af-max-rules N
```

算法：

```text
1. 对每个 scope 维护最近 N 个响应签名
2. 如果连续多个响应高度一致
3. 自动发送若干随机不存在路径作为 junk 请求
4. 如果 junk 响应签名一致
5. 将该签名加入临时过滤规则
6. 后续相同签名自动过滤
```

输出示例：

```text
[auto-filter] scope=https://example.com added rule: status=200,size=4242,words=130,lines=36
```

注意：

```text
1. 自动过滤必须可关闭
2. 自动过滤规则必须写入日志
3. 用户可以要求输出被过滤记录
4. 自动过滤不能直接删除已有命中结果
```

---

## 15. 递归扫描设计

### 15.1 目标

递归扫描用于发现目录后继续向下 fuzz。

增强点：

```text
1. 输出完整路径
2. 递归队列可查看
3. 支持初始递归路径
4. 支持 BFS / DFS
5. 支持跳过当前 job
6. 支持单 job 请求预算
7. 支持断点续扫
```

### 15.2 参数设计

```text
-recursion
-recursion-depth N
-recursion-strategy redirect|greedy|smart
-recursion-seed admin,api,static
-recursion-seed-file seeds.txt
-recursion-order bfs|dfs
-recursion-show full|relative|word
-budget-per-job N
-maxtime-job SEC
-skip-current-job
```

### 15.3 队列结构

```rust
pub struct Job {
    pub id: u64,
    pub parent_id: Option<u64>,
    pub depth: usize,
    pub base_url: Url,
    pub template: Template,
    pub scope: CalibrationScope,
    pub budget: JobBudget,
}

pub struct JobQueue {
    pub pending: VecDeque<Job>,
    pub running: HashMap<u64, Job>,
    pub finished: HashSet<u64>,
    pub seen_paths: HashSet<String>,
}
```

### 15.4 递归触发规则

可选策略：

```text
redirect：只有 301/302 且 Location 指向目录时加入队列
smart：状态码命中且路径像目录时加入队列
 greedy：只要命中结果满足条件就尝试作为目录继续 fuzz
```

建议默认：`redirect`。

### 15.5 输出设计

普通输出：

```text
/api/login      [Status: 200, Size: 9712, Words: 3415, Lines: 243, Depth: 2]
/api/logout     [Status: 302, Size: 0, Words: 1, Lines: 1, Depth: 2]
```

JSONL 输出字段：

```json
{
  "url": "https://example.com/api/login",
  "input": {"DIR": "login"},
  "status": 200,
  "size": 9712,
  "words": 3415,
  "lines": 243,
  "depth": 2,
  "job_id": 17,
  "parent_job_id": 4
}
```

### 15.6 作用域级提前停止

#### 15.6.1 问题

在批量授权测试、靶场验证或企业自查中，经常会同时 fuzz 多个目标和多个输入维度，例如：

```bash
rfuzz -w targets.txt:TARGET -w users.txt:USER -w passwords.txt:PASS \
  -u '${{TARGET}}$/login' \
  -X POST \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -d 'username=${{USER}}$&password=${{PASS}}$' \
  -match-expr 'status == 302 && location =~ "/dashboard"'
```

传统 ffuf 的执行模型更接近一个全局输入流：

```text
TARGET x USER x PASS -> 逐个发送 -> 全局完成或全局停止
```

如果某个 `TARGET` 已经找到 1 个或多个有效结果，继续尝试该 `TARGET` 下剩余的 `USER/PASS` 组合通常会浪费大量请求。  
`rfuzz` 应该支持命中后只停止对应目标或对应业务维度，而不是停止整个扫描任务。

#### 15.6.2 目标

```text
1. 支持每个目标命中 N 个结果后停止该目标
2. 支持每个 TARGET+USER 命中 N 个结果后停止该用户名
3. 支持用户用任意 keyword 组合定义停止作用域
4. 只跳过对应作用域的后续请求，不影响其他目标
5. 并发场景下允许少量已经在飞的请求自然结束
6. 停止状态可以写入 state 文件并支持 resume
```

#### 15.6.3 参数设计

```text
-stop-on-match N
    每个作用域命中 N 个有效结果后停止该作用域。

-stop-scope KEYWORDS
    停止作用域，逗号分隔。
    示例：TARGET、TARGET,USER、HOST,DIR。

-stop-match-source matched|expr
    matched：使用 matcher/filter 之后的有效命中作为停止依据。
    expr：使用 -stop-match-expr 独立判断是否计入停止命中。

-stop-match-expr EXPR
    独立的停止表达式。适合“输出条件”和“停止条件”不同的场景。

-per-scope-concurrency N
    每个停止作用域最多同时在飞的请求数，减少命中后额外浪费的请求。
```

示例 1：每个 URL 找到 1 个可用结果后停止该 URL。

```bash
rfuzz -w targets.txt:TARGET -w users.txt:USER -w passwords.txt:PASS \
  -u '${{TARGET}}$/login' \
  -X POST \
  -d 'username=${{USER}}$&password=${{PASS}}$' \
  -match-expr 'status == 302 && location =~ "/dashboard"' \
  -stop-on-match 1 \
  -stop-scope TARGET
```

示例 2：每个 URL + 用户名找到 1 个可用结果后停止该用户名，但同 URL 的其他用户名继续测试。

```bash
rfuzz -w targets.txt:TARGET -w users.txt:USER -w passwords.txt:PASS \
  -u '${{TARGET}}$/login' \
  -X POST \
  -d 'username=${{USER}}$&password=${{PASS}}$' \
  -match-expr 'status == 302 && location =~ "/dashboard"' \
  -stop-on-match 1 \
  -stop-scope TARGET,USER
```

示例 3：输出所有 200/302 结果，但只有跳转到后台时才停止该作用域。

```bash
rfuzz -w targets.txt:TARGET -w users.txt:USER -w passwords.txt:PASS \
  -u '${{TARGET}}$/login' \
  -X POST \
  -d 'username=${{USER}}$&password=${{PASS}}$' \
  -mc 200,302 \
  -stop-on-match 1 \
  -stop-scope TARGET \
  -stop-match-source expr \
  -stop-match-expr 'status == 302 && location =~ "/dashboard"'
```

#### 15.6.4 数据结构

```rust
pub struct StopPolicyConfig {
    pub enabled: bool,
    pub max_matches: usize,
    pub scope_keywords: Vec<String>,
    pub match_source: StopMatchSource,
    pub match_expr: Option<CompiledExpr>,
    pub per_scope_concurrency: Option<usize>,
}

pub enum StopMatchSource {
    Matched,
    Expr,
}

#[derive(Hash, Eq, PartialEq, Clone)]
pub struct StopScopeKey {
    pub parts: Vec<(String, Bytes)>,
}

pub struct StopScopeState {
    pub matches: usize,
    pub stopped: bool,
    pub first_match_at: Option<Instant>,
    pub last_match_at: Option<Instant>,
    pub in_flight: usize,
}
```

`StopScopeKey` 只由用户指定的 keyword 值组成，不需要工具理解 `TARGET`、`USER`、`PASS` 的业务含义。

#### 15.6.5 调度规则

```text
1. 每组 payload 生成后，先根据 -stop-scope 计算 StopScopeKey。
2. 如果该 scope 已 stopped，直接跳过，不渲染请求，不占用 rate limiter，不发送 HTTP。
3. 如果配置了 -per-scope-concurrency，发送前先获取该 scope 的并发许可。
4. HTTP 响应经过 calibration 和 filter 后，得到有效 record。
5. 先计算 matcher 是否命中，再根据 -stop-match-source 判断该 record 是否计入 stop match。
6. scope.matches += 1。
7. 当 scope.matches >= -stop-on-match 时，将 scope.stopped 置为 true。
8. 已经发出的 in-flight 请求默认不强杀，完成后仍可输出结果。
9. 后续同 scope payload 会被 scheduler/input stream 跳过。
```

提前停止应放在 `scheduler` 和 `input stream` 之间，而不是放在 `output` 层。  
如果只在输出层隐藏结果，HTTP 请求仍然会继续发送，无法节省时间。

#### 15.6.6 与 fuzz mode 的关系

```text
pitchfork：
  可以完全流式判断 scope，命中后跳过后续同 scope 行。

clusterbomb：
  最容易产生请求爆炸。命中后应尽早剪枝对应 scope 的笛卡尔积。
  如果 -stop-scope 是高层维度，例如 TARGET，推荐 input engine 把 TARGET 拆成外层 job，
  这样命中后可以直接停止整个 TARGET job，而不是继续生成无意义组合。

sniper：
  每个 keyword 位置可以独立构造 scope。若 scope keyword 当前 payload 不存在，应启动时报错。
```

#### 15.6.7 与多目标 job 的关系

当 `-stop-scope TARGET` 且 `TARGET` 来自 wordlist 时，推荐内部模型不要把所有组合视为一个大 job，而是：

```text
targets.txt -> 多个 TargetJob
每个 TargetJob 内部再跑 USER/PASS 或其他 fuzz 维度
TargetJob 达到 stop-on-match 后进入 finished/skipped 状态
其他 TargetJob 不受影响
```

这种设计可以减少无效 payload 生成，也更适合后续实现：

```text
1. per-target rate limiter
2. per-target timeout
3. per-target auto-calibration
4. per-target resume
5. per-target 结果统计
```

---

## 16. 任务恢复与状态保存

### 16.1 目标

长时间扫描时，工具需要支持中断恢复。

要求：

```text
1. 定期保存当前 job 队列
2. 保存 wordlist 读取进度
3. 保存已发现结果
4. 保存 auto-calibration baseline
5. 保存动态过滤规则
6. 保存提前停止 scope 状态
7. 恢复时能继续扫描
```

### 16.2 参数

```text
-save-state state.json
-state-interval 30
-resume state.json
```

### 16.3 状态文件结构

```json
{
  "version": 1,
  "created_at": "2026-05-18T00:00:00Z",
  "config_hash": "...",
  "jobs_pending": [],
  "jobs_finished": [],
  "wordlist_offsets": {},
  "baselines": [],
  "dynamic_filter_rules": [],
  "stop_scopes": [
    {
      "scope": {"TARGET": "https://example.com", "USER": "admin"},
      "matches": 1,
      "stopped": true
    }
  ],
  "results_count": 1234
}
```

### 16.4 恢复校验

恢复时需要检查：

```text
1. 当前命令参数是否与 state 匹配
2. wordlist 文件是否改变
3. 输出文件是否存在
4. 是否继续追加输出
5. 是否需要重新校准
```

---

## 17. 输出设计

### 17.1 Console 输出

默认输出清晰、短小：

```text
admin      [Status: 200, Size: 1234, Words: 100, Lines: 20]
login      [Status: 302, Size: 0, Words: 1, Lines: 1, Redirect: /dashboard]
```

递归时默认显示完整路径：

```text
/api/admin      [Status: 403, Size: 812, Words: 65, Lines: 12, Depth: 1]
/api/v1/users   [Status: 200, Size: 2091, Words: 180, Lines: 40, Depth: 2]
```

### 17.2 JSONL 输出

推荐默认机器可读格式使用 JSONL，而不是一个大 JSON 数组。

原因：

```text
1. 可以边扫边写
2. 不需要把结果全部保存在内存
3. 中断时文件仍然可用
4. 适合 jq / python / ELK / 数据管道处理
```

示例：

```json
{"url":"https://example.com/admin","status":403,"size":812,"words":65,"lines":12,"input":{"DIR":"admin"}}
```

### 17.3 输出格式

支持：

```text
jsonl
json
csv
markdown
html，后续可选
```

建议第一版支持：

```text
console
jsonl
csv
```

---

## 18. Replay Proxy 与可复现请求

### 18.1 功能目标

命中的请求可以转发到 Burp / ZAP：

```bash
rfuzz -w dirs.txt:DIR -u 'https://example.com/${{DIR}}$' -replay-proxy http://127.0.0.1:8080
```

### 18.2 设计注意

```text
1. replay 的请求必须和实际命中请求一致
2. 多 keyword 相似时不能替换错
3. JSONL 中应记录 payload 映射
4. raw request 模式下应保存原始请求模板 ID
```

输出中建议包含：

```json
{
  "request_raw_preview": "GET /admin HTTP/1.1\r\nHost: example.com\r\n...",
  "payloads": {"DIR": "admin"}
}
```

---

## 19. Transform 设计

第一版不做动态插件系统，先做内置 transform。

示例参数：

```bash
rfuzz -w dirs.txt:DIR -u 'https://example.com/${{DIR}}$' -transform append-slash
```

支持的 transform：

```text
append-slash       admin -> admin/
append-dot         admin -> admin.
random-case        admin -> AdMiN
double-urlencode   ../ -> %252e%252e%252f
backup-suffix      index -> index.bak,index.old,index~
```

数据结构：

```rust
pub trait Transform {
    fn name(&self) -> &'static str;
    fn apply(&self, input: &[u8]) -> Vec<Vec<u8>>;
}
```

后续再考虑：

```text
Rhai 脚本插件
WASM 插件
外部命令插件
```

---

## 20. 资源控制设计

### 20.1 并发控制

```text
-t N       并发 worker 数
-rate N    全局每秒请求数
-p DELAY   请求间隔或随机延迟
```

实现建议：

```text
1. worker 数控制并发
2. rate limiter 控制发包速率
3. 每个 host 可选独立 rate limiter
4. 错误率过高时可以自动降速
```

### 20.2 错误处理

支持：

```text
-retries N
-retry-on timeout,connect,reset,5xx
-se stop on spurious errors
-sf 403 过多时停止
-sa 所有错误触发停止
```

### 20.3 内存控制

设计要求：

```text
1. wordlist 尽量流式读取
2. 输出使用 JSONL 边扫边写
3. body 默认不长期保存
4. baseline 和 dynamic filter rule 有上限
5. clusterbomb 大组合提前提示请求量
```

---

## 21. 安全与合规提示

工具启动时可以在非 silent 模式显示：

```text
Use rfuzz only on systems you own or have explicit permission to test.
```

建议提供：

```text
-safe-defaults       开启保守速率和重试策略
-aggressive          用户显式声明后提高并发和速率
```

默认值建议：

```text
threads = 40
rate = unlimited，但提示用户可设置
timeout = 10s
retries = 0
body 保存 = false
```

---

## 22. 依赖选择

### 22.1 v0.1 推荐依赖

```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
clap = { version = "4", features = ["derive"] }
reqwest = { version = "0.12", features = ["cookies", "socks", "rustls-tls", "http2"] }
regex = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
csv = "1"
url = "2"
bytes = "1"
tracing = "0.1"
tracing-subscriber = "0.3"
dashmap = "6"
governor = "0.6"
anyhow = "1"
thiserror = "1"
```

### 22.2 后续可选依赖

```toml
hyper = { version = "1", features = ["full"] }
hyper-util = "0.1"
http-body-util = "0.1"
rustls = "0.23"
h2 = "0.4"
rhai = "1"
wasmtime = "latest"
```

---

## 23. MVP 版本规划

### v0.1：基础可用版

目标：可以替代 ffuf 的基础目录扫描、POST fuzz、Header fuzz。

功能：

```text
1. -u / -w / -X / -H / -d
2. `${{KEYWORD}}$` 原生占位符和 ffuf 兼容裸 keyword 替换
3. pitchfork / clusterbomb 基础实现
4. 状态码、大小、单词数、行数过滤
5. 并发请求
6. 限速
7. console / jsonl / csv 输出
```

验收命令：

```bash
rfuzz -w dirs.txt:DIR -u 'https://example.com/${{DIR}}$' -fc 404
```

---

### v0.2：差异化增强版

目标：做出相比 ffuf 更有价值的能力。

功能：

```text
1. -ac-scope host/job/global
2. -ac-ignore KEYWORD
3. -filter-expr
4. -auto-filter dynamic
5. 递归完整路径输出
6. recursion-seed
7. save-state / resume
8. -stop-on-match / -stop-scope
```

验收命令：

```bash
rfuzz -w targets.txt:TARGET -w dirs.txt:FUZZ \
  -u '${{TARGET}}$/${{FUZZ}}$' \
  -ac \
  -ac-ignore TARGET \
  -ac-scope host \
  -stop-on-match 1 \
  -stop-scope TARGET \
  -of jsonl \
  -o result.jsonl
```

---

### v0.3：专业增强版

目标：适合复杂请求、代理复现和高级 Web 测试。

功能：

```text
1. raw request 支持
2. replay proxy
3. HTTP/2 authority
4. transform 内置变换
5. input-cmd
6. sniper 模式
7. body 保存策略
8. 更完善的错误重试
```

---

### v0.4：长期演进版

目标：成为大型资产 fuzz 的稳定工具。

功能：

```text
1. hyper HTTP 后端
2. per-host rate limiter
3. WASM / Rhai 插件系统
4. 响应相似度聚类
5. Web UI 或 TUI
6. 分布式扫描队列
```

---

## 24. 核心执行伪代码

```rust
async fn run(config: Config) -> Result<()> {
    let templates = compile_templates(&config)?;
    let input_engine = InputEngine::new(&config.inputs, config.mode)?;
    let http_client = HttpClient::new(&config.http)?;
    let mut scheduler = Scheduler::new(
        config.recursion.clone(),
        config.stop_policy.clone(),
    );
    let matcher = Matcher::new(config.matcher.clone())?;
    let filter = Filter::new(config.filter.clone())?;
    let calibration = CalibrationEngine::new(config.calibration.clone());
    let output = OutputManager::new(config.output.clone())?;

    scheduler.push_initial_job(templates.initial_job());

    while let Some(job) = scheduler.next_job().await {
        let mut payload_stream = input_engine.stream_for_job(&job).await?;

        while let Some(payloads) = payload_stream.next().await {
            let stop_scope = scheduler.stop_scope_for(&payloads)?;
            if scheduler.should_skip_scope(&stop_scope).await {
                continue;
            }

            let _scope_permit = scheduler.acquire_scope_permit(&stop_scope).await?;
            let request = job.template.render(&payloads)?;

            if calibration.needs_baseline(&job, &request).await {
                calibration.build_baseline(&job, &http_client).await?;
            }

            let response = http_client.send(request).await?;
            let record = analyze_response(response, &payloads, &job)?;

            if calibration.should_filter(&job, &record) {
                continue;
            }

            if filter.should_filter(&record) {
                continue;
            }

            let matched = matcher.matches(&record);

            if scheduler.should_count_stop_match(&stop_scope, &record, matched).await? {
                scheduler.record_stop_match(&stop_scope, &record).await?;
            }

            if matched {
                output.emit(&record).await?;

                if let Some(new_job) = scheduler.detect_recursion(&job, &record) {
                    scheduler.push(new_job);
                }
            }

            scheduler.maybe_save_state().await?;
        }
    }

    output.finish().await?;
    Ok(())
}
```

---

## 25. 关键风险与处理方案

### 25.1 clusterbomb 请求量爆炸

风险：多个大字典笛卡尔积可能导致请求数巨大。

处理：

```text
1. 启动前估算请求量
2. 超过阈值提示用户
3. 支持 -budget-requests
4. 支持 --force 跳过确认
```

### 25.2 自动过滤误杀结果

风险：auto-calibration 或 dynamic filter 可能过滤真实结果。

处理：

```text
1. 自动规则可输出
2. 支持 -debug-filter
3. 支持输出 filtered 记录
4. 默认规则保守
5. 用户可关闭 auto-filter
```

### 25.3 raw request 解析复杂

风险：HTTP raw request 解析边界多。

处理：

```text
1. v0.1 暂不支持 raw request
2. v0.3 单独实现 raw parser
3. 保留原始换行和 header 顺序
4. 支持从 Burp 复制请求
```

### 25.4 HTTP/2 细节控制不足

风险：reqwest 对 :authority、SNI 控制有限。

处理：

```text
1. v0.1 优先 HTTP/1.1 和基础 HTTP/2
2. 抽象 HttpClient trait
3. 后续使用 hyper/h2 实现高级后端
```

### 25.5 内存占用过高

风险：大字典、大 body、大结果集导致内存升高。

处理：

```text
1. wordlist 流式读取
2. JSONL 边扫边写
3. body 默认不保存
4. baseline 设置上限
5. 定期释放 job 中间状态
```

### 25.6 作用域提前停止的并发竞态

风险：某个 scope 达到 `-stop-on-match` 后，可能已经有同 scope 请求在并发队列中或正在网络传输，导致额外请求。

处理：

```text
1. 发送前先检查 scope stopped 状态
2. rate limiter 前完成 stopped 检查，避免消耗速率预算
3. 支持 -per-scope-concurrency 限制每个 scope 的在飞请求
4. scope stopped 后让已发出的请求自然结束，避免强制取消导致输出状态混乱
5. state 文件记录 stopped scope，resume 后不重复扫描已停止作用域
```

---

## 26. 第一阶段开发优先级

建议按以下顺序实现：

```text
1. CLI 参数解析
2. Config 归一化
3. Template 编译与 keyword 替换
4. 单 wordlist 输入
5. async HTTP 请求
6. 响应 signature 计算
7. 基础 matcher/filter
8. console 输出
9. JSONL 输出
10. 多 wordlist pitchfork
11. clusterbomb
12. rate limiter
13. stop-scope / stop-on-match
14. auto-calibration 基础版
15. -ac-ignore
16. save-state / resume
17. recursion queue
18. filter-expr
```

最小可用闭环：

```text
读取字典 -> 替换 `${{KEYWORD}}$` -> 发送请求 -> 过滤 404 -> 输出结果
```

---

## 27. 项目卖点总结

建议项目 README 的一句话定位：

```text
rfuzz is a ffuf-compatible Rust web fuzzer with smarter calibration, scope-aware early stopping, resumable recursion, expressive filtering, and streaming output for large-scale asset testing.
```

中文定位：

```text
rfuzz 是一个兼容 ffuf 使用习惯的 Rust Web Fuzzer，重点增强多目标校准、按目标命中后提前停止、递归扫描、复杂过滤、断点续扫和流式输出能力。
```

核心卖点：

```text
1. 原生 `${{KEYWORD}}$` 占位符避免和普通文本混淆，同时兼容 ffuf 的 FUZZ 使用方式
2. 多目标 auto-calibration 更稳定
3. 支持 -ac-ignore，避免污染目标列表
4. 支持按 TARGET / USER 等作用域命中后剪枝，减少无效请求
5. 支持嵌套布尔过滤表达式
6. 递归扫描显示完整路径
7. 支持任务状态保存和恢复
8. JSONL 实时输出，适合大规模处理
9. Rust 异步实现，资源控制更明确
```

---

## 28. 后续待讨论问题

```text
1. 项目名称是否确定为 rfuzz？
2. 第一版是否完全兼容 ffuf 参数名？
3. HTTP 层第一版使用 reqwest 还是直接 hyper？
4. filter-expr 是否第一版就做？
5. 是否默认开启 auto-calibration？
6. raw request 是否进入 v0.1？
7. 输出格式是否优先 JSONL？
8. 是否需要 Windows 原生支持？
9. 是否需要内置 SecLists 路径识别？
10. 是否需要和 httpx / nuclei 做管道联动？
```
