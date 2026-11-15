<#
.SYNOPSIS
    Zips the local Faith & Fire world directory, uploads it to Google Drive via OAuth 2.0,
    and prunes older backups beyond the configured retention count.

.DESCRIPTION
    Creates a timestamped .zip archive of ./world (with POSIX forward-slash paths for
    cross-platform Windows/Linux deployment), authenticates to Google Drive via OAuth 2.0
    (Loopback + PKCE) on first run, stores encrypted refresh/access tokens in
    %LOCALAPPDATA%\FaithAndFire\gdrive-oauth.dpapi using Windows DPAPI (CurrentUser),
    uploads the archive into the configured Google Drive folder path, and deletes older
    Faith & Fire backup archives past the retention limit (-Keep, default 10).

.PARAMETER DriveFolder
    Target folder path inside Google Drive "My Drive" (e.g. "FaithAndFire/WorldBackups").
    Missing nested folders are created automatically. When specified, this becomes the
    new saved default in %LOCALAPPDATA%\FaithAndFire\backup-config.json.

.PARAMETER Keep
    Number of newest world backup .zip archives to retain in the target Google Drive folder.
    Pass 0 to disable pruning. When >= 0, this becomes the new saved default. Defaults to 10.

.PARAMETER WorldDir
    Path to the local world directory to back up. Defaults to <repo>\world.

.PARAMETER ClientSecretFile
    Path to a Google Cloud OAuth 2.0 "Desktop app" client secret JSON file. Needed only on
    initial setup (or when using -Reauth without saved credentials).

.PARAMETER ClientId
    Google OAuth 2.0 Client ID (optional alternative to -ClientSecretFile or
    $env:FAITHFIRE_GDRIVE_CLIENT_ID).

.PARAMETER ClientSecret
    Google OAuth 2.0 Client Secret (optional alternative to -ClientSecretFile or
    $env:FAITHFIRE_GDRIVE_CLIENT_SECRET).

.PARAMETER LocalOut
    Optional path where a local copy of the created .zip archive will be written.

.PARAMETER DryRun
    Builds and verifies the .zip archive locally without contacting Google Drive.

.PARAMETER List
    Lists existing Faith & Fire world backups stored in the configured Google Drive folder.

.PARAMETER Reauth
    Forces a fresh browser OAuth 2.0 consent flow even if a refresh token is already stored.

.PARAMETER ClearAuth
    Removes the DPAPI-encrypted OAuth credential file from %LOCALAPPDATA%\FaithAndFire\ and exits.

.EXAMPLE
    .\scripts\backup-world.ps1 -ClientSecretFile "$HOME\Downloads\client_secret.json" -DriveFolder "Backups/FaithAndFire"

.EXAMPLE
    .\scripts\backup-world.ps1

.EXAMPLE
    .\scripts\backup-world.ps1 -DriveFolder "Backups/FaithAndFire" -Keep 15

.EXAMPLE
    .\scripts\backup-world.ps1 -DryRun -LocalOut "$env:TEMP\world-preview.zip"
