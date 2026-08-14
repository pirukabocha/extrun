<#
.SYNOPSIS
    GitHub のリリースページに上げる zip を dist\ に作る。

.DESCRIPTION
    テスト → リリースビルド → 配布物の組み立て → zip → SHA256 の順に実行する。
    バージョンは Cargo.toml から読むので、このスクリプトには書かない。

    できあがるもの:
        dist\extrun-<version>-win-x64.zip
        dist\extrun-<version>-win-x64.zip.sha256

    zip の中身（extrun-<version>\ の下に入る）:
        extrun.exe
        readme.txt                 packaging\readme.txt（配布専用。README.md とは別物）
        extrun-config.sample.txt   extrun-config.txt をリネームしたもの
        extrun-config-format.md
        extrun-recipes.md          外部アプリを使った設定例集
        CHANGELOG.md               更新内容（利用者向けの区分で書く。内部の変更は載せない）
        registry\extrun-add.reg    右クリックメニューへの登録（UTF-16 LE に変換）
        registry\extrun-remove.reg 同じく解除
        LICENSE

    設定ファイルを extrun-config.txt のまま入れないのは、更新版を同じ
    フォルダに展開したユーザーの設定を上書きで消さないため。

.PARAMETER SkipTests
    テストを飛ばす。動作確認用で、実際のリリースでは使わないこと。
#>
[CmdletBinding()]
param([switch]$SkipTests)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$Root  = Split-Path -Parent $PSScriptRoot
$Dist  = Join-Path $Root 'dist'

function Invoke-Step {
    param([string]$Title, [scriptblock]$Body)
    Write-Host "==> $Title" -ForegroundColor Cyan
    & $Body
    if ($LASTEXITCODE -ne 0) { throw "$Title に失敗しました (exit $LASTEXITCODE)" }
}

# UTF-8 のテキストを CRLF に直して書き出す。
# メモ帳で開かれる前提のファイルなので、改行は CRLF、BOM ありで揃える。
# （extrun.exe は BOM の有無も CRLF/LF も問わない）
function Copy-AsWindowsText {
    param([string]$From, [string]$To, [bool]$Bom)
    $text = [System.IO.File]::ReadAllText($From)
    $text = $text -replace "`r`n", "`n" -replace "`n", "`r`n"
    [System.IO.File]::WriteAllText($To, $text, [System.Text.UTF8Encoding]::new($Bom))
}

# .reg は UTF-16 LE + BOM で書き出す。
# regedit は BOM で文字コードを判定し、BOM が無ければ ANSI と見なすので、
# UTF-8 のまま入れるとメニュー名の日本語が文字化けする（あるいは
# 先頭の署名行を読めずインポートに失敗する）。
#
# 通常は .gitattributes の working-tree-encoding が作業ツリーの時点で
# UTF-16 にしてくれるので、ここは実質そのままの書き写しになる。Git を
# 経由せずに配布物を組む場合の保険として変換を残してある
# （ReadAllText は BOM を見て読むので、入力がどちらでも通る）。
function Copy-AsRegFile {
    param([string]$From, [string]$To)
    $text = [System.IO.File]::ReadAllText($From)
    $text = $text -replace "`r`n", "`n" -replace "`n", "`r`n"
    [System.IO.File]::WriteAllText($To, $text, [System.Text.UnicodeEncoding]::new($false, $true))
}

# --- バージョンを Cargo.toml から取得 --------------------------------
Write-Host "==> バージョンを取得" -ForegroundColor Cyan
Push-Location $Root
try {
    $meta = cargo metadata --no-deps --format-version 1 | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'cargo metadata に失敗しました' }
    $pkg = $meta.packages | Where-Object { $_.name -eq 'extrun' }
    if (-not $pkg) { throw 'Cargo.toml に extrun パッケージが見つかりません' }
    $version = $pkg.version
} finally { Pop-Location }
Write-Host "    extrun $version"

# readme.txt は見出しにバージョンを書いているので、Cargo.toml とずれていたら止める
$readmeSrc = Join-Path $Root 'packaging\readme.txt'
$readmeHead = [System.IO.File]::ReadAllText($readmeSrc)
if ($readmeHead -notmatch [regex]::Escape("ExtRun ver. $version")) {
    throw "packaging\readme.txt の見出しが ExtRun ver. $version になっていません（Cargo.toml と揃えてください）"
}

