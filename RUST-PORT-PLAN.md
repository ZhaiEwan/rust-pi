# Pi Rust 移植开发计划

本文档基于对 `earendil-works/pi` TypeScript 代码库（HEAD `2b0a123de`，版本 0.87.1）的分析，给出用 Rust 重写该项目的功能范围、MVP 边界与开发计划。

假设前提：

- 单人全职开发，资深 Rust 工程师
- 目标形态为聊天式命令行界面
- 与 TS 版共享会话文件格式，便于对拍测试与互操作

## 1. 背景

### 1.1 原项目定位

`pi` 是一个可自扩展的终端编码代理，采用 npm workspaces 管理的 TypeScript monorepo，MIT 协议，要求 Node >= 22.19。核心产品是 `pi` 命令行工具，外围是一组可独立复用的运行时库。

规模数据：1583 个 TypeScript 文件、约 37.8 万行代码、626 个测试文件。

### 1.2 包结构与依赖

```
chord (应用编排运行时, 无内部依赖)
  ├─ telemetry ── pi-ai ── pi-agent-core
  ├─ pi-protocol ── pi-client
  ├─ pi-durable
  └─ pi-server

pi-coding-agent ── chord + pi-agent-core + pi-ai + pi-tui
```

| 包 | 作用 | 规模 |
|---|---|---|
| `coding-agent` | `pi` CLI 本体：工具、会话、扩展、交互模式 | 155k 行 |
| `ai` | 统一多供应商 LLM API | 69k 行 |
| `agent` | agent 循环、工具调用、压缩、会话存储 | 66k 行 |
| `tui` | 差分渲染终端 UI 库 | 38.5k 行 |
| `chord` | 应用编排：facet、service、复制状态、RPC | 17.6k 行 |
| `durable` | 持久化对话/任务/文档运行时 | 16.6k 行 |
| `client` / `server` / `protocol` | 实验性远程会话协议 | 1.5-3k 行 |
| `telemetry` | 供应商中立遥测契约 | 1.2k 行 |
| `session-backends/sqlite-node` | `node:sqlite` 会话后端 | 4.1k 行 |

### 1.3 运行链路

```
cli.ts → main.ts (参数解析/认证/信任/会话选择)
       → core/agent-session-runtime.ts → agent-session.ts
       → @pi-agent-core agent-loop.ts (请求→流式响应→工具执行→下一轮)
       → @pi-ai providers/api/* (具体供应商)
       → modes/{interactive,print,json,rpc} 或 SDK
```

关键机制：

- 会话是树。每个 JSONL 条目标 ID 与父 ID，当前条目决定活动分支；分支切换、fork、clone 共用同一模型。压缩插入摘要条目替换旧消息，原始条目保留。
- 工具在校验路径后以进程权限直接执行，没有内置沙箱。
- 扩展是进程内 TypeScript 模块，用 jiti 加载。
- 四种对外接口共用同一套 agent 与 session 机制。

### 1.4 三个移植难点

1. 扩展系统在运行时加载 TypeScript，Rust 无法直接支持，必须重新设计。
2. TUI 是 38.5k 行的自研差分渲染器，包含组件树、overlay、滚动锚定、IME 支持。
3. Provider 有 44 个，但底层只有约 15 个协议族，必须数据驱动而非逐个手写。

## 2. 移植范围总览

优先级标记：**核心** 不做就没有可用 agent；**完整** 完整替代 TS 版需要；**可选** 可后置；**重设** 不能照搬；**砍** 建议不移植。

| 模块 | 功能点数 | 优先级 |
|---|---|---|
| 消息与内容模型 | ~15 | 核心 |
| Provider 与模型 | ~25 | 核心（8-10 个）/ 完整（全量） |
| 认证 | ~10 | 核心（API key）/ 完整（OAuth） |
| Agent 循环 | ~15 | 核心 |
| 内置工具 | ~25 | 核心 |
| 会话与树 | ~20 | 核心 |
| 上下文与压缩 | ~15 | 核心 |
| 系统提示 | ~8 | 核心 |
| 资源系统（skills/templates/themes/packages） | ~20 | 完整 |
| 扩展系统 | ~20 | 重设 |
| 交互式 TUI | ~60 | 核心（基础）/ 完整（全功能） |
| CLI 与运行模式 | ~20 | 核心 |
| RPC 与 SDK | ~12 | 完整 |
| 配置与信任 | ~15 | 核心 |
| 图片多模态 | ~10 | 完整 |
| 平台适配 | ~10 | 完整 |
| 辅助运维 | ~20 | 可选 |
| 实验层（远程会话） | ~30 | 可选 |

