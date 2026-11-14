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
