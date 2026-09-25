<#
.SYNOPSIS
    Windows 本地开发测试一键脚本：卸载正式版 → 构建 → 注册 DLL → 起 Server（-SyncUpstream 按需拉产品数据）。
.DESCRIPTION
    封装本地快速验证要做的几步（apps/windows/README.md 的 1-3 步）：
      1) cargo build 出 DLL / Server（默认 debug；-Release 出 release）。
         无证书开发包设 QINGJIAN_UIACCESS=0：Server 不嵌 uiAccess，候选窗在 UWP 宿主里可能被盖住，
         桌面程序不受影响。
      2) 若检测到正式安装版（C:\Program Files\Qingjian\unins000.exe），先静默卸载它——
         否则系统进程加载的是正式版 DLL，开发版的改动看不到。卸载要管理员（弹 UAC）。
      3) regsvr32 注册 TSF DLL（写 HKCR，要管理员：非管理员时弹 UAC 提权跑）。
      4) 后台起 qingjian-server.exe（引擎在这里；DLL 起来后下一键 / 下次聚焦自动重连）。
    产品数据（.qj / .tsv / .qjm）不在 git 里（data/ 整个 gitignore），首次或数据过期时加 -SyncUpstream
    从上游 qingjian-team/qingjian 的 data Release 下载并解包；数据已就位时不重复下载。
    代理：脚本不写死，沿用调用方的 $env:HTTPS_PROXY / $env:HTTP_PROXY（下载上游数据时 curl 自动读）。
    装完在系统「语言 / 输入法」里应能看到「青简」，切到它，在任意输入框敲字；
    DLL 侧日志在 %LOCALAPPDATA%\Qingjian\tsf.<日期>.log（按天，留 7 天）。
    DLL 被进程加载后文件锁着，但 Windows 允许重命名正在运行的映像文件：构建前把旧 DLL
    重命名走（.prev 后缀），cargo 链接新文件到原路径，旧文件留待进程退出后清理。不再需要
    每次手动关浏览器 / 终端 / 注销重登。
.PARAMETER SyncUpstream
    从上游 data Release 重新下载并解包产品数据（data\generated、data\model）。默认关闭。
.PARAMETER Release
    release 构建（默认 debug）。
.PARAMETER NoBuild
    跳过 cargo build（只注册 / 起 Server）。
.PARAMETER NoUninstall
    检测到正式安装版时不自动卸载（默认会卸载，避免正式版 DLL 与开发版冲突）。
.PARAMETER Unregister
    反注册 DLL + 杀 Server（卸载开发装）。
.EXAMPLE
    powershell -ExecutionPolicy Bypass -File apps\windows\dev-build.ps1
    # 数据已就位：debug 构建 + 注册 + 起 Server
.EXAMPLE
    powershell -ExecutionPolicy Bypass -File apps\windows\dev-build.ps1 -SyncUpstream
    # 首次 / 数据过期：先从上游拉数据，再构建 + 注册 + 起
.EXAMPLE
    powershell -ExecutionPolicy Bypass -File apps\windows\dev-build.ps1 -Unregister
    # 卸载：反注册 DLL + 杀 Server
#>
[CmdletBinding()]
param(
    [switch]$SyncUpstream,
    [switch]$Release,
    [switch]$NoBuild,
    [switch]$NoUninstall,
    [switch]$Unregister
)

$ErrorActionPreference = 'Stop'

# 仓库根：本脚本在 apps\windows 下，往上两层是 ime\。
$Repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$DataDir = Join-Path $Repo 'data'

# 正式安装版目录与卸载器。
$InstallDir = "${env:ProgramFiles}\Qingjian"
$Uninstaller = Join-Path $InstallDir 'unins000.exe'

# 上游 data Release（产品数据滚动发布在这里；fork 仓库 origin 上没有）。
$UpstreamRepo = 'qingjian-team/qingjian'
$ReleaseBase = "https://github.com/$UpstreamRepo/releases/download/data"
$DataAssets = @('qingjian-data.tar.gz', 'model.qjm', 'SHA256SUMS')

# 安装包要的随包数据（Server 启动也要这些）。
$RequiredGenerated = @(
    'dict.qj', 'lm.qj', 'glossary-en.qj', 'glossary-ja.qj', 'glossary-zh.qj', 'english.tsv'
)

# 产物路径（按构建配置）。
$Profile = if ($Release) { 'release' } else { 'debug' }
$TsfDll   = Join-Path $Repo "target\$Profile\qingjian_tsf.dll"
$TsfDllDeps = Join-Path $Repo "target\$Profile\deps\qingjian_tsf.dll"
$ServerExe = Join-Path $Repo "target\$Profile\qingjian-server.exe"

function Write-Step([string]$msg) { Write-Host "==> $msg" -ForegroundColor Cyan }
function Write-Ok([string]$msg)   { Write-Host "    $msg" -ForegroundColor Green }
function Write-Warn([string]$msg) { Write-Host "    $msg" -ForegroundColor Yellow }

