[CmdletBinding()]
param(
    [switch]$CheckOnly
)

$Host.UI.RawUI.WindowTitle = "LocalBridge Community Build - 一键助手"
$RepoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $RepoRoot

function Show-Banner {
    Clear-Host
    Write-Host "=================================================================" -ForegroundColor Cyan
    Write-Host "       LocalBridge Community Build - 一键检测 / 安装 / 使用助手  " -ForegroundColor Cyan
    Write-Host "               版本: 0.1.5-community.1 (开源透明社区版)          " -ForegroundColor Cyan
    Write-Host "=================================================================" -ForegroundColor Cyan
    Write-Host ""
}

function Run-Doctor {
    Write-Host "【1. 系统与工具链环境检测】" -ForegroundColor Yellow

    # Node.js
    $nodeCmd = Get-Command "node" -ErrorAction SilentlyContinue
    if ($nodeCmd) {
        $nodeVer = & node --version
        Write-Host "  [√] Node.js   : 已安装 ($nodeVer)" -ForegroundColor Green
    } else {
        Write-Host "  [×] Node.js   : 未检测到！请安装 Node.js 20+ (https://nodejs.org)" -ForegroundColor Red
    }

    # NPM
    $npmCmd = Get-Command "npm" -ErrorAction SilentlyContinue
    if ($npmCmd) {
        $npmVer = & npm --version
        Write-Host "  [√] npm       : 已就绪 (v$npmVer)" -ForegroundColor Green
    } else {
        Write-Host "  [×] npm       : 未检测到！" -ForegroundColor Red
    }

    # Python
    $pyCmd = Get-Command "python" -ErrorAction SilentlyContinue
    if ($pyCmd) {
        $pyVer = & python --version
        Write-Host "  [√] Python    : 已就绪 ($pyVer)" -ForegroundColor Green
    } else {
        Write-Host "  [!] Python    : 系统 PATH 未检测到 (本项目已内置独立运行时)" -ForegroundColor DarkYellow
    }

    # Git
    $gitCmd = Get-Command "git" -ErrorAction SilentlyContinue
    if (-not $gitCmd -and (Test-Path "D:\SillyTavern\Git\cmd\git.exe")) {
        $env:PATH = "D:\SillyTavern\Git\cmd;$env:PATH"
        $gitCmd = Get-Command "git" -ErrorAction SilentlyContinue
    }
    if ($gitCmd) {
        $gitVer = (& git --version) -replace "git version ", ""
        Write-Host "  [√] Git       : 已就绪 (v$gitVer)" -ForegroundColor Green
    } else {
        Write-Host "  [!] Git       : 未检测到" -ForegroundColor DarkYellow
    }

    # Go (用于自编译 tunnel-client)
    $goCmd = Get-Command "go" -ErrorAction SilentlyContinue
    if ($goCmd) {
        $goVer = & go version
        Write-Host "  [√] Go 语言   : 已安装 ($goVer)" -ForegroundColor Green
    } else {
        Write-Host "  [-] Go 语言   : 未安装 (仅源码重编译 tunnel-client 时需要，日常使用可跳过)" -ForegroundColor Gray
    }

    # Rust / Cargo (用于本地编译桌面安装包)
    $cargoCmd = Get-Command "cargo" -ErrorAction SilentlyContinue
    if ($cargoCmd) {
        $cargoVer = & cargo --version
        Write-Host "  [√] Rust/Cargo: 已安装 ($cargoVer)" -ForegroundColor Green
    } else {
        Write-Host "  [-] Rust/Cargo: 未安装 (本地打包桌面原生 exe 时需要，可由 GitHub CI 自动编译)" -ForegroundColor Gray
    }

    # 内置运行时校验
    Write-Host ""
    Write-Host "【2. 内置运行时依赖状态】" -ForegroundColor Yellow
    $pyRuntime = Test-Path "runtime\python\python.exe"
    $mcpRuntime = Test-Path "runtime\coding-tools-mcp\coding_tools_mcp\__init__.py"
    $tunnelRuntime = Test-Path "runtime\tunnel-client\tunnel-client.exe"

    if ($pyRuntime -and $mcpRuntime -and $tunnelRuntime) {
        Write-Host "  [√] 内置 Python、MCP 工具包与 Tunnel 客户端均已完整就绪！" -ForegroundColor Green
    } else {
        Write-Host "  [×] 内置组件存在缺失，请检查 runtime 目录！" -ForegroundColor Red
    }

    # 前端依赖
    $nodeModules = Test-Path "node_modules"
    if ($nodeModules) {
        Write-Host "  [√] 前端依赖包已安装 (node_modules 已就绪)" -ForegroundColor Green
    } else {
        Write-Host "  [!] 前端依赖包尚未安装" -ForegroundColor DarkYellow
    }
    Write-Host ""
}

function Auto-Setup {
    Write-Host "正在检查并自动安装缺失的必要依赖..." -ForegroundColor Cyan
    if (-not (Test-Path "node_modules")) {
        Write-Host ">> 正在安装前端依赖包 (npm install)... 请稍候" -ForegroundColor Yellow
        & npm install
        if ($LASTEXITCODE -eq 0) {
            Write-Host ">> 前端依赖安装成功！" -ForegroundColor Green
        } else {
            Write-Host ">> 前端依赖安装遇到问题，请检查网络连接。" -ForegroundColor Red
        }
    } else {
        Write-Host ">> 前端依赖已存在，无需重复安装。" -ForegroundColor Green
    }

    Write-Host ">> 正在校验供应链防篡改哈希 (verify-runtime)..." -ForegroundColor Yellow
    & ".\scripts\verify-runtime.ps1"
}