#>
param(
    [string]$DriveFolder = "",
    [int]$Keep = -1,
    [string]$WorldDir = "",
    [string]$ClientSecretFile = "",
    [string]$ClientId = "",
    [string]$ClientSecret = "",
    [string]$LocalOut = "",
    [switch]$DryRun,
    [switch]$List,
    [switch]$Reauth,
    [switch]$ClearAuth
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

Add-Type -AssemblyName System.Security
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
Add-Type -AssemblyName System.Net.Http

$script:RepoRoot = Split-Path -Parent $PSScriptRoot
$script:DefaultDriveFolder = "FaithAndFire/WorldBackups"
$script:DefaultKeep = 10
$script:DriveScope = "https://www.googleapis.com/auth/drive"
$script:DpapiEntropy = [System.Text.Encoding]::UTF8.GetBytes("FaithAndFire.GoogleDriveBackup.v1")
$script:BackupNameRegex = '^faithfire-world-\d{8}-\d{6}Z?\.zip$'

$localAppData = $env:LOCALAPPDATA
if ([string]::IsNullOrWhiteSpace($localAppData)) {
    $localAppData = [Environment]::GetFolderPath("LocalApplicationData")
}
$script:StateDir = Join-Path $localAppData "FaithAndFire"
$script:ConfigPath = Join-Path $script:StateDir "backup-config.json"
$script:TokenPath = Join-Path $script:StateDir "gdrive-oauth.dpapi"

$script:HttpClient = [System.Net.Http.HttpClient]::new()
$script:HttpClient.Timeout = [TimeSpan]::FromMinutes(15)

function Get-ObjProp {
    param(
        $Obj,
        [string]$Name,
        $Default = $null
    )
    if ($null -eq $Obj) {
        return $Default
    }
    $prop = $Obj.PSObject.Properties[$Name]
    if ($null -ne $prop) {
        return $prop.Value
    }
    return $Default
}

function Ensure-StateDir {
    if (-not (Test-Path $script:StateDir)) {
        New-Item -ItemType Directory -Force -Path $script:StateDir | Out-Null
    }
}

function Load-BackupConfig {
    $driveFolderVal = $script:DefaultDriveFolder
    $keepVal = $script:DefaultKeep

    if (Test-Path $script:ConfigPath) {
        $raw = Get-Content -Path $script:ConfigPath -Raw -Encoding UTF8
        if (-not [string]::IsNullOrWhiteSpace($raw)) {
            $parsed = $raw | ConvertFrom-Json
            $savedFolder = [string](Get-ObjProp $parsed "drive_folder" $driveFolderVal)
            if (-not [string]::IsNullOrWhiteSpace($savedFolder)) {
                $driveFolderVal = $savedFolder
            }
            $savedKeep = Get-ObjProp $parsed "keep" $null
            if ($null -ne $savedKeep) {
                $keepVal = [int]$savedKeep
            }
        }
    }

    return [PSCustomObject]@{
        drive_folder = $driveFolderVal
        keep         = $keepVal
    }
}

function Save-BackupConfig {
    param(
        [string]$Folder,
        [int]$KeepCount
    )
    Ensure-StateDir
    $payload = [PSCustomObject]@{
        drive_folder = $Folder
        keep         = $KeepCount
    } | ConvertTo-Json -Depth 4
    $utf8NoBom = [System.Text.UTF8Encoding]::new($false)
    [System.IO.File]::WriteAllText($script:ConfigPath, $payload + "`n", $utf8NoBom)
}

function Protect-DpapiJson {
    param(
        [Parameter(Mandatory = $true)]$Object,
        [Parameter(Mandatory = $true)][string]$Path
    )
    Ensure-StateDir
    $json = $Object | ConvertTo-Json -Depth 6
    $plainBytes = [System.Text.Encoding]::UTF8.GetBytes($json)
    $cipherBytes = [System.Security.Cryptography.ProtectedData]::Protect(
        $plainBytes,
        $script:DpapiEntropy,
        [System.Security.Cryptography.DataProtectionScope]::CurrentUser
    )
    [System.IO.File]::WriteAllBytes($Path, $cipherBytes)
}

function Unprotect-DpapiJson {
    param([Parameter(Mandatory = $true)][string]$Path)
    if (-not (Test-Path $Path)) {
        return $null
    }
    $cipherBytes = [System.IO.File]::ReadAllBytes($Path)
    if ($cipherBytes.Length -eq 0) {
        return $null
    }
    try {
        $plainBytes = [System.Security.Cryptography.ProtectedData]::Unprotect(
            $cipherBytes,
            $script:DpapiEntropy,
            [System.Security.Cryptography.DataProtectionScope]::CurrentUser
        )
        $json = [System.Text.Encoding]::UTF8.GetString($plainBytes)
        return ($json | ConvertFrom-Json)
    }
    catch {
        throw "Could not decrypt stored OAuth token file at '$Path'. Re-run with -ClearAuth and authenticate again. Details: $($_.Exception.Message)"
    }
}

function ConvertTo-Base64Url {
    param([byte[]]$Bytes)
    $b64 = [Convert]::ToBase64String($Bytes)
    return $b64.TrimEnd('=').Replace('+', '-').Replace('/', '_')
}

function New-RandomBase64Url {
    param([int]$ByteLength)
    $bytes = New-Object byte[] $ByteLength
    $rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
    try {
        $rng.GetBytes($bytes)
    }
    finally {
        $rng.Dispose()
    }
    return (ConvertTo-Base64Url $bytes)
}

function Get-Sha256Base64Url {
    param([string]$InputText)
    $bytes = [System.Text.Encoding]::ASCII.GetBytes($InputText)
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        $hash = $sha.ComputeHash($bytes)
    }
    finally {
        $sha.Dispose()
    }
    return (ConvertTo-Base64Url $hash)
}

function Parse-QueryString {
    param([string]$Query)
    $map = @{}
    if ([string]::IsNullOrWhiteSpace($Query)) {
        return $map
    }
    $trimmed = $Query.TrimStart('?')
    foreach ($pair in $trimmed.Split('&')) {
        if ([string]::IsNullOrWhiteSpace($pair)) {
            continue
        }
        $idx = $pair.IndexOf('=')
        if ($idx -lt 0) {
            $k = [Uri]::UnescapeDataString($pair.Replace('+', ' '))
            $map[$k] = ""
        }
        else {
            $k = [Uri]::UnescapeDataString($pair.Substring(0, $idx).Replace('+', ' '))
            $v = [Uri]::UnescapeDataString($pair.Substring($idx + 1).Replace('+', ' '))
            $map[$k] = $v
        }
    }
    return $map
}