# 当前是否管理员（regsvr32 写 HKCR、卸载正式版都要管理员）。
$isAdmin = ([Security.Principal.WindowsPrincipal] `
    [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)

# 提权跑一个命令（非管理员时弹 UAC；管理员直接跑）。
function Invoke-Elevated([string]$exe, [string]$argList) {
    if ($isAdmin) {
        & $exe $argList
        return $LASTEXITCODE
    }
    $p = Start-Process -FilePath $exe -ArgumentList $argList -Verb RunAs -Wait -PassThru
    return $p.ExitCode
}

# 列出加载了任意 qingjian_tsf*.dll 的进程，返回 [{PID,Name,DLL}]；不杀任何进程。
function Get-DllHolders {
    $result = @()
    Get-Process | ForEach-Object {
        try {
            $mods = $_.Modules | Where-Object { $_.FileName -like '*qingjian_tsf*' }
            if ($mods) {
                $result += [PSCustomObject]@{ PID = $_.Id; Name = $_.ProcessName; DLL = $mods.FileName }
            }
        } catch {}
    }
    return $result
}

# —— 卸载分支：反注册 DLL + 杀 Server ——
if ($Unregister) {
    Write-Step '反注册 TSF DLL'
    $code = Invoke-Elevated 'regsvr32.exe' "/u /s `"$TsfDll`""
    if ($code -ne 0) { Write-Warn "regsvr32 退出码 $code（未注册过则正常）" }
    Write-Ok '反注册完成'
    Write-Step '停 Server'
    Get-Process -Name qingjian-server -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Write-Ok '完成（卸载开发装）'
    return
}

# 1) 产品数据：--sync-upstream 时从上游拉；否则只检测是否齐全。
Write-Step '产品数据'
$generatedDir = Join-Path $DataDir 'generated'
$modelDir     = Join-Path $DataDir 'model'

if ($SyncUpstream) {
    Write-Host '    从上游 data Release 下载…' -ForegroundColor Cyan
    New-Item -ItemType Directory -Force -Path $DataDir | Out-Null
    Push-Location $DataDir
    try {
        foreach ($name in $DataAssets) {
            $url = "$ReleaseBase/$name"
            Write-Host "    下载 $name" -ForegroundColor DarkGray
            & curl -sL -o $name $url
            if ($LASTEXITCODE -ne 0) { throw "下载 $name 失败（curl 退出码 $LASTEXITCODE）；检查代理 $env:HTTPS_PROXY" }
        }
        # 校验：只校验我们下的那两个（SHA256SUMS 还含 llm-intermediates）。
        $sums = Get-Content (Join-Path $DataDir 'SHA256SUMS')
        foreach ($f in @('qingjian-data.tar.gz', 'model.qjm')) {
            $line = $sums | Where-Object { $_ -like "*$f*" } | Select-Object -First 1
            if (-not $line) { throw "SHA256SUMS 里找不到 $f" }
            $expected = ($line -split '\s+')[0]
            $actual = (Get-FileHash (Join-Path $DataDir $f) -Algorithm SHA256).Hash.ToLower()
            if ($actual -ne $expected) { throw "$f 校验失败：期望 $expected 实际 $actual" }
            Write-Ok "$f 校验通过"
        }
        # 解包数据。
        New-Item -ItemType Directory -Force -Path $generatedDir | Out-Null
        & tar -xzf (Join-Path $DataDir 'qingjian-data.tar.gz') -C $generatedDir 2>$null
        if ($LASTEXITCODE -ne 0) { throw '解包 qingjian-data.tar.gz 失败' }
        # 清掉 mac 打包带进来的 ._ AppleDouble 文件。
        Get-ChildItem -Path $generatedDir -Recurse -Filter '._*' -Force | Remove-Item -Force
        # 模型就位。
        New-Item -ItemType Directory -Force -Path $modelDir | Out-Null
        Move-Item (Join-Path $DataDir 'model.qjm') (Join-Path $modelDir 'model.qjm') -Force
        Write-Ok '数据已解到 data\generated 与 data\model'
    } finally { Pop-Location }
} else {
    $missing = @()
    foreach ($f in $RequiredGenerated) {
        if (-not (Test-Path (Join-Path $generatedDir $f))) { $missing += "data\generated\$f" }
    }
    if (-not (Test-Path (Join-Path $modelDir 'model.qjm'))) { $missing += 'data\model\model.qjm' }
    if (-not (Test-Path (Join-Path $generatedDir 'dicts')) -or
        (Get-ChildItem (Join-Path $generatedDir 'dicts') -Filter *.qj -ErrorAction SilentlyContinue).Count -eq 0) {
        $missing += 'data\generated\dicts\*.qj'
    }
    if ($missing.Count -gt 0) {
        Write-Warn "缺产品数据：$($missing -join ', ')"
        throw '产品数据未就位：加 -SyncUpstream 从上游拉取（data/ 整个 gitignore，不在 git 里）'
    }
    Write-Ok '数据已就位（跳过下载；要刷新加 -SyncUpstream）'
}

