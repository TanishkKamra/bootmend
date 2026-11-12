Try-Section 'defender' {
  $m = Get-MpComputerStatus; $p = Get-MpPreference
  $R.defender = [ordered]@{
    Present = [bool]$m; AntivirusEnabled = $m.AntivirusEnabled; RealTime = $m.RealTimeProtectionEnabled; Tamper = $m.IsTamperProtected; BehaviorMonitor = $m.BehaviorMonitorEnabled
    Engine = $m.AMEngineVersion; Signatures = $m.AntivirusSignatureVersion; SignaturesUpdated = $(if ($m.AntivirusSignatureLastUpdated) { $m.AntivirusSignatureLastUpdated.ToString('yyyy-MM-dd HH:mm') } else { $null }); SignatureAgeDays = $m.AntivirusSignatureAge
    LastQuickScan = $(if ($m.QuickScanEndTime) { $m.QuickScanEndTime.ToString('yyyy-MM-dd HH:mm') } else { $null }); LastFullScan = $(if ($m.FullScanEndTime) { $m.FullScanEndTime.ToString('yyyy-MM-dd HH:mm') } else { $null })
    # Exclusions are only readable as administrator; a standard user sees a placeholder string.
    ExclusionPaths = @($p.ExclusionPath | Where-Object { $_ }); ExclusionProcesses = @($p.ExclusionProcess | Where-Object { $_ }); ExclusionExtensions = @($p.ExclusionExtension | Where-Object { $_ })
    PUAProtection = $p.PUAProtection; CloudProtection = $p.MAPSReporting; ControlledFolderAccess = $p.EnableControlledFolderAccess
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
  }
  $hosts = Get-Content "$env:WINDIR\System32\drivers\etc\hosts" | Where-Object { $_ -notmatch '^\s*#' -and $_.Trim() }
  $R.hosts = [ordered]@{ Entries = @($hosts).Count; Sample = @($hosts | Select-Object -First 8) }
  $R.accounts = @(Get-LocalUser | ForEach-Object { [ordered]@{ Name = $_.Name; Enabled = [bool]$_.Enabled; LastLogon = (D $_.LastLogon); PasswordRequired = [bool]$_.PasswordRequired; Source = "$($_.PrincipalSource)" } })
  $R.admins = @(Get-LocalGroupMember -SID 'S-1-5-32-544' | ForEach-Object { "$($_.Name)" })
}
Try-Section 'drivers' {
  $pd = Get-CimInstance Win32_PnPSignedDriver | Where-Object DeviceName
  $R.driverStats = [ordered]@{ Total = @($pd).Count; Unsigned = @($pd | Where-Object { $_.IsSigned -eq $false }).Count }
  $R.drivers = @($pd | Where-Object { $_.DriverProviderName -and $_.DriverProviderName -notmatch 'Microsoft' } | Sort-Object DeviceClass, DeviceName | ForEach-Object { [ordered]@{ Class = $_.DeviceClass; Device = $_.DeviceName; Provider = $_.DriverProviderName; Version = $_.DriverVersion; Date = (D $_.DriverDate); Signed = $_.IsSigned } })
  $R.kernelDrivers = @(Get-CimInstance Win32_SystemDriver | Where-Object State -EQ 'Running' | ForEach-Object {
      $path = $_.PathName -replace '^\\\?\?\\', '' -replace '^\\SystemRoot', $env:WINDIR -replace '^system32', "$env:WINDIR\system32"
      $sg = Sig $path; if ($sg -notmatch 'Microsoft') { [ordered]@{ Name = $_.Name; Display = $_.DisplayName; Signer = $sg; Path = $path } } })
}
Emit