function Invoke-HttpJson {
    param(
        [Parameter(Mandatory = $true)][System.Net.Http.HttpMethod]$Method,
        [Parameter(Mandatory = $true)][string]$Uri,
        [string]$AccessToken = "",
        [System.Net.Http.HttpContent]$Content = $null
    )
    $req = [System.Net.Http.HttpRequestMessage]::new($Method, $Uri)
    if (-not [string]::IsNullOrWhiteSpace($AccessToken)) {
        $req.Headers.Authorization = [System.Net.Http.Headers.AuthenticationHeaderValue]::new("Bearer", $AccessToken)
    }
    if ($null -ne $Content) {
        $req.Content = $Content
    }

    try {
        $resp = $script:HttpClient.SendAsync($req).GetAwaiter().GetResult()
        try {
            $body = $resp.Content.ReadAsStringAsync().GetAwaiter().GetResult()
            if (-not $resp.IsSuccessStatusCode) {
                throw "HTTP $([int]$resp.StatusCode) ($($resp.ReasonPhrase)) from $Method $Uri`: $body"
            }
            if ([string]::IsNullOrWhiteSpace($body)) {
                return $null
            }
            return ($body | ConvertFrom-Json)
        }
        finally {
            $resp.Dispose()
        }
    }
    finally {
        $req.Dispose()
    }
}

function Resolve-OAuthClientCredentials {
    param(
        [string]$SecretFile,
        [string]$ExplicitClientId,
        [string]$ExplicitClientSecret,
        [string]$ExampleDriveFolder,
        $StoredCreds
    )

    if (-not [string]::IsNullOrWhiteSpace($SecretFile)) {
        if (-not (Test-Path $SecretFile)) {
            throw "Client secret file not found: '$SecretFile'"
        }
        $raw = Get-Content -Path $SecretFile -Raw -Encoding UTF8 | ConvertFrom-Json
        $installed = Get-ObjProp $raw "installed" $null
        if ($null -eq $installed) {
            $installed = Get-ObjProp $raw "web" $null
        }
        if ($null -eq $installed) {
            throw "Invalid Google OAuth JSON '$SecretFile': expected an 'installed' (Desktop app) client object."
        }
        $cid = [string](Get-ObjProp $installed "client_id" "")
        $csec = [string](Get-ObjProp $installed "client_secret" "")
        if ([string]::IsNullOrWhiteSpace($cid) -or [string]::IsNullOrWhiteSpace($csec)) {
            throw "Google OAuth JSON '$SecretFile' is missing client_id or client_secret."
        }
        return [PSCustomObject]@{ client_id = $cid; client_secret = $csec }
    }

    if (-not [string]::IsNullOrWhiteSpace($ExplicitClientId) -and -not [string]::IsNullOrWhiteSpace($ExplicitClientSecret)) {
        return [PSCustomObject]@{ client_id = $ExplicitClientId; client_secret = $ExplicitClientSecret }
    }

    $envId = $env:FAITHFIRE_GDRIVE_CLIENT_ID
    $envSecret = $env:FAITHFIRE_GDRIVE_CLIENT_SECRET
    if (-not [string]::IsNullOrWhiteSpace($envId) -and -not [string]::IsNullOrWhiteSpace($envSecret)) {
        return [PSCustomObject]@{ client_id = $envId; client_secret = $envSecret }
    }

    $savedId = [string](Get-ObjProp $StoredCreds "client_id" "")
    $savedSecret = [string](Get-ObjProp $StoredCreds "client_secret" "")
    if (-not [string]::IsNullOrWhiteSpace($savedId) -and -not [string]::IsNullOrWhiteSpace($savedSecret)) {
        return [PSCustomObject]@{ client_id = $savedId; client_secret = $savedSecret }
    }

    $setupHelp = @"
No Google Drive OAuth client credentials are configured yet.

One-time setup (stored encrypted under your Windows user profile via DPAPI):
  1. In Google Cloud Console, enable the "Google Drive API".
  2. Configure the OAuth consent screen (External + add your Google account as a Test User, or Internal).
  3. Create Credentials -> OAuth client ID -> Application type: "Desktop app", then download the JSON file.
  4. Run once:
     .\scripts\backup-world.ps1 -ClientSecretFile "C:\path\to\client_secret.json" -DriveFolder "$ExampleDriveFolder"

Subsequent runs only need:
  .\scripts\backup-world.ps1
"@
    throw $setupHelp
}

function Write-HttpResponseAndClose {
    param(
        [System.Net.Sockets.TcpClient]$Client,
        [int]$StatusCode,
        [string]$StatusText,
        [string]$HtmlBody
    )
    $bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($HtmlBody)
    $header = "HTTP/1.1 $StatusCode $StatusText`r`nContent-Type: text/html; charset=utf-8`r`nContent-Length: $($bodyBytes.Length)`r`nConnection: close`r`n`r`n"
    $headerBytes = [System.Text.Encoding]::ASCII.GetBytes($header)
    $stream = $Client.GetStream()
    $stream.Write($headerBytes, 0, $headerBytes.Length)
    $stream.Write($bodyBytes, 0, $bodyBytes.Length)
    $stream.Flush()
    $Client.Close()
}

