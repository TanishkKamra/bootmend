Try-Section 'apps' {
  $keys = 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*', 'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*', 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*'
  $R.apps = @(Get-ItemProperty $keys | Where-Object { $_.DisplayName -and -not $_.SystemComponent -and $_.DisplayName -notmatch '^(Update for|Security Update)' } | Sort-Object DisplayName -Unique | ForEach-Object {
      [ordered]@{ Name = $_.DisplayName.Trim(); Version = $_.DisplayVersion; Publisher = $_.Publisher
        Installed = $(if ($_.InstallDate -match '^\d{8}$') { "$($_.InstallDate.Substring(0,4))-$($_.InstallDate.Substring(4,2))-$($_.InstallDate.Substring(6,2))" } else { $null })
        SizeMB = $(if ($_.EstimatedSize) { [math]::Round($_.EstimatedSize / 1024) } else { $null }); Location = $_.InstallLocation; Uninstall = $_.UninstallString } })
  $R.storeApps = @(Get-AppxPackage | Where-Object { -not $_.IsFramework -and $_.SignatureKind -ne 'System' } | Sort-Object Name -Unique | ForEach-Object { [ordered]@{ Name = $_.Name; Version = "$($_.Version)"; Publisher = $_.PublisherDisplayName } })
  $br = [ordered]@{}
  @{Chrome = "$env:ProgramFiles\Google\Chrome\Application\chrome.exe"; Edge = "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe"; Brave = "$env:ProgramFiles\BraveSoftware\Brave-Browser\Application\brave.exe"; Firefox = "$env:ProgramFiles\Mozilla Firefox\firefox.exe"; Opera = "$env:LOCALAPPDATA\Programs\Opera\opera.exe"; Vivaldi = "$env:LOCALAPPDATA\Vivaldi\Application\vivaldi.exe" }.GetEnumerator() |
    ForEach-Object { if (Test-Path $_.Value) { $br[$_.Key] = (Get-Item $_.Value).VersionInfo.ProductVersion } }
  $R.browsers = $br
  $R.runtimes = @(@(Get-ChildItem 'HKLM:\SOFTWARE\Microsoft\NET Framework Setup\NDP\v4\Full' | Get-ItemProperty | ForEach-Object { ".NET Framework $($_.Version)" }) + @(Get-ChildItem "$env:ProgramFiles\dotnet\shared\Microsoft.NETCore.App" -Directory | ForEach-Object { ".NET $($_.Name)" }))
}
Try-Section 'startup' {
  $appr = @{}
  foreach ($k in 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run', 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run', 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run32', 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder', 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder') {
    $p = Get-ItemProperty $k; if ($p) { $p.PSObject.Properties | Where-Object { $_.Name -notlike 'PS*' } | ForEach-Object { $appr["$k|$($_.Name)"] = $_.Value } }
  }
  $sources = @(
    @{Key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'; Appr = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run'; Scope = 'User' },
    @{Key = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Run'; Appr = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run'; Scope = 'Machine' },
    @{Key = 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Run'; Appr = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run32'; Scope = 'Machine (32-bit)' })
  $list = New-Object System.Collections.Generic.List[object]
  foreach ($s in $sources) { $p = Get-ItemProperty $s.Key; if ($p) { $p.PSObject.Properties | Where-Object { $_.Name -notlike 'PS*' } | ForEach-Object {
        $v = $appr["$($s.Appr)|$($_.Name)"]; $list.Add([ordered]@{ Name = $_.Name; Command = "$($_.Value)"; Scope = $s.Scope; ApprovedKey = $s.Appr; Enabled = -not ($v -and ($v[0] % 2) -eq 1) }) } } }
  foreach ($f in @(@{Dir = [Environment]::GetFolderPath('Startup'); Appr = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder'; Scope = 'User folder' }, @{Dir = [Environment]::GetFolderPath('CommonStartup'); Appr = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder'; Scope = 'All users folder' })) {
    Get-ChildItem $f.Dir -File | Where-Object Name -NE 'desktop.ini' | ForEach-Object { $v = $appr["$($f.Appr)|$($_.Name)"]; $list.Add([ordered]@{ Name = $_.Name; Command = $_.FullName; Scope = $f.Scope; ApprovedKey = $f.Appr; Enabled = -not ($v -and ($v[0] % 2) -eq 1) }) } }
  # @() on a List of ordered dictionaries throws "Argument types do not match" in Windows PowerShell 5.1.
  $R.startup = $list.ToArray()
}
Try-Section 'services' {
  $R.services = @(Get-CimInstance Win32_Service | Where-Object { $_.StartMode -in 'Auto', 'Manual' -and $_.PathName -and $_.PathName -notmatch '(?i)\\Windows\\(system32|SysWOW64)\\(svchost|lsass|services)\.exe' } | ForEach-Object {
      $exe = $_.PathName -replace '^"([^"]+)".*', '$1' -replace '^(\S+\.exe).*', '$1'
      $sg = Sig $exe
      if ($sg -notmatch '^Microsoft Windows') { [ordered]@{ Name = $_.Name; Display = $_.DisplayName; State = $_.State; StartMode = $_.StartMode; Signer = $sg; Path = $exe } } })
}
Try-Section 'tasks' {
  $R.tasks = @(Get-ScheduledTask | Where-Object { $_.State -ne 'Disabled' -and $_.TaskPath -notmatch '^\\Microsoft\\' } | ForEach-Object {
      $a = @($_.Actions | ForEach-Object { "$($_.Execute) $($_.Arguments)".Trim() } | Where-Object { $_ }) -join ' ; '
      [ordered]@{ Path = $_.TaskPath; Name = $_.TaskName; Action = $a; Author = $_.Author; State = "$($_.State)" } })
}
Emit