最小可用子集约 150 个功能点，完整替代约 330 个。

建议直接砍掉：运行时 TS 扩展加载、npm/git 包管理、Bun 专有路径、worker_threads 图片缩放、mermaid/LaTeX 渲染、会话分享上传、远程会话实验层。

## 3. MVP 定义

### 3.1 一句话定义

一个 Rust 单文件二进制，以聊天式命令行界面为主，能完成「给出任务 → 读文件 → 改文件 → 跑命令 → 回答」的闭环，会话落盘为 TS 版可直接打开的 JSONL。

### 3.2 形态决策

采用 pi 默认的 regular 模式：对话内联在终端中滚动，底部固定输入行，不做全屏 alt-screen。

交互式聊天是 MVP 的主界面，print 模式降级为附带产物。print 几乎免费（两种模式消费同一事件流），且是最便宜的自动化测试入口，必须保留。

必须明确定义交互语义：agent 忙碌时用户的输入如何处理。

| 方案 | 说明 | 选择 |
|---|---|---|
| A. 忽略输入 | 最简单，但用户感觉卡死 | 否 |
| B. 排队为 steering message | 实现成本仅一个队列 | **是（MVP）** |
| C. 完整排队语义（steering + follow-up 分离） | 更完整但复杂度上升 | 后置 |

### 3.3 MVP 包含

| 模块 | 内容 |
|---|---|
| 消息与内容模型 | text/thinking/toolCall 三种内容块，system/user/assistant/toolResult 四种消息，完整 Usage 字段 |
| Provider 层 | OpenAI Chat Completions + Anthropic Messages 两个协议族，数据驱动 provider 表，8-10 个模型 |
| 认证 | 环境变量 API key + 凭证文件 |
| Agent 循环 | turn 状态机、串行工具批处理、abort、最大轮数保护、事件流 |
| 内置工具 | read、write、edit、bash、grep、find、ls |
| 会话持久化 | JSONL v3 读写（单分支）、按 id 恢复、`--no-session` |
| 上下文装配 | 活动分支重建、消息转换、溢出检测、用量统计 |
| 系统提示 | 基础提示、AGENTS.md 发现、工具说明注入、`--append-system-prompt` |
| CLI | 约 15 个参数、print 模式 |
| 聊天客户端 | 终端会话管理、渲染、输入编辑器、斜杠命令、中断与队列 |
| 配置 | agent 目录、settings.json 最小字段集、核心环境变量 |

### 3.4 MVP 不包含

| 项 | 原因 |
|---|---|
| TUI 差分渲染框架、组件树、overlay、视口 | 独立大工程，用行式聊天界面替代 |
| markdown 渲染、语法高亮、mermaid、LaTeX | 纯展示，不影响 agent 能力 |
| 压缩与分支摘要 | 溢出报错可接受，长会话体验后置 |
| 树形分支、fork、clone、会话选择器 | 单分支足够跑通闭环 |
| JSON 事件流模式、RPC 模式、SDK | 对外契约，单独排期 |
| 扩展系统、skills、prompt templates、themes、packages | 需先定扩展形态 |
| OAuth 登录 | API key 覆盖开发场景 |
| 多模态图片输入 | 涉及编码、缩放、能力协商 |
| 44 个 provider | 2 个协议族覆盖主流 |
| 遥测、bug 报告、自动更新、会话分享、HTML 导出 | 运维功能 |
| 实验层（chord/protocol/server/durable） | 独立赛道 |

## 4. 开发计划

### 4.1 里程碑总表

