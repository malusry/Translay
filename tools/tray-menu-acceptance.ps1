param([switch]$SkipBuild, [switch]$Manual, [switch]$CheckManual)
$ErrorActionPreference = 'Stop'
if ($Manual -and $CheckManual) { throw 'Choose -Manual or -CheckManual, not both' }
$projectRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$manualMode = $Manual -or $CheckManual
$marker = 'translay-tray-manual-v1'
$executable = Join-Path $projectRoot 'src-tauri\target\debug\translay.exe'
$sandbox = Join-Path $projectRoot ('tests\artifacts\' + $(if ($manualMode) { 'tray-menu-manual-sandbox' } else { 'tray-menu-sandbox' }))
$reportPath = Join-Path $projectRoot ('tests\artifacts\' + $(if ($Manual) { 'tray-menu-manual.json' } elseif ($CheckManual) { 'tray-menu-manual-check.json' } else { 'tray-menu-native.txt' }))
$testProcess = $null
$verifiedRun = $false
$ownsFixture = $false
$sha = [Security.Cryptography.SHA256]::Create()
try { $fixtureId = [BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($projectRoot))).Replace('-', '') }
finally { $sha.Dispose() }
$fixtureLock = New-Object Threading.Mutex($false, ('Local\TranslayTrayAcceptance-' + $fixtureId))
Push-Location $projectRoot
try {
    try { $ownsFixture = $fixtureLock.WaitOne(0) }
    catch [Threading.AbandonedMutexException] { $ownsFixture = $true }
    if (-not $ownsFixture) { throw 'Another tray acceptance session is running; finish it before starting another' }
    if (-not $SkipBuild) {
        npx.cmd tauri build --debug --no-bundle
        if ($LASTEXITCODE -ne 0) { throw 'Native acceptance build failed' }
    }
    # Unknown flags in an older binary could start the normal application.
    # Check the fixture marker before using any new debug-only entry point.
    if (-not (Test-Path -LiteralPath $executable) -or -not [Text.Encoding]::UTF8.GetString([IO.File]::ReadAllBytes($executable)).Contains($marker)) {
        throw 'This debug binary lacks the manual fixture; rerun without -SkipBuild to rebuild'
    }
    if (Test-Path -LiteralPath $reportPath) { Remove-Item -LiteralPath $reportPath }
    $argument = if ($Manual) { '--tray-menu-manual' } elseif ($CheckManual) { '--tray-menu-manual-check' } else { '--tray-menu-smoke' }
    $testProcess = Start-Process -FilePath $executable -ArgumentList $argument -WorkingDirectory $projectRoot -WindowStyle Hidden -PassThru
    if ($Manual) {
        Write-Output 'Manual fixture started. Use the tray icon marked Translay / temporary manual test, then choose Quit to clean up. Session expires after 30 minutes.'
    }
    $timeout = if ($Manual) { 1815000 } else { 45000 }
    if (-not $testProcess.WaitForExit($timeout)) {
        Stop-Process -Id $testProcess.Id
        $testProcess.WaitForExit()
        throw 'Native acceptance timed out; the owned test process was stopped'
    }
    if ($testProcess.ExitCode -ne 0) { throw "Native acceptance failed (exit $($testProcess.ExitCode)); inspect $reportPath" }
    if ($manualMode) {
        $report = Get-Content -LiteralPath $reportPath -Raw | ConvertFrom-Json
        if ($report.marker -ne $marker -or $report.processId -ne $testProcess.Id -or -not $report.temporaryDataOnly -or $report.modelRequestsEnabled) {
            throw 'Missing isolated manual fixture evidence'
        }
        if ($CheckManual -and ($report.status -ne 'fixture-check-passed' -or -not $report.automaticFixtureChecksPassed -or -not $report.automationOnly)) {
            throw 'Manual fixture automatic check did not pass'
        }
        if ($Manual -and ($report.status -ne 'closed' -or $report.automationOnly)) { throw 'Manual fixture did not exit normally' }
        if ($report.humanVerification -ne 'pending' -or $report.hiddenTrayPanel -ne 'not_verified' -or $report.multiMonitorVisual -ne 'not_verified') {
            throw 'The fixture must leave human verification pending'
        }
    } else {
        $report = Get-Content -LiteralPath $reportPath -Raw
        if (-not $report.StartsWith('PASS:')) { throw 'Missing native acceptance evidence' }
        foreach ($evidence in @('concurrent first-open x12', 'stale action/close/paint isolation', 'blocked late settings open', 'single/double tray gestures', 'selection startup rule:', 'tray visuals:', 'tray fold animation:', 'tray immediate response:', 'local UI fonts:', 'settings mode wash:', 'settings mode alignment:', 'settings save styling:', 'settings connection styling:', 'late exit refresh suppressed', 'pending single cancelled on quit', 'synthetic credential deletion verified', 'Windows hidden tray panel')) {
            if (-not $report.Contains($evidence)) { throw "Missing lifecycle evidence: $evidence (rebuild before using -SkipBuild)" }
        }
        $report = $report.Replace('Runner must verify exit=0.', 'Verified normal process exit=0.')
        Set-Content -LiteralPath $reportPath -Value $report -Encoding utf8
    }
    $verifiedRun = $true
} finally {
    try {
        if ($ownsFixture -and $testProcess) {
            if (-not $testProcess.HasExited) {
                Stop-Process -Id $testProcess.Id
                $testProcess.WaitForExit()
            }
            # Covers unexpected exits too. The helper derives only this process's
            # .invalid credential scope and returns before starting Tauri.
            $cleanupProcess = Start-Process -FilePath $executable -ArgumentList "--tray-menu-cleanup=$($testProcess.Id)" -WorkingDirectory $projectRoot -WindowStyle Hidden -PassThru
            if (-not $cleanupProcess.WaitForExit(10000)) {
                Stop-Process -Id $cleanupProcess.Id
                throw 'Synthetic credential cleanup timed out'
            }
            if ($cleanupProcess.ExitCode -ne 0) { throw 'Synthetic credential cleanup could not be verified' }
            if (Test-Path -LiteralPath $sandbox) {
                $resolved = (Resolve-Path -LiteralPath $sandbox).Path
                if ($resolved -ne $sandbox) { throw 'Unexpected cleanup target' }
                $linked = @((Get-Item -LiteralPath $resolved)) + @(Get-ChildItem -LiteralPath $resolved -Directory -Recurse -Force) | Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint }
                if ($linked) { throw 'Refusing to clean linked test directories' }
                for ($attempt = 0; $attempt -lt 20; $attempt++) {
                    try { Remove-Item -LiteralPath $resolved -Recurse -Force; break }
                    catch { if ($attempt -eq 19) { throw }; Start-Sleep -Milliseconds 500 }
                }
            }
            if (Test-Path -LiteralPath $sandbox) { throw 'Isolated native fixture remains after cleanup' }
            if ($manualMode -and (Test-Path -LiteralPath $reportPath)) {
                $report = Get-Content -LiteralPath $reportPath -Raw | ConvertFrom-Json
                if ($report.marker -eq $marker -and $report.processId -eq $testProcess.Id) {
                    $report | Add-Member -NotePropertyName processExitVerified -NotePropertyValue $true -Force
                    $report | Add-Member -NotePropertyName processExitCode -NotePropertyValue $testProcess.ExitCode -Force
                    $report | Add-Member -NotePropertyName isolatedDataRemoved -NotePropertyValue $true -Force
                    $report.credentialDeletionVerified = $true
                    $report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $reportPath -Encoding utf8
                }
            } elseif ($verifiedRun) {
                Add-Content -LiteralPath $reportPath -Value 'CLEANUP: test process exited; isolated model config and WebView cache removed.' -Encoding utf8
            }
        }
    } finally {
        if ($ownsFixture) { $fixtureLock.ReleaseMutex() }
        $fixtureLock.Dispose()
        Pop-Location
    }
}
if ($verifiedRun) { Get-Content -LiteralPath $reportPath -Raw }