function Invoke-OAuthBrowserFlow {
    param(
        [Parameter(Mandatory = $true)][string]$OAuthClientId,
        [Parameter(Mandatory = $true)][string]$OAuthClientSecret
    )

    $codeVerifier = New-RandomBase64Url 32
    $codeChallenge = Get-Sha256Base64Url $codeVerifier
    $expectedState = New-RandomBase64Url 16

    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
    $listener.Start()
    try {
        $port = ([System.Net.IPEndPoint]$listener.LocalEndpoint).Port
        $redirectUri = "http://127.0.0.1:$port/callback"

        $authQuery = @(
            "client_id=" + [Uri]::EscapeDataString($OAuthClientId),
            "redirect_uri=" + [Uri]::EscapeDataString($redirectUri),
            "response_type=code",
            "scope=" + [Uri]::EscapeDataString($script:DriveScope),
            "access_type=offline",
            "prompt=consent",
            "state=" + [Uri]::EscapeDataString($expectedState),
            "code_challenge=" + [Uri]::EscapeDataString($codeChallenge),
            "code_challenge_method=S256"
        ) -join "&"
        $authUrl = "https://accounts.google.com/o/oauth2/v2/auth?$authQuery"

        Write-Host ""
        Write-Host "Opening browser for Google Drive OAuth authorization..."
        Write-Host "If the browser does not open automatically, visit:"
        Write-Host "  $authUrl"
        Write-Host ""
        Start-Process $authUrl | Out-Null

        $deadline = [DateTime]::UtcNow.AddMinutes(3)
        $authCode = $null

        while ($null -eq $authCode) {
            if ([DateTime]::UtcNow -gt $deadline) {
                throw "Timed out after 3 minutes waiting for Google OAuth browser callback."
            }
            if (-not $listener.Pending()) {
                Start-Sleep -Milliseconds 100
                continue
            }

            $tcpClient = $listener.AcceptTcpClient()
            try {
                $reader = [System.IO.StreamReader]::new($tcpClient.GetStream(), [System.Text.Encoding]::ASCII, $false, 1024, $true)
                $requestLine = $reader.ReadLine()
                while ($true) {
                    $line = $reader.ReadLine()
                    if ([string]::IsNullOrEmpty($line)) {
                        break
                    }
                }

                if ([string]::IsNullOrWhiteSpace($requestLine)) {
                    $tcpClient.Close()
                    continue
                }

                $parts = $requestLine.Split(' ')
                if ($parts.Length -lt 2) {
                    Write-HttpResponseAndClose $tcpClient 400 "Bad Request" "<html><body>Bad request</body></html>"
                    continue
                }

                $target = $parts[1]
                $qIdx = $target.IndexOf('?')
                $reqPath = if ($qIdx -ge 0) { $target.Substring(0, $qIdx) } else { $target }
                $reqQuery = if ($qIdx -ge 0) { $target.Substring($qIdx) } else { "" }

                if ($reqPath -ne "/callback") {
                    Write-HttpResponseAndClose $tcpClient 404 "Not Found" "<html><body>Not found</body></html>"
                    continue
                }

                $qParams = Parse-QueryString $reqQuery
                if ($qParams.ContainsKey("error")) {
                    $errMsg = [string]$qParams["error"]
                    Write-HttpResponseAndClose $tcpClient 400 "OAuth Error" "<html><body><h2>Authorization failed</h2><p>You can close this tab and inspect the terminal.</p></body></html>"
                    throw "Google OAuth authorization declined or failed: $errMsg"
                }

                $stateVal = if ($qParams.ContainsKey("state")) { [string]$qParams["state"] } else { "" }
                if ($stateVal -cne $expectedState) {
                    Write-HttpResponseAndClose $tcpClient 400 "Invalid State" "<html><body><h2>Invalid OAuth state</h2></body></html>"
                    throw "Google OAuth callback failed CSRF state verification."
                }

                if (-not $qParams.ContainsKey("code") -or [string]::IsNullOrWhiteSpace([string]$qParams["code"])) {
                    Write-HttpResponseAndClose $tcpClient 400 "Missing Code" "<html><body><h2>Missing authorization code</h2></body></html>"
                    throw "Google OAuth callback did not include an authorization code."
                }

                $authCode = [string]$qParams["code"]
                Write-HttpResponseAndClose $tcpClient 200 "OK" @"
<!doctype html>
<html><body style="font-family:system-ui,sans-serif;background:#11161f;color:#e8eef7;padding:40px">
<h2>Faith &amp; Fire World Backup Authorized</h2>
<p>Google Drive credentials have been saved securely for this Windows account. You may close this tab.</p>
</body></html>
"@
            }
            catch {
                if ($tcpClient.Connected) {
                    try { $tcpClient.Close() } catch {}
                }
                throw
            }
        }

        $formPairs = [System.Collections.Generic.Dictionary[string, string]]::new()
        $formPairs["client_id"] = $OAuthClientId
        $formPairs["client_secret"] = $OAuthClientSecret
        $formPairs["code"] = $authCode
        $formPairs["code_verifier"] = $codeVerifier
        $formPairs["grant_type"] = "authorization_code"
        $formPairs["redirect_uri"] = $redirectUri
        $formContent = [System.Net.Http.FormUrlEncodedContent]::new($formPairs)
        try {
            $tokenResp = Invoke-HttpJson -Method ([System.Net.Http.HttpMethod]::Post) -Uri "https://oauth2.googleapis.com/token" -Content $formContent
        }
        finally {
            $formContent.Dispose()
        }

        $refresh = [string](Get-ObjProp $tokenResp "refresh_token" "")
        if ([string]::IsNullOrWhiteSpace($refresh)) {
            throw "Google OAuth token response did not contain a refresh_token. Re-run with -Reauth and approve offline access."
        }

        $nowUnix = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
        $expiresIn = [int64](Get-ObjProp $tokenResp "expires_in" 3600)
        return [PSCustomObject]@{
            client_id       = $OAuthClientId
            client_secret   = $OAuthClientSecret
            access_token    = [string](Get-ObjProp $tokenResp "access_token" "")
            refresh_token   = $refresh
            expires_at_unix = ($nowUnix + $expiresIn)
        }
    }
    finally {
        $listener.Stop()
    }
}