| 里程碑 | 内容 | 周期 | 交付物 |
|---|---|---|---|
| M0 | 项目骨架与对拍测试框架 | 第 1 周 | 可编译 workspace + fixture 流程 |
| M1 | 类型与序列化契约 | 第 1-2 周 | 消息/会话类型 + round-trip 测试 |
| M2 | 配置、路径、认证 | 第 2 周 | 配置加载 + API key 解析 |
| M3 | Provider 层（OpenAI 协议族） | 第 2-4 周 | 可流式调用 OpenAI 兼容端点 |
| M4 | Agent 循环 + read/bash 工具 | 第 4-6 周 | 能读文件、跑命令、多轮工具调用 |
| M5 | print 模式端到端 | 第 6 周 | 非交互闭环可用 |
| M6 | 会话持久化 | 第 6-7 周 | JSONL v3 读写 + 恢复 |
| M7 | 上下文装配与系统提示 | 第 7-8 周 | AGENTS.md 注入、溢出检测 |
| M8 | 补齐 write/edit/grep/find/ls | 第 8-9 周 | 完整工具集 |
| M9 | 聊天客户端 | 第 9-12 周 | 交互式 CLI 可用 |
| M10 | Anthropic 协议族 + 收尾 | 第 12-14 周 | MVP 完成 |

总计 14 周。M2 可与 M3 并行，M6/M7 可与 M8 并行。

### 4.2 项目结构

```
pi-rs/
├── Cargo.toml                    # workspace
├── crates/
│   ├── pi-types/                 # 消息、内容块、usage、事件（零内部依赖）
│   ├── pi-ai/                    # provider、协议族、SSE、模型目录、认证
│   ├── pi-session/               # JSONL 会话持久化
│   ├── pi-tools/                 # 内置工具
│   ├── pi-agent/                 # agent 循环、上下文装配、系统提示
│   ├── pi-config/                # settings、路径、环境变量
│   └── pi-cli/                   # 二进制：print 模式 + 聊天客户端
└── fixtures/                     # 对拍用的录制数据与期望输出
```

`pi-types` 必须保持零内部依赖。它是契约层，所有对拍测试直接针对它。

### 4.3 M0 项目骨架与对拍框架（第 1 周）

目标：在写业务代码前先解决"怎么验证正确性"。

- [ ] 创建 cargo workspace，7 个 crate，配置 `resolver = "2"`、共享 `[workspace.dependencies]`、release profile（LTO、strip）
- [ ] 引入基础依赖：tokio、serde、serde_json、anyhow、thiserror、tracing
- [ ] 建立 fixture 目录约定：`fixtures/<场景名>/{input,expected}.json`
- [ ] 写 TS 版 fixture 生成脚本：调用 `pi --mode json` 录一轮真实会话，存下 SSE 原始事件、最终消息序列、usage
- [ ] 建立对拍 runner：读 fixture、跑 Rust 实现、对比期望值、失败时打印结构化 diff
- [ ] CI：`cargo fmt --check`、`cargo clippy -D warnings`、`cargo test`

验收：能跑通一个空对拍用例（fixture 放固定 SSE，断言解析结果符合期望）。

### 4.4 M1 类型与序列化契约（第 1-2 周）

目标：冻结消息模型，字段与 TS 版逐一对齐。

- [ ] `ContentBlock`：text（`textSignature` 透传）、thinking（`thinkingSignature`、`redacted`）、toolCall（`id`/`name`/`arguments`/`thoughtSignature`）
- [ ] `Message`：system、user、assistant、toolResult，含全部可选字段
- [ ] `Usage`：input/output/cacheRead/cacheWrite/cacheWrite1h/reasoning/totalTokens 与 5 个 cost 字段
- [ ] `StopReason`：pending/stop/length/toolUse/error/aborted/deferred
- [ ] 事件枚举：消息开始/增量/结束、工具开始/增量/结束、轮次开始/结束
- [ ] 会话条目：header、message、model_change、thinking_level_change、session_info
- [ ] 决策：`arguments` 用 `serde_json::Value` + 运行时 schema 校验，不用强类型
- [ ] 未知字段保留：provider 签名类字段必须原样回传，用 `#[serde(flatten)] extra: Map<String, Value>` 兜底
- [ ] 时间戳统一：消息用毫秒整数，会话条目用 ISO 8601

