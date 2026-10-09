$ErrorActionPreference = 'Stop'
$taskRoot = 'D:\CodexProjects\Translay'
$taskTarget = Join-Path $taskRoot '.git'
$taskRecord = Join-Path $taskRoot 'docs\sandbox-owner-repair.json'

try {
    $taskItem = Get-Item -LiteralPath $taskTarget -Force
    if (-not $taskItem.PSIsContainer -or
        ($taskItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
        $taskItem.FullName -ne $taskTarget) {
        throw 'Expected the exact, non-reparse .git directory.'
    }
    $taskIdentity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $taskPrincipal = [Security.Principal.WindowsPrincipal]::new($taskIdentity)
    if (-not $taskPrincipal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'Administrator approval is required to restore directory ownership.'
    }
    $taskDesiredOwner = (Get-Acl -LiteralPath $taskRoot).Owner
    if ($taskDesiredOwner -ne 'DESKTOP-FM1BT2V\malus') {
        throw 'Project owner changed; review before proceeding.'
    }
    $taskBefore = Get-Acl -LiteralPath $taskTarget
    if ($taskBefore.Owner -eq $taskDesiredOwner) {
        Write-Output 'Owner is already correct; no change made.'
        exit 0
    }
    if ($taskBefore.Owner -ne 'DESKTOP-FM1BT2V\CodexSandboxOffline') {
        throw 'Unexpected current owner; no change made.'
    }
    if (Test-Path -LiteralPath $taskRecord) {
        throw 'A repair record already exists; review it instead of overwriting it.'
    }
    $taskDacl = $taskBefore.GetSecurityDescriptorSddlForm([Security.AccessControl.AccessControlSections]::Access)
    $taskSnapshot = [ordered]@{
        Target = $taskTarget
        BeforeOwner = $taskBefore.Owner
        DesiredOwner = $taskDesiredOwner
        BeforeSddl = $taskBefore.Sddl
        StartedAt = [DateTimeOffset]::Now.ToString('o')
        Status = 'prepared'
    }
    $taskSnapshot | ConvertTo-Json | Set-Content -LiteralPath $taskRecord -Encoding UTF8

    # Single directory only: no /T recursion, ACL reset, grants, or removal of deny rules.
    & "$env:SystemRoot\System32\icacls.exe" $taskTarget /setowner $taskDesiredOwner
    if ($LASTEXITCODE -ne 0) { throw "icacls failed with exit code $LASTEXITCODE" }
    $taskAfter = Get-Acl -LiteralPath $taskTarget
    if ($taskAfter.Owner -ne $taskDesiredOwner) { throw 'Owner verification failed.' }
    if ($taskAfter.GetSecurityDescriptorSddlForm([Security.AccessControl.AccessControlSections]::Access) -ne $taskDacl) {
        throw 'Access rules changed unexpectedly; inspect the saved snapshot.'
    }
    $taskSnapshot['AfterOwner'] = $taskAfter.Owner
    $taskSnapshot['Status'] = 'owner-restored-access-rules-unchanged'
    $taskSnapshot | ConvertTo-Json | Set-Content -LiteralPath $taskRecord -Encoding UTF8
    Write-Output 'Directory owner restored; access rules unchanged.'
} catch {
    Write-Error $_
    exit 1
}