function Refresh-OAuthToken {
    param([Parameter(Mandatory = $true)]$Creds)

    $formPairs = [System.Collections.Generic.Dictionary[string, string]]::new()
    $formPairs["client_id"] = [string]$Creds.client_id
    $formPairs["client_secret"] = [string]$Creds.client_secret
    $formPairs["refresh_token"] = [string]$Creds.refresh_token
    $formPairs["grant_type"] = "refresh_token"
    $formContent = [System.Net.Http.FormUrlEncodedContent]::new($formPairs)
    try {
        $tokenResp = Invoke-HttpJson -Method ([System.Net.Http.HttpMethod]::Post) -Uri "https://oauth2.googleapis.com/token" -Content $formContent
    }
    finally {
        $formContent.Dispose()
    }

    $nowUnix = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
    $expiresIn = [int64](Get-ObjProp $tokenResp "expires_in" 3600)
    $newRefresh = [string](Get-ObjProp $tokenResp "refresh_token" "")
    if ([string]::IsNullOrWhiteSpace($newRefresh)) {
        $newRefresh = [string]$Creds.refresh_token
    }

    return [PSCustomObject]@{
        client_id       = [string]$Creds.client_id
        client_secret   = [string]$Creds.client_secret
        access_token    = [string](Get-ObjProp $tokenResp "access_token" "")
        refresh_token   = $newRefresh
        expires_at_unix = ($nowUnix + $expiresIn)
    }
}

