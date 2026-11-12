Try-Section 'perf' {
  $os = Get-CimInstance Win32_OperatingSystem
  $R.perf = [ordered]@{ UptimeHours = [math]::Round(((Get-Date) - $os.LastBootUpTime).TotalHours, 1); RamTotalGB = [math]::Round($os.TotalVisibleMemorySize / 1MB, 1); RamFreeGB = [math]::Round($os.FreePhysicalMemory / 1MB, 1)
    CommitUsedGB = [math]::Round(($os.TotalVirtualMemorySize - $os.FreeVirtualMemory) / 1MB, 1); CommitLimitGB = [math]::Round($os.TotalVirtualMemorySize / 1MB, 1)
    Pagefiles = @(Get-CimInstance Win32_PageFileUsage | ForEach-Object { "$($_.Name) $($_.AllocatedBaseSize) MB" }); Processes = @(Get-Process).Count; ServicesRunning = @(Get-Service | Where-Object Status -EQ Running).Count
    CpuLoadPct = (Get-CimInstance Win32_Processor | Measure-Object LoadPercentage -Average).Average }
  $R.topMemory = @(Get-Process | Group-Object ProcessName | ForEach-Object { [pscustomobject]@{ N = $_.Name; M = ($_.Group | Measure-Object WorkingSet64 -Sum).Sum } } | Sort-Object M -Descending | Select-Object -First 12 | ForEach-Object { [ordered]@{ Name = $_.N; MB = [math]::Round($_.M / 1MB) } })
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
  $R.minidumps = @(Get-ChildItem "$env:WINDIR\Minidump" -Filter *.dmp | Sort-Object LastWriteTime -Descending | ForEach-Object { [ordered]@{ Path = $_.FullName; Date = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm'); KB = [math]::Round($_.Length / 1KB) } })
  $R.appCrashes = @(Get-WinEvent -FilterHashtable @{LogName = 'Application'; Id = 1000; ProviderName = 'Application Error'; StartTime = (Get-Date).AddDays(-30) } -MaxEvents 500 | ForEach-Object { ($_.Properties[0].Value) } | Group-Object | Sort-Object Count -Descending | Select-Object -First 10 | ForEach-Object { [ordered]@{ App = $_.Name; Count = $_.Count } })
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