# 2) 检测并卸载正式安装版（避免正式版 DLL 与开发版冲突）。
if (-not $NoUninstall -and (Test-Path $Uninstaller)) {
    Write-Step '检测到正式安装版，先卸载'
    Write-Host "    $InstallDir" -ForegroundColor DarkGray
    # 卸载器会 regsvr32 /u 正式版 DLL、删 Program Files 文件；要管理员。
    $code = Invoke-Elevated $Uninstaller '/VERYSILENT /NORESTART /SUPPRESSMSGBOXES'
    if ($code -ne 0) { throw "卸载正式版失败（退出码 $code）；可加 -NoUninstall 跳过，但正式版 DLL 会与开发版冲突" }
    Write-Ok '正式版已卸载'
    # 卸载后仍可能有进程加载着旧 DLL（文件已删但内存映像还在），列出供用户处理。
    $holders = Get-DllHolders
    if ($holders) {
        Write-Warn '以下进程仍加载旧 DLL（需重启它们 / 注销重登才能彻底释放）：'
        $holders | Format-Table -AutoSize | Out-Host
    }
} elseif ($NoUninstall -and (Test-Path $Uninstaller)) {
    Write-Warn '检测到正式安装版但跳过卸载（-NoUninstall）：系统进程可能加载正式版 DLL，开发版改动看不到'
}

# 3) 构建 DLL / Server。构建前把被进程加载的旧 DLL 重命名走（Windows 允许重命名正在运行的
#    映像文件，进程内存里已映射的代码不受影响）：cargo 链接新文件到原路径，不再因文件锁失败。
#    旧 .prev 文件能删就删，删不掉（仍有进程持有旧句柄）留着下次清理。
if (-not $NoBuild) {
    Write-Step "cargo build $Profile（DLL + Server）"
    # 清理上轮留下的 .prev（能删的删，删不掉的忽略）。
    foreach ($prev in @("$TsfDll.prev", "$TsfDllDeps.prev")) {
        if (Test-Path $prev) {
            Remove-Item $prev -Force -ErrorAction SilentlyContinue
        }
    }
    # 旧 DLL 被进程加载时重命名走，空出原路径给 cargo 链接新文件。cargo 先把 cdylib 链接到
    # deps\ 下再复制到上层，两个路径都要腾空，否则 deps\ 下的文件锁会让链接器 LNK1104。
    foreach ($dll in @($TsfDll, $TsfDllDeps)) {
        if (-not (Test-Path $dll)) { continue }
        try {
            Rename-Item $dll "$dll.prev" -Force -ErrorAction Stop
            if (Test-Path "$dll.prev") {
                Write-Ok "旧 DLL 已重命名走：$dll（进程仍持有旧映像，新构建写到原路径）"
            }
        } catch {
            # 重命名也失败（罕见，可能权限）：回退到旧的检测 + 报错流程。
            $holders = Get-DllHolders | Where-Object { $_.DLL -eq $dll }
            if ($holders) {
                Write-Warn "DLL 被占用且无法重命名，构建会失败："
                $holders | Format-Table -AutoSize | Out-Host
                throw "DLL 被占用且无法重命名：$dll；关闭占用进程或注销重登后重试"
            }
        }
    }
    $env:QINGJIAN_UIACCESS = '0'
    Push-Location $Repo
    try {
        if ($Release) {
            cargo build --release --locked -p qingjian-windows-tsf -p qingjian-windows-server
        } else {
            cargo build --locked -p qingjian-windows-tsf -p qingjian-windows-server
        }
        if ($LASTEXITCODE -ne 0) { throw "cargo build 失败（退出码 $LASTEXITCODE）" }
    } finally { Pop-Location }
    Write-Ok '构建完成'
} else {
    Write-Step '跳过 cargo build（-NoBuild）'
}

# 缺产物就早报错。
foreach ($p in @($TsfDll, $ServerExe)) {
    if (-not (Test-Path $p)) { throw "缺产物 $p，先跑一次不带 -NoBuild 的构建" }
}

# 4) 注册 TSF DLL（写 HKCR，要管理员：非管理员时弹 UAC 提权）。
Write-Step '注册 TSF DLL'
$code = Invoke-Elevated 'regsvr32.exe' "/s `"$TsfDll`""
if ($code -ne 0) { throw "regsvr32 失败（退出码 $code）" }
Write-Ok "已注册 $TsfDll"

# 5) 起 Server（后台，不阻塞；先停旧的避免管道被占，Server 自身也有单实例保护）。
Write-Step '起 Server'
Get-Process -Name qingjian-server -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
$server = Start-Process -FilePath $ServerExe -WorkingDirectory $Repo -WindowStyle Minimized -PassThru
Write-Ok "Server 已起（PID $($server.Id)）；DLL 起来后下一键 / 下次聚焦自动重连"

Write-Host ''
Write-Host '完成。在系统「语言 / 输入法」里切到「青简」，在任意输入框敲字。' -ForegroundColor Green
Write-Host "DLL 日志：%LOCALAPPDATA%\Qingjian\tsf.<日期>.log" -ForegroundColor DarkGray
Write-Host '卸载：加 -Unregister（反注册 DLL + 杀 Server）' -ForegroundColor DarkGray
Write-Host '注意：已开着的应用进程仍用旧 DLL，重启它们或注销重登后才加载新 DLL。' -ForegroundColor DarkGray
