# AHEIAPI — 对接你的 New API 中转（成品版）

本仓库基于 codex-rs 源码，内置 `codex login --newapi` 流程，登录你自己的
New API (newapi) 中转后即可创建/选择密钥，并直接下发给 `codex` 使用。

## 功能

- `codex login --newapi`：交互式登录
  - 输入中转地址（不带 `/v1`）
  - 输入用户名 / 密码
  - 列出已有密钥，可选择已有 key，或新建一个
  - 输入模型 ID（默认 `gpt-4o`）
- `codex login --newapi --newapi-base-url <URL>`：一次性提供中转地址
- `codex login --newapi-with-token`：跳过登录，直接粘贴访问令牌
- 登录成功后自动写入 `config.toml`：
  ```toml
  [model_providers.newapi]
  name = "New API"
  base_url = "<base>/v1"
  wire_api = "responses"
  experimental_bearer_token = "<key>"

  model = "gpt-4o"
  model_provider = "newapi"
  ```

## 使用

```bash
# 登录并选择/创建密钥
codex login --newapi

# 或直接用已有 token
codex login --newapi-with-token

# 登录完成后，直接运行
codex
```

## 前置注意

- 你的 New API 中转必须支持 OpenAI Responses API（`/v1/responses`）。
  当前 Codex 仅使用 `wire_api = "responses"`；如果中转只支持
  `/v1/chat/completions`，则需要在你的 New API 侧开启/确认 Responses 支持。
- 登录接口按 New API `/api/user/login`、`/api/token/`（列表/创建）封装。
  token 列表兼容 `data.items` 与 `data: [...]` 两种返回形式。
- 如果登录/取列表返回非 2xx 或 `success=false`，会打印后端 message，
  方便定位中转版本兼容问题。

## 本地源码构建

需要安装 Rust 工具链（rustup + stable）。在仓库根目录执行：

```bash
# 检查能否编译
cd codex-rs
cargo check -p codex-cli

# 格式化（可选但推荐）
just fmt

# 跑 cli 测试
just test -p codex-cli
```

## 打包成 Windows / macOS 成品

项目自带 `assemble-codex-package` 打包器（`scripts/build_codex_package.py`），
会生成自包含目录 + 压缩包，内含入口二进制、code-mode-host、平台辅助工具等。
压缩包解压后运行其中的 `codex[.exe]`，执行 `codex login --newapi` 即可开箱即用。

> 构建要求：机器需安装 Rust（rustup + stable/对应 target）与 Python 3，
> 且首次构建需联网拉取 crates 与 Codex V8 release。

### 一键打包脚本（推荐）

Windows（在仓库根目录，PowerShell）：

```powershell
# 只出 Windows x64 包，并把你自己的中转地址烧进默认值
.\scripts\build_cnapi_packages.ps1 -NewApiBaseUrl "https://new.ahei.asia" -Targets x86_64-pc-windows-msvc
```

macOS / Linux（需在对应系统上运行，或使用 CI 交叉构建）：

```bash
# macOS Apple Silicon
NEWAPI_BASE_URL="https://new.ahei.asia" ./scripts/build_cnapi_packages.sh aarch64-apple-darwin
# Intel Mac
NEWAPI_BASE_URL="https://new.ahei.asia" ./scripts/build_cnapi_packages.sh x86_64-apple-darwin
```

### 手动打包命令

Windows（在 Windows + MSVC 目标上运行）：

```powershell
cd codex-rs
just assemble-codex-package ^
  --variant codex ^
  --target x86_64-pc-windows-msvc ^
  --cargo-profile release ^
  --archive-output ..\dist\codex-win-x64.zip ^
  --force
```

macOS（需在 macOS 上运行）：

```bash
cd codex-rs
just assemble-codex-package   --variant codex   --target aarch64-apple-darwin   --cargo-profile release   --archive-output ../dist/codex-mac-aarch64.tar.gz   --force
# Intel Mac:
just assemble-codex-package   --variant codex   --target x86_64-apple-darwin   --cargo-profile release   --archive-output ../dist/codex-mac-x64.tar.gz   --force
```

> 说明：Linux/Darwin 目标构建时打包器会下载并校验对应的 Codex V8 release，首次构建需联网。
> Windows 目标走 Cargo 的 MSVC 产物路径。若想跳过下载直接指定本地二进制可用
> `--entrypoint-bin`、`--code-mode-host-bin` 等。
> 解压后运行包里的 `codex[.exe]`，执行 `codex login --newapi` 即可开箱即用。

### 烧录默认中转地址（可选用）

编译时读取环境变量 `NEWAPI_BASE_URL`，会作为 `codex login --newapi` 的默认中转地址：
用户登录时只需输入用户名/密码，地址自动填好。

```bash
# 构建前设置即可（PowerShell 用 $env:）
export NEWAPI_BASE_URL="https://new.ahei.asia"

# 或使用上面的一键脚本 -NewApiBaseUrl 参数
```

若未设置，登录时仍会交互式询问中转地址。