# 仅检测模式
if ($CheckOnly) {
    Show-Banner
    Run-Doctor
    Write-Host "【检测完成】" -ForegroundColor Cyan
    exit 0
}

# 交互主循环
while ($true) {
    Show-Banner
    Run-Doctor

    Write-Host "=================================================================" -ForegroundColor Cyan
    Write-Host " 请选择你要执行的操作：" -ForegroundColor White
    Write-Host "   [1] 启动前端控制台界面 (推荐：浏览器即开即看，可视化调试)" -ForegroundColor Green
    Write-Host "   [2] 一键执行全套自动化自检 (前端18项测试 + 供应链防伪核验)" -ForegroundColor Yellow
    Write-Host "   [3] 尝试启动 LocalBridge 原生桌面应用 (需已安装 Rust)" -ForegroundColor White
    Write-Host "   [4] 重新安装 / 修复依赖环境" -ForegroundColor White
    Write-Host "   [5] 一键打包前端发布资源 (npm run build)" -ForegroundColor White
    Write-Host "   [6] 安装缺失编译器指南 (Go / Rustup)" -ForegroundColor Gray
    Write-Host "   [0] 退出" -ForegroundColor DarkGray
    Write-Host "=================================================================" -ForegroundColor Cyan

    $choice = ""
    try {
        $choice = (Read-Host "请输入选项数字 [0-6]").Trim()
    } catch {
        break
    }
    if ([string]::IsNullOrWhiteSpace($choice)) {
        Write-Host "已退出助手。" -ForegroundColor Gray
        break
    }

    switch ($choice) {
        "1" {
            Show-Banner
            Write-Host "正在启动 LocalBridge 前端控制台界面..." -ForegroundColor Green
            Write-Host "启动成功后，请在浏览器中访问终端显示的地址（通常是 http://localhost:5173）" -ForegroundColor Yellow
            Write-Host "按 Ctrl + C 可停止运行并返回菜单。`n" -ForegroundColor Gray
            if (-not (Test-Path "node_modules")) { Auto-Setup }
            & npm run dev
            Read-Host "`n按回车键返回主菜单..."
        }
        "2" {
            Show-Banner
            Write-Host "正在运行全套自动化自检流水线..." -ForegroundColor Green
            & ".\scripts\build-community.ps1" -SkipRustBuild
            Read-Host "`n自检完成！按回车键返回主菜单..."
        }
        "3" {
            Show-Banner
            $cargoCmd = Get-Command "cargo" -ErrorAction SilentlyContinue
            if (-not $cargoCmd) {
                Write-Host "未检测到 Rust 编译环境 (Cargo)！" -ForegroundColor Red
                Write-Host "提示：LocalBridge 桌面应用由 Rust 编写。若需在本地编译完整桌面窗口程序，" -ForegroundColor Yellow
                Write-Host "请先访问 https://rustup.rs/ 安装 Rust，并安装 Visual Studio C++ 生成工具。" -ForegroundColor Yellow
                Write-Host "（推荐：若仅需体验或二次开发，使用选项 [1] 即可通过浏览器完整体验）" -ForegroundColor Cyan
            } else {
                Write-Host "正在拉起 LocalBridge 桌面程序..." -ForegroundColor Green
                & ".\start-localbridge.cmd"
            }
            Read-Host "`n按回车键返回主菜单..."
        }
        "4" {
            Show-Banner
            Auto-Setup
            Read-Host "`n依赖修复完毕！按回车键返回主菜单..."
        }
        "5" {
            Show-Banner
            Write-Host "正在编译前端生产版本 (tsc -b && vite build)..." -ForegroundColor Green
            & npm run build
            Read-Host "`n打包完成！产物位于 dist 目录。按回车键返回主菜单..."
        }
        "6" {
            Show-Banner
            Write-Host "【编译器安装指导】" -ForegroundColor Yellow
            Write-Host "1. 安装 Go 语言（用于本地自行编译 OpenAI tunnel-client）："
            Write-Host "   可在 PowerShell 中运行命令：winget install GoLang.Go`n" -ForegroundColor Cyan
            Write-Host "2. 安装 Rust 编译器（用于本地编译桌面原生 exe）："
            Write-Host "   可在 PowerShell 中运行命令：winget install Rustlang.Rustup"
            Write-Host "   或直接访问官网下载：https://rustup.rs/`n" -ForegroundColor Cyan
            Write-Host "3. 无论本地是否安装，项目已包含 GitHub Actions 自动化 CI，推送代码即可在云端全量打包！" -ForegroundColor Green
            Read-Host "`n按回车键返回主菜单..."
        }
        "0" {
            Write-Host "已退出助手。" -ForegroundColor Gray
            exit 0
        }
        default {
            Write-Host "无效输入，请重新选择。" -ForegroundColor Red
            Start-Sleep -Seconds 1
        }
    }
}