验收：

- 从 TS 版录制的 JSONL 能完整反序列化再序列化，字节级差异为零
- 每个字段有专门的 round-trip 测试
- `cargo test -p pi-types` 全绿

### 4.5 M2 配置、路径、认证（第 2 周）

- [ ] agent 目录解析：`PI_CODING_AGENT_DIR` → `~/.pi/agent`
- [ ] settings.json 解析：`defaultProvider`、`defaultModel`、`defaultThinkingLevel`、`defaultTools`、`sessionDir`、`shellPath`
- [ ] 配置层级：全局 + 项目 `.pi/settings.json`，字段级覆盖，数组字段合并
- [ ] 环境变量：`PI_MODEL`、`PI_PROVIDER`、`PI_OFFLINE`、`PI_CODING_AGENT_DIR`、`PI_SESSION_DIR`
- [ ] API key 解析：`<PROVIDER>_API_KEY` 环境变量 → 凭证文件
- [ ] 凭证文件读写：路径与格式与 TS 版一致
- [ ] 缺失凭证的错误提示：指出具体该设哪个变量

验收：`pi-rs --list-models` 能读到配置并列出模型；无 key 时错误信息明确指出缺失的变量名。

### 4.6 M3 Provider 层（第 2-4 周）

目标：一个协议族跑通，抽象留好扩展位。

- [ ] provider 数据表：id、displayName、baseUrl、apiKeyEnvVar、默认 header、协议族
- [ ] 模型目录：手写 8-10 个常用模型，含 context window、max output、input 模态、input/output 单价、thinking 等级
- [ ] 请求组装：内部消息 → OpenAI Chat Completions；system 消息抽取；工具定义转 JSON Schema
- [ ] SSE 解析：手写解析器，不用现成库（各家流语义差异大）
- [ ] 流式事件映射：OpenAI 增量 → 内部事件
- [ ] usage 归一化与成本计算
- [ ] 工具调用增量拼接：参数是分片到达的 JSON 片段，需拼出完整对象
- [ ] HTTP 层：reqwest + rustls、代理、超时、空闲超时
- [ ] 重试：429/5xx 分类、指数退避、尊重 `Retry-After`
- [ ] 取消：`CancellationToken` 贯穿 HTTP 流，中断时立即停止读取
- [ ] 错误映射：HTTP 状态 → 可读错误类型

验收：

- 同一提示下 Rust 版与 TS 版报出的 input/output/cache token 完全一致
- 录制的 SSE fixture 覆盖五种路径：正常结束、工具调用、长度截断、错误、中断
- 中断能在 100ms 内停止流读取

风险：只做一个协议族容易让抽象被带偏。若第 4 周有人力，提前插入 Anthropic 骨架验证抽象。

### 4.7 M4 Agent 循环 + read/bash（第 4-6 周）

- [ ] turn 状态机：组装请求 → 流式接收 → 记录 assistant → 执行工具 → 记录结果 → 判断继续
- [ ] 工具批处理：串行执行（MVP 不做并行），逐个记录结果
- [ ] abort 传播：中断 HTTP 流与正在执行的工具
- [ ] 最大轮数保护
- [ ] `read` 工具：文本读取、offset/limit、行号、截断提示、二进制检测
- [ ] `bash` 工具：执行、stdout/stderr 捕获、输出截断（保留头尾）、退出码、超时、中断、环境注入
- [ ] 工具参数 JSON Schema 校验，失败回灌给模型而非抛异常
- [ ] 路径规范化与越界校验
- [ ] 输出截断策略统一
- [ ] 事件发射：订阅者收到完整生命周期事件

验收：

- 端到端测试：模型调用 read → bash → 给出回答，事件序列与 fixture 一致
- bash 截断边界用例：超长单行、超多行、含 ANSI
- Ctrl+C 能在工具执行中中断

