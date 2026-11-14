<#
.SYNOPSIS
  Creates the Play upload keystore for ONE app and (optionally) stores it as GitHub secrets.

.DESCRIPTION
  Run it on your own computer, never in a chat or CI:

      powershell -ExecutionPolicy Bypass -File apps\gridcraft-android\make-upload-key.ps1 -App gridcraft

  It writes <App>-upload.jks and <App>-upload-secrets.txt to %USERPROFILE%\play-keys\ (back both up,
  and never commit them or paste them anywhere), then, if the GitHub CLI is installed and logged in,
  sets the four secrets on sutriadi/<App>. Losing the key is recoverable (Play lets you reset an
  upload key); leaking it is not safe, so keep it private.
#>
param(
    [Parameter(Mandatory = $true)][ValidateSet('gridcraft', 'deckcraft', 'wordcraft')][string]$App,
    [string]$Owner = 'sutriadi'
)
$ErrorActionPreference = 'Stop'

# keytool: PATH, JAVA_HOME, or Android Studio's bundled JDK.
$keytool = (Get-Command keytool -ErrorAction SilentlyContinue).Source
if (-not $keytool -and $env:JAVA_HOME) { $keytool = Join-Path $env:JAVA_HOME 'bin\keytool.exe' }
if (-not $keytool -or -not (Test-Path $keytool)) {
    $keytool = @("$env:ProgramFiles\Android\Android Studio\jbr\bin\keytool.exe", "$env:LOCALAPPDATA\Programs\Android Studio\jbr\bin\keytool.exe") |
        Where-Object { Test-Path $_ } | Select-Object -First 1
}
if (-not $keytool) { throw 'keytool not found. Install a JDK 17 (https://adoptium.net) or Android Studio.' }

$dir = Join-Path $env:USERPROFILE 'play-keys'
New-Item -ItemType Directory -Force $dir | Out-Null
$jks = Join-Path $dir "$App-upload.jks"
if (Test-Path $jks) { throw "$jks already exists. Move it away first; never overwrite a key you have uploaded to Play." }

function New-Secret([int]$n) { -join ((48..57) + (65..90) + (97..122) | Get-Random -Count $n | ForEach-Object { [char]$_ }) }
$pass = New-Secret 24
$alias = 'upload'
& $keytool -genkeypair -v -keystore $jks -storetype PKCS12 -alias $alias -keyalg RSA -keysize 2048 -validity 10000 `
    -storepass $pass -keypass $pass -dname "CN=$App, O=$Owner, C=ID"
if ($LASTEXITCODE -ne 0) { throw 'keytool failed' }

$b64 = [Convert]::ToBase64String([IO.File]::ReadAllBytes($jks))
$secrets = [ordered]@{
    ANDROID_KEYSTORE_BASE64   = $b64
    ANDROID_KEYSTORE_PASSWORD = $pass
    ANDROID_KEY_ALIAS         = $alias
    ANDROID_KEY_PASSWORD      = $pass
}
$txt = Join-Path $dir "$App-upload-secrets.txt"
($secrets.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) | Set-Content $txt

if (Get-Command gh -ErrorAction SilentlyContinue) {
    foreach ($kv in $secrets.GetEnumerator()) { $kv.Value | gh secret set $kv.Key --repo "$Owner/$App" }
    Write-Host "Secrets set on $Owner/$App." -ForegroundColor Green
} else {
    Write-Host "GitHub CLI not found: add the four secrets by hand at https://github.com/$Owner/$App/settings/secrets/actions" -ForegroundColor Yellow
    Write-Host "Values are in $txt (open it locally; do not share it)."
}
Write-Host "Keystore: $jks`nBack up $dir somewhere private." -ForegroundColor Cyan