function Get-DriveAccessToken {
    param(
        [string]$SecretFile,
        [string]$ExplicitClientId,
        [string]$ExplicitClientSecret,
        [string]$ExampleDriveFolder,
        [switch]$ForceReauth
    )

    $stored = Unprotect-DpapiJson -Path $script:TokenPath
    $hasCustomClient = (-not [string]::IsNullOrWhiteSpace($SecretFile)) -or
        (-not [string]::IsNullOrWhiteSpace($ExplicitClientId) -and -not [string]::IsNullOrWhiteSpace($ExplicitClientSecret))

    if (-not $ForceReauth -and -not $hasCustomClient -and $null -ne $stored) {
        $storedRefresh = [string](Get-ObjProp $stored "refresh_token" "")
        $storedAccess = [string](Get-ObjProp $stored "access_token" "")
        $expiresAt = [int64](Get-ObjProp $stored "expires_at_unix" 0)
        $nowUnix = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()

        if (-not [string]::IsNullOrWhiteSpace($storedAccess) -and $expiresAt -gt ($nowUnix + 120)) {
            return $storedAccess
        }

        if (-not [string]::IsNullOrWhiteSpace($storedRefresh)) {
            try {
                $refreshed = Refresh-OAuthToken -Creds $stored
                Protect-DpapiJson -Object $refreshed -Path $script:TokenPath
                return [string]$refreshed.access_token
            }
            catch {
                Write-Warning "Stored refresh token could not be refreshed ($($_.Exception.Message)). Re-authorizing..."
            }
        }
    }

    $clientCreds = Resolve-OAuthClientCredentials `
        -SecretFile $SecretFile `
        -ExplicitClientId $ExplicitClientId `
        -ExplicitClientSecret $ExplicitClientSecret `
        -ExampleDriveFolder $ExampleDriveFolder `
        -StoredCreds $stored

    $freshTokens = Invoke-OAuthBrowserFlow `
        -OAuthClientId $clientCreds.client_id `
        -OAuthClientSecret $clientCreds.client_secret

    Protect-DpapiJson -Object $freshTokens -Path $script:TokenPath
    Write-Host "Saved encrypted Google Drive credentials -> $script:TokenPath"
    return [string]$freshTokens.access_token
}

function Escape-DriveQueryLiteral {
    param([string]$Value)
    return $Value.Replace('\', '\\').Replace("'", "\'")
}

function Resolve-DriveFolderPath {
    param(
        [Parameter(Mandatory = $true)][string]$AccessToken,
        [Parameter(Mandatory = $true)][string]$FolderPath,
        [switch]$CreateMissing
    )

    $rawSegments = @($FolderPath.Split(@('/', '\'), [System.StringSplitOptions]::RemoveEmptyEntries))
    $segments = @()
    foreach ($seg in $rawSegments) {
        $trimmed = $seg.Trim()
        if ($trimmed -eq "" -or $trimmed -eq ".") {
            continue
        }
        if ($trimmed -eq "..") {
            throw "Invalid Google Drive folder segment '..' in path '$FolderPath'."
        }
        $segments += $trimmed
    }

    $parentId = "root"
    foreach ($seg in $segments) {
        $escapedName = Escape-DriveQueryLiteral $seg
        $escapedParent = Escape-DriveQueryLiteral $parentId
        $query = "name = '$escapedName' and '$escapedParent' in parents and mimeType = 'application/vnd.google-apps.folder' and trashed = false"
        $listUri = "https://www.googleapis.com/drive/v3/files?q=$([Uri]::EscapeDataString($query))&fields=files(id,name)&pageSize=10"
        $res = Invoke-HttpJson -Method ([System.Net.Http.HttpMethod]::Get) -Uri $listUri -AccessToken $AccessToken
        $files = @(Get-ObjProp $res "files" @())

        if ($files.Count -gt 0) {
            $parentId = [string]$files[0].id
            continue
        }

        if (-not $CreateMissing) {
            return $null
        }

        $metaJson = [PSCustomObject]@{
            name     = $seg
            mimeType = "application/vnd.google-apps.folder"
            parents  = @($parentId)
        } | ConvertTo-Json -Depth 4
        $content = [System.Net.Http.StringContent]::new($metaJson, [System.Text.Encoding]::UTF8, "application/json")
        try {
            $created = Invoke-HttpJson -Method ([System.Net.Http.HttpMethod]::Post) -Uri "https://www.googleapis.com/drive/v3/files?fields=id,name" -AccessToken $AccessToken -Content $content
            $parentId = [string]$created.id
            Write-Host "Created Google Drive folder '$seg' ($parentId)"
        }
        finally {
            $content.Dispose()
        }
    }

    return $parentId
}

function New-WorldArchive {
    param(
        [Parameter(Mandatory = $true)][string]$SourceDir,
        [Parameter(Mandatory = $true)][string]$ZipPath
    )

    if (-not (Test-Path $SourceDir)) {
        throw "World directory not found: '$SourceDir'"
    }

    $rootFull = [System.IO.Path]::GetFullPath($SourceDir).TrimEnd('\', '/')
    $rootPrefix = $rootFull + [System.IO.Path]::DirectorySeparatorChar
    $files = @(Get-ChildItem -Path $rootFull -File -Recurse | Sort-Object FullName)
    if ($files.Count -eq 0) {
        throw "World directory '$rootFull' contains no files to back up."
    }

    $zipParent = Split-Path -Parent $ZipPath
    if (-not [string]::IsNullOrWhiteSpace($zipParent) -and -not (Test-Path $zipParent)) {
        New-Item -ItemType Directory -Force -Path $zipParent | Out-Null
    }
    if (Test-Path $ZipPath) {
        Remove-Item -Path $ZipPath -Force
    }

    $archive = [System.IO.Compression.ZipFile]::Open($ZipPath, [System.IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($file in $files) {
            $relPath = $file.FullName.Substring($rootPrefix.Length).Replace('\', '/')
            [System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile(
                $archive,
                $file.FullName,
                $relPath,
                [System.IO.Compression.CompressionLevel]::Optimal
            ) | Out-Null
        }
    }
    finally {
        $archive.Dispose()
    }

    $zipInfo = Get-Item -Path $ZipPath
    return [PSCustomObject]@{
        path       = $ZipPath
        file_count = $files.Count
        size_bytes = $zipInfo.Length
    }
}

function Upload-BackupArchive {
    param(
        [Parameter(Mandatory = $true)][string]$AccessToken,
        [Parameter(Mandatory = $true)][string]$FolderId,
        [Parameter(Mandatory = $true)][string]$ZipPath,
        [Parameter(Mandatory = $true)][string]$ArchiveName
    )

    $initMeta = [PSCustomObject]@{
        name          = $ArchiveName
        parents       = @($FolderId)
        appProperties = [PSCustomObject]@{
            faithfireBackup = "true"
        }
    } | ConvertTo-Json -Depth 5

    $zipLength = (Get-Item -Path $ZipPath).Length
    $initReq = [System.Net.Http.HttpRequestMessage]::new(
        [System.Net.Http.HttpMethod]::Post,
        "https://www.googleapis.com/upload/drive/v3/files?uploadType=resumable&fields=id,name,size,createdTime"
    )
    $initReq.Headers.Authorization = [System.Net.Http.Headers.AuthenticationHeaderValue]::new("Bearer", $AccessToken)
    $initReq.Headers.TryAddWithoutValidation("X-Upload-Content-Type", "application/zip") | Out-Null
    $initReq.Headers.TryAddWithoutValidation("X-Upload-Content-Length", [string]$zipLength) | Out-Null
    $initReq.Content = [System.Net.Http.StringContent]::new($initMeta, [System.Text.Encoding]::UTF8, "application/json")

    $sessionUri = $null
    try {
        $initResp = $script:HttpClient.SendAsync($initReq).GetAwaiter().GetResult()
        try {
            $initBody = $initResp.Content.ReadAsStringAsync().GetAwaiter().GetResult()
            if (-not $initResp.IsSuccessStatusCode) {
                throw "Failed to initiate Google Drive resumable upload (HTTP $([int]$initResp.StatusCode)): $initBody"
            }
            if ($null -eq $initResp.Headers.Location) {
                throw "Google Drive resumable upload response did not include a Location header."
            }
            $sessionUri = $initResp.Headers.Location.AbsoluteUri
        }
        finally {
            $initResp.Dispose()
        }
    }
    finally {
        $initReq.Dispose()
    }

    $fileStream = [System.IO.File]::OpenRead($ZipPath)
    try {
        $putReq = [System.Net.Http.HttpRequestMessage]::new([System.Net.Http.HttpMethod]::Put, $sessionUri)
        $streamContent = [System.Net.Http.StreamContent]::new($fileStream)
        $streamContent.Headers.ContentType = [System.Net.Http.Headers.MediaTypeHeaderValue]::Parse("application/zip")
        $streamContent.Headers.ContentLength = $zipLength
        $putReq.Content = $streamContent

        try {
            $putResp = $script:HttpClient.SendAsync($putReq).GetAwaiter().GetResult()
            try {
                $putBody = $putResp.Content.ReadAsStringAsync().GetAwaiter().GetResult()
                if (-not $putResp.IsSuccessStatusCode) {
                    throw "Google Drive upload failed (HTTP $([int]$putResp.StatusCode)): $putBody"
                }
                return ($putBody | ConvertFrom-Json)
            }
            finally {
                $putResp.Dispose()
            }
        }
        finally {
            $putReq.Dispose()
        }
    }
    finally {
        $fileStream.Dispose()
    }
}

function Get-DriveWorldBackups {
    param(
        [Parameter(Mandatory = $true)][string]$AccessToken,
        [Parameter(Mandatory = $true)][string]$FolderId
    )

    $escapedParent = Escape-DriveQueryLiteral $FolderId
    $query = "'$escapedParent' in parents and trashed = false and appProperties has { key='faithfireBackup' and value='true' }"
    $allFiles = @()
    $pageToken = ""

    do {
        $uri = "https://www.googleapis.com/drive/v3/files?q=$([Uri]::EscapeDataString($query))&orderBy=createdTime%20desc&pageSize=100&fields=nextPageToken,files(id,name,size,createdTime)"
        if (-not [string]::IsNullOrWhiteSpace($pageToken)) {
            $uri += "&pageToken=$([Uri]::EscapeDataString($pageToken))"
        }
        $res = Invoke-HttpJson -Method ([System.Net.Http.HttpMethod]::Get) -Uri $uri -AccessToken $AccessToken
        $pageFiles = @(Get-ObjProp $res "files" @())
        foreach ($f in $pageFiles) {
            if ([string]$f.name -match $script:BackupNameRegex) {
                $allFiles += $f
            }
        }
        $pageToken = [string](Get-ObjProp $res "nextPageToken" "")
    } while (-not [string]::IsNullOrWhiteSpace($pageToken))

    return $allFiles
}

function Prune-OldBackups {
    param(
        [Parameter(Mandatory = $true)][string]$AccessToken,
        [Parameter(Mandatory = $true)][string]$FolderId,
        [Parameter(Mandatory = $true)][int]$KeepCount
    )

    if ($KeepCount -le 0) {
        return 0
    }

    $backups = @(Get-DriveWorldBackups -AccessToken $AccessToken -FolderId $FolderId)
    if ($backups.Count -le $KeepCount) {
        return 0
    }

    $deleted = 0
    for ($i = $KeepCount; $i -lt $backups.Count; $i++) {
        $old = $backups[$i]
        $deleteUri = "https://www.googleapis.com/drive/v3/files/$([Uri]::EscapeDataString([string]$old.id))"
        Invoke-HttpJson -Method ([System.Net.Http.HttpMethod]::Delete) -Uri $deleteUri -AccessToken $AccessToken | Out-Null
        Write-Host "Pruned old backup: $($old.name) (id: $($old.id))"
        $deleted++
    }
    return $deleted
}

# --- Main Entrypoint ---

if ($ClearAuth) {
    if (Test-Path $script:TokenPath) {
        Remove-Item -Path $script:TokenPath -Force
        Write-Host "Removed stored Google Drive OAuth credentials: $script:TokenPath"
    }
    else {
        Write-Host "No stored Google Drive OAuth credentials found at: $script:TokenPath"
    }
    exit 0
}

$cfg = Load-BackupConfig
$effectiveDriveFolder = if (-not [string]::IsNullOrWhiteSpace($DriveFolder)) {
    $DriveFolder.Trim()
}
else {
    $cfg.drive_folder
}

$effectiveKeep = if ($Keep -ge 0) {
    $Keep
}
else {
    $cfg.keep
}

$effectiveWorldDir = if (-not [string]::IsNullOrWhiteSpace($WorldDir)) {
    $WorldDir
}
else {
    Join-Path $script:RepoRoot "world"
}

if (-not [string]::IsNullOrWhiteSpace($DriveFolder) -or $Keep -ge 0 -or -not (Test-Path $script:ConfigPath)) {
    Save-BackupConfig -Folder $effectiveDriveFolder -KeepCount $effectiveKeep
}

if ($List) {
    $token = Get-DriveAccessToken `
        -SecretFile $ClientSecretFile `
        -ExplicitClientId $ClientId `
        -ExplicitClientSecret $ClientSecret `
        -ExampleDriveFolder $effectiveDriveFolder `
        -ForceReauth:$Reauth

    $folderId = Resolve-DriveFolderPath -AccessToken $token -FolderPath $effectiveDriveFolder
    if ([string]::IsNullOrWhiteSpace($folderId)) {
        Write-Host "Google Drive folder '$effectiveDriveFolder' does not exist yet (0 backups)."
        exit 0
    }

    $existing = @(Get-DriveWorldBackups -AccessToken $token -FolderId $folderId)
    Write-Host "Google Drive backups in '$effectiveDriveFolder' ($($existing.Count) total, keeping latest $effectiveKeep):"
    foreach ($item in $existing) {
        $sizeVal = Get-ObjProp $item "size" "0"
        Write-Host ("  {0}  |  {1,10} bytes  |  {2}  |  id={3}" -f $item.name, $sizeVal, $item.createdTime, $item.id)
    }
    exit 0
}

