Try-Section 'defender' {
  $m = Get-MpComputerStatus; $p = Get-MpPreference
  $R.defender = [ordered]@{
    Present = [bool]$m; AntivirusEnabled = $m.AntivirusEnabled; RealTime = $m.RealTimeProtectionEnabled; Tamper = $m.IsTamperProtected; BehaviorMonitor = $m.BehaviorMonitorEnabled
    Engine = $m.AMEngineVersion; Signatures = $m.AntivirusSignatureVersion; SignaturesUpdated = $(if ($m.AntivirusSignatureLastUpdated) { $m.AntivirusSignatureLastUpdated.ToString('yyyy-MM-dd HH:mm') } else { $null }); SignatureAgeDays = $m.AntivirusSignatureAge
    LastQuickScan = $(if ($m.QuickScanEndTime) { $m.QuickScanEndTime.ToString('yyyy-MM-dd HH:mm') } else { $null }); LastFullScan = $(if ($m.FullScanEndTime) { $m.FullScanEndTime.ToString('yyyy-MM-dd HH:mm') } else { $null })
    # Exclusions are only readable as administrator; a standard user sees a placeholder string.
    ExclusionPaths = @($p.ExclusionPath | Where-Object { $_ }); ExclusionProcesses = @($p.ExclusionProcess | Where-Object { $_ }); ExclusionExtensions = @($p.ExclusionExtension | Where-Object { $_ })
    PUAProtection = $p.PUAProtection; CloudProtection = $p.MAPSReporting; ControlledFolderAccess = $p.EnableControlledFolderAccess
    SmartAppControl = $m.SmartAppControlState
  }
  $R.otherAntivirus = @(Get-CimInstance -Namespace root\SecurityCenter2 -ClassName AntiVirusProduct | Where-Object { $_.displayName -notmatch 'Windows Defender|Microsoft Defender' } | ForEach-Object { $_.displayName })
  $R.threats = @(Get-MpThreatDetection | Sort-Object InitialDetectionTime -Descending | Select-Object -First 30 | ForEach-Object { $t = Get-MpThreat -ThreatID $_.ThreatID
      [ordered]@{ Date = (D $_.InitialDetectionTime); Name = $t.ThreatName; Severity = [int]$t.SeverityID; ActionSuccess = $_.ActionSuccess; Resources = @($_.Resources | Select-Object -First 3) } })
  $R.exclusionEvents = @(Get-WinEvent -FilterHashtable @{LogName = 'Microsoft-Windows-Windows Defender/Operational'; Id = 5007 } -MaxEvents 500 | Where-Object { $_.Message -match 'Exclusions\\(Paths|Processes|Extensions)' } | Select-Object -First 20 | ForEach-Object {
      [ordered]@{ Date = $_.TimeCreated.ToString('yyyy-MM-dd HH:mm'); Change = (($_.Message -split "`n" | Where-Object { $_ -match 'New value' }) -join ' ').Trim() } })
}
Try-Section 'firewall' {
  $R.firewallProfiles = @(Get-NetFirewallProfile | ForEach-Object { [ordered]@{ Name = $_.Name; Enabled = [bool]$_.Enabled; DefaultInbound = "$($_.DefaultInboundAction)" } })
  $R.firewallRules = @(Get-NetFirewallRule -Direction Inbound -Action Allow -Enabled True | Where-Object { -not $_.DisplayGroup } | ForEach-Object {
      $app = ($_ | Get-NetFirewallApplicationFilter).Program
      [ordered]@{ Name = $_.Name; DisplayName = $_.DisplayName; Profile = "$($_.Profile)"; Program = $app; ProgramExists = $(if ($app -and $app -ne 'Any') { Test-Path -LiteralPath ([Environment]::ExpandEnvironmentVariables($app)) } else { $null }) } })
}
Try-Section 'access' {
  $sys = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System'
  $crd = Get-Service chromoting
  $R.access = [ordered]@{
    UAC = ($sys.EnableLUA -eq 1); UACLevel = $sys.ConsentPromptBehaviorAdmin
    RemoteDesktop = ((Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Terminal Server').fDenyTSConnections -eq 0)
    RemoteAssistance = ((Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Remote Assistance').fAllowToGetHelp -eq 1)
    ChromeRemoteDesktop = $(if ($crd) { "$($crd.Status) / $($crd.StartType)" } else { $null })
    SMB1 = (Get-SmbServerConfiguration).EnableSMB1Protocol
    SmartScreen = (Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer').SmartScreenEnabled
    AutoLogon = ((Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon').AutoAdminLogon -eq '1')
    LsaPpl = (Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Lsa').RunAsPPL
    # Windows' own proof: logged at start-up when LSA starts as a protected process.
    LsaRunning = [bool](Get-WinEvent -FilterHashtable @{ LogName = 'System'; ProviderName = 'Microsoft-Windows-Wininit'; Id = 12; StartTime = (Get-CimInstance Win32_OperatingSystem).LastBootUpTime } -MaxEvents 1)
  }
  $hosts = Get-Content "$env:WINDIR\System32\drivers\etc\hosts" | Where-Object { $_ -notmatch '^\s*#' -and $_.Trim() }
  $R.hosts = [ordered]@{ Entries = @($hosts).Count; Sample = @($hosts | Select-Object -First 8) }
  $R.accounts = @(Get-LocalUser | ForEach-Object { [ordered]@{ Name = $_.Name; Enabled = [bool]$_.Enabled; LastLogon = (D $_.LastLogon); PasswordRequired = [bool]$_.PasswordRequired; Source = "$($_.PrincipalSource)" } })
  $R.admins = @(Get-LocalGroupMember -SID 'S-1-5-32-544' | ForEach-Object { "$($_.Name)" })
}
Try-Section 'dailyAccount' {
  # The person signed in at the screen, not the account that gave Bootmend administrator rights.
  $who = (Get-CimInstance Win32_ComputerSystem).UserName
  if (-not $who) { $who = "$env:USERDOMAIN\$env:USERNAME" }
  $sid = ([System.Security.Principal.NTAccount]$who).Translate([System.Security.Principal.SecurityIdentifier]).Value
  $enabled = @{}; Get-LocalUser | Where-Object Enabled | ForEach-Object { $enabled["$($_.SID)"] = $_.Name }
  $members = @(Get-LocalGroupMember -SID 'S-1-5-32-544' | ForEach-Object { "$($_.SID)" })
  $R.dailyAccount = [ordered]@{ Name = $who; Admin = ($members -contains $sid); OtherAdmins = @($members | Where-Object { $_ -ne $sid -and $enabled.ContainsKey($_) } | ForEach-Object { $enabled[$_] }) }
}
Try-Section 'drivers' {
  $pd = Get-CimInstance Win32_PnPSignedDriver | Where-Object DeviceName
  $R.driverStats = [ordered]@{ Total = @($pd).Count; Unsigned = @($pd | Where-Object { $_.IsSigned -eq $false }).Count }
  $R.drivers = @($pd | Where-Object { $_.DriverProviderName -and $_.DriverProviderName -notmatch 'Microsoft' } | Sort-Object DeviceClass, DeviceName | ForEach-Object { [ordered]@{ Class = $_.DeviceClass; Device = $_.DeviceName; Provider = $_.DriverProviderName; Version = $_.DriverVersion; Date = (D $_.DriverDate); Signed = $_.IsSigned } })
  $R.kernelDrivers = @(Get-CimInstance Win32_SystemDriver | Where-Object State -EQ 'Running' | ForEach-Object {
      $path = $_.PathName -replace '^\\\?\?\\', '' -replace '^\\SystemRoot', $env:WINDIR -replace '^system32', "$env:WINDIR\system32"
      $sg = Sig $path; if ($sg -notmatch 'Microsoft') { [ordered]@{ Name = $_.Name; Display = $_.DisplayName; Signer = $sg; Path = $path } } })
}
Try-Section 'deviceAccess' {
  # Windows records when each app last used the camera, microphone or location. Only times and app names are read.
  $list = New-Object System.Collections.Generic.List[object]
  foreach ($hive in 'HKCU', 'HKLM') {
    foreach ($cap in 'webcam', 'microphone', 'location') {
      $base = "${hive}:\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\$cap"
      Get-ChildItem -Path $base -Recurse | ForEach-Object {
        $p = Get-ItemProperty -Path $_.PSPath
        if ($p.LastUsedTimeStart) {
          $raw = $_.PSChildName
          $desktop = $raw -like '*#*'
          $path = $(if ($desktop) { $raw -replace '#', '\' } else { $null })
          $name = $(if ($desktop) { [IO.Path]::GetFileNameWithoutExtension($path) } else { (($raw -split '_')[0] -split '\.')[-1] })
          # A program's own description ("Google Chrome") reads better than its file name; Windows' own files keep the file name.
          if ($desktop -and $path -notlike "$env:WINDIR\*" -and (Test-Path -LiteralPath $path)) {
            $vi = (Get-Item -LiteralPath $path).VersionInfo
            $nice = "$($vi.FileDescription)".Trim(); if (-not $nice) { $nice = "$($vi.ProductName)".Trim() }
            if ($nice) { $name = $nice }
          }
          $list.Add([ordered]@{ Kind = $cap; App = $name; Path = $path; Store = -not $desktop; LastUsed = [datetime]::FromFileTime($p.LastUsedTimeStart).ToString('yyyy-MM-dd HH:mm'); InUse = ($p.LastUsedTimeStop -eq 0); Allowed = "$($p.Value)" })
        }
      }
    }
  }
  $R.deviceAccess = $list.ToArray()
  $R.deviceAccessSwitch = [ordered]@{}
  foreach ($cap in 'webcam', 'microphone', 'location') { $R.deviceAccessSwitch[$cap] = "$((Get-ItemProperty "HKCU:\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\$cap").Value)" }
}
Try-Section 'restore' {
  $sr = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\SystemRestore'
  $rp = [ordered]@{ Frequency = $sr.SystemRestorePointCreationFrequency; COn = $null; Latest = $null; Count = $null }
  if ($IsAdmin) {
    $rp.COn = [bool]((vssadmin list shadowstorage 2>$null | Out-String) -match 'For volume: \(C:\)')
    $pts = @(Get-ComputerRestorePoint)
    $rp.Count = $pts.Count
    if ($pts.Count) { $l = $pts[-1]; $rp.Latest = [ordered]@{ When = ([Management.ManagementDateTimeConverter]::ToDateTime($l.CreationTime)).ToString('yyyy-MM-dd HH:mm'); What = $l.Description } }
  }
  # $R and $r are the same name in PowerShell, so this block uses $rp.
  $R.restore = $rp
}
Emit
