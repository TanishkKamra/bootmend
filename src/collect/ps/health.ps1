Try-Section 'perf' {
  $os = Get-CimInstance Win32_OperatingSystem
  $R.perf = [ordered]@{ UptimeHours = [math]::Round(((Get-Date) - $os.LastBootUpTime).TotalHours, 1); RamTotalGB = [math]::Round($os.TotalVisibleMemorySize / 1MB, 1); RamFreeGB = [math]::Round($os.FreePhysicalMemory / 1MB, 1)
    CommitUsedGB = [math]::Round(($os.TotalVirtualMemorySize - $os.FreeVirtualMemory) / 1MB, 1); CommitLimitGB = [math]::Round($os.TotalVirtualMemorySize / 1MB, 1)
    Pagefiles = @(Get-CimInstance Win32_PageFileUsage | ForEach-Object { "$($_.Name) $($_.AllocatedBaseSize) MB" }); Processes = @(Get-Process).Count; ServicesRunning = @(Get-Service | Where-Object Status -EQ Running).Count
    CpuLoadPct = (Get-CimInstance Win32_Processor | Measure-Object LoadPercentage -Average).Average }
  $R.topMemory = @(Get-Process | Group-Object ProcessName | ForEach-Object { [pscustomobject]@{ N = $_.Name; M = ($_.Group | Measure-Object WorkingSet64 -Sum).Sum } } | Sort-Object M -Descending | Select-Object -First 12 | ForEach-Object { [ordered]@{ Name = $_.N; MB = [math]::Round($_.M / 1MB) } })
}
Try-Section 'crashControl' {
  # Whether Windows saves a crash file (0 = never), and where it can write one besides the page file.
  $cc = Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\CrashControl'
  $R.crashControl = [ordered]@{ Enabled = $cc.CrashDumpEnabled; DedicatedDumpFile = "$($cc.DedicatedDumpFile)"; SystemDrive = $env:SystemDrive }
}
Try-Section 'stability' {
  $st = Get-CimInstance Win32_ReliabilityStabilityMetrics | Sort-Object TimeGenerated -Descending | Select-Object -First 1
  $R.stabilityIndex = $(if ($st) { [math]::Round($st.SystemStabilityIndex, 1) } else { $null })
  $since = (Get-Date).AddDays(-90)
  $R.unexpectedShutdowns = @(Get-WinEvent -FilterHashtable @{LogName = 'System'; Id = 41; StartTime = $since } -MaxEvents 200 | ForEach-Object {
      $h = @{}; ([xml]$_.ToXml()).Event.EventData.Data | ForEach-Object { $h[$_.Name] = $_.'#text' }
      [ordered]@{ Date = $_.TimeCreated.ToString('yyyy-MM-dd HH:mm'); Code = ('0x{0:X}' -f ([long]$h.BugcheckCode -band 0xFFFFFFF)); PowerButton = ([long]$h.PowerButtonTimestamp -ne 0) } })
  $R.bugchecks = @(Get-WinEvent -FilterHashtable @{LogName = 'System'; Id = 1001; ProviderName = 'Microsoft-Windows-WER-SystemErrorReporting'; StartTime = $since } -MaxEvents 200 | ForEach-Object {
      $m = ($_.Message -replace '\s+', ' '); [ordered]@{ Date = $_.TimeCreated.ToString('yyyy-MM-dd HH:mm'); Text = $m.Substring(0, [math]::Min(400, $m.Length)) } })
  $R.whea = @(Get-WinEvent -FilterHashtable @{LogName = 'System'; ProviderName = 'Microsoft-Windows-WHEA-Logger'; StartTime = $since } -MaxEvents 100 | ForEach-Object { [ordered]@{ Date = $_.TimeCreated.ToString('yyyy-MM-dd HH:mm'); Id = $_.Id; Text = (($_.Message -split "`n")[0]).Trim() } })
  # Crash files: volmgr 162 is one saved, 161 one Windows failed to write while crashing, and 46 a
  # start-up where it couldn't get ready to write one (usually no page file on the system drive).
  $R.dumpEvents = @(Get-WinEvent -FilterHashtable @{ LogName = 'System'; ProviderName = 'volmgr'; Id = 161, 162, 46; StartTime = $since } -MaxEvents 300 |
      ForEach-Object { [ordered]@{ Date = $_.TimeCreated.ToString('yyyy-MM-dd HH:mm'); Id = $_.Id } })
  # The graphics driver freezing and Windows restarting it: Display 4101, and the WATCHDOG reports
  # Windows Error Reporting sends later. Those are dated by the report's file name, because the
  # event itself is logged when the report is queued, sometimes months on.
  $R.gpuResets = [ordered]@{
    Tdr = @(Get-WinEvent -FilterHashtable @{ LogName = 'System'; ProviderName = 'Display'; Id = 4101; StartTime = (Get-Date).AddDays(-30) } -MaxEvents 200 | ForEach-Object { $_.TimeCreated.ToString('yyyy-MM-dd HH:mm') })
    Watchdog = @(Get-WinEvent -FilterHashtable @{ LogName = 'Application'; ProviderName = 'Windows Error Reporting'; Id = 1001; StartTime = $since } -MaxEvents 1000 |
        Where-Object { $_.Properties[2].Value -eq 'LiveKernelEvent' -and "$($_.Properties[5].Value)" -in '141', '117' } |
        ForEach-Object { if ($_.Message -match '(?:WATCHDOG|WD)-(\d{4})(\d{2})(\d{2})-(\d{2})(\d{2})') { "$($matches[1])-$($matches[2])-$($matches[3]) $($matches[4]):$($matches[5])" } } | Select-Object -Unique)
  }
  $R.minidumps = @(Get-ChildItem "$env:WINDIR\Minidump" -Filter *.dmp | Sort-Object LastWriteTime -Descending | ForEach-Object { [ordered]@{ Path = $_.FullName; Date = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm'); KB = [math]::Round($_.Length / 1KB) } })
  # Event 1000 holds the program's file name (0) and full path (10). The file's own description
  # ("Google Chrome") reads better than "chrome.exe".
  $R.appCrashes = @(Get-WinEvent -FilterHashtable @{LogName = 'Application'; Id = 1000; ProviderName = 'Application Error'; StartTime = (Get-Date).AddDays(-30) } -MaxEvents 500 | ForEach-Object { [pscustomobject]@{ App = $_.Properties[0].Value; Path = $_.Properties[10].Value } } | Group-Object App | Sort-Object Count -Descending | Select-Object -First 10 | ForEach-Object {
      $p = $_.Group[0].Path; $vi = $(if ($p -and (Test-Path -LiteralPath $p)) { (Get-Item -LiteralPath $p).VersionInfo } else { $null })
      [ordered]@{ App = $_.Name; Name = $(if ($vi.FileDescription) { $vi.FileDescription.Trim() } elseif ($vi.ProductName) { $vi.ProductName.Trim() } else { $null }); Count = $_.Count } })
  # Apps that stopped responding and were closed (Application Hang 1002: file name 0, full path 5).
  $R.appHangs = @(Get-WinEvent -FilterHashtable @{LogName = 'Application'; Id = 1002; ProviderName = 'Application Hang'; StartTime = (Get-Date).AddDays(-30) } -MaxEvents 500 | ForEach-Object { [pscustomobject]@{ App = $_.Properties[0].Value; Path = $_.Properties[5].Value } } | Group-Object App | Sort-Object Count -Descending | Select-Object -First 10 | ForEach-Object {
      $p = $_.Group[0].Path; $vi = $(if ($p -and (Test-Path -LiteralPath $p)) { (Get-Item -LiteralPath $p).VersionInfo } else { $null })
      [ordered]@{ App = $_.Name; Name = $(if ($vi.FileDescription) { $vi.FileDescription.Trim() } elseif ($vi.ProductName) { $vi.ProductName.Trim() } else { $null }); Count = $_.Count } })
  # The latest Windows Memory Diagnostic result: 1101 or 1201 passed, 1102 or 1202 found errors.
  $R.memoryTest = Get-WinEvent -FilterHashtable @{ LogName = 'System'; ProviderName = 'Microsoft-Windows-MemoryDiagnostics-Results'; Id = 1101, 1102, 1201, 1202; StartTime = $since } -MaxEvents 1 |
    ForEach-Object { [ordered]@{ Date = $_.TimeCreated.ToString('yyyy-MM-dd HH:mm'); Id = $_.Id } }
  # Windows running out of memory to hand out (Resource Exhaustion Detector 2004). The message names
  # the programs using the most as "chrome.exe (1234)"; file names aren't translated.
  $R.lowMemory = @(Get-WinEvent -FilterHashtable @{ LogName = 'System'; ProviderName = 'Microsoft-Windows-Resource-Exhaustion-Detector'; Id = 2004; StartTime = (Get-Date).AddDays(-30) } -MaxEvents 100 | ForEach-Object {
      [ordered]@{ Date = $_.TimeCreated.ToString('yyyy-MM-dd HH:mm'); Apps = @([regex]::Matches("$($_.Message)", '([^\s,:;()]+\.exe) \(\d+\)') | ForEach-Object { $_.Groups[1].Value } | Select-Object -Unique) } })
}
Try-Section 'driverUpdates' {
  # Drivers that changed on this PC, newest change per device, so blue screens that began after an
  # update can offer the version before it. Kernel-PnP 400 with DeviceUpdated is logged each time a
  # driver is installed on a device, Windows' own included; the log keeps about a month. Software-only
  # devices are left out, and so are devices whose newest driver is one of Windows' own.
  $q = "*[System[EventID=400] and EventData[Data[@Name='DeviceUpdated']='true']]"
  $ev = @(Get-WinEvent -LogName 'Microsoft-Windows-Kernel-PnP/Configuration' -FilterXPath $q -MaxEvents 3000 | ForEach-Object {
      $d = @{}; ([xml]$_.ToXml()).Event.EventData.Data | ForEach-Object { $d[$_.Name] = $_.'#text' }
      if ($d.DeviceInstanceId -notlike 'SWD\*') {
        [pscustomobject]@{ Id = $d.DeviceInstanceId; Date = $_.TimeCreated; Version = $d.DriverVersion; Inbox = ($d.DriverInbox -eq 'true') } } })
  $groups = @($ev | Group-Object Id | Where-Object { -not ($_.Group | Sort-Object Date -Descending | Select-Object -First 1).Inbox })
  # One lookup for all the devices, because each takes about half a second.
  $devs = @{}; if ($groups) { Get-PnpDevice -InstanceId $groups.Name | ForEach-Object { $devs[$_.InstanceId] = $_ } }
  $R.driverUpdates = @($groups | ForEach-Object {
      $g = @($_.Group | Sort-Object Date -Descending); $id = $_.Name; $dev = $devs[$id]
      if ($dev -and $dev.Present) {
        # The same version installed again (a device plugged back in) isn't an update.
        $same = @($g | Where-Object { $_.Version -eq $g[0].Version }); $older = $g | Where-Object { $_.Version -ne $g[0].Version } | Select-Object -First 1
        # The driver file names the device in crash files, unless it's one of Windows' own that
        # many devices share (usbxhci.sys).
        $ip = "$((Get-ItemProperty "HKLM:\SYSTEM\CurrentControlSet\Services\$($dev.Service)").ImagePath)"
        $file = $ip -replace '^\\SystemRoot\\', "$env:WINDIR\" -replace '^\\\?\?\\', '' -replace '^system32\\', "$env:WINDIR\System32\"
        $ms = $ip -and ("$((Get-Item -LiteralPath $file).VersionInfo.CompanyName)" -like 'Microsoft*')
        [ordered]@{ Device = $id; Name = $dev.FriendlyName; Class = $dev.Class; Image = $(if ($ip -and -not $ms) { Split-Path $ip -Leaf } else { $null })
          Version = $g[0].Version; Before = $(if ($older) { $older.Version } else { $null }); BeforeInbox = [bool]$older.Inbox
          Date = $same[-1].Date.ToString('yyyy-MM-dd HH:mm') } } })
}
Try-Section 'imageHealth' {
  # Whether Windows has flagged damage to its own files (DISM CheckHealth). It reads a flag rather
  # than scanning, and needs administrator rights.
  if ($IsAdmin) { $R.imageHealth = "$((Repair-WindowsImage -Online -CheckHealth).ImageHealthState)" }
}
Try-Section 'boot' {
  function EvData($e) { $h = @{}; ([xml]$e.ToXml()).Event.EventData.Data | ForEach-Object { $h[$_.Name] = $_.'#text' }; $h }
  $R.boots = @(Get-WinEvent -FilterHashtable @{LogName = 'Microsoft-Windows-Diagnostics-Performance/Operational'; Id = 100 } -MaxEvents 10 | ForEach-Object { $d = EvData $_
      [ordered]@{ Date = $_.TimeCreated.ToString('yyyy-MM-dd HH:mm'); TotalSec = [math]::Round([int]$d.BootTime / 1000, 1); MainPathSec = [math]::Round([int]$d.MainPathBootTime / 1000, 1); PostBootSec = [math]::Round([int]$d.BootPostBootTime / 1000, 1) } })
  $R.bootDelays = @(Get-WinEvent -FilterHashtable @{LogName = 'Microsoft-Windows-Diagnostics-Performance/Operational'; Id = 101, 102, 103, 109; StartTime = (Get-Date).AddDays(-60) } -MaxEvents 300 | ForEach-Object { $d = EvData $_
      [pscustomobject]@{ K = "$(if ($d.FriendlyName) { $d.FriendlyName } else { $d.Name })"; Kind = @{101 = 'App'; 102 = 'Driver'; 103 = 'Service'; 109 = 'Device' }[$_.Id]; T = [int]$d.DegradationTime } } |
    Group-Object K | ForEach-Object { [ordered]@{ Name = $_.Name; Kind = $_.Group[0].Kind; Times = $_.Count; AvgSec = [math]::Round((($_.Group | Measure-Object T -Average).Average) / 1000, 1) } } | Sort-Object { - $_.AvgSec } | Select-Object -First 12)
  $R.serviceTimeouts = @(Get-WinEvent -FilterHashtable @{LogName = 'System'; Id = 7009, 7011, 7000; StartTime = (Get-Date).AddDays(-7) } -MaxEvents 200 | ForEach-Object { ($_.Message -replace '\s+', ' ').Substring(0, [math]::Min(160, ($_.Message -replace '\s+', ' ').Length)) } | Group-Object | ForEach-Object { [ordered]@{ Message = $_.Name; Count = $_.Count } })
}
Emit