$timestamp = [DateTime]::UtcNow.ToString("yyyyMMdd-HHmmss") + "Z"
$archiveName = "faithfire-world-$timestamp.zip"
$tempZip = if (-not [string]::IsNullOrWhiteSpace($LocalOut)) {
    $LocalOut
}
else {
    Join-Path ([System.IO.Path]::GetTempPath()) $archiveName
}

$keepLocalFile = -not [string]::IsNullOrWhiteSpace($LocalOut)

try {
    if ($DryRun) {
        $archiveInfo = New-WorldArchive -SourceDir $effectiveWorldDir -ZipPath $tempZip
        $kb = [Math]::Round($archiveInfo.size_bytes / 1024.0, 2)
        Write-Host "Archived $($archiveInfo.file_count) files from '$effectiveWorldDir' -> '$tempZip' ($kb KB)"
        Write-Host "[DryRun] Target Google Drive folder: '$effectiveDriveFolder' | Retain latest: $effectiveKeep"
        Write-Host "[DryRun] Skipping Google Drive authentication and upload."
        exit 0
    }

    $token = Get-DriveAccessToken `
        -SecretFile $ClientSecretFile `
        -ExplicitClientId $ClientId `
        -ExplicitClientSecret $ClientSecret `
        -ExampleDriveFolder $effectiveDriveFolder `
        -ForceReauth:$Reauth

    $archiveInfo = New-WorldArchive -SourceDir $effectiveWorldDir -ZipPath $tempZip
    $kb = [Math]::Round($archiveInfo.size_bytes / 1024.0, 2)
    Write-Host "Archived $($archiveInfo.file_count) files from '$effectiveWorldDir' -> '$tempZip' ($kb KB)"

    $folderId = Resolve-DriveFolderPath -AccessToken $token -FolderPath $effectiveDriveFolder -CreateMissing
    $uploaded = Upload-BackupArchive `
        -AccessToken $token `
        -FolderId $folderId `
        -ZipPath $tempZip `
        -ArchiveName $archiveName

    $uploadedId = [string](Get-ObjProp $uploaded "id" "")
    if ([string]::IsNullOrWhiteSpace($uploadedId)) {
        throw "Upload completed without returning a Google Drive file ID; skipping retention pruning."
    }

    Write-Host "Uploaded '$($uploaded.name)' to Google Drive '$effectiveDriveFolder' (id: $uploadedId)"

    $pruned = Prune-OldBackups -AccessToken $token -FolderId $folderId -KeepCount $effectiveKeep
    Write-Host "Backup complete (retention limit: $effectiveKeep, pruned: $pruned)."
}
finally {
    if (-not $keepLocalFile -and (Test-Path $tempZip)) {
        Remove-Item -Path $tempZip -Force -ErrorAction SilentlyContinue
    }
}