$stem    = "extrun-$version-win-x64"
$Staging = Join-Path $Dist $stem
$Payload = Join-Path $Staging "extrun-$version"
$ZipPath = Join-Path $Dist "$stem.zip"

# --- テストとビルド ---------------------------------------------------
Push-Location $Root
try {
    if (-not $SkipTests) {
        # 設定ファイルの書式エラーもここで検出される
        # （menu.rs のテストが extrun-config.txt をパースするため）
        Invoke-Step 'テスト' { cargo test --quiet }
    } else {
        Write-Host '==> テストを飛ばしました' -ForegroundColor Yellow
    }
    Invoke-Step 'リリースビルド' { cargo build --release --quiet }
} finally { Pop-Location }

$exe = Join-Path $Root 'target\release\extrun.exe'
if (-not (Test-Path -LiteralPath $exe)) { throw "ビルド結果が見つかりません: $exe" }

# --- 配布物の組み立て -------------------------------------------------
Write-Host '==> 配布物を組み立て' -ForegroundColor Cyan
if (Test-Path -LiteralPath $Dist) { Remove-Item -LiteralPath $Dist -Recurse -Force }
$null = New-Item -ItemType Directory -Path $Payload -Force

Copy-Item -LiteralPath $exe -Destination (Join-Path $Payload 'extrun.exe')

Copy-AsWindowsText (Join-Path $Root 'packaging\readme.txt') `
                   (Join-Path $Payload 'readme.txt') $true
Copy-AsWindowsText (Join-Path $Root 'extrun-config.txt') `
                   (Join-Path $Payload 'extrun-config.sample.txt') $true
Copy-AsWindowsText (Join-Path $Root 'extrun-config-format.md') `
                   (Join-Path $Payload 'extrun-config-format.md') $false
Copy-AsWindowsText (Join-Path $Root 'extrun-recipes.md') `
                   (Join-Path $Payload 'extrun-recipes.md') $false
Copy-AsWindowsText (Join-Path $Root 'CHANGELOG.md') `
                   (Join-Path $Payload 'CHANGELOG.md') $false
Copy-AsWindowsText (Join-Path $Root 'LICENSE') `
                   (Join-Path $Payload 'LICENSE') $false

# 右クリック登録用の .reg は子フォルダにまとめる（誤ってダブルクリック
# しにくくする意味もある）
$RegDir = Join-Path $Payload 'registry'
$null = New-Item -ItemType Directory -Path $RegDir -Force
Copy-AsRegFile (Join-Path $Root 'packaging\registry\extrun-add.reg') `
               (Join-Path $RegDir 'extrun-add.reg')
Copy-AsRegFile (Join-Path $Root 'packaging\registry\extrun-remove.reg') `
               (Join-Path $RegDir 'extrun-remove.reg')

# --- zip 化 -----------------------------------------------------------
# Compress-Archive は古い PowerShell だとパス区切りに \ を埋め込むことがあるため、
# .NET の ZipFile を直接使う。
Write-Host '==> zip を作成' -ForegroundColor Cyan
Add-Type -AssemblyName System.IO.Compression.FileSystem
[System.IO.Compression.ZipFile]::CreateFromDirectory(
    $Staging, $ZipPath, [System.IO.Compression.CompressionLevel]::Optimal, $false)
Remove-Item -LiteralPath $Staging -Recurse -Force

# --- SHA256 -----------------------------------------------------------
$hash = (Get-FileHash -LiteralPath $ZipPath -Algorithm SHA256).Hash.ToLower()
"$hash  $stem.zip`r`n" | Set-Content -LiteralPath "$ZipPath.sha256" -NoNewline -Encoding ascii

# --- 結果 -------------------------------------------------------------
Write-Host ''
Write-Host "完成: $ZipPath" -ForegroundColor Green
Write-Host ("       {0:N0} バイト" -f (Get-Item -LiteralPath $ZipPath).Length)
Write-Host "SHA256: $hash"
Write-Host ''
Write-Host '中身:'
[System.IO.Compression.ZipFile]::OpenRead($ZipPath).Entries |
    ForEach-Object { '  {0,10:N0}  {1}' -f $_.Length, $_.FullName }
