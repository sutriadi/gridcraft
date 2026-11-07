<#
.SYNOPSIS
  Checks whether this Windows machine can build GridCraft for Android.

.DESCRIPTION
  Read-only: it installs and changes nothing. Run it from PowerShell (5.1 or 7):

      powershell -ExecutionPolicy Bypass -File apps\gridcraft-android\check-env.ps1

  Exit code 0 = every required item is present, 1 = something required is missing.
#>

$ErrorActionPreference = 'Continue'
$script:failed = 0
$script:warned = 0

function Show([string]$status, [string]$name, [string]$detail, [string]$fix) {
    $color = switch ($status) { 'OK' { 'Green' } 'WARN' { 'Yellow' } default { 'Red' } }
    Write-Host ("[{0,-4}] {1}" -f $status, $name) -ForegroundColor $color -NoNewline
    if ($detail) { Write-Host ("  {0}" -f $detail) } else { Write-Host '' }
    if ($fix -and $status -ne 'OK') { Write-Host ("       -> {0}" -f $fix) -ForegroundColor DarkGray }
    if ($status -eq 'FAIL') { $script:failed++ }
    if ($status -eq 'WARN') { $script:warned++ }
}

function Run([string]$cmd, [string[]]$arguments) {
    # Returns the combined output, or $null when the command does not exist.
    if (-not (Get-Command $cmd -ErrorAction SilentlyContinue)) { return $null }
    try {
        $out = & $cmd @arguments 2>&1 | ForEach-Object { "$_" }
        return ($out -join "`n").Trim()
    } catch { return $null }
}

function ParseVersion([string]$text) {
    if ($text -match '(\d+)\.(\d+)(?:\.(\d+))?') {
        return [version]("{0}.{1}.{2}" -f $Matches[1], $Matches[2], $(if ($Matches[3]) { $Matches[3] } else { '0' }))
    }
    return $null
}

Write-Host ''
Write-Host 'GridCraft for Android - build environment check' -ForegroundColor Cyan
Write-Host ('Windows {0}, PowerShell {1}' -f [Environment]::OSVersion.Version, $PSVersionTable.PSVersion)
Write-Host ''

# --- Git ---------------------------------------------------------------------------------------
$git = Run 'git' @('--version')
if ($git) { Show 'OK' 'Git' $git '' } else { Show 'FAIL' 'Git' 'not found' 'Install from https://git-scm.com/download/win' }

# --- Rust --------------------------------------------------------------------------------------
$rustc = Run 'rustc' @('--version')
if (-not $rustc) {
    Show 'FAIL' 'Rust (rustc)' 'not found' 'Install rustup from https://rustup.rs, then reopen the terminal'
} else {
    $v = ParseVersion $rustc
    if ($v -and $v -ge [version]'1.95.0') { Show 'OK' 'Rust (rustc >= 1.95)' $rustc '' }
    else { Show 'FAIL' 'Rust (rustc >= 1.95)' $rustc 'Run: rustup update stable' }
}

$cargo = Run 'cargo' @('--version')
if ($cargo) { Show 'OK' 'Cargo' $cargo '' } else { Show 'FAIL' 'Cargo' 'not found' 'Comes with rustup' }

# --- Rust Android targets ----------------------------------------------------------------------
$installed = Run 'rustup' @('target', 'list', '--installed')
if ($null -eq $installed) {
    Show 'FAIL' 'rustup' 'not found' 'Install rustup from https://rustup.rs'
} else {
    foreach ($t in @('aarch64-linux-android', 'x86_64-linux-android')) {
        if ($installed -match [regex]::Escape($t)) { Show 'OK' "Rust target $t" '' '' }
        else { Show 'FAIL' "Rust target $t" 'missing' "Run: rustup target add $t" }
    }
}

# --- cargo-ndk ---------------------------------------------------------------------------------
$ndkTool = Run 'cargo' @('ndk', '--version')
if ($ndkTool -and $ndkTool -notmatch 'no such (command|subcommand)|error') { Show 'OK' 'cargo-ndk' $ndkTool '' }
else { Show 'FAIL' 'cargo-ndk' 'missing' 'Run: cargo install cargo-ndk' }