### 4.8 M5 print 模式端到端（第 6 周）

目标：先拿到可自动化测试的完整闭环，为聊天客户端铺路。

- [ ] `-p/--print` 参数
- [ ] 流式文本输出到 stdout，工具与思考信息输出到 stderr
- [ ] 退出码语义
- [ ] 最小参数集：`--provider`、`--model`、`--thinking`、`--no-session`、`@file`、`--system-prompt`
- [ ] `--version`、`--help`
- [ ] 端到端脚本测试：在 fixture repo 上跑真实任务

验收：`pi-rs -p "把 X 改成 Y 并跑测试"` 能真实完成，退出码正确。

这一里程碑是分水岭。到这里整条链路已验证，剩下的是界面与补齐。

### 4.9 M6 会话持久化（第 6-7 周）

- [ ] JSONL 写入：header（version/id/timestamp/cwd）+ 条目（id/parentId/timestamp）
- [ ] 条目类型：message、model_change、thinking_level_change、session_info
- [ ] 加载与解析：v3 解析、未知条目类型忽略、活动分支定位
- [ ] 单分支操作：新建、按 id 续接、按路径恢复
- [ ] 文件位置规则：`~/.pi/agent/sessions/--<path>--/<timestamp>_<id>.jsonl`
- [ ] 原子追加，崩溃后文件仍可解析
- [ ] 文件锁，避免同目录多进程互踩
- [ ] `--no-session` 纯内存模式
- [ ] 会话 id 生成

验收：

- Rust 写出的会话能被 TS 版 `pi --resume` 打开并继续
- TS 版写的会话能被 Rust 版加载并继续
- 两边交替写入后文件仍可解析

### 4.10 M7 上下文装配与系统提示（第 7-8 周）

- [ ] 从活动分支重建模型消息序列
- [ ] 消息转换：system 抽取、toolResult 与 toolCall 配对、未知角色过滤
- [ ] 基础系统提示：身份、行为准则、工具规范、环境信息（cwd、平台、shell、git 状态）
- [ ] 上下文文件发现：向上查找 `AGENTS.md` 并注入
- [ ] 工具说明自动生成
- [ ] `--append-system-prompt`
- [ ] 上下文溢出检测：基于 context window 估算，给出明确错误
- [ ] token 估算：粗估即可，仅用于溢出预警；真实值以 provider usage 为准
- [ ] 会话级累计用量统计

验收：溢出用例给出可读错误且进程不崩溃；系统提示中的工具列表与环境信息正确。

### 4.11 M8 补齐工具（第 8-9 周）

- [ ] `write`：创建/覆盖、父目录自动创建
- [ ] `edit`：精确文本替换、多处替换、失败时给出明确原因与上下文、diff 计算
- [ ] `grep`：复用 ripgrep 的 `ignore` + `globset` crate，支持 glob 过滤、上下文行、结果截断
- [ ] `find`：glob 路径查找、排序、截断
- [ ] `ls`：目录列举、隐藏文件处理
- [ ] 工具注册表：名称 → 工具定义，支持启用/排除
- [ ] `-t/--tools`、`-xt/--exclude-tools` 参数

验收：每个工具的行为用例与 TS 版对拍，尤其 edit 的失败信息与 diff 格式。

### 4.12 M9 聊天客户端（第 9-12 周）

MVP 的核心交付物，分四个子阶段。

**9a 终端基础设施（第 9 周）**

- [ ] raw mode 进入/退出，`Drop` + panic hook 双重保障
- [ ] 光标显隐控制、resize 事件
- [ ] 终端能力探测：ANSI 颜色、真彩色（可退化到 16 色）
- [ ] 单线程事件循环：`tokio::select!` 同时处理终端事件与 agent 事件
- [ ] 信号处理：SIGINT、SIGTERM、SIGWINCH

**9b 渲染（第 9-11 周）**

