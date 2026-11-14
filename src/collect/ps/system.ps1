$cs = Get-CimInstance Win32_ComputerSystem; $csp = Get-CimInstance Win32_ComputerSystemProduct; $os = Get-CimInstance Win32_OperatingSystem
$bios = Get-CimInstance Win32_BIOS; $bb = Get-CimInstance Win32_BaseBoard; $encl = Get-CimInstance Win32_SystemEnclosure
$cv = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'

Try-Section 'device' {
  $tpm = Get-Tpm; $tpmc = Get-CimInstance -Namespace root\cimv2\security\microsofttpm -ClassName Win32_Tpm
  $sb = $null; try { $sb = Confirm-SecureBootUEFI } catch { }
  # Confirm-SecureBootUEFI needs administrator rights; Windows also records the state here.
  if ($null -eq $sb) { $st = (Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\SecureBoot\State').UEFISecureBootEnabled; if ($null -ne $st) { $sb = ($st -eq 1) } }
  # Progress of the switch to the 2023 Secure Boot certificates (community-documented values).
  $sbs = Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\SecureBoot\Servicing'
  $R.device = [ordered]@{
    Manufacturer = Clean $cs.Manufacturer; Model = Clean $cs.Model; Family = Clean $cs.SystemFamily; SKU = Clean $cs.SystemSKUNumber
    Serial = Clean $bios.SerialNumber; UUID = $csp.UUID; ChassisTypes = @($encl.ChassisTypes); Hostname = $env:COMPUTERNAME; Workgroup = $cs.Workgroup
    BoardMaker = Clean $bb.Manufacturer; BoardProduct = Clean $bb.Product; BoardVersion = Clean $bb.Version
    BIOSVendor = Clean $bios.Manufacturer; BIOSVersion = Clean $bios.SMBIOSBIOSVersion; BIOSDate = (D $bios.ReleaseDate)
    FirmwareMode = $env:firmware_type; SecureBoot = $sb
    SecureBootCert = $sbs.UEFICA2023Status; SecureBootCertCapable = $sbs.WindowsUEFICA2023Capable; SecureBootCertError = [bool]$sbs.UEFICA2023Error
    # Joined to a work or school domain, or to Microsoft Entra: its IT department manages it.
    Joined = ([bool]$cs.PartOfDomain -or [bool](Get-ChildItem 'HKLM:\SYSTEM\CurrentControlSet\Control\CloudDomainJoin\JoinInfo'))
    TpmPresent = $tpm.TpmPresent; TpmReady = $tpm.TpmReady; TpmMaker = Clean $tpm.ManufacturerIdTxt; TpmVersion = Clean $tpm.ManufacturerVersion
    TpmSpec = $(if ($tpmc.SpecVersion) { ($tpmc.SpecVersion -split ',')[0] } else { $null })
    HasBattery = [bool](Get-CimInstance Win32_Battery)
  }
  # Get-Tpm needs administrator rights; tpmtool answers the basic questions without them.
  if ($null -eq $R.device.TpmPresent) {
    $t = KV (tpmtool getdeviceinformation 2>$null | ForEach-Object { $_ -replace '^-', '' })
    if ($t['TPM Present']) { $R.device.TpmPresent = ($t['TPM Present'] -eq 'True'); $R.device.TpmSpec = $t['TPM Version']; $R.device.TpmMaker = $t['TPM Manufacturer ID'] }
  }
}

Try-Section 'os' {
  $lics = @(Get-CimInstance SoftwareLicensingProduct -Filter "ApplicationID='55c92734-d682-4d71-983e-d6ec3f16059f' AND PartialProductKey IS NOT NULL")
  # Extended Security Updates come as an add-on licence beside Windows' own.
  $lic = $lics | Where-Object { $_.Name -notmatch 'ESU' } | Select-Object -First 1
  $sh = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\WindowsSelfHost\Applicability'
  $dg = Get-CimInstance -Namespace root\Microsoft\Windows\DeviceGuard -ClassName Win32_DeviceGuard
  $win10 = $os.Caption -match 'Windows 10'
  # Windows 10 consumer ESU sign-up: 3 = this device enrolled, 5 = enrolled through a Microsoft account.
  $esu = $(if ($win10) { Get-ItemProperty 'HKCU:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Windows\ConsumerESU' } else { $null })
  $R.os = [ordered]@{
    Edition = $os.Caption; Version = $cv.DisplayVersion; Build = "$($cv.CurrentBuild).$($cv.UBR)"; Architecture = $os.OSArchitecture
    InsiderChannel = "$($sh.BranchName) $($sh.Ring)".Trim(); Installed = (D $os.InstallDate); LastBoot = $os.LastBootUpTime.ToString('yyyy-MM-dd HH:mm')
    Activation = $(switch ($lic.LicenseStatus) { 1 { 'Activated' } 0 { 'Unlicensed' } $null { $null } default { "Status $($lic.LicenseStatus)" } }); LicenseChannel = $lic.ProductKeyChannel
    Locale = (Get-Culture).Name; TimeZone = (Get-TimeZone).DisplayName; RegisteredOwner = $cv.RegisteredOwner
    FastStartup = (Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Power').HiberbootEnabled
    VBS = $(switch ($dg.VirtualizationBasedSecurityStatus) { 0 { 'Off' } 1 { 'Enabled, not running' } 2 { 'Running' } default { $null } })
    MemoryIntegrity = $(if ($null -eq $dg) { $null } elseif ($dg.SecurityServicesRunning -contains 2) { 'On' } elseif ($dg.SecurityServicesConfigured -contains 2) { 'Enabled, not running' } else { 'Off' })
    VbsAvailable = $(if ($null -eq $dg) { $null } else { $dg.AvailableSecurityProperties -contains 1 })
    EsuState = $esu.ESUEligibility
    EsuLicensed = $(if ($win10) { [bool]($lics | Where-Object { $_.Name -match 'ESU' -and $_.LicenseStatus -eq 1 }) } else { $null })
    PendingReboot = (Test-Path 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing\RebootPending') -or (Test-Path 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\RebootRequired')
  }
  $R.updates = @(Get-HotFix | Sort-Object InstalledOn -Descending | Select-Object -First 15 | ForEach-Object { [ordered]@{ Id = $_.HotFixID; Type = $_.Description; Installed = (D $_.InstalledOn) } })
}

Try-Section 'cpu' {
  $c = Get-CimInstance Win32_Processor | Select-Object -First 1
  $R.cpu = [ordered]@{ Name = (Clean $c.Name); Maker = $c.Manufacturer; Cores = $c.NumberOfCores; Threads = $c.NumberOfLogicalProcessors; RatedMHz = $c.MaxClockSpeed; CurrentMHz = $c.CurrentClockSpeed
    L2KB = $c.L2CacheSize; L3KB = $c.L3CacheSize; Socket = $c.SocketDesignation; Identifier = $c.Caption; VirtualizationFirmware = $c.VirtualizationFirmwareEnabled; LoadPct = $c.LoadPercentage }
}

Try-Section 'memory' {
  $arr = Get-CimInstance Win32_PhysicalMemoryArray | Select-Object -First 1
  $types = @{20 = 'DDR'; 21 = 'DDR2'; 24 = 'DDR3'; 26 = 'DDR4'; 30 = 'LPDDR4'; 34 = 'DDR5'; 35 = 'LPDDR5' }
  $R.memory = [ordered]@{ TotalGB = (GB $cs.TotalPhysicalMemory); Slots = $arr.MemoryDevices; MaxSupportedGB = $(if ($arr.MaxCapacityEx) { [math]::Round($arr.MaxCapacityEx / 1MB) } else { $null })
    Sticks = @(Get-CimInstance Win32_PhysicalMemory | ForEach-Object { [ordered]@{ Slot = $_.DeviceLocator; Bank = $_.BankLabel; SizeGB = (GB $_.Capacity 0); Type = $types[[int]$_.SMBIOSMemoryType]
          SpeedMTs = $_.Speed; ConfiguredMTs = $_.ConfiguredClockSpeed; Maker = (Clean $_.Manufacturer); Part = (Clean $_.PartNumber)
          FormFactor = $(switch ([int]$_.FormFactor) { 12 { 'SODIMM' } 8 { 'DIMM' } default { "$($_.FormFactor)" } }) } }) }
}

Try-Section 'gpu' {
  $vram = @{}; Get-ItemProperty 'HKLM:\SYSTEM\ControlSet001\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}\0*' | Where-Object DriverDesc | ForEach-Object { $vram[$_.DriverDesc] = $_.'HardwareInformation.qwMemorySize' }
  $R.gpus = @(Get-CimInstance Win32_VideoController | ForEach-Object { [ordered]@{ Name = $_.Name; Driver = $_.DriverVersion; DriverDate = (D $_.DriverDate); Processor = $_.VideoProcessor
        VramMB = $(if ($vram[$_.Name]) { [math]::Round([double]$vram[$_.Name] / 1MB) } else { $null }); Width = $_.CurrentHorizontalResolution; Height = $_.CurrentVerticalResolution; RefreshHz = $_.CurrentRefreshRate; Status = $_.Status } })
}

Try-Section 'displays' {
  function Dec($a) { if ($a) { (($a | Where-Object { $_ -ne 0 } | ForEach-Object { [char]$_ }) -join '').Trim() } }
  $ids = Get-CimInstance -Namespace root\wmi WmiMonitorID; $bas = Get-CimInstance -Namespace root\wmi WmiMonitorBasicDisplayParams; $con = Get-CimInstance -Namespace root\wmi WmiMonitorConnectionParams
  $R.displays = @($ids | ForEach-Object { $iname = $_.InstanceName; $b = $bas | Where-Object InstanceName -EQ $iname; $cn = $con | Where-Object InstanceName -EQ $iname
      $diag = if ($b.MaxHorizontalImageSize) { [math]::Round([math]::Sqrt([math]::Pow($b.MaxHorizontalImageSize, 2) + [math]::Pow($b.MaxVerticalImageSize, 2)) / 2.54, 1) } else { $null }
      [ordered]@{ Name = (Dec $_.UserFriendlyName); Maker = (Dec $_.ManufacturerName); Product = (Dec $_.ProductCodeID); Year = $_.YearOfManufacture; WidthCm = $b.MaxHorizontalImageSize; HeightCm = $b.MaxVerticalImageSize; DiagonalIn = $diag
        Connection = $(switch ([long]$cn.VideoOutputTechnology) { 2147483648 { 'Internal' } 0 { 'VGA' } 4 { 'DVI' } 5 { 'HDMI' } 10 { 'DisplayPort' } 11 { 'DisplayPort (internal)' } default { "Other ($($cn.VideoOutputTechnology))" } }) } })
}

Try-Section 'directx' {
  $dx = Join-Path $env:BOOTMEND_TMP 'dxdiag.txt'
  Remove-Item $dx -Force
  $p = Start-Process dxdiag -ArgumentList '/whql:off', '/t', $dx -WindowStyle Hidden -PassThru
  if (-not $p.WaitForExit(90000)) { Stop-Process -Id $p.Id -Force }
  if (Test-Path $dx) { $k = KV (Get-Content $dx)
    $R.directx = [ordered]@{ DirectX = $k['DirectX Version']; WDDM = $k['Driver Model']; FeatureLevels = $k['Feature Levels']; HDR = $k['HDR Support']; NativeMode = $k['Native Mode']; HWScheduling = $k['Hardware Scheduling'] } }
}

Try-Section 'battery' {
  if ($R.device.HasBattery) {
    $bx = Join-Path $env:BOOTMEND_TMP 'battery.xml'; powercfg /batteryreport /xml /output $bx | Out-Null
    if (Test-Path $bx) { [xml]$x = Get-Content $bx; $b = $x.BatteryReport.Batteries.Battery | Select-Object -First 1
      $R.battery = [ordered]@{ Name = $b.Id; Maker = $b.Manufacturer; Chemistry = $b.Chemistry; DesignmWh = [int]$b.DesignCapacity; FullChargemWh = [int]$b.FullChargeCapacity; Cycles = [int]$b.CycleCount
        ChargePct = (Get-CimInstance Win32_Battery).EstimatedChargeRemaining; OnAC = ((Get-CimInstance -Namespace root\wmi BatteryStatus).PowerOnline | Select-Object -First 1) } }
  }
}

Try-Section 'power' {
  $R.power = [ordered]@{ Plan = ((powercfg /getactivescheme) -replace '.*\((.*)\).*', '$1'); Available = @((powercfg /a) | Select-Object -Skip 1 | Where-Object { $_ -match '^\s+\S' -and $_ -notmatch 'not|does not|has not' } | ForEach-Object { $_.Trim() } | Select-Object -First 6) }
}

Try-Section 'hardware' {
  $R.audio = @(Get-CimInstance Win32_SoundDevice | ForEach-Object { $_.Name } | Select-Object -Unique)
  $R.cameras = @(Get-PnpDevice -PresentOnly | Where-Object { $_.Class -in 'Camera', 'Image' } | ForEach-Object { $_.FriendlyName } | Select-Object -Unique)
  $R.input = @((Get-PnpDevice -Class Keyboard -PresentOnly | ForEach-Object { "Keyboard: $($_.FriendlyName)" }) + (Get-PnpDevice -Class Mouse -PresentOnly | ForEach-Object { "Pointer: $($_.FriendlyName)" }) | Select-Object -Unique)
  $R.usbControllers = @(Get-CimInstance Win32_USBController | ForEach-Object { $_.Name } | Select-Object -Unique)
  $R.bluetooth = @(Get-PnpDevice -Class Bluetooth -PresentOnly | Where-Object { $_.FriendlyName -notmatch 'Enumerator|RFCOMM|LE Generic|Device Identification|Avrcp|Hands-Free|Audio Gateway' } | ForEach-Object { $_.FriendlyName } | Select-Object -Unique)
  $R.problemDevices = @(Get-PnpDevice -PresentOnly | Where-Object { $_.Status -eq 'Error' -or $_.ConfigManagerErrorCode -ne 0 } | ForEach-Object { [ordered]@{ Name = $_.FriendlyName; Class = $_.Class; Code = [int]$_.ConfigManagerErrorCode } })
}
$R.isAdmin = $IsAdmin
Try-Section 'winupdate' {
  # Reads Windows Update's local history only; nothing here contacts Microsoft.
  $q = (New-Object -ComObject Microsoft.Update.Session).CreateUpdateSearcher()
  $n = $q.GetTotalHistoryCount()
  $hist = @($q.QueryHistory(0, [math]::Min($n, 200)) | Where-Object { $_.Title -and $_.Title -notmatch 'Security Intelligence Update|Definition Update' })
  $since = (Get-Date).AddDays(-60)
  $failed = @($hist | Where-Object { $_.ResultCode -eq 4 -and $_.Date -gt $since } | Group-Object Title | ForEach-Object { [ordered]@{ Title = $_.Name; Times = $_.Count; Last = $_.Group[0].Date.ToString('yyyy-MM-dd'); Code = ('0x{0:X8}' -f $_.Group[0].HResult) } })
  # Microsoft Store app updates (titles like '9NBLGGH537C2-Microsoft.XboxGameOverlay') aren't Windows updates.
  $ok = $hist | Where-Object { $_.ResultCode -eq 2 -and $_.Operation -eq 1 -and $_.Title -cnotmatch '^[0-9A-Z]{12}-' } | Select-Object -First 1
  $ux = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\WindowsUpdate\UX\Settings'
  # The first update installed since the last restart, so a waiting restart has a start date. Update
  # history times are UTC.
  $boot = $os.LastBootUpTime.ToUniversalTime()
  $first = $hist | Where-Object { $_.ResultCode -eq 2 -and $_.Operation -eq 1 -and $_.Date -gt $boot -and $_.Title -cnotmatch '^[0-9A-Z]{12}-' } | Sort-Object Date | Select-Object -First 1
  # Windows also logs the moment an installed package first needs a restart. The earlier of the two wins.
  $staged = $(if ($R.os.PendingReboot) { Get-WinEvent -FilterHashtable @{ LogName = 'Setup'; ProviderName = 'Microsoft-Windows-Servicing'; Id = 4; StartTime = $os.LastBootUpTime } | Sort-Object TimeCreated | Select-Object -First 1 } else { $null })
  $waiting = @($(if ($first) { [DateTime]::SpecifyKind($first.Date, 'Utc').ToLocalTime() }), $staged.TimeCreated) | Where-Object { $_ } | Sort-Object | Select-Object -First 1
  $pol = Get-ItemProperty 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate\AU'
  $R.winupdate = [ordered]@{
    LastInstalled = $(if ($ok) { $ok.Date.ToString('yyyy-MM-dd') } else { $null }); LastInstalledTitle = $(if ($ok) { $ok.Title } else { $null })
    Failed = $failed
    PausedUntil = $(if ($ux.PauseUpdatesExpiryTime) { D $ux.PauseUpdatesExpiryTime } else { $null })
    PausedSince = $(if ($ux.PauseUpdatesStartTime) { D $ux.PauseUpdatesStartTime } else { $null })
    RestartWaitingSince = $(if ($waiting) { $waiting.ToString('yyyy-MM-dd') } else { $null })
    AutoUpdateOff = ($pol.NoAutoUpdate -eq 1)
  }
}
Emit