# --- C++ build tools (a linker for host tools such as cargo-ndk itself) ------------------------
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$msvc = $null
if (Test-Path $vswhere) {
    $msvc = (& $vswhere -products '*' -requires 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64' -property installationPath 2>$null | Select-Object -First 1)
}
if ($msvc) { Show 'OK' 'Visual Studio C++ Build Tools' $msvc '' }
else { Show 'FAIL' 'Visual Studio C++ Build Tools' 'not found' 'Install "Build Tools for Visual Studio" with the "Desktop development with C++" workload (needed by rustup on Windows)' }

# --- Java --------------------------------------------------------------------------------------
$java = Run 'java' @('-version')
if (-not $java) {
    Show 'FAIL' 'JDK 17+' 'java not found' 'Install a JDK 17 (e.g. Temurin: https://adoptium.net) and set JAVA_HOME'
} else {
    $jv = ParseVersion (($java -split "`n")[0])
    if ($jv -and $jv.Major -ge 17) { Show 'OK' 'JDK 17+' (($java -split "`n")[0]) '' }
    else { Show 'FAIL' 'JDK 17+' (($java -split "`n")[0]) 'Android Gradle Plugin 8.7 needs JDK 17 or newer' }
}

# --- Android SDK / NDK -------------------------------------------------------------------------
$sdk = if ($env:ANDROID_HOME) { $env:ANDROID_HOME } elseif ($env:ANDROID_SDK_ROOT) { $env:ANDROID_SDK_ROOT } else { $null }
if (-not $sdk -or -not (Test-Path $sdk)) {
    Show 'FAIL' 'Android SDK' 'ANDROID_HOME is not set (or the folder is missing)' 'Install Android Studio (or the command-line tools) and set ANDROID_HOME, usually %LOCALAPPDATA%\Android\Sdk'
} else {
    Show 'OK' 'Android SDK' $sdk ''
    if (Test-Path (Join-Path $sdk 'platforms\android-35')) { Show 'OK' 'SDK platform android-35' '' '' }
    else { Show 'FAIL' 'SDK platform android-35' 'missing' 'SDK Manager > SDK Platforms > Android 15 (API 35)' }
    if (Test-Path (Join-Path $sdk 'build-tools')) {
        $bt = Get-ChildItem (Join-Path $sdk 'build-tools') -Directory -ErrorAction SilentlyContinue | Sort-Object Name | Select-Object -Last 1
        if ($bt) { Show 'OK' 'SDK build-tools' $bt.Name '' } else { Show 'FAIL' 'SDK build-tools' 'empty' 'SDK Manager > SDK Tools > Android SDK Build-Tools' }
    } else { Show 'FAIL' 'SDK build-tools' 'missing' 'SDK Manager > SDK Tools > Android SDK Build-Tools' }

    $ndk = $env:ANDROID_NDK_HOME
    if (-not $ndk -and (Test-Path (Join-Path $sdk 'ndk'))) {
        $latest = Get-ChildItem (Join-Path $sdk 'ndk') -Directory -ErrorAction SilentlyContinue | Sort-Object Name | Select-Object -Last 1
        if ($latest) { $ndk = $latest.FullName }
    }
    if ($ndk -and (Test-Path $ndk)) {
        if ($env:ANDROID_NDK_HOME) { Show 'OK' 'Android NDK' $ndk '' }
        else { Show 'WARN' 'Android NDK' "$ndk (found, but ANDROID_NDK_HOME is not set)" ('Run: setx ANDROID_NDK_HOME "{0}"  (then reopen the terminal)' -f $ndk) }
    } else {
        Show 'FAIL' 'Android NDK' 'not found' 'SDK Manager > SDK Tools > NDK (Side by side), then set ANDROID_NDK_HOME'
    }
}

# --- Gradle and bash (build-android.sh is a bash script) ---------------------------------------
$gradle = Run 'gradle' @('--version')
if ($gradle) { Show 'OK' 'Gradle' (($gradle -split "`n" | Where-Object { $_ -match '^Gradle' } | Select-Object -First 1)) '' }
else { Show 'WARN' 'Gradle' 'not on PATH' 'Install Gradle 8.x, or build from Android Studio, which brings its own' }

$bash = Run 'bash' @('--version')
if ($bash) { Show 'OK' 'bash (for build-android.sh)' (($bash -split "`n")[0]) '' }
else { Show 'WARN' 'bash (for build-android.sh)' 'not found' 'Use Git Bash (installed with Git for Windows) or WSL to run build-android.sh' }

# --- Device tools and disk ---------------------------------------------------------------------
$adb = Run 'adb' @('version')
if ($adb) { Show 'OK' 'adb (optional)' (($adb -split "`n")[0]) '' }
else { Show 'WARN' 'adb (optional)' 'not on PATH' 'Needed only to install the APK from the terminal: <sdk>\platform-tools' }

try {
    $drive = (Get-Item (Get-Location)).PSDrive
    $freeGb = [math]::Round($drive.Free / 1GB, 1)
    if ($freeGb -ge 20) { Show 'OK' 'Free disk space' ("{0} GB on {1}:" -f $freeGb, $drive.Name) '' }
    else { Show 'WARN' 'Free disk space' ("{0} GB on {1}:" -f $freeGb, $drive.Name) 'A first build needs roughly 10-20 GB (Rust target folder, Gradle, NDK)' }
} catch { }

# --- Summary -----------------------------------------------------------------------------------
Write-Host ''
if ($script:failed -eq 0) {
    Write-Host ("Ready: all required items are present ({0} warning(s))." -f $script:warned) -ForegroundColor Green
    Write-Host 'Next: bash apps/gridcraft-android/build-android.sh --apk   (from Git Bash or WSL)'
    exit 0
} else {
    Write-Host ("Not ready: {0} required item(s) missing, {1} warning(s). Fix the FAIL lines above and run this again." -f $script:failed, $script:warned) -ForegroundColor Red
    exit 1
}