- [ ] 输出区：消息直接写入终端，交给 scrollback
- [ ] 活动区：ANSI 光标覆写最后 N 行，用于工具状态与 spinner
- [ ] 输入行始终位于最后一行：输出到来时"清除输入行 → 写输出 → 重画输入行"
- [ ] 流式文本按块 flush，不逐 token 重绘
- [ ] 渲染节流：30-60fps 合并
- [ ] 宽度计算：`unicode-width` + `unicode-segmentation`，CJK 与 emoji 正确
- [ ] ANSI 序列不参与宽度计算
- [ ] 思考块灰显或折叠为一行
- [ ] 工具调用单行摘要：运行中/成功/失败 + 耗时
- [ ] 错误红色输出
- [ ] 状态行：模型名、thinking 等级、累计 token 与成本

**9c 输入（第 10-11 周）**

- [ ] 行内编辑：左右、Home/End、词级移动、删除前后字符、删除到行首行尾
- [ ] 输入历史（上下键）
- [ ] 多行输入：Alt+Enter 换行
- [ ] 多行粘贴正确处理
- [ ] Ctrl+U / Ctrl+K / Ctrl+W
- [ ] 斜杠命令解析：`/model`、`/thinking`、`/new`、`/resume`、`/help`、`/quit`
- [ ] 斜杠命令前缀补全
- [ ] 未知命令提示，不发给模型

**9d 交互语义（第 11-12 周）**

| 场景 | 行为 |
|---|---|
| agent 运行中按 Ctrl+C | 中断当前轮，保留已完成结果，回到输入态 |
| 空输入时按 Ctrl+C | 退出程序 |
| 任意时刻 Ctrl+D | 退出程序 |
| agent 运行中输入内容 | 排队为 steering message，当前轮结束后送达 |
| 工具执行中按 Ctrl+C | 中断工具 |

另外：

- [ ] 退出时保留 transcript 于 scrollback，打印恢复提示（会话 id + 恢复命令）

### 4.13 M10 Anthropic 协议族 + 收尾（第 12-14 周）

- [ ] Anthropic Messages 协议：thinking 块、cache control、tool use
- [ ] 两个协议族的 usage 字段统一到内部模型
- [ ] 模型目录补 Anthropic 模型
- [ ] 全局收尾：错误文案统一、`--help` 完整、日志与 `--verbose`
- [ ] 性能检查：启动时间、内存占用、大输出场景
- [ ] 文档：README、参数说明、与 TS 版的差异清单

验收：两个 provider 都能完成聊天闭环，MVP 验收清单全部通过。

## 5. 技术选型

| 领域 | crate | 说明 |
|---|---|---|
| 异步运行时 | tokio | 单 runtime，取消用 `CancellationToken` |
| HTTP | reqwest + rustls | 避免 openssl 依赖 |
| SSE | 自研 | 各协议流语义差异大 |
| 序列化 | serde + serde_json | `#[serde(flatten)]` 保留未知字段 |
| CLI 解析 | clap | `@file`、`--` 需自定义处理 |
| 终端 | crossterm | 不用 ratatui，见下 |
| 宽度计算 | unicode-width、unicode-segmentation | 渲染正确性的关键 |
| 文件搜索 | ignore、globset | 直接复用 ripgrep 的 crate |
| diff | similar | edit 工具用 |
| 时间 | time | 会话条目 ISO 8601 序列化 |
| id | uuid | 会话 id |
| 路径 | dirs | 配置目录解析 |
| 错误 | thiserror + anyhow | 库用 thiserror，二进制用 anyhow |
| 日志 | tracing + tracing-subscriber | `--verbose` 控制 |
| 终端测试 | portable-pty + vt100 | 断言渲染输出 |

不用 ratatui 的原因：它的模型是"拥有全屏 buffer + 全量重绘"，而聊天模式要把输出交给终端 scrollback，只维护底部活动区。全屏模式留到 MVP 之后，那时再评估 ratatui。

## 6. 质量策略

### 6.1 对拍测试（核心手段）

用 TS 版做 oracle，而不是靠重读实现。

1. SSE 录制：跑 TS 版录下真实响应流，Rust 侧离线解析，断言事件序列与最终消息一致
2. 会话文件互操作：两边交替读写同一会话文件
3. 工具行为：同一输入下比对输出、截断位置、错误信息
4. usage 一致：同一请求的 token 统计必须完全相等

### 6.2 终端渲染测试

用 `portable-pty` 起伪终端运行聊天客户端，用 `vt100` 解析输出并断言屏幕内容。这样 CJK 宽度、换行、resize、中断恢复都能自动化测试。

必须覆盖：中文与 emoji 混排、超长行、终端宽度变化、流式输出期间输入、Ctrl+C 中断后状态、故意 panic 后终端恢复。

### 6.3 端到端测试

准备一个小的 fixture 仓库，用 print 模式跑真实任务（读文件 → 改文件 → 跑测试），断言结果。这类测试需要 API key，标记为可选并在 CI 中跳过。

### 6.4 每阶段 Definition of Done

- `cargo fmt`、`cargo clippy -D warnings`、`cargo test` 全绿
- 该阶段验收用例全部有自动化测试
- 与 TS 版的差异（如有）记录在差异清单里

## 7. 风险与对策

| 风险 | 影响 | 对策 |
|---|---|---|
| 协议抽象被单一 provider 带偏 | 加第二个 provider 时大改 | M3 后尽早插入 Anthropic 骨架验证 |
| CJK/宽字符渲染错位 | 中文用户体验受损 | 从第一天用 unicode-width，写专项测试 |
| SSE 解析各家差异 | 流式输出异常 | 手写解析器 + 录制 fixture 覆盖边界 |
| 工具行为与 TS 版偏差（尤其 edit） | 模型改错文件、反复失败 | 逐用例对拍，把 TS 版当 oracle |
| 上下文 token 估算不准 | 溢出未被拦截 | 估算只做预警，以真实 usage 校准 |
| abort 未传播到 HTTP 与子进程 | 中断后仍有后台活动 | `CancellationToken` 贯穿，测试中断延迟 |
| 终端状态未恢复 | 用户终端留在 raw mode | Drop + panic hook，故意注入 panic 测试 |
| 成本计算浮点累计误差 | 统计数字漂移 | f64 累加，展示时统一舍入，加对拍测试 |
| 会话文件并发写损坏 | 会话丢失 | 文件锁 + 原子追加 |
| 编译时间随 crate 增长 | 迭代变慢 | 保持 crate 边界，避免单 crate 过大 |

## 8. MVP 验收清单

- [ ] `pi-rs -p "..."` 能完成"读文件 → 改文件 → 跑测试"闭环，退出码正确
- [ ] 交互式聊天可正常输入、流式输出、随时中断
- [ ] 支持 OpenAI 兼容端点与 Anthropic 两个协议族
- [ ] Rust 写的会话能被 TS 版打开，反向也能
- [ ] 同一请求的 usage 与 TS 版完全一致
- [ ] 中文与 emoji 混排渲染正确
- [ ] 终端 resize 后无残影
- [ ] 中断后终端状态正常，会话包含已完成部分
- [ ] agent 运行中提交的消息被正确排队送达
- [ ] 上下文溢出给出明确错误，不崩溃
- [ ] 工具失败时 agent 能自我纠正继续

## 9. MVP 之后的第一批工作

按优先级：

1. 上下文压缩与分支摘要（长会话可用性）
2. JSON 事件流模式（自动化集成）
3. 全屏模式 + markdown 渲染 + 语法高亮
4. 树形分支、fork、clone、会话选择器
5. 扩展系统（先定 WASM 还是进程外方案）
6. skills、prompt templates、themes
7. RPC 模式与客户端库

## 10. 立即开始

第一周按这个顺序执行：

1. 建 workspace 与 7 个 crate 的空骨架，跑通 `cargo test`
2. 写 fixture 生成脚本，录一段 TS 版的真实 SSE 响应存进 `fixtures/`
3. 写对拍 runner，让它跑绿一个空用例
4. 定义 `ContentBlock`、`Message`、`Usage` 三个类型，加上 round-trip 测试

第 1 周末应能证明"Rust 能正确解析 TS 版的消息与流式响应"。这是整个计划里最重要的一次早期验证。
